//! Desktop launchers start apps with a bare session PATH, missing whatever
//! the user's shell profile adds (mise, cargo, bun, go, ...). Without it,
//! the dev servers' tools (npm, cargo, ...) aren't found.

#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg_attr(windows, allow(dead_code))]
const MARK: &str = "__BLUEPRINT_PATH__";

/// When not started from a terminal, add the login shell's PATH and common
/// tool folders to ours. Call before spawning any threads.
#[cfg(windows)]
pub fn import_path() {
    // Windows apps get the user's full PATH from the registry already.
}

#[cfg(unix)]
pub fn import_path() {
    if unsafe { libc::isatty(0) } == 1 {
        return; // A terminal already has the user's PATH.
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<String> = current.split(':').filter(|d| !d.is_empty()).map(String::from).collect();
    let extra = login_shell_path().unwrap_or_default();
    let home = dirs::home_dir().unwrap_or_default();
    let known = [".local/share/mise/shims", ".cargo/bin", ".bun/bin", ".deno/bin", "go/bin", ".local/bin"]
        .map(|d| home.join(d))
        .into_iter()
        .filter(|d| d.is_dir())
        .map(|d| d.to_string_lossy().into_owned());
    for dir in extra.split(':').map(String::from).chain(known) {
        if !dir.is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    let path = dirs.join(":");
    if path != current {
        // Safe: called at startup, before any other thread exists.
        unsafe { std::env::set_var("PATH", path) };
    }
}

#[cfg(unix)]
/// PATH as an interactive login shell sets it, or `None` if the shell
/// doesn't answer within a few seconds.
fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").map(PathBuf::from).unwrap_or_else(|| "/bin/sh".into());
    let mut child = Command::new(shell)
        .args(["-lic", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse(&out)
}

#[cfg_attr(windows, allow(dead_code))]
fn parse(out: &str) -> Option<String> {
    let start = out.find(MARK)? + MARK.len();
    let len = out[start..].find(MARK)?;
    Some(out[start..start + len].to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_between_marks() {
        // Profiles may print banners around the answer.
        let out = "welcome!\n__BLUEPRINT_PATH__/a:/b__BLUEPRINT_PATH__bye";
        assert_eq!(parse(out).as_deref(), Some("/a:/b"));
        assert_eq!(parse("nothing"), None);
    }
}
