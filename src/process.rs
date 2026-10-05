//! Spawning dev servers through portless and streaming their output.

use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Project;
use crate::guardian;

/// How long a server gets to shut down after SIGTERM before it is SIGKILLed.
pub const STOP_GRACE: Duration = Duration::from_secs(4);

pub enum Event {
    /// One line of output from a project's process.
    Log { id: u64, line: String },
    /// Result of a background `portless proxy ...` command.
    ProxyCmd { ok: bool, message: String },
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

    /// portless has exited. Anything left in its process group (say, a dev
    /// server whose parent died) gets SIGTERM; the guardian keeps tracking the
    /// group until it is empty.
    fn reaped(&mut self) {
        let pgid = self.pid();
        if guardian::group_alive(pgid) {
            signal_group(pgid, libc::SIGTERM);
        } else {
            guardian::unwatch(pgid);
        }
    }

    pub fn stopping(&self) -> bool {
        self.stop_sent.is_some()
    }

    /// Ask the whole process group to exit. portless forwards the signal to the
    /// dev server and removes its route; signalling the group also catches
    /// grandchildren (e.g. `npm run dev` -> `sh` -> `node`).
    pub fn stop(&mut self) {
        if self.stop_sent.is_none() {
            signal_group(self.pid(), libc::SIGTERM);
            self.stop_sent = Some(Instant::now());
        }
    }

    /// Escalate to SIGKILL once the grace period has elapsed.
    pub fn enforce_deadline(&mut self) {
        if let Some(t) = self.stop_sent
            && !self.killed
            && t.elapsed() >= STOP_GRACE
        {
            signal_group(self.pid(), libc::SIGKILL);
            self.killed = true;
        }
    }

    pub fn kill_now(&mut self) {
        signal_group(self.pid(), libc::SIGKILL);
        self.killed = true;
        let _ = self.child.wait();
        self.reaped();
    }
}

fn signal_group(pid: u32, sig: libc::c_int) {
    // Children are spawned with process_group(0), so their pgid equals their pid.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

/// Shell syntax that `shell_words` can't express as a plain argv.
fn needs_shell(cmd: &str) -> bool {
    // `$` covers `$PORT`, which portless sets for the server to read.
    ["&&", "||", "|", ";", ">", "<", "$", "`", "*", "~"]
        .iter()
        .any(|t| cmd.contains(t))
}

/// Arguments passed to `portless` for a project.
pub fn portless_args(project: &Project) -> Result<Vec<String>, String> {
    let cmd = project.command.trim();
    let mut args = Vec::new();
    if cmd.is_empty() {
        // `portless run` runs the package.json "dev" script.
        args.push("run".to_string());
    }
    args.push("--name".into());
    args.push(project.name.clone());
    if let Some(port) = project.port {
        args.push("--app-port".into());
        args.push(port.to_string());
    }
    if !cmd.is_empty() {
        args.push("--".into());
        if needs_shell(cmd) {
            args.extend(["sh".into(), "-c".into(), cmd.to_string()]);
        } else {
            let words = shell_words::split(cmd).map_err(|e| format!("bad command: {e}"))?;
            args.extend(words);
        }
    }
    Ok(args)
}

pub fn spawn(
    project: &Project,
    proxy_port: u16,
    id: u64,
    tx: Sender<Event>,
) -> Result<Running, String> {
    let args = portless_args(project)?;
    if !project.path.is_dir() {
        return Err(format!("folder not found: {}", project.path.display()));
    }
    let mut child = Command::new("portless")
        .args(&args)
        .current_dir(&project.path)
        .env("PORTLESS_PORT", proxy_port.to_string())
        .env("FORCE_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => {
                "`portless` not found — install with: npm i -g portless".to_string()
            }
            _ => format!("failed to start: {e}"),
        })?;

    guardian::watch(child.id());
    let _ = tx.send(Event::Log {
        id,
        line: format!("$ portless {}", shell_words::join(&args)),
    });
    if let Some(out) = child.stdout.take() {
        pipe_lines(out, id, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pipe_lines(err, id, tx);
    }
    Ok(Running {
        child,
        started: Instant::now(),
        stop_sent: None,
        killed: false,
    })
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
                    if tx
                        .send(Event::Log {
                            id,
                            line: visible.to_string(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}

/// Pick the URL portless prints, e.g. `  -> https://app.localhost:1355`.
pub fn parse_url(line: &str) -> Option<String> {
    let plain = crate::ansi::strip(line);
    let rest = plain.trim().strip_prefix("->")?.trim();
    (rest.starts_with("http://") || rest.starts_with("https://"))
        .then(|| rest.split_whitespace().next().unwrap_or(rest).to_string())
}

/// The port portless gave the dev server: `Running: PORT=4728 ...` or `-- Using port 4728`.
pub fn parse_app_port(line: &str) -> Option<u16> {
    let plain = crate::ansi::strip(line);
    let rest = if let Some(i) = plain.find("PORT=") {
        &plain[i + 5..]
    } else if let Some(i) = plain.find("Using port ") {
        &plain[i + 11..]
    } else {
        return None;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

// ---------------------------------------------------------------------------
// Proxy

fn state_dir() -> PathBuf {
    std::env::var_os("PORTLESS_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".portless"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub tls: bool,
}

pub fn proxy_status() -> ProxyStatus {
    let dir = state_dir();
    let read = |f: &str| {
        std::fs::read_to_string(dir.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let running = read("proxy.pid")
        .and_then(|p| p.parse::<u32>().ok())
        .is_some_and(|pid| PathBuf::from(format!("/proc/{pid}")).exists());
    let port = read("proxy.port").and_then(|p| p.parse().ok());
    let tls = read("proxy.tls").is_none_or(|t| t != "0" && t != "false");
    ProxyStatus { running, port, tls }
}

pub fn portless_installed() -> bool {
    Command::new("portless")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run `portless proxy start|stop` off the UI thread.
pub fn proxy_command(start: bool, port: u16, tx: Sender<Event>) {
    thread::spawn(move || {
        let mut cmd = Command::new("portless");
        cmd.arg("proxy");
        if start {
            cmd.args(["start", "-p", &port.to_string()]);
        } else {
            cmd.arg("stop");
        }
        let res = cmd.stdin(Stdio::null()).output();
        let event = match res {
            Ok(out) => {
                let text = String::from_utf8_lossy(if out.status.success() {
                    &out.stdout
                } else {
                    &out.stderr
                });
                let last = text
                    .lines()
                    .map(str::trim)
                    .rfind(|l| !l.is_empty())
                    .unwrap_or("");
                let verb = if start { "start" } else { "stop" };
                Event::ProxyCmd {
                    ok: out.status.success(),
                    message: if out.status.success() {
                        format!("proxy {verb}: {last}")
                    } else {
                        format!("proxy {verb} failed: {last}")
                    },
                }
            }
            Err(e) => Event::ProxyCmd {
                ok: false,
                message: format!("could not run portless: {e}"),
            },
        };
        let _ = tx.send(event);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj(cmd: &str, port: Option<u16>) -> Project {
        Project {
            name: "app".into(),
            path: "/tmp".into(),
            port,
            command: cmd.into(),
        }
    }

    #[test]
    fn args_default_script() {
        assert_eq!(
            portless_args(&proj("", None)).unwrap(),
            ["run", "--name", "app"]
        );
    }

    #[test]
    fn args_command_and_port() {
        assert_eq!(
            portless_args(&proj("pnpm dev --open", Some(3000))).unwrap(),
            [
                "--name",
                "app",
                "--app-port",
                "3000",
                "--",
                "pnpm",
                "dev",
                "--open"
            ]
        );
    }

    #[test]
    fn args_shell() {
        assert_eq!(
            portless_args(&proj("cd web && bun dev", None)).unwrap(),
            ["--name", "app", "--", "sh", "-c", "cd web && bun dev"]
        );
    }

    #[test]
    fn args_expand_port() {
        assert_eq!(
            portless_args(&proj("trunk serve --port $PORT", None)).unwrap(),
            ["--name", "app", "--", "sh", "-c", "trunk serve --port $PORT"]
        );
    }

    #[test]
    fn app_port_parsing() {
        assert_eq!(parse_app_port("Running: PORT=4728 HOST=127.0.0.1 node x"), Some(4728));
        assert_eq!(parse_app_port("-- Using port 3100 (fixed)"), Some(3100));
        assert_eq!(parse_app_port("PORTLESS_URL=https://a.localhost"), None);
        assert_eq!(parse_app_port("hello"), None);
    }

    #[test]
    fn url_parsing() {
        assert_eq!(
            parse_url("  -> https://a.localhost:1355").as_deref(),
            Some("https://a.localhost:1355")
        );
        assert_eq!(
            parse_url("\x1b[36m  -> https://a.localhost\x1b[0m").as_deref(),
            Some("https://a.localhost")
        );
        assert_eq!(parse_url("-> Using port 4000"), None);
        assert_eq!(parse_url("hello"), None);
    }
}
