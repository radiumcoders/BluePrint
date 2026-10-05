//! Spawning dev servers and streaming their output.

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Project;
use crate::platform;

/// How long a server gets to shut down after SIGTERM before it is SIGKILLed.
pub const STOP_GRACE: Duration = Duration::from_secs(4);

pub enum Event {
    /// One line of output from a project's process.
    Log { id: u64, line: String },
}

pub struct Running {
    child: Child,
    pub started: Instant,
    stop_sent: Option<Instant>,
    killed: bool,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped();
        }
        Ok(status)
    }

    /// The shell has exited. Anything left in its process group (say, a dev
    /// server whose parent died) gets SIGTERM; the guardian keeps tracking the
    /// group until it is empty.
    fn reaped(&mut self) {
        let pid = self.pid();
        if platform::tree_alive(pid) {
            platform::terminate(pid);
        } else {
            platform::release(pid);
        }
    }

    pub fn stopping(&self) -> bool {
        self.stop_sent.is_some()
    }

    /// Ask the whole process group to exit; signalling the group also catches
    /// grandchildren (e.g. `sh` -> `npm run dev` -> `node`).
    pub fn stop(&mut self) {
        if self.stop_sent.is_none() {
            platform::terminate(self.pid());
            self.stop_sent = Some(Instant::now());
        }
    }

    /// Escalate to SIGKILL once the grace period has elapsed.
    pub fn enforce_deadline(&mut self) {
        if let Some(t) = self.stop_sent
            && !self.killed
            && t.elapsed() >= STOP_GRACE
        {
            platform::kill(self.pid());
            self.killed = true;
        }
    }

    pub fn kill_now(&mut self) {
        platform::kill(self.pid());
        self.killed = true;
        let _ = self.child.wait();
        self.reaped();
    }
}

/// Ports handed out to projects without a fixed one.
pub const AUTO_PORTS: std::ops::RangeInclusive<u16> = 4000..=4999;

/// Run `command` (shell syntax) in the project's folder with `PORT` set.
/// It's logged first so the output shows exactly what ran.
pub fn spawn(project: &Project, command: &str, port: u16, id: u64, tx: Sender<Event>) -> Result<Running, String> {
    if !project.path.is_dir() {
        return Err(format!("folder not found: {}", project.path.display()));
    }
    let mut shell = platform::shell(command);
    platform::isolate(&mut shell);
    let mut child = shell
        .current_dir(&project.path)
        .env("PORT", port.to_string())
        .env("FORCE_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start: {e}"))?;

    platform::adopt(&child);
    let _ = tx.send(Event::Log { id, line: format!("$ PORT={port} {command}") });
    if let Some(out) = child.stdout.take() {
        pipe_lines(out, id, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pipe_lines(err, id, tx);
    }
    Ok(Running { child, started: Instant::now(), stop_sent: None, killed: false })
}

fn pipe_lines<R: Read + Send + 'static>(r: R, id: u64, tx: Sender<Event>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&buf);
                    let text = text.trim_end_matches(['\n', '\r']);
                    // Progress bars redraw with \r; keep only what would be visible.
                    let visible = text.rsplit('\r').next().unwrap_or(text);
                    if tx.send(Event::Log { id, line: visible.to_string() }).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// A free port from [`AUTO_PORTS`], trying `preferred` first (so a project
/// keeps its port across restarts) and skipping `taken`.
pub fn free_port(preferred: Option<u16>, taken: &[u16]) -> Option<u16> {
    preferred
        .into_iter()
        .chain(AUTO_PORTS)
        .find(|p| AUTO_PORTS.contains(p) && !taken.contains(p) && !port_in_use(*p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_ports() {
        let first = free_port(None, &[]).unwrap();
        assert!(AUTO_PORTS.contains(&first));
        assert_ne!(free_port(None, &[first]), Some(first));
        // A preferred port outside the range isn't handed out.
        assert_ne!(free_port(Some(80), &[]), Some(80));
        let held = std::net::TcpListener::bind(("127.0.0.1", 4999)).ok();
        if held.is_some() {
            assert_ne!(free_port(Some(4999), &[]), Some(4999));
        }
    }
}
