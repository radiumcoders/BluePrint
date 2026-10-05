//! Cleanup watchdog so dev servers never outlive blueprint.
//!
//! blueprint re-executes itself as `blueprint --guardian` with a pipe on stdin
//! and reports each server's process group as `+<pgid>` / `-<pgid>` lines.
//! When blueprint exits for any reason (normal quit, terminal closed, crash,
//! SIGKILL) the pipe closes, and the guardian SIGTERMs every group it still
//! knows about, then SIGKILLs whatever survives the grace period.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::os::unix::process::CommandExt;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::process::STOP_GRACE;

pub const FLAG: &str = "--guardian";

static PIPE: Mutex<Option<ChildStdin>> = Mutex::new(None);

/// Start the guardian. Failure is non-fatal: normal quit still stops servers.
pub fn spawn() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let child = Command::new(exe)
        .arg(FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Own process group, so terminal signals aimed at blueprint miss it.
        .process_group(0)
        .spawn();
    if let Ok(mut child) = child {
        *PIPE.lock().unwrap() = child.stdin.take();
        // Reap it when it exits so it doesn't linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn send(line: String) {
    if let Some(pipe) = PIPE.lock().unwrap().as_mut() {
        let _ = writeln!(pipe, "{line}");
    }
}

pub fn watch(pgid: u32) {
    send(format!("+{pgid}"));
}

pub fn unwatch(pgid: u32) {
    send(format!("-{pgid}"));
}

pub fn group_alive(pgid: u32) -> bool {
    unsafe { libc::kill(-(pgid as libc::pid_t), 0) == 0 }
}

/// Entry point for `blueprint --guardian`.
pub fn run() -> ! {
    for sig in [libc::SIGHUP, libc::SIGINT, libc::SIGTERM] {
        unsafe {
            libc::signal(sig, libc::SIG_IGN);
        }
    }
    let mut groups = BTreeSet::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let (op, num) = line.split_at(line.len().min(1));
        if let Ok(pgid) = num.trim().parse::<u32>() {
            match op {
                "+" => {
                    groups.insert(pgid);
                }
                "-" => {
                    groups.remove(&pgid);
                }
                _ => {}
            }
        }
    }

    // blueprint is gone.
    let kill = |sig| {
        for &g in &groups {
            unsafe {
                libc::kill(-(g as libc::pid_t), sig);
            }
        }
    };
    kill(libc::SIGTERM);
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline && groups.iter().any(|&g| group_alive(g)) {
        std::thread::sleep(Duration::from_millis(100));
    }
    kill(libc::SIGKILL);
    std::process::exit(0)
}
