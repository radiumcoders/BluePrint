//! Spawning dev servers and streaming their output.

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Project;
use crate::platform::{self, Tree};

/// How long a server gets to shut down after SIGTERM before it is SIGKILLed.
pub const STOP_GRACE: Duration = Duration::from_secs(4);
/// How long after SIGKILL to keep waiting for the tree to disappear (a
/// process stuck in the kernel can't die at once) before giving up on it.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// Longest line of output kept; the rest of a longer line is dropped.
pub const MAX_LINE: usize = 4096;

pub enum Event {
    /// One line of output from a project's process.
    Log { id: u64, line: String },
}

pub struct Running {
    child: Child,
    tree: Tree,
    pub started: Instant,
    /// The project as it was when started; edits apply on the next start.
    pub project: Project,
    /// The command that ran, with an empty one resolved.
    pub command: String,
    /// The port it was given in `PORT`.
    pub port: u16,
    /// The user asked it to stop.
    stop_requested: bool,
    /// When the tree was asked to exit, which starts the clock to SIGKILL.
    term_sent: Option<Instant>,
    killed: Option<Instant>,
    /// The shell's exit status, once it has exited.
    exited: Option<ExitStatus>,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Check on the server. Its exit status comes back once the shell has
    /// exited *and* nothing it started is left. A shell that exits leaving
    /// processes behind (say, a server whose parent died) gets those asked to
    /// exit and killed after [`STOP_GRACE`], like a stop.
    pub fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.exited.is_none() {
            self.exited = self.child.try_wait()?;
        }
        let Some(status) = self.exited else {
            self.enforce_deadline();
            return Ok(None);
        };
        let gave_up = self.killed.is_some_and(|t| t.elapsed() >= KILL_GRACE);
        if !self.tree.alive() || gave_up {
            return Ok(Some(status));
        }
        if self.term_sent.is_none() {
            self.tree.terminate();
            self.term_sent = Some(Instant::now());
        }
        self.enforce_deadline();
        Ok(None)
    }

    /// The shell exited but processes it started are still being stopped.
    pub fn lingering(&self) -> bool {
        self.exited.is_some()
    }

    /// The user asked it to stop.
    pub fn stop_requested(&self) -> bool {
        self.stop_requested
    }

    /// On its way out: asked to stop, or its shell has exited.
    pub fn stopping(&self) -> bool {
        self.term_sent.is_some() || self.exited.is_some()
    }

    /// Ask the whole tree to exit; signalling the group also catches
    /// grandchildren (e.g. `sh` -> `npm run dev` -> `node`).
    pub fn stop(&mut self) {
        self.stop_requested = true;
        if self.term_sent.is_none() {
            self.tree.terminate();
            self.term_sent = Some(Instant::now());
        }
    }

    /// Escalate to SIGKILL once the grace period has elapsed.
    fn enforce_deadline(&mut self) {
        if let Some(t) = self.term_sent
            && self.killed.is_none()
            && t.elapsed() >= STOP_GRACE
        {
            self.tree.kill();
            self.killed = Some(Instant::now());
        }
    }

    /// Kill the tree and wait for the shell.
    pub fn kill_now(&mut self) {
        self.tree.kill();
        self.killed = Some(Instant::now());
        let _ = self.child.wait();
    }

    /// Stop it and wait until it's gone, on a thread of its own so the
    /// caller doesn't block.
    pub fn stop_in_background(mut self) {
        self.stop();
        thread::spawn(move || {
            while let Ok(None) = self.poll() {
                thread::sleep(Duration::from_millis(100));
            }
        });
    }
}

/// Ports handed out to projects without a fixed one.
pub const AUTO_PORTS: std::ops::RangeInclusive<u16> = 4000..=4999;

/// Run `command` (shell syntax) in the project's folder with `PORT` set,
/// streaming its output to `tx`.
pub fn spawn(project: &Project, command: &str, port: u16, id: u64, tx: SyncSender<Event>) -> Result<Running, String> {
    if !project.path.is_dir() {
        return Err(format!("folder not found: {}", project.path.display()));
    }
    let mut shell = platform::shell(command).map_err(|e| format!("can't run this command: {e}"))?;
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

    let tree = match platform::adopt(&child) {
        Ok(tree) => tree,
        Err(e) => {
            // Untracked, it could outlive blueprint, so it doesn't get to run.
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("failed to start: {e}"));
        }
    };
    if let Some(out) = child.stdout.take() {
        pipe_lines(out, id, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pipe_lines(err, id, tx);
    }
    Ok(Running {
        child,
        tree,
        started: Instant::now(),
        project: project.clone(),
        command: command.to_string(),
        port,
        stop_requested: false,
        term_sent: None,
        killed: None,
        exited: None,
    })
}

fn pipe_lines<R: Read + Send + 'static>(r: R, id: u64, tx: SyncSender<Event>) {
    thread::spawn(move || {
        let mut lines = Lines::default();
        let mut reader = BufReader::with_capacity(64 * 1024, r);
        loop {
            let n = match reader.fill_buf() {
                Ok([]) | Err(_) => break,
                Ok(buf) => {
                    for line in lines.feed(buf) {
                        // A full queue blocks here, which in turn blocks a
                        // server that writes faster than the window keeps up.
                        if tx.send(Event::Log { id, line }).is_err() {
                            return;
                        }
                    }
                    buf.len()
                }
            };
            reader.consume(n);
        }
        if let Some(line) = lines.finish() {
            let _ = tx.send(Event::Log { id, line });
        }
    });
}

/// Splits a byte stream into display lines. A carriage return not followed
/// by a newline starts the line over, as a progress bar redrawing itself
/// would look; lines are cut at [`MAX_LINE`] bytes.
#[derive(Default)]
struct Lines {
    line: Vec<u8>,
    /// A `\r` was seen; what follows decides whether it ended the line.
    cr: bool,
    cut: bool,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in bytes {
            match b {
                b'\r' => self.cr = true,
                b'\n' => {
                    self.cr = false;
                    out.push(self.take());
                }
                b => {
                    if std::mem::take(&mut self.cr) {
                        self.line.clear();
                        self.cut = false;
                    }
                    if self.line.len() < MAX_LINE {
                        self.line.push(b);
                    } else {
                        self.cut = true;
                    }
                }
            }
        }
        out
    }

    fn finish(&mut self) -> Option<String> {
        (!self.line.is_empty()).then(|| self.take())
    }

    fn take(&mut self) -> String {
        let mut line = String::from_utf8_lossy(&self.line).into_owned();
        if std::mem::take(&mut self.cut) {
            line.push_str(" …[cut]");
        }
        self.line.clear();
        line
    }
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
    fn splitting_lines() {
        let mut l = Lines::default();
        assert_eq!(l.feed(b"a\nb\r\nc"), ["a", "b"]);
        // Progress bars redraw with \r; only what would be visible is kept.
        assert_eq!(l.feed(b"\r10%\r50%\r\n"), ["50%"]);
        assert_eq!(l.feed(b"x\r"), Vec::<String>::new());
        assert_eq!(l.feed(b"\r\n"), ["x"]);
        assert_eq!(l.feed(b"tail"), Vec::<String>::new());
        assert_eq!(l.finish().as_deref(), Some("tail"));
        assert_eq!(l.finish(), None);
        // Invalid UTF-8 is replaced, not dropped or fatal.
        assert_eq!(l.feed(b"\xff\xfeok\n"), ["\u{fffd}\u{fffd}ok"]);
    }

    #[test]
    fn long_lines_are_cut() {
        let mut l = Lines::default();
        let long = vec![b'x'; MAX_LINE * 3];
        assert!(l.feed(&long).is_empty());
        let out = l.feed(b"\nnext\n");
        assert_eq!(out[0].len(), MAX_LINE + " …[cut]".len());
        assert!(out[0].ends_with(" …[cut]"));
        assert_eq!(out[1], "next");
    }

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
