//! GPU rendering boundary, independent of the pure document model.
mod allocation;
mod observation;
pub use allocation::{GpuAllocationStats, GpuNodeResourcePeak, GpuResourceUsage};
pub mod color;
mod resident;
mod resource_cache;
pub use resident::{RESIDENT_MEDIA_SHADER, ResidentImage, ResidentUpload};
pub use resource_cache::{DiskRasterConfig, GpuCacheConfig, GpuCacheStats, SurfaceLease};
pub mod render_adapter;
mod renderer;
mod scene;
mod scene_gpu;
pub use renderer::{GpuContext, RenderOutput, SHADER, decode_rgba16f};
pub use scene::*;
pub use scene_gpu::{ExternalFrame, SceneFramePair};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    AdapterUnavailable(String),
    DeviceUnavailable(String),
    UnsupportedFeature(&'static str),
    /// The DAG's estimated intermediate surface footprint exceeded the scene
    /// budget; callers may retry at a smaller output region.
    SurfaceBudgetExceeded,
    InvalidInput(&'static str),
    Readback(String),
    CacheIo(String),
    ObservationBusy,
}
impl GpuError {
    /// Stable error code surfaced through `RenderError::Backend` and
    /// `ServiceError`; single source of truth so all adapters agree.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            Self::SurfaceBudgetExceeded => "SURFACE_BUDGET_EXCEEDED",
            Self::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
            Self::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::Readback(_) => "READBACK_FAILED",
            Self::CacheIo(_) => "CACHE_IO",
            Self::ObservationBusy => "RENDER_BACKEND_BUSY",
        }
    }
}
impl Display for GpuError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {self:?}", self.code())
    }
}
impl std::error::Error for GpuError {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingSpace {
    LinearRec709,
    LinearRec2020,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSpace {
    Srgb,
    LinearRec709,
    LinearRec2020,
}

/// Explicit straight-alpha input. Encoded premultiplied formats are not accepted.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
    pub space: InputSpace,
}
impl Image {
    pub fn solid(color: [f32; 4], space: InputSpace) -> Self {
        Self {
            width: 1,
            height: 1,
            pixels: vec![color],
            space,
        }
    }
    /// Restricted PAM adapter: RGB_ALPHA, DEPTH 4, MAXVAL 255, explicit sRGB.
    pub fn from_pam(bytes: &[u8]) -> Result<Self, GpuError> {
        let end = bytes
            .windows(7)
            .position(|s| s == b"ENDHDR\n")
            .ok_or(GpuError::InvalidInput("missing PAM ENDHDR"))?
            + 7;
        let header = std::str::from_utf8(&bytes[..end])
            .map_err(|_| GpuError::InvalidInput("invalid PAM header"))?;
        if !header.starts_with("P7\n") {
            return Err(GpuError::UnsupportedFeature("only P7 PAM supported"));
        }
        let mut width = None;
        let mut height = None;
        let mut depth = None;
        let mut maxval = None;
        let mut tuple = None;
        for line in header.lines().skip(1) {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line == "ENDHDR" {
                continue;
            }
            let (key, value) = line
                .split_once(char::is_whitespace)
                .ok_or(GpuError::InvalidInput("invalid PAM field"))?;
            let value = value.trim();
            let destination = match key {
                "WIDTH" => &mut width,
                "HEIGHT" => &mut height,
                "DEPTH" => &mut depth,
                "MAXVAL" => &mut maxval,
                "TUPLTYPE" => {
                    if tuple.replace(value).is_some() {
                        return Err(GpuError::InvalidInput("duplicate PAM field"));
                    }
                    continue;
                }
                _ => return Err(GpuError::UnsupportedFeature("unknown PAM field")),
            };
            if destination
                .replace(
                    value
                        .parse::<u32>()
                        .map_err(|_| GpuError::InvalidInput("invalid PAM number"))?,
                )
                .is_some()
            {
                return Err(GpuError::InvalidInput("duplicate PAM field"));
            }
        }
        if depth != Some(4) || maxval != Some(255) || tuple != Some("RGB_ALPHA") {
            return Err(GpuError::UnsupportedFeature("PAM requires 8-bit RGB_ALPHA"));
        }
        let width = width.ok_or(GpuError::InvalidInput("missing PAM width"))?;
        let height = height.ok_or(GpuError::InvalidInput("missing PAM height"))?;
        let count = pixel_count(width, height)?;
        if bytes.len() - end
            != count
                .checked_mul(4)
                .ok_or(GpuError::InvalidInput("PAM too large"))?
        {
            return Err(GpuError::InvalidInput("PAM payload size mismatch"));
        }
        Ok(Self {
            width,
            height,
            space: InputSpace::Srgb,
            pixels: bytes[end..]
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(|v| f32::from(v) / 255.0))
                .collect(),
        })
    }
    pub fn validate(&self) -> Result<(), GpuError> {
        if self.pixels.len() != pixel_count(self.width, self.height)? {
            return Err(GpuError::InvalidInput("image size mismatch"));
        }
        for &p in &self.pixels {
            color::validate_straight(p, self.space)?;
        }
        Ok(())
    }
}
pub(crate) fn pixel_count(width: u32, height: u32) -> Result<usize, GpuError> {
    if width == 0 || height == 0 {
        return Err(GpuError::InvalidInput("zero image size"));
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or(GpuError::InvalidInput("image too large"))
}
/// Local rectangle [0,size.x) x [0,size.y), rotated about its origin, then translated.
/// Output pixels have top-left origin, centers (x+0.5,y+0.5), nearest sampling.
#[derive(Debug, Clone)]
pub struct Layer {
    pub image: Image,
    pub size: [f32; 2],
    pub translation: [f32; 2],
    pub rotation_degrees: f32,
}
impl Layer {
    pub fn rectangle(size: [f32; 2], color: [f32; 4], space: InputSpace) -> Self {
        Self {
            image: Image::solid(color, space),
            size,
            translation: [0.0; 2],
            rotation_degrees: 0.0,
        }
    }
    /// Map a local design point to parent design coordinates (+Y down).
    pub fn transform_point(&self, point: [f32; 2]) -> Result<[f32; 2], GpuError> {
        self.validate()?;
        if point.iter().any(|v| !v.is_finite()) {
            return Err(GpuError::InvalidInput("non-finite design point"));
        }
        let (sin, cos) = self.rotation_degrees.to_radians().sin_cos();
        let output = [
            cos * point[0] - sin * point[1] + self.translation[0],
            sin * point[0] + cos * point[1] + self.translation[1],
        ];
        if output.iter().any(|v| !v.is_finite()) {
            return Err(GpuError::InvalidInput("design transform overflow"));
        }
        Ok(output)
    }
    pub fn validate(&self) -> Result<(), GpuError> {
        self.image.validate()?;
        if self
            .size
            .iter()
            .chain(&self.translation)
            .chain(std::iter::once(&self.rotation_degrees))
            .any(|v| !v.is_finite())
            || self.size.iter().any(|v| *v <= 0.0)
        {
            return Err(GpuError::InvalidInput("invalid layer transform"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferStats {
    pub cpu_upload_pixel_bytes: u64,
    pub cpu_upload_pixel_operations: u64,
    pub cpu_upload_control_bytes: u64,
    pub cpu_upload_control_operations: u64,
    pub gpu_copy_bytes: u64,
    pub gpu_copy_operations: u64,
    /// Includes required row padding, unlike logical RGBA16F output bytes.
    pub gpu_readback_bytes: u64,
    pub gpu_readback_operations: u64,
    pub gpu_wait_operations: u64,
    pub gpu_compute_dispatches: u64,
}
/// Design coordinates stay fixed when only output resolution changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSize {
    pub design_extent: [f32; 2],
    pub output_resolution: [u32; 2],
}
impl RenderSize {
    pub fn pixels(width: u32, height: u32) -> Self {
        Self {
            design_extent: [width as f32, height as f32],
            output_resolution: [width, height],
        }
    }
    pub fn validate(self) -> Result<(), GpuError> {
        pixel_count(self.output_resolution[0], self.output_resolution[1])?;
        if self
            .design_extent
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(GpuError::InvalidInput(
                "positive finite design extent required",
            ));
        }
        Ok(())
    }
    pub fn pixel_scale(self) -> [f32; 2] {
        [
            self.design_extent[0] / self.output_resolution[0] as f32,
            self.design_extent[1] / self.output_resolution[1] as f32,
        ]
    }
}
/// CPU raster oracle using the same coordinate contract, without binary16 rounding.
pub fn render_reference(
    size: RenderSize,
    layers: &[Layer],
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    size.validate()?;
    let [width, height] = size.output_resolution;
    let scale = size.pixel_scale();
    let mut pixels = vec![[0.0; 4]; pixel_count(width, height)?];
    for layer in layers {
        layer.validate()?;
        let (sin, cos) = layer.rotation_degrees.to_radians().sin_cos();
        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32 + 0.5) * scale[0] - layer.translation[0];
                let dy = (y as f32 + 0.5) * scale[1] - layer.translation[1];
                let local = [cos * dx + sin * dy, -sin * dx + cos * dy];
                if local[0] >= 0.0
                    && local[1] >= 0.0
                    && local[0] < layer.size[0]
                    && local[1] < layer.size[1]
                {
                    let sx = (local[0] / layer.size[0] * layer.image.width as f32) as u32;
                    let sy = (local[1] / layer.size[1] * layer.image.height as f32) as u32;
                    let p = layer.image.pixels[(sy * layer.image.width + sx) as usize];
                    let index = (y * width + x) as usize;
                    pixels[index] = color::source_over(
                        color::to_working(p, layer.image.space, working),
                        pixels[index],
                    );
                }
            }
        }
    }
    Ok(pixels)
}

mod effect;
pub use effect::{EFFECT_KERNEL_VERSION, PixelEffect};
pub const EFFECT_SHADER: &str = include_str!("effect.wgsl");

impl TransferStats {
    pub fn accumulate(&mut self, other: &Self) {
        self.cpu_upload_pixel_bytes += other.cpu_upload_pixel_bytes;
        self.cpu_upload_pixel_operations += other.cpu_upload_pixel_operations;
        self.cpu_upload_control_bytes += other.cpu_upload_control_bytes;
        self.cpu_upload_control_operations += other.cpu_upload_control_operations;
        self.gpu_copy_bytes += other.gpu_copy_bytes;
        self.gpu_copy_operations += other.gpu_copy_operations;
        self.gpu_readback_bytes += other.gpu_readback_bytes;
        self.gpu_readback_operations += other.gpu_readback_operations;
        self.gpu_wait_operations += other.gpu_wait_operations;
        self.gpu_compute_dispatches += other.gpu_compute_dispatches;
    }
    pub fn render_stats(&self) -> kronello_render::RenderTransferStats {
        kronello_render::RenderTransferStats {
            cpu_upload_pixel_bytes: self.cpu_upload_pixel_bytes,
            cpu_upload_pixel_operations: self.cpu_upload_pixel_operations,
            cpu_upload_control_bytes: self.cpu_upload_control_bytes,
            cpu_upload_control_operations: self.cpu_upload_control_operations,
            gpu_copy_bytes: self.gpu_copy_bytes,
            gpu_copy_operations: self.gpu_copy_operations,
            gpu_readback_bytes: self.gpu_readback_bytes,
            gpu_readback_operations: self.gpu_readback_operations,
            gpu_wait_operations: self.gpu_wait_operations,
            gpu_compute_dispatches: self.gpu_compute_dispatches,
        }
    }
}

#[cfg(test)]
mod perf_tests;
