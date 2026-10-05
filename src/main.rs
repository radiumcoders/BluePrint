// Release builds on Windows are GUI apps: no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ansi;
mod config;
mod folders;
#[cfg(unix)]
mod guardian;
mod gui;
mod manager;
mod platform;
mod process;
mod shellenv;
mod stack;

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use crate::config::Config;
use crate::manager::Manager;

/// Set by SIGHUP/SIGTERM/SIGINT (e.g. Ctrl+C in the terminal that launched
/// blueprint) so the window can quit and stop servers cleanly.
static TERMINATED: AtomicBool = AtomicBool::new(false);

pub fn terminated() -> bool {
    TERMINATED.load(Ordering::SeqCst)
}

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    TERMINATED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn install_signal_handlers() {
    for sig in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
        unsafe {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
    // If a clean quit stalls, exit anyway; the guardian stops the servers.
    std::thread::spawn(|| {
        while !terminated() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        std::thread::sleep(process::STOP_GRACE + std::time::Duration::from_secs(2));
        std::process::exit(130);
    });
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(unix)]
    if args.first().map(String::as_str) == Some(guardian::FLAG) {
        guardian::run();
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "blueprint {}\nRun many dev servers at once through portless, each at https://<name>.localhost.\n\n\
             Config: {}\n(override with BLUEPRINT_CONFIG=/path/to/config.toml)",
            env!("CARGO_PKG_VERSION"),
            Config::path().display()
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("blueprint {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    shellenv::import_path();
    let path = Config::path();
    let config = Config::load(&path)?;
    #[cfg(unix)]
    {
        guardian::spawn();
        install_signal_handlers();
    }
    gui::run(Manager::new(config, path));
    Ok(())
}
