//! The UI-independent core: projects and their processes.

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::config::{Config, Project, validate_name};
use crate::process::{self, Event, Running};

const MAX_LOG_LINES: usize = 5000;
pub const MESSAGE_TTL: Duration = Duration::from_secs(6);

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
    /// The dev server accepts connections.
    pub ready: bool,
    pub logs: VecDeque<String>,
    /// Bumped whenever `logs` changes, so views can tell cheaply.
    pub log_rev: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Stopped,
    /// Process running, server not accepting connections yet.
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl Entry {
    pub fn status(&self) -> Status {
        match &self.run {
            Some(r) if r.stopping() => Status::Stopping,
            Some(_) if self.ready => Status::Running,
            Some(_) => Status::Starting,
            None if self.last_exit.is_some_and(|c| c != 0) => Status::Crashed,
            None => Status::Stopped,
        }
    }

    pub fn is_active(&self) -> bool {
        self.run.is_some()
    }

    /// The port the server listens on: its fixed one, or the one it was given
    /// while it runs.
    pub fn port(&self) -> Option<u16> {
        self.project.port.or(self.app_port.filter(|_| self.run.is_some()))
    }

    /// Where to open it, once its port is known.
    pub fn url(&self) -> Option<String> {
        self.port().map(|p| format!("http://localhost:{p}"))
    }

    fn log(&mut self, line: impl Into<String>) {
        if self.logs.len() >= MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line.into());
        self.log_rev += 1;
    }

    pub fn clear_logs(&mut self) {
        self.logs.clear();
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

pub struct Manager {
    pub config: Config,
    config_path: PathBuf,
    pub entries: Vec<Entry>,
    ready_checked: Instant,
    pub message: Option<(String, MsgKind, Instant)>,
    next_id: Id,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Manager {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut m = Self {
            config_path,
            entries: Vec::new(),
            ready_checked: Instant::now(),
            message: None,
            next_id: 0,
            tx,
            rx,
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
            logs: VecDeque::new(),
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

    /// Drain process output, reap exited children and notice servers that
    /// started listening. Returns whether anything visible changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
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
            run.enforce_deadline();
            match run.try_wait() {
                Ok(Some(status)) => {
                    changed = true;
                    let code = status.code();
                    let stopped_by_us = run.stopping();
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

        // Starting -> Running once the dev server accepts connections.
        if self.ready_checked.elapsed() >= Duration::from_millis(400) {
            self.ready_checked = Instant::now();
            for e in &mut self.entries {
                if e.run.is_some() && !e.ready {
                    if e.port().is_some_and(listening) {
                        e.ready = true;
                        changed = true;
                    }
                }
            }
        }

        if self.message.as_ref().is_some_and(|(_, _, t)| t.elapsed() > MESSAGE_TTL) {
            self.message = None;
            changed = true;
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
            .flat_map(|(_, e)| [e.project.port, e.app_port.filter(|_| e.run.is_some())])
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
        e.last_exit = None;
        match process::spawn(&e.project, &command, port, e.id, tx) {
            Ok(run) => e.run = Some(run),
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
        let deadline = Instant::now() + process::STOP_GRACE;
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
        let needs_restart = e.project != project && e.run.is_some();
        e.project = project;
        if self.save() {
            self.info(if needs_restart { format!("Saved. Restart {name} to apply the changes.") } else { "Saved".into() });
        }
    }

    pub fn remove(&mut self, id: Id) {
        let Some(i) = self.index(id) else { return };
        if let Some(mut run) = self.entries[i].run.take() {
            run.stop();
            // Reap in the background so the UI doesn't block.
            std::thread::spawn(move || {
                let deadline = Instant::now() + process::STOP_GRACE;
                while Instant::now() < deadline {
                    if !matches!(run.try_wait(), Ok(None)) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                run.kill_now();
            });
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
    let timeout = Duration::from_millis(40);
    ["127.0.0.1", "::1"].iter().any(|host| {
        host.parse()
            .map(|ip| SocketAddr::new(ip, port))
            .is_ok_and(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
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

    /// Starts a real server and waits until it answers on the port it was
    /// given in `PORT`.
    #[cfg(unix)]
    #[test]
    fn runs_a_server_on_its_port() {
        if std::process::Command::new("python3").arg("--version").output().is_err() {
            return;
        }
        let mut m = manager();
        let dir = std::env::temp_dir().join(format!("blueprint-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let command = "python3 -m http.server $PORT --bind 127.0.0.1".to_string();
        let id = m.push_entry(Project { name: "web".into(), path: dir.clone(), port: None, command });
        m.start(id);
        let deadline = Instant::now() + Duration::from_secs(10);
        while m.get(id).unwrap().status() != Status::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            m.tick();
        }
        let e = m.get(id).unwrap();
        assert_eq!(e.status(), Status::Running, "logs: {:?}", e.logs);
        let port = e.port().unwrap();
        assert!(process::AUTO_PORTS.contains(&port));
        assert_eq!(e.url(), Some(format!("http://localhost:{port}")));
        assert!(e.logs[0].starts_with(&format!("$ PORT={port} python3")));
        m.shutdown();
        assert_eq!(m.running_count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
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
