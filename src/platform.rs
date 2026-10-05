//! What differs between Unix and Windows: finding programs, keeping each
//! server's process tree together, stopping it, and the shell its command
//! runs in.
//!
//! On Unix every server gets its own process group and the guardian process
//! signals the groups if blueprint dies. On Windows every server joins a job
//! object that the system kills when blueprint exits, and stopping one kills
//! its process tree.

pub use imp::*;

/// Rewrite a POSIX-style command for `cmd.exe`, the shell on Windows:
/// `$NAME` becomes `%NAME%`, and leading `NAME=value` assignments become
/// `set "NAME=value" &&`.
#[cfg_attr(unix, allow(dead_code))] // Used on Windows; tested everywhere.
pub fn posix_to_cmd(command: &str) -> String {
    let mut rest = command.trim();
    let mut out = String::new();
    while let Some((word, tail)) = rest.split_once(' ') {
        let is_assignment = word.split_once('=').is_some_and(|(name, _)| is_var_name(name));
        if !is_assignment {
            break;
        }
        out.push_str(&format!("set \"{}\" && ", expand_vars(word)));
        rest = tail.trim_start();
    }
    out.push_str(&expand_vars(rest));
    out
}

#[cfg_attr(unix, allow(dead_code))]
fn is_var_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg_attr(unix, allow(dead_code))]
fn expand_vars(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_alphanumeric() || n == '_' {
                name.push(n);
                chars.next();
            } else {
                break;
            }
        }
        if is_var_name(&name) {
            out.push_str(&format!("%{name}%"));
        } else {
            out.push('$');
            out.push_str(&name);
        }
    }
    out
}

#[cfg(unix)]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    use crate::guardian;

    /// Start the child in its own process group, so it can be stopped as a whole.
    pub fn isolate(cmd: &mut Command) {
        cmd.process_group(0);
    }

    /// Make sure the child's tree dies with blueprint.
    pub fn adopt(child: &Child) {
        guardian::watch(child.id());
    }

    /// The child's tree is gone; stop tracking it.
    pub fn release(pid: u32) {
        guardian::unwatch(pid);
    }

    /// Whether anything is left of the child's tree.
    pub fn tree_alive(pid: u32) -> bool {
        guardian::group_alive(pid)
    }

    /// Ask the tree to exit.
    pub fn terminate(pid: u32) {
        signal_group(pid, libc::SIGTERM);
    }

    /// Kill the tree outright.
    pub fn kill(pid: u32) {
        signal_group(pid, libc::SIGKILL);
    }

    fn signal_group(pid: u32, sig: libc::c_int) {
        // Children are spawned with process_group(0), so their pgid equals their pid.
        unsafe {
            libc::kill(-(pid as libc::pid_t), sig);
        }
    }

    /// `command` run by `sh`, so pipes, `&&` and `$PORT` work.
    pub fn shell(command: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        cmd
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    /// A command for `program`, found on PATH with any PATHEXT extension
    /// (`Command::new` alone only looks for `.exe`), run without a console
    /// window.
    pub fn command(program: &str) -> Command {
        let mut cmd = Command::new(resolve(program).unwrap_or_else(|| program.into()));
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    fn resolve(program: &str) -> Option<PathBuf> {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
            exts.split(';')
                .filter(|e| !e.is_empty())
                .map(|ext| dir.join(format!("{program}{}", ext.to_lowercase())))
                .find(|p| p.is_file())
        })
    }

    pub fn isolate(_cmd: &mut Command) {}

    /// A job object that kills everything in it when blueprint exits, however
    /// it exits. Stored as an address because handles aren't `Sync`.
    fn job() -> Option<HANDLE> {
        static JOB: OnceLock<usize> = OnceLock::new();
        let job = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            job as usize
        });
        (job != 0).then_some(job as HANDLE)
    }

    /// Put the child (and the processes it starts) in the job.
    pub fn adopt(child: &Child) {
        if let Some(job) = job() {
            unsafe {
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
            }
        }
    }

    pub fn release(_pid: u32) {}

    /// The job cleans up leftovers, so there's nothing to chase.
    pub fn tree_alive(_pid: u32) -> bool {
        false
    }

    /// Console programs can't be asked to close politely from a windowless
    /// parent, so stopping kills the tree.
    pub fn terminate(pid: u32) {
        kill(pid);
    }

    pub fn kill(pid: u32) {
        let _ = command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// `command` rewritten for and run by `cmd.exe`, which also finds the
    /// `.cmd` shims npm installs. Passed raw: `/s` strips the outer quotes and
    /// leaves the command exactly as written, which Rust's argument quoting
    /// wouldn't.
    pub fn shell(command: &str) -> Command {
        let mut cmd = self::command("cmd");
        cmd.raw_arg(format!("/d /s /c \"{}\"", super::posix_to_cmd(command)));
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::posix_to_cmd;

    #[test]
    fn cmd_rewrites() {
        assert_eq!(posix_to_cmd("trunk serve --port $PORT"), "trunk serve --port %PORT%");
        assert_eq!(
            posix_to_cmd("LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch"),
            "set \"LEPTOS_SITE_ADDR=127.0.0.1:%PORT%\" && cargo leptos watch"
        );
        assert_eq!(posix_to_cmd("echo $5 a=b"), "echo $5 a=b");
        assert_eq!(posix_to_cmd("pnpm dev"), "pnpm dev");
    }
}
