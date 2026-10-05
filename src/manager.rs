//! The UI-independent core: projects and their processes.

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::{Duration, Instant};

use crate::config::{Config, Project, validate_name};
use crate::process::{self, Event, Running};

/// A project's log keeps its newest lines, up to both of these limits.
pub const MAX_LOG_LINES: usize = 5000;
const MAX_LOG_BYTES: usize = 4 << 20;
/// Output lines waiting for the window. When the queue is full, servers
/// writing more wait, rather than memory growing without bound.
const LOG_QUEUE: usize = 2048;
pub const MESSAGE_TTL: Duration = Duration::from_secs(6);
/// How often a starting server is checked for accepting connections, and
/// how often a running one is checked again.
const PROBE_STARTING: Duration = Duration::from_millis(400);
const PROBE_RUNNING: Duration = Duration::from_secs(2);
/// Failed checks in a row before a running server counts as not responding.
const PROBE_MISSES: u8 = 2;
/// After this long without accepting connections, say so in the logs.
pub const SLOW_START: Duration = Duration::from_secs(60);

pub type Id = u64;

pub struct Entry {
    /// Stable id used to route process output; survives reordering and edits.
    pub id: Id,
    pub project: Project,
    pub run: Option<Running>,
    /// Start again as soon as the current process exits.
    pub restart: bool,
    pub last_exit: Option<i32>,
    /// The port the server was last given in `PORT`. Kept after it stops so
    /// an auto-assigned port is reused on the next start.
    pub app_port: Option<u16>,
    /// The server accepted a connection at the last check.
    pub ready: bool,
    /// It has accepted connections since it started, so not being ready
    /// means it stopped responding rather than that it is still starting.
    was_ready: bool,
    misses: u8,
    probing: bool,
    probed: Option<Instant>,
    slow_warned: bool,
    pub logs: VecDeque<String>,
    /// The line number of `logs[0]`, counting every line ever logged, so a
    /// line keeps its number as older ones are dropped or cleared.
    pub log_start: u64,
    log_bytes: usize,
    /// Bumped whenever `logs` changes, so views can tell cheaply.
    pub log_rev: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Stopped,
    /// Process running, server not accepting connections yet.
    Starting,
    /// Accepting connections on its port.
    Running,
    /// Was accepting connections, but no longer is.
    Unresponsive,
    Stopping,
    Crashed,
}

impl Entry {
    pub fn status(&self) -> Status {
        match &self.run {
            Some(r) if r.stopping() => Status::Stopping,
            Some(_) if self.ready => Status::Running,
            Some(_) if self.was_ready => Status::Unresponsive,
            Some(_) => Status::Starting,
            None if self.last_exit.is_some_and(|c| c != 0) => Status::Crashed,
            None => Status::Stopped,
        }
    }

    pub fn is_active(&self) -> bool {
        self.run.is_some()
    }

    /// The port the server listens on: the one it was given while it runs
    /// (even if the project has since been edited), else its fixed one.
    pub fn port(&self) -> Option<u16> {
        match &self.run {
            Some(r) => Some(r.port),
            None => self.project.port,
        }
    }

    /// Where to open it, once its port is known.
    pub fn url(&self) -> Option<String> {
        self.port().map(|p| format!("http://localhost:{p}"))
    }

    /// The project was edited since it started, so a restart would change it.
    pub fn edited_while_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.project != self.project)
    }

    pub(crate) fn log(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.log_bytes += line.len();
        self.logs.push_back(line);
        while self.logs.len() > MAX_LOG_LINES || self.log_bytes > MAX_LOG_BYTES {
            let Some(old) = self.logs.pop_front() else { break };
            self.log_bytes -= old.len();
            self.log_start += 1;
        }
        self.log_rev += 1;
    }

    pub fn clear_logs(&mut self) {
        self.log_start += self.logs.len() as u64;
        self.logs.clear();
        self.log_bytes = 0;
        self.log_rev += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Info,
    Error,
}

/// Which form field a validation error belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Folder,
    Port,
    Command,
}

/// The answer to whether a run's port accepts connections.
struct Probe {
    id: Id,
    /// Identifies the run that was checked.
    started: Instant,
    ok: bool,
}

pub struct Manager {
    pub config: Config,
    config_path: PathBuf,
    pub entries: Vec<Entry>,
    pub message: Option<(String, MsgKind, Instant)>,
    next_id: Id,
    tx: SyncSender<Event>,
    rx: Receiver<Event>,
    probe_tx: Sender<Probe>,
    probe_rx: Receiver<Probe>,
}

impl Manager {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel(LOG_QUEUE);
        let (probe_tx, probe_rx) = mpsc::channel();
        let mut m = Self {
            config_path,
            entries: Vec::new(),
            message: None,
            next_id: 0,
            tx,
            rx,
            probe_tx,
            probe_rx,
            config,
        };
        for p in m.config.projects.clone() {
            m.push_entry(p);
        }
        m
    }

    fn push_entry(&mut self, project: Project) -> Id {
        self.next_id += 1;
        self.entries.push(Entry {
            id: self.next_id,
            project,
            run: None,
            restart: false,
            last_exit: None,
            app_port: None,
            ready: false,
            was_ready: false,
            misses: 0,
            probing: false,
            probed: None,
            slow_warned: false,
            logs: VecDeque::new(),
            log_start: 0,
            log_bytes: 0,
            log_rev: 0,
        });
        self.next_id
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), MsgKind::Info, Instant::now()));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), MsgKind::Error, Instant::now()));
    }

    pub fn index(&self, id: Id) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    pub fn get(&self, id: Id) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }

    pub fn running_count(&self) -> usize {
        self.entries.iter().filter(|e| e.run.is_some()).count()
    }

    /// Write the projects to the config file. On failure the error replaces
    /// any message and `false` comes back, so callers only report success
    /// that really happened.
    fn save(&mut self) -> bool {
        self.config.projects = self.entries.iter().map(|e| e.project.clone()).collect();
        match self.config.save(&self.config_path) {
            Ok(()) => true,
            Err(e) => {
                self.error(format!("Couldn't save the config: {e:#}"));
                false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Background work

    /// Take in process output, reap exited servers and keep track of which
    /// ones accept connections. Never blocks: output is taken a bounded batch
    /// at a time and ports are checked on other threads. Returns whether
    /// anything visible changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for ev in self.rx.try_iter().take(LOG_QUEUE) {
            changed = true;
            let Event::Log { id, line } = ev;
            if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                e.log(line);
            }
        }

        let mut crashed = Vec::new();
        let mut restart = Vec::new();
        for e in &mut self.entries {
            let Some(run) = &mut e.run else { continue };
            let was_lingering = run.lingering();
            match run.poll() {
                Ok(Some(status)) => {
                    changed = true;
                    let code = status.code();
                    let stopped_by_us = run.stop_requested();
                    e.run = None;
                    e.ready = false;
                    e.last_exit = Some(if stopped_by_us { 0 } else { code.unwrap_or(-1) });
                    e.log(match (stopped_by_us, code) {
                        (true, _) => "── stopped ──".to_string(),
                        (false, Some(c)) => format!("── exited with code {c} ──"),
                        (false, None) => "── killed by a signal ──".to_string(),
                    });
                    if std::mem::take(&mut e.restart) {
                        restart.push(e.id);
                    } else if !stopped_by_us && code != Some(0) {
                        crashed.push(e.project.name.clone());
                    }
                }
                Ok(None) if !was_lingering && run.lingering() && !run.stop_requested() => {
                    changed = true;
                    e.ready = false;
                    e.log("── the command exited; stopping what it left running ──");
                }
                Ok(None) => {}
                Err(err) => e.log(format!("── wait failed: {err} ──")),
            }
        }
        if let Some(name) = crashed.first() {
            self.error(format!("{name} stopped unexpectedly. See its logs."));
        }
        for id in restart {
            self.start(id);
        }

        changed |= self.check_ports();

        if self.message.as_ref().is_some_and(|(_, _, t)| t.elapsed() > MESSAGE_TTL) {
            self.message = None;
            changed = true;
        }
        changed
    }

    /// Starting -> Running once a server accepts connections, Running ->
    /// Unresponsive if it stops. Checks run on their own threads, since a
    /// connection attempt can take a while.
    fn check_ports(&mut self) -> bool {
        let mut changed = false;
        for p in self.probe_rx.try_iter() {
            let Some(e) = self.entries.iter_mut().find(|e| e.id == p.id) else { continue };
            e.probing = false;
            let Some(port) = e.run.as_ref().filter(|r| r.started == p.started && !r.stopping()).map(|r| r.port)
            else {
                continue;
            };
            if p.ok {
                e.misses = 0;
                if !e.ready {
                    changed = true;
                    if e.was_ready {
                        e.log(format!("── accepting connections on port {port} again ──"));
                    }
                    e.ready = true;
                    e.was_ready = true;
                }
            } else if e.ready {
                e.misses += 1;
                if e.misses >= PROBE_MISSES {
                    changed = true;
                    e.ready = false;
                    e.log(format!("── port {port} stopped accepting connections ──"));
                }
            }
        }

        let mut slow = Vec::new();
        for e in &mut self.entries {
            let Some((started, port)) = e.run.as_ref().filter(|r| !r.stopping()).map(|r| (r.started, r.port))
            else {
                continue;
            };
            if !e.was_ready && !e.slow_warned && started.elapsed() >= SLOW_START {
                e.slow_warned = true;
                changed = true;
                let msg = format!(
                    "nothing has accepted connections on port {port} for {}s. If the server is up, it isn't listening on $PORT.",
                    SLOW_START.as_secs()
                );
                e.log(format!("── {msg} ──"));
                slow.push(format!("{}: {msg}", e.project.name));
            }
            let every = if e.ready { PROBE_RUNNING } else { PROBE_STARTING };
            if e.probing || e.probed.is_some_and(|t| t.elapsed() < every) {
                continue;
            }
            e.probing = true;
            e.probed = Some(Instant::now());
            let (tx, id) = (self.probe_tx.clone(), e.id);
            std::thread::spawn(move || {
                let _ = tx.send(Probe { id, started, ok: listening(port) });
            });
        }
        if let Some(msg) = slow.into_iter().next() {
            self.error(msg);
        }
        changed
    }

    // -----------------------------------------------------------------------
    // Process control

    pub fn start(&mut self, id: Id) {
        let Some(i) = self.index(id) else { return };
        let e = &self.entries[i];
        if e.is_active() {
            return;
        }
        if let Some(msg) = missing_command(&e.project.path, &e.project.command) {
            let msg = format!("{} can't start: {msg}", e.project.name);
            self.entries[i].log(format!("── {msg} ──"));
            self.error(msg);
            return;
        }
        if let Some(port) = e.project.port
            && process::port_in_use(port)
        {
            let msg = format!("Port {port} is already in use, so {} can't start", e.project.name);
            self.entries[i].log(format!("── {msg} ──"));
            self.error(msg);
            return;
        }
        self.spawn(i);
    }

    /// Ports other projects hold: every fixed port, and the ports running
    /// projects were given.
    fn taken_ports(&self, except: usize) -> Vec<u16> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != except)
            .flat_map(|(_, e)| [e.project.port, e.run.as_ref().map(|r| r.port)])
            .flatten()
            .collect()
    }

    fn spawn(&mut self, i: usize) {
        let e = &self.entries[i];
        let command = match e.project.command.trim() {
            "" => crate::stack::dev_script_command(&e.project.path).unwrap_or_default(),
            c => c.to_string(),
        };
        let port = match e.project.port {
            Some(p) => Some(p),
            None => process::free_port(e.app_port, &self.taken_ports(i)),
        };
        let Some(port) = port else {
            let (a, b) = (process::AUTO_PORTS.start(), process::AUTO_PORTS.end());
            let msg = format!("No free port between {a} and {b} for {}", e.project.name);
            self.entries[i].log(format!("── {msg} ──"));
            self.error(msg);
            return;
        };
        let tx = self.tx.clone();
        let e = &mut self.entries[i];
        if !e.logs.is_empty() {
            e.log("");
        }
        e.app_port = Some(port);
        e.ready = false;
        e.was_ready = false;
        e.misses = 0;
        e.probed = None;
        e.slow_warned = false;
        e.last_exit = None;
        match process::spawn(&e.project, &command, port, e.id, tx) {
            Ok(run) => {
                // Logged first so the output shows exactly what ran.
                e.log(format!("$ PORT={port} {command}"));
                e.run = Some(run);
            }
            Err(msg) => {
                e.log(format!("── {msg} ──"));
                e.last_exit = Some(-1);
                self.error(msg);
            }
        }
    }

    pub fn stop(&mut self, id: Id) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        e.restart = false;
        if let Some(run) = &mut e.run {
            run.stop();
        }
    }

    pub fn toggle(&mut self, id: Id) {
        match self.get(id) {
            Some(e) if e.is_active() => self.stop(id),
            Some(_) => self.start(id),
            None => {}
        }
    }

    /// Stop, then start again from `tick` once the old process has exited.
    pub fn restart(&mut self, id: Id) {
        let Some(i) = self.index(id) else { return };
        if self.entries[i].run.is_some() {
            self.stop(id);
            self.entries[i].restart = true;
        } else {
            self.start(id);
        }
    }

    pub fn start_all(&mut self) {
        let ids: Vec<Id> = self.entries.iter().map(|e| e.id).collect();
        for id in ids {
            self.start(id);
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<Id> = self.entries.iter().map(|e| e.id).collect();
        for id in ids {
            self.stop(id);
        }
    }

    /// SIGTERM everything, wait for the grace period, then SIGKILL stragglers.
    pub fn shutdown(&mut self) {
        for e in &mut self.entries {
            if let Some(run) = &mut e.run {
                run.stop();
            }
        }
        // Long enough for `tick` to SIGKILL at the deadline and see them go.
        let deadline = Instant::now() + process::STOP_GRACE + Duration::from_secs(1);
        while self.running_count() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            self.tick();
        }
        for e in &mut self.entries {
            if let Some(mut run) = e.run.take() {
                run.kill_now();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Projects

    /// Validate form input. `editing` is the project being edited, if any.
    pub fn validate(
        &self,
        editing: Option<Id>,
        name: &str,
        folder: Option<&PathBuf>,
        port: &str,
        command: &str,
    ) -> Result<Project, (Field, String)> {
        let others = || self.entries.iter().filter(move |e| Some(e.id) != editing).map(|e| &e.project);
        let name = name.trim().to_string();
        validate_name(&name).map_err(|e| (Field::Name, e))?;
        if others().any(|p| p.name == name) {
            return Err((Field::Name, format!("\u{201c}{name}\u{201d} is already taken")));
        }
        let Some(folder) = folder.filter(|f| f.is_dir()) else {
            return Err((Field::Folder, "Pick a project folder".into()));
        };
        let port = match port.trim() {
            "" => None,
            s => {
                let port: u16 = s
                    .parse()
                    .ok()
                    .filter(|p| *p > 0)
                    .ok_or_else(|| (Field::Port, "Use a port from 1 to 65535".to_string()))?;
                if let Some(p) = others().find(|p| p.port == Some(port)) {
                    return Err((Field::Port, format!("{} already uses port {port}", p.name)));
                }
                Some(port)
            }
        };
        let command = command.trim().to_string();
        if let Some(msg) = missing_command(folder, &command) {
            return Err((Field::Command, msg));
        }
        Ok(Project { name, path: folder.clone(), port, command })
    }

    pub fn add(&mut self, project: Project) -> Id {
        let name = project.name.clone();
        let id = self.push_entry(project);
        if self.save() {
            self.info(format!("Added {name}. Press start when you're ready."));
        }
        id
    }

    pub fn update(&mut self, id: Id, project: Project) {
        let Some(i) = self.index(id) else { return };
        let e = &mut self.entries[i];
        let name = project.name.clone();
        e.project = project;
        let needs_restart = e.edited_while_running();
        if self.save() {
            self.info(if needs_restart { format!("Saved. Restart {name} to apply the changes.") } else { "Saved".into() });
        }
    }

    pub fn remove(&mut self, id: Id) {
        let Some(i) = self.index(id) else { return };
        if let Some(run) = self.entries[i].run.take() {
            run.stop_in_background();
        }
        let name = self.entries.remove(i).project.name;
        if self.save() {
            self.info(format!("Removed {name}"));
        }
    }

    pub fn move_by(&mut self, id: Id, delta: isize) {
        let Some(i) = self.index(id) else { return };
        let j = i as isize + delta;
        if j < 0 || j as usize >= self.entries.len() {
            return;
        }
        self.entries.swap(i, j as usize);
        self.save();
    }
}

/// Whether something accepts TCP connections on localhost:`port`.
fn listening(port: u16) -> bool {
    let timeout = Duration::from_millis(250);
    ["127.0.0.1", "::1"].iter().any(|host| {
        host.parse()
            .map(|ip| SocketAddr::new(ip, port))
            .is_ok_and(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
    })
}

/// Why an empty command can't work in `dir`: it means the package.json dev
/// script. `None` when it's fine.
pub fn missing_command(dir: &std::path::Path, command: &str) -> Option<String> {
    if !command.trim().is_empty() || crate::stack::has_dev_script(dir) {
        return None;
    }
    Some(match crate::stack::detect(dir) {
        Some(r) if !r.command.is_empty() => format!("there's no dev script here, so set a command, e.g. {}", r.command),
        _ => "there's no package.json dev script here, so set the command that starts the server".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> Manager {
        let dir = std::env::temp_dir().join(format!("blueprint-mgr-{}", std::process::id()));
        Manager::new(Config::default(), dir.join("config.toml"))
    }

    #[test]
    fn validation() {
        let mut m = manager();
        let tmp = std::env::temp_dir();
        m.push_entry(Project { name: "taken".into(), path: tmp.clone(), port: Some(3000), command: String::new() });
        let v = |m: &Manager, name: &str, port: &str| m.validate(None, name, Some(&tmp), port, "serve").map_err(|e| e.0);
        assert_eq!(v(&m, "taken", "").unwrap_err(), Field::Name);
        assert_eq!(v(&m, "Bad Name", "").unwrap_err(), Field::Name);
        assert_eq!(v(&m, "fresh", "3000").unwrap_err(), Field::Port);
        assert_eq!(v(&m, "fresh", "99999").unwrap_err(), Field::Port);
        assert_eq!(v(&m, "fresh", "3001").unwrap().port, Some(3001));
        assert_eq!(m.validate(None, "fresh", None, "", "").unwrap_err().0, Field::Folder);
        // No dev script in the folder, so an empty command can't run.
        assert_eq!(m.validate(None, "fresh", Some(&tmp), "", "").unwrap_err().0, Field::Command);
        // Editing a project may keep its own name and port.
        let id = m.entries[0].id;
        assert!(m.validate(Some(id), "taken", Some(&tmp), "3000", "pnpm dev").is_ok());
    }

    use crate::testkit::{add_helper, logged, wait_until};

    const SOON: Duration = Duration::from_secs(15);

    /// Starts a server and waits until it answers on the port it was given
    /// in `PORT`.
    #[test]
    fn runs_a_server_on_its_port() {
        let mut m = crate::testkit::manager("run");
        let id = add_helper(&mut m, "web", "serve", 4900);
        m.start(id);
        wait_until(&mut m, id, SOON, "running", |m| m.get(id).unwrap().status() == Status::Running);
        let e = m.get(id).unwrap();
        let port = e.port().unwrap();
        assert_eq!(port, 4900);
        assert_eq!(e.url(), Some(format!("http://localhost:{port}")));
        assert!(e.logs[0].starts_with(&format!("$ PORT={port} BLUEPRINT_HELPER=serve")), "{:?}", e.logs);
        m.shutdown();
        assert_eq!(m.running_count(), 0);
    }

    /// Edits to a running project wait for a restart: until then its URL is
    /// the port it really listens on.
    #[test]
    fn edits_apply_on_restart() {
        let mut m = crate::testkit::manager("edit");
        let id = add_helper(&mut m, "web", "serve", 4910);
        m.start(id);
        wait_until(&mut m, id, SOON, "running", |m| m.get(id).unwrap().status() == Status::Running);
        let mut edited = m.get(id).unwrap().project.clone();
        edited.port = Some(4911);
        m.update(id, edited);
        let e = m.get(id).unwrap();
        assert!(e.edited_while_running());
        assert_eq!(e.url().as_deref(), Some("http://localhost:4910"));
        assert!(m.message.as_ref().unwrap().0.contains("Restart web"));

        m.restart(id);
        wait_until(&mut m, id, SOON, "the restart", |m| {
            m.get(id).unwrap().port() == Some(4911) && m.get(id).unwrap().status() == Status::Running
        });
        assert!(!m.get(id).unwrap().edited_while_running());
        m.shutdown();
    }

    /// A shell that exits leaving a server behind that ignores SIGTERM: the
    /// server is killed at the deadline, and the project stays "stopping"
    /// until it's gone.
    #[test]
    fn stops_what_the_command_leaves_behind() {
        let mut m = crate::testkit::manager("orphan");
        let id = add_helper(&mut m, "orphan", "orphan", 4920);
        m.start(id);
        wait_until(&mut m, id, SOON, "the shell to exit", |m| logged(m, id, "stopping what it left running"));
        let child: u32 = {
            let e = m.get(id).unwrap();
            let line = e.logs.iter().find_map(|l| l.strip_prefix("child ")).expect("child pid logged");
            line.trim().parse().unwrap()
        };
        assert!(m.get(id).unwrap().is_active());
        assert_eq!(m.get(id).unwrap().status(), Status::Stopping);
        let killed_by = Instant::now() + process::STOP_GRACE + Duration::from_secs(4);
        wait_until(&mut m, id, process::STOP_GRACE + Duration::from_secs(4), "the leftover to be killed", |m| {
            !m.get(id).unwrap().is_active()
        });
        assert!(Instant::now() < killed_by);
        #[cfg(unix)]
        {
            // Gone, not just untracked (allowing init a moment to reap it).
            let deadline = Instant::now() + Duration::from_secs(2);
            while unsafe { libc::kill(child as libc::pid_t, 0) } == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_ne!(unsafe { libc::kill(child as libc::pid_t, 0) }, 0, "child {child} survived");
        }
        let _ = child;
    }

    /// Far more output than the log keeps: memory stays bounded, a huge line
    /// is cut, and the newest lines survive.
    #[test]
    fn output_floods_stay_bounded() {
        let mut m = crate::testkit::manager("flood");
        let id = add_helper(&mut m, "flood", "flood", 4930);
        m.start(id);
        let mut biggest_batch = 0;
        let deadline = Instant::now() + Duration::from_secs(60);
        while m.get(id).unwrap().is_active() || !logged(&m, id, "flood done") {
            assert!(Instant::now() < deadline, "flood never finished");
            let before = m.get(id).unwrap().log_rev;
            m.tick();
            biggest_batch = biggest_batch.max(m.get(id).unwrap().log_rev - before);
            std::thread::sleep(Duration::from_millis(5));
        }
        // One tick never takes in more than a queue's worth (plus a status line or two).
        assert!(biggest_batch <= LOG_QUEUE as u64 + 4, "{biggest_batch}");
        let e = m.get(id).unwrap();
        assert!(e.logs.len() <= MAX_LOG_LINES);
        assert!(e.log_bytes <= MAX_LOG_BYTES);
        assert_eq!(e.log_bytes, e.logs.iter().map(String::len).sum::<usize>());
        let huge = e.logs.iter().find(|l| l.starts_with("xxxx")).expect("the huge line");
        assert!(huge.len() <= process::MAX_LINE + 16 && huge.ends_with("[cut]"));
        assert!(e.logs.iter().any(|l| l.starts_with("line 19999 ")));
        // Dropped lines are counted, so line numbers stay stable.
        assert!(e.log_start > 0);
        assert!(e.logs.iter().position(|l| l.starts_with("line 19999 ")).unwrap() as u64 + e.log_start > 20_000);
    }

    /// Invalid UTF-8 and broken escapes come through as lines and render
    /// without panicking.
    #[test]
    fn malformed_output() {
        let mut m = crate::testkit::manager("malformed");
        let id = add_helper(&mut m, "bytes", "malformed", 4940);
        m.start(id);
        wait_until(&mut m, id, SOON, "exit", |m| !m.get(id).unwrap().is_active() && logged(m, id, "malformed done"));
        let e = m.get(id).unwrap();
        assert!(e.logs.iter().any(|l| l.contains("\u{fffd}\u{fffd} not utf-8")));
        assert!(e.logs.iter().any(|l| l == "progress 100%"));
        for line in &e.logs {
            crate::ansi::parse(line);
        }
        assert_eq!(e.status(), Status::Stopped);
    }

    /// A server that stops accepting connections is noticed.
    #[test]
    fn notices_a_server_that_stops_responding() {
        let mut m = crate::testkit::manager("unresponsive");
        let id = add_helper(&mut m, "flaky", "serve-briefly", 4950);
        m.start(id);
        wait_until(&mut m, id, SOON, "running", |m| m.get(id).unwrap().status() == Status::Running);
        wait_until(&mut m, id, SOON, "not responding", |m| m.get(id).unwrap().status() == Status::Unresponsive);
        assert!(logged(&m, id, "port 4950 stopped accepting connections"));
        m.shutdown();
    }

    /// A failed write must not be reported as saved.
    #[test]
    fn failed_saves_are_reported() {
        // The config's parent is a file, so the directory can't be created.
        let blocker = std::env::temp_dir().join(format!("blueprint-blocker-{}", std::process::id()));
        std::fs::write(&blocker, "").unwrap();
        let mut m = Manager::new(Config::default(), blocker.join("config.toml"));
        let tmp = std::env::temp_dir();
        let project = |name: &str| Project { name: name.into(), path: tmp.clone(), port: None, command: "x".into() };
        let is_error = |m: &Manager| matches!(&m.message, Some((msg, MsgKind::Error, _)) if msg.starts_with("Couldn't save"));

        let id = m.add(project("a"));
        assert!(is_error(&m), "{:?}", m.message.as_ref().map(|x| &x.0));
        m.message = None;
        m.update(id, project("b"));
        assert!(is_error(&m));
        m.message = None;
        m.remove(id);
        assert!(is_error(&m));
        std::fs::remove_file(blocker).unwrap();

        // And a successful one is.
        let mut m = manager();
        m.add(project("a"));
        assert!(matches!(&m.message, Some((msg, MsgKind::Info, _)) if msg.starts_with("Added a")));
    }

    #[test]
    fn urls_follow_the_port() {
        let mut m = manager();
        let tmp = std::env::temp_dir();
        m.push_entry(Project { name: "a".into(), path: tmp.clone(), port: Some(3000), command: String::new() });
        m.push_entry(Project { name: "b".into(), path: tmp, port: None, command: String::new() });
        assert_eq!(m.entries[0].url().as_deref(), Some("http://localhost:3000"));
        // An auto port only counts while the server runs.
        m.entries[1].app_port = Some(4001);
        assert_eq!(m.entries[1].url(), None);
        assert_eq!(m.taken_ports(1), [3000]);
    }
}
