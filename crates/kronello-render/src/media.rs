//! Plain decoded media contracts. Native decoder handles remain in backends.
use kronello_time::Rational;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq)]
pub struct VideoImage {
    pub size: [u32; 2],
    /// Linear working-space premultiplied RGBA, prepared by the explicit media path.
    pub pixels: Vec<[f32; 4]>,
}
/// Decoded video presented as a lazy per-pixel sampler in source
/// coordinates. Backends return this from `RenderDag::resolve_video` so only
/// mapped output pixels are converted — a full working-space frame is never
/// materialized when the draw covers a smaller window.
pub struct VideoSource {
    pub size: [u32; 2],
    sample: Box<dyn Fn(usize, usize) -> [f32; 4]>,
}
impl VideoSource {
    /// `sample` returns the premultiplied linear working-space pixel at
    /// integer source coordinates; callers clamp to `size - 1` first.
    pub fn new(size: [u32; 2], sample: impl Fn(usize, usize) -> [f32; 4] + 'static) -> Self {
        Self {
            size,
            sample: Box::new(sample),
        }
    }
    pub fn from_image(image: VideoImage) -> Self {
        let width = image.size[0] as usize;
        Self::new(image.size, move |x, y| image.pixels[y * width + x])
    }
    /// Integer coordinates must already be clamped inside `size`.
    pub fn sample(&self, x: usize, y: usize) -> [f32; 4] {
        (self.sample)(x, y)
    }
}
impl std::fmt::Debug for VideoSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoSource")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Native pixel values, packed with alignment 1, in FFmpeg's named plane order.
/// This is source-encoded color; it must be color-converted before compositing.
/// No HDR reduction, working-space conversion or alpha premultiplication occurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedVideoFrame {
    pub pts: Rational,
    pub end: Rational,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub color_primaries: String,
    pub color_transfer: String,
    pub color_matrix: String,
    pub color_range: String,
    pub pixels: Vec<u8>,
}

pub trait VideoDecodeBackend {
    fn frame_at(&mut self, time: Rational) -> Result<DecodedVideoFrame, crate::RenderError>;
}
