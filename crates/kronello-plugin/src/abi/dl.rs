//! Minimal dynamic-library wrapper (dlopen/dlsym/dlclose) used only by the
//! VST3 host. Plugin images are opened RTLD_NOW|RTLD_LOCAL so symbol
//! resolution is complete before any plugin code runs and plugin symbols
//! never leak into the host's global namespace.
use crate::PluginError;
use std::ffi::{CStr, CString, c_void};
use std::path::Path;

pub struct Library {
    handle: *mut c_void,
}
impl Library {
    pub fn open(path: &Path) -> Result<Self, PluginError> {
        #[cfg(not(unix))]
        {
            let _ = path;
            return Err(PluginError::Unsupported(
                "vst3 dynamic loading is implemented for unix platforms".into(),
            ));
        }
        #[cfg(unix)]
        {
            let c_path = CString::new(path.as_os_str().as_encoded_bytes())
                .map_err(|e| PluginError::InvalidInput(e.to_string()))?;
            let handle = unsafe {
                nix::libc::dlopen(c_path.as_ptr(), nix::libc::RTLD_NOW | nix::libc::RTLD_LOCAL)
            };
            if handle.is_null() {
                return Err(PluginError::Unsupported(format!(
                    "dlopen({}) failed: {}",
                    path.display(),
                    last_error()
                )));
            }
            Ok(Self { handle })
        }
    }
    /// `dlsym` lookup returning `None` when the symbol is absent.
    pub unsafe fn symbol(&self, name: &str) -> Option<*mut c_void> {
        #[cfg(unix)]
        {
            let c_name = CString::new(name).ok()?;
            unsafe { nix::libc::dlerror() };
            let ptr = unsafe { nix::libc::dlsym(self.handle, c_name.as_ptr()) };
            if unsafe { nix::libc::dlerror() }.is_null() {
                Some(ptr)
            } else {
                None
            }
        }
        #[cfg(not(unix))]
        {
            let _ = name;
            None
        }
    }
}
#[cfg(unix)]
fn last_error() -> String {
    let ptr = unsafe { nix::libc::dlerror() };
    if ptr.is_null() {
        "unknown".into()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            nix::libc::dlclose(self.handle);
        }
    }
}
// The raw handle is only used inside the helper's single loader thread.
unsafe impl Send for Library {}
