//! Optional, isolated Metal/VideoToolbox experiment for PERF-014.
//! SDR Rec.709 premultiplied input, opaque black matte, H.264, 24 fps only.
use std::{
    ffi::CString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

#[derive(Debug)]
pub enum SurfaceError {
    InvalidInput,
    Io(std::io::Error),
    Native(i32),
}
impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput => f.write_str("unsupported prototype input"),
            Self::Io(error) => error.fmt(f),
            Self::Native(status) => write!(f, "encoder surface prototype failed: {status}"),
        }
    }
}
impl std::error::Error for SurfaceError {}
impl From<std::io::Error> for SurfaceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct SurfaceMeasurement {
    pub total_ms: f64,
    pub prepare_ms: f64,
    pub conversion_ms: f64,
    pub encode_ms: f64,
    pub readback_bytes: u64,
    /// FNV-1a over meaningful BGRA bytes, only when verification is requested.
    pub surface_hash: u64,
    pub encoded_frames: u64,
}
unsafe extern "C" {
    fn kr_surface_probe(
        path: *const std::ffi::c_char,
        width: u32,
        height: u32,
        count: u32,
        gpu_mode: i32,
        verify: i32,
        cancel_after: i32,
        invalid: i32,
        lut: *const u16,
        result: *mut SurfaceMeasurement,
    ) -> i32;
}
fn half_value(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 31;
    let fraction = bits & 1023;
    match exponent {
        0 => sign * fraction as f32 * 2.0_f32.powi(-24),
        31 => {
            if fraction == 0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + fraction as f32 / 1024.0) * 2.0_f32.powi(i32::from(exponent) - 15),
    }
}
fn transfer_table() -> Vec<u16> {
    (0..=u16::MAX)
        .map(|bits| {
            let value = half_value(bits);
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return 256;
            }
            let encoded = if value < 0.018 {
                4.5 * value
            } else {
                1.099 * value.powf(0.45) - 0.099
            };
            (encoded * 255.0).round() as u16
        })
        .collect()
}
/// Creates a fresh Annex B stream. Existing destinations are preserved. The
/// generated half-float fixture uses exact 24 fps spans and alternating alpha.
/// Verification reads BGRA surfaces and must be disabled for transfer timings.
pub fn probe(
    path: &Path,
    dimensions: [u32; 2],
    frames: u32,
    gpu: bool,
    verify: bool,
    cancel_after: Option<u32>,
    invalid: bool,
) -> Result<SurfaceMeasurement, SurfaceError> {
    let start = Instant::now();
    let [width, height] = dimensions;
    if width == 0
        || height == 0
        || width % 2 != 0
        || height % 2 != 0
        || width > 3840
        || height > 2160
        || frames == 0
        || frames > 2400
        || cancel_after.is_some_and(|n| n >= frames)
    {
        return Err(SurfaceError::InvalidInput);
    }
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(SurfaceError::InvalidInput);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stage = path.parent().unwrap_or(Path::new(".")).join(format!(
        ".encoder-surface-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&stage)?;
    let stage = Stage(stage);
    let file = stage.0.join("video.h264");
    let native_path = CString::new(file.as_os_str().as_encoded_bytes())
        .map_err(|_| SurfaceError::InvalidInput)?;
    let table = transfer_table();
    let mut result = SurfaceMeasurement::default();
    // SAFETY: Synchronous native call borrows live strings/table/output. Native
    // waits for GPU and invalidates VT before releasing callback/input ownership.
    let status = unsafe {
        kr_surface_probe(
            native_path.as_ptr(),
            width,
            height,
            frames,
            i32::from(gpu),
            i32::from(verify),
            cancel_after.map_or(-1, |n| n as i32),
            i32::from(invalid),
            table.as_ptr(),
            &mut result,
        )
    };
    if status == 0 {
        std::fs::File::open(&file)?.sync_all()?;
        std::fs::hard_link(&file, path)?;
        result.total_ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(result)
    } else {
        Err(SurfaceError::Native(status))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_transfer_contract() {
        let table = transfer_table();
        assert_eq!(table.len(), 65536);
        assert_eq!(half_value(0x3c00), 1.0);
        assert_eq!(half_value(1), 2.0_f32.powi(-24));
        assert_eq!(table[0], 0);
        assert_eq!(table[0x3c00], 255);
        for n in [0x7c00, 0x7e00, 0xfc00, 0xbc00, 0x4000] {
            assert_eq!(table[n], 256);
        }
    }
    #[test]
    fn native_surface_pixels_timing_and_cleanup() {
        let root = std::env::temp_dir().join(format!("kronello-surface-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let cpu = probe(
            &root.join("cpu.h264"),
            [66, 18],
            8,
            false,
            true,
            None,
            false,
        )
        .unwrap();
        let gpu = probe(&root.join("gpu.h264"), [66, 18], 8, true, true, None, false).unwrap();
        assert_eq!(cpu.surface_hash, gpu.surface_hash);
        assert_eq!(cpu.encoded_frames, 8);
        assert_eq!(gpu.encoded_frames, 8);
        assert_eq!(cpu.readback_bytes, 66 * 18 * 8 * 8);
        assert_eq!(gpu.readback_bytes, 0);
        for gpu in [false, true] {
            let cancel = root.join(format!("cancel-{gpu}.h264"));
            assert!(
                probe(&cancel, [66, 18], 8, gpu, false, Some(3), false)
                    .unwrap_err()
                    .to_string()
                    .ends_with("-20")
            );
            assert!(!cancel.exists());
            let invalid = root.join(format!("invalid-{gpu}.h264"));
            assert!(
                probe(&invalid, [66, 18], 8, gpu, false, None, true)
                    .unwrap_err()
                    .to_string()
                    .ends_with("-21")
            );
            assert!(!invalid.exists());
        }
        let existing = root.join("cpu.h264");
        let saved = std::fs::read(&existing).unwrap();
        assert!(probe(&existing, [66, 18], 8, true, false, None, false).is_err());
        assert_eq!(std::fs::read(existing).unwrap(), saved);
        std::fs::remove_dir_all(root).unwrap();
    }
}
