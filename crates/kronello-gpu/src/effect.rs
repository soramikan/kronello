//! Identical float32 kernels for the GPU passes and independent CPU oracle.
use crate::{GpuError, WorkingSpace, color, scene::validate_surface_pixels};
pub use kronello_render::{EFFECT_KERNEL_VERSION, PixelEffect, gaussian_kernel};

pub(crate) fn kernel(sigma: f32) -> Result<Vec<f32>, GpuError> {
    gaussian_kernel(sigma).map_err(|e| match e {
        kronello_render::RenderError::UnsupportedFeature(_) => {
            GpuError::UnsupportedFeature("effect kernel radius exceeds 1024 output pixels")
        }
        _ => GpuError::InvalidInput("invalid gaussian sigma"),
    })
}
pub(crate) fn shadow_color(effect: &PixelEffect, working: WorkingSpace) -> [f32; 4] {
    let PixelEffect::DropShadow {
        color: c, opacity, ..
    } = effect
    else {
        return [0.0; 4];
    };
    let c0 = c.components();
    let space = match c.space() {
        kronello_model::ColorSpace::Srgb => crate::InputSpace::Srgb,
        kronello_model::ColorSpace::LinearRec709 => crate::InputSpace::LinearRec709,
        kronello_model::ColorSpace::LinearRec2020 => crate::InputSpace::LinearRec2020,
    };
    color::to_working(
        [
            c0.r.get() as f32,
            c0.g.get() as f32,
            c0.b.get() as f32,
            c0.alpha.get() as f32,
        ],
        space,
        working,
    )
    .map(|v| v * opacity)
}
fn load(pixels: &[[f32; 4]], size: [u32; 2], x: i32, y: i32) -> [f32; 4] {
    if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
        [0.0; 4]
    } else {
        pixels[(y as u32 * size[0] + x as u32) as usize]
    }
}
fn convolve(
    source: &[[f32; 4]],
    size: [u32; 2],
    weights: &[f32],
    axis: usize,
) -> Result<Vec<[f32; 4]>, GpuError> {
    let radius = weights.len() as i32 / 2;
    let norm: f32 = weights.iter().sum();
    let mut output = vec![[0.0; 4]; source.len()];
    for y in 0..size[1] as i32 {
        for x in 0..size[0] as i32 {
            let mut result = [0.0; 4];
            for (i, &w) in weights.iter().enumerate() {
                let d = i as i32 - radius;
                let p = load(
                    source,
                    size,
                    x + if axis == 0 { d } else { 0 },
                    y + if axis == 1 { d } else { 0 },
                );
                for c in 0..4 {
                    result[c] += p[c] * w;
                }
            }
            output[(y as u32 * size[0] + x as u32) as usize] = result.map(|v| v / norm);
        }
    }
    validate_surface_pixels(&output)?;
    Ok(output)
}
fn bilinear(source: &[[f32; 4]], size: [u32; 2], p: [f32; 2]) -> [f32; 4] {
    let x = p[0].floor() as i32;
    let y = p[1].floor() as i32;
    let f = [p[0] - x as f32, p[1] - y as f32];
    let a = load(source, size, x, y);
    let b = load(source, size, x + 1, y);
    let c = load(source, size, x, y + 1);
    let d = load(source, size, x + 1, y + 1);
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * f[0];
        let bottom = c[i] + (d[i] - c[i]) * f[0];
        top + (bottom - top) * f[1]
    })
}
pub(crate) fn apply_reference(
    source: &[[f32; 4]],
    size: [u32; 2],
    effect: &PixelEffect,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    effect
        .validate()
        .map_err(|_| GpuError::InvalidInput("invalid effect parameters"))?;
    let [sx, sy] = effect.sigma();
    let horizontal = convolve(source, size, &kernel(sx)?, 0)?;
    let blurred = convolve(&horizontal, size, &kernel(sy)?, 1)?;
    if let PixelEffect::DropShadow { offset, .. } = effect {
        let color = shadow_color(effect, working);
        let mut output = source.to_vec();
        for y in 0..size[1] {
            for x in 0..size[0] {
                let alpha =
                    bilinear(&blurred, size, [x as f32 - offset[0], y as f32 - offset[1]])[3];
                let i = (y * size[0] + x) as usize;
                output[i] = color::source_over(source[i], color.map(|v| v * alpha));
            }
        }
        validate_surface_pixels(&output)?;
        Ok(output)
    } else {
        Ok(blurred)
    }
}
