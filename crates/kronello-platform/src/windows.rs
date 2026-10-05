//! Windows-only audited calls. Handles are closed on every path.
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, HANDLE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::MoveFileExW,
    System::{
        Console::{
            GetConsoleWindow, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        },
        JobObjects::IsProcessInJob,
        Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT,
            CreateProcessW, DETACHED_PROCESS, GetCurrentProcess, INFINITE, OpenProcess,
            PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
        },
    },
};

pub fn initialize_stdio() -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    static STDIO: OnceLock<(std::fs::File, std::fs::File)> = OnceLock::new();
    let path = std::env::var_os("KRONELLO_WORKER_LOG")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "worker log missing"))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let input = std::fs::File::open("NUL")?;
    let stdio = STDIO.get_or_init(|| (input, log));
    for (kind, file) in [
        (STD_INPUT_HANDLE, &stdio.0),
        (STD_OUTPUT_HANDLE, &stdio.1),
        (STD_ERROR_HANDLE, &stdio.1),
    ] {
        // SAFETY: the files are retained for process lifetime in STDIO; the
        // process is detached and has not started transport/worker threads yet.
        if unsafe { SetStdHandle(kind, file.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn quote(value: &std::ffi::OsStr) -> Vec<u16> {
    // Standard Windows argv quoting, including trailing backslashes and quotes.
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in value.encode_wide() {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        out.push(unit);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    out
}
pub struct Child {
    pub pid: u32,
    pub process: Process,
}
impl Child {
    pub fn wait(&self) -> io::Result<()> {
        // SAFETY: the owned process handle remains live for this wait.
        if unsafe { WaitForSingleObject(self.process.handle(), INFINITE) } == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
pub fn spawn(command: &Command) -> io::Result<Child> {
    let program = wide(Path::new(command.get_program()))?;
    let mut argv = quote(command.get_program());
    for arg in command.get_args() {
        argv.push(b' ' as u16);
        argv.extend(quote(arg));
    }
    if argv.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in arguments",
        ));
    }
    argv.push(0);
    let mut env: std::collections::BTreeMap<_, _> = std::env::vars_os()
        .map(|(key, value)| (key.to_string_lossy().to_uppercase(), (key, value)))
        .collect();
    for (key, value) in command.get_envs() {
        let index = key.to_string_lossy().to_uppercase();
        if let Some(value) = value {
            env.insert(index, (key.to_owned(), value.to_owned()));
        } else {
            env.remove(&index);
        }
    }
    let mut environment = Vec::new();
    for (_, (key, value)) in env {
        environment.extend(key.encode_wide());
        environment.push(b'=' as u16);
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    let cwd = command.get_current_dir().map(wide).transpose()?;
    // SAFETY: these Win32 POD structs permit zero initialization; cb is set.
    let (mut startup, mut info): (STARTUPINFOW, PROCESS_INFORMATION) =
        unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    // SAFETY: all UTF-16 buffers are terminated and remain live, argv is mutable,
    // outputs point to initialized structs, no security attributes/handles are
    // inherited. Fixed creation flags forbid attachment fallback.
    if unsafe {
        CreateProcessW(
            program.as_ptr(),
            argv.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_BREAKAWAY_FROM_JOB
                | CREATE_NEW_PROCESS_GROUP
                | DETACHED_PROCESS
                | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
            &startup,
            &mut info,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateProcessW succeeded and transferred a thread handle to us.
    unsafe {
        CloseHandle(info.hThread);
    }
    Ok(Child {
        pid: info.dwProcessId,
        process: Process(info.hProcess as usize),
    })
}

pub fn verify_detached() -> io::Result<()> {
    let mut in_job = 0;
    // SAFETY: current-process pseudo handle is valid; output points to live BOOL.
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetConsoleWindow has no pointer arguments or ownership transfer.
    if in_job != 0 || !unsafe { GetConsoleWindow() }.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "worker retained a parent job object or console",
        ));
    }
    Ok(())
}

#[cfg(feature = "test-support")]
pub fn prohibit_parent_breakaway() -> io::Result<()> {
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
    // SAFETY: null security/name creates a private non-inheritable job handle.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: valid owned job handle and current-process pseudo handle. No
    // BREAKAWAY_OK limits are set. Called only in a disposable test parent.
    let assigned = unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) };
    let error = io::Error::last_os_error();
    // SAFETY: close exactly once; membership persists until the parent exits.
    unsafe {
        CloseHandle(job);
    }
    if assigned == 0 { Err(error) } else { Ok(()) }
}
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
    }
    value.push(0);
    Ok(value)
}
pub fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source)?;
    let destination_path = destination;
    let destination = wide(destination)?;
    // SAFETY: both buffers are NUL-terminated and live for this call. Flags zero
    // prohibit replacement and COPY_ALLOWED (cross-volume copy/delete).
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error().map(|e| e as u32) {
        Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS) => {
            Err(io::Error::new(io::ErrorKind::AlreadyExists, error))
        }
        // Windows may return ACCESS_DENIED for an existing empty directory.
        Some(ERROR_ACCESS_DENIED) if destination_path.symlink_metadata().is_ok() => {
            Err(io::Error::new(io::ErrorKind::AlreadyExists, error))
        }
        _ => Err(error),
    }
}

pub struct Process(usize);
impl Process {
    pub fn wait(&self, timeout: std::time::Duration) -> io::Result<()> {
        // SAFETY: owned process handle remains live for the bounded wait.
        match unsafe {
            WaitForSingleObject(
                self.handle(),
                timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(io::Error::new(io::ErrorKind::TimedOut, "worker not exited")),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn open(pid: u32) -> io::Result<Self> {
        // SAFETY: no inherited handle; requested rights only query/wait/terminate
        // the identified test worker. Successful handle is owned by this struct.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle as usize))
        }
    }
    fn handle(&self) -> HANDLE {
        self.0 as HANDLE
    }
    pub fn terminate_and_wait(&self) -> io::Result<()> {
        // SAFETY: self owns a valid process handle; zero timeout is nonblocking.
        if unsafe { WaitForSingleObject(self.handle(), 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        // SAFETY: handle has PROCESS_TERMINATE; no pointer arguments.
        if unsafe { TerminateProcess(self.handle(), 1) } == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: owned handle remains valid even when process has exited.
            if unsafe { WaitForSingleObject(self.handle(), 0) } != WAIT_OBJECT_0 {
                return Err(error);
            }
        }
        // SAFETY: owned handle has SYNCHRONIZE and remains live for the wait.
        match unsafe { WaitForSingleObject(self.handle(), 10000) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(io::Error::new(io::ErrorKind::TimedOut, "worker not reaped")),
            _ => Err(io::Error::last_os_error()),
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle was opened successfully and is closed exactly once.
        unsafe {
            CloseHandle(self.handle());
        }
    }
}
pub fn is_alive(pid: u32) -> bool {
    // SAFETY: opens a non-inheritable handle, owned and closed below if successful.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return io::Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED as i32);
    }
    // SAFETY: valid process handle; probe sends no signal and closes exactly once.
    let status = unsafe {
        let status = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        status
    };
    status == WAIT_TIMEOUT
}
