//! FLOW-004 capture sources (ADR-0135): the typed platform boundary a
//! detached worker uses to obtain opaque RGBA8 frames. ScreenCaptureKit runs
//! on macOS only, inside the worker process — UI/service code never holds a
//! capture resource. Deck ingest is a vendor-SDK adapter boundary; without a
//! vendor build the only honest answer is `UNSUPPORTED_FEATURE`.
use std::time::Duration;

use kronello_gpu::GpuError;

/// Capture target selection for an operating-system source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    /// `CGDirectDisplayID`; `None` selects the main display.
    Screen { display: Option<u32> },
    /// `CGWindowID` of the target window.
    Window { window: u32 },
    /// Bundle identifier, for example `com.apple.finder`.
    Application { bundle_id: String },
}

/// Static capture parameters fixed at job submission. The worker assigns one
/// container tick per delivered frame at `fps_num / fps_den`; capture never
/// drops or re-times frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
}

/// Vendor deck ingest adapters (DeckLink, RS-422 controllers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckAdapter {
    Decklink,
    Rs422,
}

/// One discovered deck device. Populated only by a vendor SDK adapter build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckDevice {
    pub adapter: DeckAdapter,
    /// Stable vendor-reported identity for addressing the device.
    pub id: String,
    pub label: String,
}

const DECK_UNAVAILABLE: &str =
    "deck capture requires a vendor SDK adapter build (DeckLink/RS-422); none is linked";

/// Enumerate devices a linked vendor adapter reports. No vendor SDK is linked
/// in this repository build, so the honest result is `UNSUPPORTED_FEATURE`.
pub fn probe_deck_devices() -> Result<Vec<DeckDevice>, GpuError> {
    Err(GpuError::UnsupportedFeature(DECK_UNAVAILABLE))
}

/// Opaque deck ingest session. Unconstructible without a vendor SDK adapter.
pub struct DeckCapture {
    _private: (),
}

impl DeckCapture {
    /// Open a deck ingest session. Deck devices are delivered through a
    /// vendor SDK adapter; absent the adapter this is `UNSUPPORTED_FEATURE`.
    pub fn open(
        _adapter: DeckAdapter,
        _device: Option<&str>,
        _config: CaptureConfig,
    ) -> Result<Self, GpuError> {
        Err(GpuError::UnsupportedFeature(DECK_UNAVAILABLE))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{CaptureConfig, CaptureTarget};
    use kronello_gpu::GpuError;
    use std::ffi::c_void;
    use std::time::Duration;

    #[repr(C)]
    struct Spec {
        /// 0 = display, 1 = window, 2 = application.
        kind: i32,
        display_id: u32,
        window_id: u32,
        bundle_id: [u8; 256],
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
    }

    unsafe extern "C" {
        fn kronello_fb_capture_open(
            spec: *const Spec,
            session: *mut *mut c_void,
            error: *mut u8,
            error_size: usize,
        ) -> i32;
        fn kronello_fb_capture_next(
            session: *mut c_void,
            destination: *mut u8,
            capacity: usize,
            timeout_ms: i64,
            error: *mut u8,
            error_size: usize,
        ) -> i32;
        fn kronello_fb_capture_close(session: *mut c_void);
    }

    fn read_error(error: &[u8]) -> String {
        let end = error.iter().position(|b| *b == 0).unwrap_or(error.len());
        String::from_utf8_lossy(&error[..end]).into_owned()
    }

    pub struct Session(*mut c_void);
    impl Session {
        pub fn open(config: &CaptureConfig, target: &CaptureTarget) -> Result<Self, GpuError> {
            if config.width == 0
                || config.height == 0
                || config.width & 1 != 0
                || config.height & 1 != 0
                || config.fps_num == 0
                || config.fps_den == 0
            {
                return Err(GpuError::InvalidInput(
                    "capture requires positive even dimensions and a positive frame rate",
                ));
            }
            let mut bundle = [0u8; 256];
            let (kind, display_id, window_id) = match target {
                CaptureTarget::Screen { display } => (0, display.unwrap_or(0), 0),
                CaptureTarget::Window { window } => (1, 0, *window),
                CaptureTarget::Application { bundle_id } => {
                    let bytes = bundle_id.as_bytes();
                    if bytes.is_empty() || bytes.len() >= 256 {
                        return Err(GpuError::InvalidInput("bundle identifier length"));
                    }
                    bundle[..bytes.len()].copy_from_slice(bytes);
                    (2, 0, 0)
                }
            };
            let spec = Spec {
                kind,
                display_id,
                window_id,
                bundle_id: bundle,
                width: config.width,
                height: config.height,
                fps_num: config.fps_num,
                fps_den: config.fps_den,
            };
            let mut session: *mut c_void = std::ptr::null_mut();
            let mut error = [0u8; 512];
            // SAFETY: pointers are valid for the call; on non-zero status the
            // session remains null and `error` is NUL-terminated by the callee.
            let status = unsafe {
                kronello_fb_capture_open(&spec, &mut session, error.as_mut_ptr(), error.len())
            };
            if status != 0 || session.is_null() {
                let message = read_error(&error);
                return Err(if status == -4 {
                    GpuError::UnsupportedFeature(
                        "ScreenCaptureKit screen capture requires macOS 14.0 or later",
                    )
                } else {
                    GpuError::AdapterUnavailable(format!("screen capture unavailable: {message}"))
                });
            }
            Ok(Self(session))
        }
        /// Block up to `timeout` for the next opaque RGBA8 frame. `Ok(false)`
        /// reports a timeout so the worker can re-check control signals.
        pub fn next(&self, destination: &mut [u8], timeout: Duration) -> Result<bool, GpuError> {
            let mut error = [0u8; 512];
            // SAFETY: `destination` is writable for `capacity` bytes; the callee
            // writes at most one whole frame sized at open and NUL-terminates
            // `error`.
            let status = unsafe {
                kronello_fb_capture_next(
                    self.0,
                    destination.as_mut_ptr(),
                    destination.len(),
                    timeout.as_millis().min(i64::MAX as u128) as i64,
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            match status {
                1 => Ok(true),
                0 => Ok(false),
                _ => Err(GpuError::Readback(format!(
                    "screen capture stream ended: {}",
                    read_error(&error)
                ))),
            }
        }
        fn close(&mut self) {
            if !self.0.is_null() {
                // SAFETY: `self.0` is a live session returned by open and this
                // is its only close.
                unsafe { kronello_fb_capture_close(self.0) };
                self.0 = std::ptr::null_mut();
            }
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            self.close();
        }
    }
    // The session is owned by the single capture loop in the detached worker;
    // the ObjC object serializes callbacks on its own dispatch queue.
    unsafe impl Send for Session {}
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::{CaptureConfig, CaptureTarget};
    use kronello_gpu::GpuError;
    use std::time::Duration;
    pub struct Session;
    impl Session {
        pub fn open(_config: &CaptureConfig, _target: &CaptureTarget) -> Result<Self, GpuError> {
            Err(GpuError::UnsupportedFeature(
                "operating-system capture sources require the macOS ScreenCaptureKit adapter",
            ))
        }
        pub fn next(&self, _destination: &mut [u8], _timeout: Duration) -> Result<bool, GpuError> {
            unreachable!("unopenable session")
        }
    }
}

/// A live operating-system capture session (ScreenCaptureKit on macOS).
/// `next` returns opaque `width * height * 4` RGBA8 frames in delivery order.
pub struct ScreenCapture {
    session: imp::Session,
    /// Byte length of one RGBA8 frame (`width * height * 4`).
    frame_bytes: usize,
}

impl ScreenCapture {
    pub fn open(target: CaptureTarget, config: CaptureConfig) -> Result<Self, GpuError> {
        let frame_bytes = (config.width as usize)
            .checked_mul(config.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(GpuError::InvalidInput("capture frame size overflow"))?;
        Ok(Self {
            session: imp::Session::open(&config, &target)?,
            frame_bytes,
        })
    }
    pub const fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }
    /// Wait up to `timeout` for one frame. `Ok(None)` is a poll timeout so the
    /// caller can re-check stop/cancel signals; `Ok(Some)` fills the frame.
    pub fn next_rgba(&self, timeout: Duration) -> Result<Option<Vec<u8>>, GpuError> {
        let mut buffer = vec![0u8; self.frame_bytes];
        if self.session.next(&mut buffer, timeout)? {
            Ok(Some(buffer))
        } else {
            Ok(None)
        }
    }
}
