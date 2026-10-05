//! What differs between Unix and Windows: finding programs, keeping each
//! server's process tree together, stopping it, and the shell its command
//! runs in.
//!
//! Each server's processes are a [`Tree`]. On Unix that's a process group,
//! which the guardian process also signals if blueprint dies. On Windows it's
//! a job object the server is spawned into (suspended until it has joined, so
//! nothing it starts can slip out), which the system kills when blueprint
//! exits however it exits.

pub use imp::*;

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    use crate::guardian;

    /// Start the child in its own process group, so it can be stopped as a whole.
    pub fn isolate(cmd: &mut Command) {
        cmd.process_group(0);
    }

    /// A server's processes: the process group its shell leads.
    pub struct Tree {
        pgid: u32,
    }

    /// Track the child's tree, and make sure it dies with blueprint.
    pub fn adopt(child: &Child) -> io::Result<Tree> {
        // Children are spawned with process_group(0), so their pgid is their pid.
        let pgid = child.id();
        guardian::watch(pgid);
        Ok(Tree { pgid })
    }

    impl Tree {
        /// Whether any process of the tree is left.
        pub fn alive(&self) -> bool {
            guardian::group_alive(self.pgid)
        }

        /// Ask the tree to exit.
        pub fn terminate(&self) {
            self.signal(libc::SIGTERM);
        }

        /// Kill the tree outright.
        pub fn kill(&self) {
            self.signal(libc::SIGKILL);
        }

        fn signal(&self, sig: libc::c_int) {
            unsafe {
                libc::kill(-(self.pgid as libc::pid_t), sig);
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            guardian::unwatch(self.pgid);
        }
    }

    /// `command` run by `sh`, so pipes, `&&` and `$PORT` work.
    pub fn shell(command: &str) -> io::Result<Command> {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        Ok(cmd)
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
    };

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

    /// Start the child suspended: [`adopt`] resumes it once it's in its job,
    /// so not even its first child can start outside it.
    pub fn isolate(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    /// A server's processes: a job object holding the shell and everything it
    /// starts. Closing the last handle to it (when blueprint exits, however it
    /// exits) kills whatever is still in it.
    pub struct Tree {
        job: HANDLE,
    }

    // The handle is only an identifier for the kernel object; the job APIs
    // can be called from any thread.
    unsafe impl Send for Tree {}

    fn check(ok: i32, what: &str) -> io::Result<()> {
        if ok == 0 {
            let e = io::Error::last_os_error();
            return Err(io::Error::new(e.kind(), format!("{what}: {e}")));
        }
        Ok(())
    }

    /// Put the suspended child in a job of its own, then let it run.
    pub fn adopt(child: &Child) -> io::Result<Tree> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            let e = io::Error::last_os_error();
            return Err(io::Error::new(e.kind(), format!("couldn't create a job object: {e}")));
        }
        // From here on, dropping the tree closes the job.
        let tree = Tree { job };
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            check(
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ),
                "couldn't configure the job object",
            )?;
            check(
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE),
                "couldn't add the server to its job object",
            )?;
        }
        resume(child.id())?;
        Ok(tree)
    }

    /// Resume the main thread of a process created suspended (its only thread).
    fn resume(pid: u32) -> io::Result<()> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap == INVALID_HANDLE_VALUE {
                let e = io::Error::last_os_error();
                return Err(io::Error::new(e.kind(), format!("couldn't list threads: {e}")));
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut thread = None;
            let mut more = Thread32First(snap, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    thread = Some(entry.th32ThreadID);
                    break;
                }
                more = Thread32Next(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            let Some(tid) = thread else {
                return Err(io::Error::other("couldn't find the server's main thread"));
            };
            let handle = OpenThread(THREAD_SUSPEND_RESUME, 0, tid);
            if handle.is_null() {
                let e = io::Error::last_os_error();
                return Err(io::Error::new(e.kind(), format!("couldn't open the server's main thread: {e}")));
            }
            let resumed = ResumeThread(handle);
            CloseHandle(handle);
            if resumed == u32::MAX {
                let e = io::Error::last_os_error();
                return Err(io::Error::new(e.kind(), format!("couldn't resume the server: {e}")));
            }
        }
        Ok(())
    }

    impl Tree {
        /// Whether any process of the tree is left.
        pub fn alive(&self) -> bool {
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                let ok = QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut c_void,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                );
                // If the job can't be asked, don't wait on it forever.
                ok != 0 && info.ActiveProcesses > 0
            }
        }

        /// Console programs can't be asked to close politely from a windowless
        /// parent, so stopping kills the tree.
        pub fn terminate(&self) {
            self.kill();
        }

        pub fn kill(&self) {
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.job);
            }
        }
    }

    /// `command` translated for and run by `cmd.exe`, which also finds the
    /// `.cmd` shims npm installs. Passed raw: `/s` strips the outer quotes and
    /// leaves the command exactly as written, which Rust's argument quoting
    /// wouldn't.
    pub fn shell(command: &str) -> io::Result<Command> {
        let translated = crate::wincmd::translate(command).map_err(io::Error::other)?;
        let mut cmd = self::command("cmd");
        cmd.raw_arg(format!("/d /s /c \"{translated}\""));
        Ok(cmd)
    }
}
