//! The UI-independent core: projects, their processes, and the portless proxy.

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::config::{self, Config, Project, validate_name};
use crate::process::{self, Event, ProxyStatus, Running};

const MAX_LOG_LINES: usize = 5000;
pub const MESSAGE_TTL: Duration = Duration::from_secs(6);
/// Give up waiting for a setup terminal after this long.
const SETUP_TIMEOUT: Duration = Duration::from_secs(600);

pub type Id = u64;

pub struct Entry {
    /// Stable id used to route process output; survives reordering and edits.
    pub id: Id,
    pub project: Project,
    pub run: Option<Running>,
    /// Waiting for the proxy to come up before spawning.
    pub queued: bool,
    /// Start again as soon as the current process exits.
    pub restart: bool,
    pub last_exit: Option<i32>,
    pub url: Option<String>,
    /// The port the dev server was told to use (fixed or assigned by portless).
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
    /// Queued until the proxy is up.
    Waiting,
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
            None if self.queued => Status::Waiting,
            None if self.last_exit.is_some_and(|c| c != 0) => Status::Crashed,
            None => Status::Stopped,
        }
    }

    pub fn is_active(&self) -> bool {
        self.run.is_some() || self.queued
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

/// One-time root setup for a privileged proxy port (443).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    None,
    /// Projects are waiting; ask the user how to bring the proxy up.
    Needed,
    /// A terminal is open running the setup; poll until the proxy appears.
    Waiting(Instant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKind {
    /// `portless service install`: starts on boot, trusts the certificate.
    Service,
    /// `portless proxy start`: until reboot.
    Once,
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
    pub proxy: ProxyStatus,
    proxy_checked: Instant,
    ready_checked: Instant,
    proxy_busy: bool,
    pub portless_ok: bool,
    pub message: Option<(String, MsgKind, Instant)>,
    pub setup: Setup,
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
            proxy: process::proxy_status(),
            proxy_checked: Instant::now(),
            ready_checked: Instant::now(),
            proxy_busy: false,
            portless_ok: process::portless_installed(),
            message: None,
            setup: Setup::None,
            next_id: 0,
            tx,
            rx,
            config,
        };
        for p in m.config.projects.clone() {
            m.push_entry(p);
        }
        if !m.portless_ok {
            m.error("portless isn't installed. Run: npm i -g portless");
        }
        m
    }

    fn push_entry(&mut self, project: Project) -> Id {
        self.next_id += 1;
        self.entries.push(Entry {
            id: self.next_id,
            project,
            run: None,
            queued: false,
            restart: false,
            last_exit: None,
            url: None,
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

    pub fn running_count(&self) -> usize {
        self.entries.iter().filter(|e| e.run.is_some()).count()
    }

    /// The port children should talk to: the live proxy's, or the configured one.
    pub fn effective_proxy_port(&self) -> u16 {
        if self.proxy.running {
            self.proxy.port.unwrap_or(self.config.proxy_port)
        } else {
            self.config.proxy_port
        }
    }

    pub fn url_for(&self, name: &str) -> String {
        let scheme = if self.proxy.tls { "https" } else { "http" };
        let port = if self.proxy_ready() { self.effective_proxy_port() } else { self.config.proxy_port };
        let default = if self.proxy.tls { 443 } else { 80 };
        if port == default {
            format!("{scheme}://{name}.localhost")
        } else {
            format!("{scheme}://{name}.localhost:{port}")
        }
    }

    /// The URL a project gets; portless's own report wins once it's printed.
    pub fn expected_url(&self, e: &Entry) -> String {
        e.url.clone().unwrap_or_else(|| self.url_for(&e.project.name))
    }

    fn save(&mut self) {
        self.config.projects = self.entries.iter().map(|e| e.project.clone()).collect();
        if let Err(e) = self.config.save(&self.config_path) {
            self.error(format!("Couldn't save the config: {e:#}"));
        }
    }

    // -----------------------------------------------------------------------
    // Background work

    /// Drain process output, reap exited children and refresh the proxy
    /// status. Returns whether anything visible changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            changed = true;
            match ev {
                Event::Log { id, line } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                        if e.url.is_none()
                            && let Some(url) = process::parse_url(&line)
                        {
                            e.url = Some(url);
                        }
                        if e.app_port.is_none() {
                            e.app_port = process::parse_app_port(&line);
                        }
                        e.log(line);
                    }
                }
                Event::ProxyCmd { ok, message } => self.on_proxy_result(ok, message),
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
                    let port = e.project.port.or(e.app_port);
                    if port.is_some_and(listening) {
                        e.ready = true;
                        changed = true;
                    }
                }
            }
        }

        if self.proxy_checked.elapsed() >= Duration::from_secs(1) {
            let before = self.proxy;
            self.proxy = process::proxy_status();
            self.proxy_checked = Instant::now();
            changed |= self.proxy != before;
            if let Setup::Waiting(since) = self.setup {
                if self.proxy_ready() {
                    self.setup = Setup::None;
                    self.info("The proxy is up");
                    self.spawn_queued();
                    changed = true;
                } else if since.elapsed() > SETUP_TIMEOUT {
                    self.setup = Setup::None;
                    self.cancel_queued("the proxy never came up");
                    changed = true;
                }
            }
        }
        if self.message.as_ref().is_some_and(|(_, _, t)| t.elapsed() > MESSAGE_TTL) {
            self.message = None;
            changed = true;
        }
        changed
    }

    fn on_proxy_result(&mut self, ok: bool, message: String) {
        self.proxy_busy = false;
        self.proxy = process::proxy_status();
        self.proxy_checked = Instant::now();
        if ok {
            self.info(message);
            self.spawn_queued();
        } else {
            self.error(message.clone());
            self.cancel_queued(&message);
        }
    }

    fn spawn_queued(&mut self) {
        let queued: Vec<Id> = self.entries.iter().filter(|e| e.queued).map(|e| e.id).collect();
        for id in queued {
            if let Some(i) = self.index(id) {
                self.entries[i].queued = false;
                self.spawn(i);
            }
        }
    }

    fn cancel_queued(&mut self, reason: &str) {
        for e in self.entries.iter_mut().filter(|e| e.queued) {
            e.queued = false;
            e.log(format!("── not started: {reason} ──"));
        }
    }

    // -----------------------------------------------------------------------
    // Process control

    pub fn start(&mut self, id: Id) {
        let Some(i) = self.index(id) else { return };
        let e = &self.entries[i];
        if e.is_active() {
            return;
        }
        if !self.portless_ok {
            self.portless_ok = process::portless_installed();
            if !self.portless_ok {
                self.error("portless isn't installed. Run: npm i -g portless");
                return;
            }
        }
        if let Some(port) = e.project.port
            && process::port_in_use(port)
        {
            let msg = format!("Port {port} is already in use, so {} can't start", e.project.name);
            self.entries[i].log(format!("── {msg} ──"));
            self.error(msg);
            return;
        }
        // Start the proxy once ourselves rather than letting several portless
        // processes race to auto-start it.
        if !self.proxy_ready() {
            self.entries[i].queued = true;
            self.request_proxy_start();
            return;
        }
        self.spawn(i);
    }

    fn spawn(&mut self, i: usize) {
        let port = self.effective_proxy_port();
        let tx = self.tx.clone();
        let e = &mut self.entries[i];
        if !e.logs.is_empty() {
            e.log("");
        }
        e.url = None;
        e.app_port = None;
        e.ready = false;
        e.last_exit = None;
        match process::spawn(&e.project, port, e.id, tx) {
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
        e.queued = false;
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
            e.queued = false;
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
        if !command.is_empty() {
            shell_words::split(&command).map_err(|e| (Field::Command, format!("Can't parse this command: {e}")))?;
        }
        Ok(Project { name, path: folder.clone(), port, command })
    }

    pub fn add(&mut self, project: Project) -> Id {
        let id = self.push_entry(project);
        self.save();
        id
    }

    /// Returns true if the project is running and needs a restart to apply.
    pub fn update(&mut self, id: Id, project: Project) -> bool {
        let Some(i) = self.index(id) else { return false };
        let e = &mut self.entries[i];
        let changed = e.project != project;
        e.project = project;
        e.url = None;
        let needs_restart = changed && e.run.is_some();
        self.save();
        needs_restart
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
        self.entries.remove(i);
        self.save();
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

    // -----------------------------------------------------------------------
    // Proxy

    /// Ports below 1024 (443 for plain `https://name.localhost`) need root.
    pub fn proxy_needs_root(&self) -> bool {
        self.config.proxy_port < 1024
    }

    /// Port of a running proxy that isn't on the configured port.
    pub fn stray_proxy_port(&self) -> Option<u16> {
        self.proxy.port.filter(|&p| self.proxy.running && p != self.config.proxy_port)
    }

    /// Whether projects can start now. A proxy on another port is fine for an
    /// unprivileged config, but when the config asks for clean URLs (port 443)
    /// a leftover `:1355` proxy has to be replaced first.
    pub fn proxy_ready(&self) -> bool {
        self.proxy.running && !(self.proxy_needs_root() && self.stray_proxy_port().is_some())
    }

    fn request_proxy_start(&mut self) {
        if self.proxy_busy || self.setup != Setup::None {
            return;
        }
        if self.proxy_needs_root() {
            self.setup = Setup::Needed;
            return;
        }
        self.proxy_busy = true;
        self.info(format!("Starting the proxy on :{}", self.config.proxy_port));
        process::proxy_command(true, self.config.proxy_port, self.tx.clone());
    }

    /// Open a terminal that runs the root setup; `tick` notices the proxy.
    pub fn run_setup(&mut self, kind: SetupKind) {
        let port = self.config.proxy_port;
        let mut steps = Vec::new();
        if self.stray_proxy_port().is_some() {
            steps.push("portless proxy stop".to_string());
        }
        steps.push(match kind {
            SetupKind::Service => format!("portless service install -p {port}"),
            SetupKind::Once => format!("portless proxy start -p {port}"),
        });
        match open_terminal("portboard: one-time proxy setup", &steps) {
            Ok(()) => self.setup = Setup::Waiting(Instant::now()),
            Err(e) => {
                self.setup = Setup::None;
                self.cancel_queued("couldn't open a terminal");
                self.error(format!("Couldn't open a terminal: {e}. Run `{}` yourself.", steps.join(" && ")));
            }
        }
    }

    /// Switch to the unprivileged port: URLs get `:1355`, no root needed.
    pub fn use_fallback_port(&mut self) {
        self.config.proxy_port = config::FALLBACK_PROXY_PORT;
        self.save();
        self.setup = Setup::None;
        if self.proxy_ready() {
            self.spawn_queued();
        } else {
            self.request_proxy_start();
        }
    }

    /// Switch back to port 443 for clean URLs.
    pub fn use_clean_urls(&mut self) {
        self.config.proxy_port = config::DEFAULT_PROXY_PORT;
        self.save();
    }

    pub fn cancel_setup(&mut self) {
        self.setup = Setup::None;
        self.cancel_queued("the proxy isn't running");
    }

    pub fn toggle_proxy(&mut self) {
        if self.proxy_busy {
            return;
        }
        if !self.proxy_ready() {
            self.request_proxy_start();
            return;
        }
        if self.proxy_needs_root() {
            let steps = [format!("portless proxy stop -p {}", self.config.proxy_port)];
            if let Err(e) = open_terminal("portboard: stop the proxy", &steps) {
                self.error(format!("Couldn't open a terminal: {e}"));
            }
            return;
        }
        self.proxy_busy = true;
        self.info("Stopping the proxy");
        process::proxy_command(false, self.config.proxy_port, self.tx.clone());
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

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Run shell `steps` in a new terminal window (sudo needs a TTY to ask for a password).
fn open_terminal(title: &str, steps: &[String]) -> std::io::Result<()> {
    let path = std::env::var("PATH").unwrap_or_default();
    let script = format!(
        "export PATH={path}\n\
         printf '\\033[1;33m%s\\033[0m\\n\\n' {title}\n\
         if {steps}; then\n\
           printf '\\n\\033[32mDone.\\033[0m This window can be closed.\\n'\n\
         else\n\
           printf '\\n\\033[31mSomething went wrong (see above).\\033[0m\\n'\n\
         fi\n\
         printf 'Press Enter to close'; read _\n",
        path = sh_quote(&path),
        title = sh_quote(title),
        steps = steps.join(" && "),
    );
    let launchers: [(&str, &[&str]); 6] = [
        ("xdg-terminal-exec", &[]),
        ("x-terminal-emulator", &["-e"]),
        ("ghostty", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("foot", &[]),
    ];
    let mut last_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no terminal emulator found");
    for (bin, prefix) in launchers {
        match Command::new(bin)
            .args(prefix)
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> Manager {
        let dir = std::env::temp_dir().join(format!("portboard-mgr-{}", std::process::id()));
        Manager::new(Config::default(), dir.join("config.toml"))
    }

    #[test]
    fn validation() {
        let mut m = manager();
        let tmp = std::env::temp_dir();
        m.push_entry(Project { name: "taken".into(), path: tmp.clone(), port: Some(3000), command: String::new() });
        let v = |m: &Manager, name: &str, port: &str| m.validate(None, name, Some(&tmp), port, "").map_err(|e| e.0);
        assert_eq!(v(&m, "taken", "").unwrap_err(), Field::Name);
        assert_eq!(v(&m, "Bad Name", "").unwrap_err(), Field::Name);
        assert_eq!(v(&m, "fresh", "3000").unwrap_err(), Field::Port);
        assert_eq!(v(&m, "fresh", "99999").unwrap_err(), Field::Port);
        assert_eq!(v(&m, "fresh", "3001").unwrap().port, Some(3001));
        assert_eq!(m.validate(None, "fresh", None, "", "").unwrap_err().0, Field::Folder);
        // Editing a project may keep its own name and port.
        let id = m.entries[0].id;
        assert!(m.validate(Some(id), "taken", Some(&tmp), "3000", "pnpm dev").is_ok());
    }

    #[test]
    fn urls() {
        let mut m = manager();
        m.proxy = ProxyStatus { running: false, port: None, tls: true };
        m.config.proxy_port = 443;
        assert_eq!(m.url_for("shop"), "https://shop.localhost");
        m.config.proxy_port = 1355;
        assert_eq!(m.url_for("shop"), "https://shop.localhost:1355");
    }

    #[test]
    fn stray_proxy_blocks_clean_urls() {
        let mut m = manager();
        m.proxy = ProxyStatus { running: true, port: Some(1355), tls: true };
        m.config.proxy_port = 443;
        assert!(!m.proxy_ready());
        assert_eq!(m.stray_proxy_port(), Some(1355));
        m.config.proxy_port = 1355;
        assert!(m.proxy_ready());
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }
}
