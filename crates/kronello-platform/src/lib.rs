//! Audited native process/file boundary. Public APIs contain no OS handles.
use std::io;
#[cfg(windows)]
use std::process::Command;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(any(windows, test))]
mod detach_policy;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

/// Windows uses CreateProcessW with bInheritHandles=FALSE. Worker stdio is
/// opened in the worker itself, never inherited from a CLI/MCP transport.
#[cfg(windows)]
pub fn spawn_detached(command: &Command) -> io::Result<DetachedChild> {
    windows::spawn(command).map(|child| DetachedChild { child })
}
#[cfg(all(windows, feature = "test-support"))]
pub fn prohibit_test_parent_breakaway() -> io::Result<()> {
    windows::prohibit_parent_breakaway()
}
#[cfg(windows)]
pub struct DetachedChild {
    child: windows::Child,
}
#[cfg(windows)]
impl DetachedChild {
    pub fn id(&self) -> u32 {
        self.child.pid
    }
    pub fn detach_mode(&self) -> &'static str {
        self.child.detach_mode
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.process.terminate_and_wait()
    }
    pub fn wait(&mut self) -> io::Result<()> {
        self.child.wait()
    }
}

/// Called before state/transport setup in the launched worker.
pub fn detach_worker() -> io::Result<()> {
    #[cfg(unix)]
    {
        nix::unistd::setsid().map_err(io::Error::from)?;
        log_detach_mode("setsid")?;
        Ok(())
    }
    #[cfg(windows)]
    {
        windows::initialize_stdio()?;
        let mode = windows::verify_detached()?;
        log_detach_mode(mode)?;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    Err(io::Error::new(io::ErrorKind::Unsupported, "worker detach"))
}

fn log_detach_mode(mode: &str) -> io::Result<()> {
    use std::io::Write;
    std::io::stderr().write_all(format!("worker detach_mode: \"{mode}\"\n").as_bytes())
}

pub fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        pid > 0
            && matches!(
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
                Ok(()) | Err(nix::errno::Errno::EPERM)
            )
    }
    #[cfg(windows)]
    {
        windows::is_alive(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(windows)]
pub fn rename_noreplace(source: &std::path::Path, destination: &std::path::Path) -> io::Result<()> {
    windows::rename_noreplace(source, destination)
}

/// Enable adoption of orphaned grandchildren in Linux test harnesses only.
/// Production launch never enables this policy.
pub fn adopt_test_workers() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    nix::sys::prctl::set_child_subreaper(true).map_err(io::Error::from)?;
    Ok(())
}

/// Own a test worker until it exits, including panic/timeout cleanup.
/// On Windows this owns a process handle, avoiding PID reuse after capture.
pub struct ProcessGuard {
    pid: u32,
    reaped: std::cell::Cell<bool>,
    #[cfg(windows)]
    process: windows::Process,
}
impl ProcessGuard {
    pub fn capture(pid: u32) -> io::Result<Self> {
        if pid == 0 || pid == std::process::id() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid worker PID",
            ));
        }
        #[cfg(windows)]
        let process = windows::Process::open(pid)?;
        Ok(Self {
            pid,
            reaped: std::cell::Cell::new(false),
            #[cfg(windows)]
            process,
        })
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn terminate_and_wait(&self) -> io::Result<()> {
        if self.reaped.get() {
            return Ok(());
        }
        self.terminate_inner()?;
        self.reaped.set(true);
        Ok(())
    }
    pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<()> {
        if self.reaped.get() {
            return Ok(());
        }
        self.wait_inner(timeout)?;
        self.reaped.set(true);
        Ok(())
    }
    fn terminate_inner(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.process.terminate_and_wait()
        }
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            let pid =
                nix::unistd::Pid::from_raw(i32::try_from(self.pid).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid worker PID")
                })?);
            match kill(pid, Signal::SIGKILL) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => (),
                Err(e) => return Err(e.into()),
            }
            self.wait_inner(Duration::from_secs(10))
        }
        #[cfg(not(any(unix, windows)))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "worker termination",
        ))
    }
    fn wait_inner(&self, timeout: Duration) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.process.wait(timeout)
        }
        #[cfg(unix)]
        {
            use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
            let pid =
                nix::unistd::Pid::from_raw(i32::try_from(self.pid).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid worker PID")
                })?);
            let start = Instant::now();
            loop {
                match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => (),
                    Ok(_) => return Ok(()),
                    Err(nix::errno::Errno::ECHILD) if !process_is_alive(self.pid) => return Ok(()),
                    Err(nix::errno::Errno::ECHILD | nix::errno::Errno::EINTR) => (),
                    Err(e) => return Err(e.into()),
                }
                if start.elapsed() > timeout {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "worker not reaped"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        #[cfg(not(any(unix, windows)))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "worker termination",
        ))
    }
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Err(error) = self.terminate_and_wait() {
            eprintln!("test worker cleanup failed pid={}: {error}", self.pid);
        }
    }
}
