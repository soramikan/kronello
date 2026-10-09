//! Safety audit: `kr_raw_*` handles are uniquely owned by `RawSession`, LibRaw
//! strings are copied while the session lives, and every pixel copy uses a
//! byte count reported by LibRaw into a caller-sized Rust buffer. None of
//! these types implement Send/Sync.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;

#[repr(C)]
pub(crate) struct RawInfo {
    pub raw_width: u32,
    pub raw_height: u32,
    pub width: u32,
    pub height: u32,
    pub iwidth: u32,
    pub iheight: u32,
    pub top_margin: u32,
    pub left_margin: u32,
    pub colors: i32,
    pub filters: u32,
    pub dng_version: u32,
    pub raw_count: u32,
    pub flip: i32,
    pub tiff_compression: u32,
    pub tiff_bps: u32,
    pub is_foveon: u32,
    pub as_shot_neutral: [f32; 4],
    pub cam_mul: [f32; 4],
    pub cam_xyz: [[f32; 3]; 4],
    pub cmatrix: [[f32; 4]; 3],
    pub black: f32,
    pub maximum: f32,
    pub dng_color_count: u32,
    pub make: [c_char; 64],
    pub model: [c_char; 64],
}
impl Default for RawInfo {
    fn default() -> Self {
        // SAFETY: repr(C) plain-old-data output struct; all-zero is valid.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct RawImageInfo {
    pub width: u32,
    pub height: u32,
    pub colors: u32,
    pub bits: u32,
    pub data_size: u64,
}

unsafe extern "C" {
    fn kr_raw_bind(path: *const c_char) -> c_int;
    fn kr_raw_capabilities() -> u32;
    fn kr_raw_version() -> *const c_char;
    fn kr_raw_strerror(code: c_int) -> *const c_char;
    fn kr_raw_open(path: *const c_char) -> *mut c_void;
    fn kr_raw_opened(k: *mut c_void) -> c_int;
    fn kr_raw_last_error(k: *mut c_void) -> c_int;
    fn kr_raw_info(k: *mut c_void, out: *mut RawInfo) -> c_int;
    fn kr_raw_process(k: *mut c_void, out: *mut RawImageInfo) -> c_int;
    fn kr_raw_image_copy(k: *mut c_void, dst: *mut u8, capacity: u64) -> c_int;
    fn kr_raw_close(k: *mut c_void);
}

/// Bind the shared LibRaw exactly once per process. Unix builds resolve the
/// symbols at link time; on Windows the vendored MinGW DLL is loaded from an
/// explicit directory so the process never depends on loader search paths.
pub(crate) fn ensure_bound() -> bool {
    static BOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *BOUND.get_or_init(bind)
}

#[cfg(windows)]
fn bind() -> bool {
    let Some(dll) = libraw_dll_path() else {
        return false;
    };
    let Ok(path) = CString::new(dll.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: NUL-terminated UTF-8 path owned for the call duration.
    unsafe { kr_raw_bind(path.as_ptr()) == 1 }
}

#[cfg(not(windows))]
fn bind() -> bool {
    // SAFETY: The Unix shim ignores the path and reports link-time binding.
    unsafe { kr_raw_bind(std::ptr::null()) == 1 }
}

/// Runtime directory for the vendored `libraw_r*.dll`: explicit
/// `KRONELLO_LIBRAW_LIB_DIR`, the shared `KRONELLO_FFMPEG_LIB_DIR` runtime
/// directory, the packaged `lib/` next to a manifest, then the build prefix.
#[cfg(windows)]
fn libraw_dll_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("KRONELLO_LIBRAW_LIB_DIR")
        .or_else(|| std::env::var_os("KRONELLO_FFMPEG_LIB_DIR"))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let executable = std::env::current_exe().ok()?;
            let root = executable.parent()?.parent()?;
            root.join("package-manifest.json")
                .is_file()
                .then(|| root.join("lib"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from(env!("KRONELLO_LIBRAW_BUILD_LIB_DIR")));
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with("libraw_r") && name.ends_with(".dll")
        })
}

pub(crate) fn capabilities() -> u32 {
    if !ensure_bound() {
        return 0;
    }
    // SAFETY: Pure C query, no ownership transfer.
    unsafe { kr_raw_capabilities() }
}
pub(crate) fn version() -> String {
    // SAFETY: Returned pointer is a process-lifetime C string; the shim
    // returns "" when unbound.
    unsafe { CStr::from_ptr(kr_raw_version()) }
        .to_string_lossy()
        .into_owned()
}
pub(crate) fn strerror(code: i32) -> String {
    // SAFETY: Returned pointer is a process-lifetime C string; the shim
    // returns a static string when unbound.
    unsafe { CStr::from_ptr(kr_raw_strerror(code)) }
        .to_string_lossy()
        .into_owned()
}

/// One LibRaw instance per decode; never shared across threads.
pub(crate) struct RawSession {
    handle: NonNull<c_void>,
    opened: bool,
}
impl RawSession {
    pub(crate) fn open(path: &Path) -> Result<(Self, i32), String> {
        if !ensure_bound() {
            return Err("LibRaw library not bound".to_string());
        }
        let path = path
            .canonicalize()
            .map_err(|e| format!("raw path canonicalize: {e}"))?;
        let utf8 = path
            .to_str()
            .ok_or_else(|| "raw path is not UTF-8".to_string())?;
        let cpath =
            CString::new(utf8.as_bytes()).map_err(|_| "raw path contains NUL".to_string())?;
        // SAFETY: NUL-terminated local path; returned handle is uniquely owned
        // by RawSession and released exactly once in Drop.
        let raw = unsafe { kr_raw_open(cpath.as_ptr()) };
        let handle = NonNull::new(raw).ok_or_else(|| "LibRaw allocation failed".to_string())?;
        // SAFETY: Valid live handle just returned.
        let opened = unsafe { kr_raw_opened(handle.as_ptr()) } != 0;
        // SAFETY: Same.
        let code = unsafe { kr_raw_last_error(handle.as_ptr()) };
        Ok((Self { handle, opened }, code))
    }
    pub(crate) fn opened(&self) -> bool {
        self.opened
    }
    pub(crate) fn info(&self) -> Result<RawInfo, i32> {
        let mut info = RawInfo::default();
        // SAFETY: Owned live handle; writes into a caller-sized struct.
        let code = unsafe { kr_raw_info(self.handle.as_ptr(), &mut info) };
        if code == 0 { Ok(info) } else { Err(code) }
    }
    /// Pinned deterministic pipeline defined by the shim (ADR-0136).
    pub(crate) fn process(&self) -> Result<RawImageInfo, i32> {
        let mut image = RawImageInfo::default();
        // SAFETY: Owned live opened handle; single process call per session is
        // enforced by the shim (second call returns OUT_OF_ORDER).
        let code = unsafe { kr_raw_process(self.handle.as_ptr(), &mut image) };
        if code == 0 { Ok(image) } else { Err(code) }
    }
    pub(crate) fn copy_image(&self, dst: &mut [u8]) -> Result<(), i32> {
        // SAFETY: Owned processed handle; dst length is the LibRaw-reported
        // byte count and is the authoritative capacity.
        let code =
            unsafe { kr_raw_image_copy(self.handle.as_ptr(), dst.as_mut_ptr(), dst.len() as u64) };
        if code == 0 { Ok(()) } else { Err(code) }
    }
}
impl Drop for RawSession {
    fn drop(&mut self) {
        // SAFETY: Sole owner; released once.
        unsafe { kr_raw_close(self.handle.as_ptr()) }
    }
}
