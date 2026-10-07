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
    let Some((_, c, opacity)) = effect.shadow() else {
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
/// A surface boundary uses binary16 round-to-nearest, ties-to-even. Normalize
/// RGB only when stored alpha becomes zero, never at the external epsilon.
pub(crate) fn surface_pixels(pixels: &[[f32; 4]]) -> Result<Vec<[f32; 4]>, GpuError> {
    validate_surface_pixels(pixels)?;
    Ok(pixels
        .iter()
        .map(|p| {
            let p = p.map(|v| half::f16::from_f32(v).to_f32());
            if p[3] == 0.0 { [0.0; 4] } else { p }
        })
        .collect())
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
    surface_pixels(&output)
}
fn bilinear(source: &[[f32; 4]], size: [u32; 2], p: [f32; 2]) -> [f32; 4] {
    let x = p[0].floor() as i32;
    let y = p[1].floor() as i32;
    let f = [p[0] - x as f32, p[1] - y as f32];
    bilinear_parts(source, size, [x, y], f)
}
fn bilinear_parts(source: &[[f32; 4]], size: [u32; 2], base: [i32; 2], f: [f32; 2]) -> [f32; 4] {
    let [x, y] = base;
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
    effect.validate().map_err(|e| match e {
        kronello_render::RenderError::UnsupportedFeature(_) => {
            GpuError::UnsupportedFeature("effect transform or kernel budget")
        }
        _ => GpuError::InvalidInput("invalid effect parameters"),
    })?;
    let source = surface_pixels(source)?;
    // COLOR-002 pointwise pass: identical f32 math on CPU and in WGSL.
    if effect.is_pointwise_color() {
        let output: Vec<[f32; 4]> = source
            .iter()
            .map(|&p| color::apply_color(p, effect))
            .collect();
        return surface_pixels(&output);
    }
    let blurred = if let Some(c) = effect.covariance() {
        let taps = kronello_render::affine_gaussian_kernel(c).map_err(|_| {
            GpuError::UnsupportedFeature("affine Gaussian covariance or kernel budget")
        })?;
        let norm: f32 = taps.iter().map(|t| t.weight).sum();
        let mut output = vec![[0.0; 4]; source.len()];
        for y in 0..size[1] as i32 {
            for x in 0..size[0] as i32 {
                let mut result = [0.0; 4];
                for tap in &taps {
                    let p = load(&source, size, x + tap.offset[0], y + tap.offset[1]);
                    for ch in 0..4 {
                        result[ch] += p[ch] * tap.weight;
                    }
                }
                output[(y as u32 * size[0] + x as u32) as usize] = result.map(|v| v / norm);
            }
        }
        surface_pixels(&output)?
    } else {
        let [sx, sy] = effect.sigma();
        let horizontal = convolve(&source, size, &kernel(sx)?, 0)?;
        convolve(&horizontal, size, &kernel(sy)?, 1)?
    };
    if let Some((offset, _, _)) = effect.shadow() {
        let color = shadow_color(effect, working);
        let mut output = source.to_vec();
        for y in 0..size[1] {
            for x in 0..size[0] {
                let alpha = if effect.covariance().is_some() {
                    // Separate integer translation from fractional taps so
                    // changing the tile origin cannot change interpolation.
                    let shift = offset.map(|v| (-v).floor());
                    let fraction = [0, 1].map(|i| -offset[i] - shift[i]);
                    bilinear_parts(
                        &blurred,
                        size,
                        [x as i32 + shift[0] as i32, y as i32 + shift[1] as i32],
                        fraction,
                    )[3]
                } else {
                    bilinear(&blurred, size, [x as f32 - offset[0], y as f32 - offset[1]])[3]
                };
                let i = (y * size[0] + x) as usize;
                output[i] = color::source_over(source[i], color.map(|v| v * alpha));
            }
        }
        surface_pixels(&output)
    } else {
        Ok(blurred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_fx_binary16_rounding_and_alpha_underflow_are_explicit() {
        let input = [
            [0.3333, -0.3333, 2.0007, 0.6501],
            [0.001, 0.0, 0.0, 1.0 / 67_108_864.0],
        ];
        let rounded = surface_pixels(&input).unwrap();
        assert_ne!(rounded[0], input[0]);
        assert_eq!(rounded[0], [0.33325195, -0.33325195, 2.0, 0.64990234]);
        assert_eq!(rounded[1], [0.0; 4]);
        // Positive half subnormal alpha retains RGB despite external epsilon.
        let tiny = surface_pixels(&[[0.001, 0.0, 0.0, 1.0 / 16_777_216.0]]).unwrap();
        assert!(tiny[0][0] > 0.0);
        assert_eq!(tiny[0][3], 1.0 / 16_777_216.0);
    }
    #[test]
    fn cpu_fx_vertical_pass_consumes_rounded_horizontal_surface() {
        let size = [3, 3];
        let source = vec![[0.25, 0.0, 0.0, 0.65]; 9];
        let stored = surface_pixels(&source).unwrap();
        let weights = kernel(1.2).unwrap();
        let horizontal = convolve(&stored, size, &weights, 0).unwrap();
        let norm: f32 = weights.iter().sum();
        let r = weights.len() as i32 / 2;
        let mut value = [0.0; 4];
        let mut full_precision = 0.0;
        for (i, &w) in weights.iter().enumerate() {
            let p = load(&horizontal, size, 1, 1 + i as i32 - r);
            for c in 0..4 {
                value[c] += p[c] * w;
            }
            // Analytic unrounded horizontal/vertical response to a flat 3x3 patch.
            if (0..3).contains(&(1 + i as i32 - r)) {
                let horizontal_sum: f32 = weights[(r - 1) as usize..=(r + 1) as usize].iter().sum();
                full_precision += 0.65 * horizontal_sum / norm * w;
            }
        }
        let expected = surface_pixels(&[value.map(|v| v / norm)]).unwrap()[0];
        let effect = PixelEffect::GaussianBlur { sigma: [1.2; 2] };
        let actual =
            apply_reference(&source, size, &effect, WorkingSpace::LinearRec709).unwrap()[4];
        assert_eq!(actual, expected);
        assert_ne!(actual[3], full_precision / norm);
        assert_eq!(actual.map(|v| half::f16::from_f32(v).to_f32()), actual);
    }
}
