//! Plain decoded media contracts. Native decoder handles remain in backends.
use kronello_time::Rational;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq)]
pub struct VideoImage {
    pub size: [u32; 2],
    /// Linear working-space premultiplied RGBA, prepared by the explicit media path.
    pub pixels: Vec<[f32; 4]>,
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
