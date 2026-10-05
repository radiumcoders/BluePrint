//! Test helpers: stand-in dev servers and waiting on a [`Manager`].
//!
//! The stand-in is this test binary itself, running the ignored [`helper`]
//! test with `BLUEPRINT_HELPER` naming how it behaves. That needs nothing
//! installed, works on every platform, and can misbehave on purpose.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use crate::config::Project;
use crate::manager::{Id, Manager};

/// How long a stand-in lives at most, so a failed test can't leave one behind.
const HELPER_LIFETIME: Duration = Duration::from_secs(60);

/// A shell command that runs the stand-in in `mode`. The `NAME=value`
/// prefix also exercises the Windows translation of shell syntax.
pub fn helper_command(mode: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    format!(
        "BLUEPRINT_HELPER={mode} \"{}\" testkit::helper --exact --ignored --nocapture --test-threads=1 -q",
        exe.display()
    )
}

/// A project running the stand-in in `mode`, first offered `port` (so tests
/// running at once don't race for the same one).
pub fn add_helper(m: &mut Manager, name: &str, mode: &str, port: u16) -> Id {
    let dir = std::env::temp_dir();
    let id = m.add(Project { name: name.into(), path: dir, port: None, command: helper_command(mode) });
    let i = m.index(id).unwrap();
    m.entries[i].app_port = Some(port);
    id
}

/// Tick `m` until `done` holds, or panic with the project's logs after `timeout`.
pub fn wait_until(m: &mut Manager, id: Id, timeout: Duration, what: &str, done: impl Fn(&Manager) -> bool) {
    let deadline = Instant::now() + timeout;
    while !done(m) {
        if Instant::now() > deadline {
            let logs = m.get(id).map(|e| e.logs.iter().cloned().collect::<Vec<_>>());
            panic!("timed out waiting for {what}; logs: {logs:#?}");
        }
        std::thread::sleep(Duration::from_millis(20));
        m.tick();
    }
}

pub fn logged(m: &Manager, id: Id, needle: &str) -> bool {
    m.get(id).is_some_and(|e| e.logs.iter().any(|l| l.contains(needle)))
}

/// A manager with its config in a fresh temporary folder.
pub fn manager(name: &str) -> Manager {
    let dir = std::env::temp_dir().join(format!("blueprint-{name}-{}", std::process::id()));
    Manager::new(crate::config::Config::default(), dir.join("config.toml"))
}

/// The stand-in dev server. Not a real test: other tests run it as a child.
#[test]
#[ignore = "a stand-in server that other tests start"]
fn helper() {
    let Ok(mode) = std::env::var("BLUEPRINT_HELPER") else { return };
    let mut out = std::io::stdout().lock();
    match mode.as_str() {
        // Answer HTTP on $PORT.
        "serve" => serve(listen(), HELPER_LIFETIME),
        // Answer for a moment, then stop listening but keep running.
        "serve-briefly" => {
            serve(listen(), Duration::from_secs(1));
            std::thread::sleep(HELPER_LIFETIME);
        }
        // Far more output than the log keeps, including one huge line.
        "flood" => {
            for i in 0..20_000 {
                writeln!(out, "line {i} {}", "-".repeat(60)).unwrap();
            }
            writeln!(out, "{}", "x".repeat(1_000_000)).unwrap();
            writeln!(out, "flood done").unwrap();
        }
        // Bytes no terminal would be proud of.
        "malformed" => {
            out.write_all(b"\xff\xfe not utf-8\n").unwrap();
            out.write_all("\x1b[31mred then a lone ESC\x1b\n".as_bytes()).unwrap();
            out.write_all("ESC before a multibyte char: \x1bé\n".as_bytes()).unwrap();
            out.write_all(b"\x1b]8;;an unterminated link\n").unwrap();
            out.write_all(b"progress 10%\rprogress 100%\n").unwrap();
            out.write_all(b"malformed done\n").unwrap();
        }
        // Leave a child behind that ignores SIGTERM, and exit at once.
        #[allow(clippy::zombie_processes)] // Not waiting for it is the point.
        "orphan" => {
            ignore_sigterm();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["testkit::helper", "--exact", "--ignored", "--nocapture", "--test-threads=1", "-q"])
                .env("BLUEPRINT_HELPER", "stubborn")
                .spawn()
                .unwrap();
            writeln!(out, "child {}", child.id()).unwrap();
        }
        "stubborn" => {
            ignore_sigterm();
            std::thread::sleep(HELPER_LIFETIME);
        }
        other => panic!("unknown helper mode {other}"),
    }
}

fn listen() -> TcpListener {
    let port: u16 = std::env::var("PORT").unwrap().parse().unwrap();
    TcpListener::bind(("127.0.0.1", port)).unwrap()
}

/// Answer every connection with a tiny HTTP response for `how_long`.
fn serve(listener: TcpListener, how_long: Duration) {
    let until = Instant::now() + how_long;
    listener.set_nonblocking(true).unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    while Instant::now() < until {
        match listener.accept() {
            Ok((mut conn, _)) => {
                let _ = conn.set_nonblocking(false);
                let _ = conn.set_read_timeout(Some(Duration::from_millis(200)));
                let _ = conn.read(&mut [0; 1024]);
                let _ = conn.write_all(b"HTTP/1.0 200 OK\r\ncontent-length: 2\r\n\r\nok");
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn ignore_sigterm() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
}
