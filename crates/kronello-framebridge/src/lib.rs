//! Explicit transfer probes and resident decode paths. Native APIs stay on macOS.
use kronello_gpu::{GpuContext, GpuError, TransferStats};
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod resident;
#[cfg(target_os = "macos")]
pub mod videotoolbox;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    CpuUpload,
    GpuCopy,
    GpuReadback,
    IoSurfaceImport,
    IoSurfaceOutput,
    /// Compatibility-only rejected selector. Never aliases a concrete decode path
    /// and never selects encoding. Omitted from [`CONCRETE_PATHS`].
    VideoToolbox,
    CvPixelBufferImport,
    VideoToolboxDecodeBgra8,
    VideoToolboxDecodeNv12Biplanar,
}
/// Explicit probe selectors, not a promise that native runtime capabilities exist.
/// Call `require_gpu_resident` for platform/policy admission, then execute the path
/// to establish actual availability. Generic VideoToolbox is never advertised.
pub const CONCRETE_PATHS: &[PathKind] = &[
    PathKind::CpuUpload,
    PathKind::GpuCopy,
    PathKind::GpuReadback,
    PathKind::IoSurfaceImport,
    PathKind::IoSurfaceOutput,
    PathKind::CvPixelBufferImport,
    PathKind::VideoToolboxDecodeBgra8,
    PathKind::VideoToolboxDecodeNv12Biplanar,
];
const GENERIC_VIDEOTOOLBOX_REJECTION: &str = "generic VideoToolbox selector is unsupported; explicitly select VideoToolboxDecodeBgra8 or VideoToolboxDecodeNv12Biplanar for decode; this selector provides no encode path";
impl PathKind {
    /// Admit a transfer policy on this platform. Native hardware/format support
    /// must still be verified by executing the concrete path; no implicit fallback.
    pub fn require_gpu_resident(self) -> Result<(), GpuError> {
        match self {
            Self::CpuUpload | Self::GpuReadback => Err(GpuError::UnsupportedFeature(
                "require_gpu_resident rejects CPU transfer",
            )),
            Self::VideoToolbox => Err(GpuError::UnsupportedFeature(GENERIC_VIDEOTOOLBOX_REJECTION)),
            Self::GpuCopy => Ok(()),
            Self::IoSurfaceImport
            | Self::IoSurfaceOutput
            | Self::CvPixelBufferImport
            | Self::VideoToolboxDecodeBgra8
            | Self::VideoToolboxDecodeNv12Biplanar => {
                if cfg!(target_os = "macos") {
                    Ok(())
                } else {
                    Err(GpuError::UnsupportedFeature("IOSurface requires macOS"))
                }
            }
        }
    }
}
#[derive(Debug)]
pub struct Measurement {
    pub path: PathKind,
    pub elapsed: Duration,
    pub transfers: TransferStats,
    pub detail: String,
}
pub trait TransferPath {
    fn kind(&self) -> PathKind;
    /// One completed transfer, including CPU submission and synchronization overhead.
    fn measure(&self, gpu: &GpuContext) -> Result<Measurement, GpuError>;
}
#[derive(Debug)]
pub struct SpikePath(pub PathKind);
impl TransferPath for SpikePath {
    fn kind(&self) -> PathKind {
        self.0
    }
    fn measure(&self, gpu: &GpuContext) -> Result<Measurement, GpuError> {
        if matches!(
            self.0,
            PathKind::CvPixelBufferImport
                | PathKind::VideoToolboxDecodeBgra8
                | PathKind::VideoToolboxDecodeNv12Biplanar
        ) {
            #[cfg(target_os = "macos")]
            {
                let result = match self.0 {
                    PathKind::CvPixelBufferImport => videotoolbox::probe_cvpixelbuffer_import(gpu),
                    PathKind::VideoToolboxDecodeBgra8 => {
                        videotoolbox::probe_videotoolbox_decode_bgra8(gpu)
                    }
                    _ => videotoolbox::probe_videotoolbox_decode(gpu, true),
                };
                return result.map_err(|error| {
                    eprintln!("{error}");
                    GpuError::UnsupportedFeature("CoreVideo/VideoToolbox probe failed; see native stage and OSStatus diagnostic")
                });
            }
            #[cfg(not(target_os = "macos"))]
            return Err(GpuError::UnsupportedFeature(
                "CoreVideo/VideoToolbox requires macOS",
            ));
        }
        if matches!(
            self.0,
            PathKind::IoSurfaceImport | PathKind::IoSurfaceOutput
        ) {
            #[cfg(target_os = "macos")]
            return macos::measure(gpu, self.0);
            #[cfg(not(target_os = "macos"))]
            return Err(GpuError::UnsupportedFeature("IOSurface requires macOS"));
        }
        if self.0 == PathKind::VideoToolbox {
            return Err(GpuError::UnsupportedFeature(GENERIC_VIDEOTOOLBOX_REJECTION));
        }
        let texture = gpu.texture(
            64,
            64,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        )?;
        let bytes: Vec<u8> = (0..64 * 64)
            .flat_map(|_| [0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c])
            .collect();
        // Preparation is outside the timing and transfer counters for copy/readback.
        if self.0 != PathKind::CpuUpload {
            upload(gpu, &texture, &bytes);
            gpu.wait()?;
        }
        let copy = if self.0 == PathKind::GpuCopy {
            Some(gpu.texture(
                64,
                64,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            )?)
        } else {
            None
        };
        let start = Instant::now();
        let mut transfers = TransferStats::default();
        match self.0 {
            PathKind::CpuUpload => {
                upload(gpu, &texture, &bytes);
                gpu.queue.submit([]);
                gpu.wait()?;
                transfers.cpu_upload_pixel_bytes = bytes.len() as u64;
                transfers.cpu_upload_pixel_operations = 1;
            }
            PathKind::GpuCopy => {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                encoder.copy_texture_to_texture(
                    texture.as_image_copy(),
                    copy.as_ref().unwrap().as_image_copy(),
                    texture.size(),
                );
                gpu.queue.submit([encoder.finish()]);
                gpu.wait()?;
                transfers.gpu_copy_bytes = bytes.len() as u64;
                transfers.gpu_copy_operations = 1;
            }
            PathKind::GpuReadback => {
                let result = gpu.read_texture(&texture, 8, &mut transfers)?;
                if result != bytes {
                    return Err(GpuError::Readback("transfer probe pixel mismatch".into()));
                }
            }
            _ => unreachable!(),
        }
        let elapsed = start.elapsed();
        // Validation readback for upload/copy is outside measured time/counters.
        if self.0 != PathKind::GpuReadback {
            let result = gpu.read_texture(
                copy.as_ref().unwrap_or(&texture),
                8,
                &mut TransferStats::default(),
            )?;
            if result != bytes {
                return Err(GpuError::Readback("transfer probe pixel mismatch".into()));
            }
        }
        Ok(Measurement {path:self.0,elapsed,transfers,detail:"64x64 RGBA16F; host elapsed including completion fence; verification readback excluded for upload/copy".into()})
    }
}
fn upload(gpu: &GpuContext, texture: &wgpu::Texture, bytes: &[u8]) {
    gpu.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(64 * 8),
            rows_per_image: Some(64),
        },
        texture.size(),
    );
}
