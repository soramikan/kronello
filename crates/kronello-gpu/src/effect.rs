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
/// Straight working-space RGB of a premultiplied pixel; transparent texels
/// have undefined chroma and report black.
fn straight(p: [f32; 4]) -> [f32; 3] {
    if p[3] > 0.0 {
        [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
    } else {
        [0.0; 3]
    }
}
/// Working-space luma weights for Y and normalized linear Cb/Cr (half-scale,
/// matching the effect shader).
pub(crate) fn luma_weights(working: WorkingSpace) -> [f32; 3] {
    crate::scene::luma_weights(working)
}
/// Normalized linear-light chroma: Cb = (B-Y)/(2(1-wb)), Cr = (R-Y)/(2(1-wr)).
fn chroma(rgb: [f32; 3], w: [f32; 3]) -> [f32; 2] {
    let y = rgb[0] * w[0] + rgb[1] * w[1] + rgb[2] * w[2];
    [
        (rgb[2] - y) * (0.5 / (1.0 - w[2])),
        (rgb[0] - y) * (0.5 / (1.0 - w[0])),
    ]
}
/// Key color converted to straight working-space chroma coordinates.
pub(crate) fn key_chroma(effect: &PixelEffect, working: WorkingSpace) -> [f32; 2] {
    let PixelEffect::ChromaKey { key_color, .. } = effect else {
        return [0.0; 2];
    };
    let c0 = key_color.components();
    let space = match key_color.space() {
        kronello_model::ColorSpace::Srgb => crate::InputSpace::Srgb,
        kronello_model::ColorSpace::LinearRec709 => crate::InputSpace::LinearRec709,
        kronello_model::ColorSpace::LinearRec2020 => crate::InputSpace::LinearRec2020,
    };
    let rgba = color::to_working(
        [
            c0.r.get() as f32,
            c0.g.get() as f32,
            c0.b.get() as f32,
            c0.alpha.get() as f32,
        ],
        space,
        working,
    );
    chroma(straight(rgba), luma_weights(working))
}
/// FX-005 matte extraction (ADR-0115): binary key matte in alpha. `distance`
/// is the normalized Cb/Cr or luminance distance to the keyed value;
/// `cutoff` is `similarity * sqrt(0.5)` for chroma or `tolerance` for luma.
fn matte_extract(
    source: &[[f32; 4]],
    effect: &PixelEffect,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    let w = luma_weights(working);
    let key = key_chroma(effect, working);
    let chroma_cut = |similarity: f32| similarity * 0.5_f32.sqrt();
    let matte = |p: [f32; 4]| -> f32 {
        let s = straight(p);
        match effect {
            PixelEffect::ChromaKey { similarity, .. } => {
                let c = chroma(s, w);
                let d =
                    ((c[0] - key[0]) * (c[0] - key[0]) + (c[1] - key[1]) * (c[1] - key[1])).sqrt();
                if d >= chroma_cut(*similarity) {
                    1.0
                } else {
                    0.0
                }
            }
            PixelEffect::LumaKey {
                key_luma,
                tolerance,
                ..
            } => {
                let l = s[0] * w[0] + s[1] * w[1] + s[2] * w[2];
                if (l - key_luma).abs() >= *tolerance {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    };
    let mut output = vec![[0.0; 4]; source.len()];
    for (i, &p) in source.iter().enumerate() {
        output[i] = [0.0, 0.0, 0.0, matte(p)];
    }
    surface_pixels(&output)
}
/// FX-005 separable matte erosion (ADR-0115): Chebyshev min filter with
/// linear interpolation between the floor and ceil radii.
fn matte_erode(
    source: &[[f32; 4]],
    size: [u32; 2],
    shrink: f32,
    axis: usize,
) -> Result<Vec<[f32; 4]>, GpuError> {
    if shrink <= 0.0 {
        return Ok(source.to_vec());
    }
    let inner = shrink.floor() as i32;
    let frac = shrink - inner as f32;
    let outer = inner + 1;
    let mut output = vec![[0.0; 4]; source.len()];
    for y in 0..size[1] as i32 {
        for x in 0..size[0] as i32 {
            let mut min_inner = f32::MAX;
            let mut min_outer = f32::MAX;
            for d in -outer..=outer {
                let (sx, sy) = if axis == 0 { (x + d, y) } else { (x, y + d) };
                let a = load(source, size, sx, sy)[3];
                min_outer = min_outer.min(a);
                if d.abs() <= inner {
                    min_inner = min_inner.min(a);
                }
            }
            let m = min_inner * (1.0 - frac) + min_outer * frac;
            output[(y as u32 * size[0] + x as u32) as usize] = [0.0, 0.0, 0.0, m];
        }
    }
    surface_pixels(&output)
}
/// FX-005 key composite (ADR-0115): despill straight working color, then
/// premultiply by `source_alpha * matte` so transparent input stays clear.
fn key_composite(
    matte: &[[f32; 4]],
    original: &[[f32; 4]],
    effect: &PixelEffect,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    let w = luma_weights(working);
    let key = key_chroma(effect, working);
    let kmag = (key[0] * key[0] + key[1] * key[1]).sqrt();
    let mut output = vec![[0.0; 4]; original.len()];
    for (i, (&m, &p)) in matte.iter().zip(original).enumerate() {
        let a = p[3] * m[3];
        let mut s = straight(p);
        if let PixelEffect::ChromaKey { spill, .. } = effect
            && kmag > 1e-6
        {
            // Clamp the chroma component along the key axis at the key's own
            // magnitude; `spill` scales how much excess is removed.
            let c = chroma(s, w);
            let axis = [key[0] / kmag, key[1] / kmag];
            let excess = (c[0] * axis[0] + c[1] * axis[1] - kmag).max(0.0) * spill;
            let cb = c[0] - axis[0] * excess;
            let cr = c[1] - axis[1] * excess;
            let db = (cb - c[0]) * 2.0 * (1.0 - w[2]);
            let dr = (cr - c[1]) * 2.0 * (1.0 - w[0]);
            s = [s[0] + dr, s[1] - (w[0] * dr + w[2] * db) / w[1], s[2] + db];
        }
        output[i] = [s[0] * a, s[1] * a, s[2] * a, a];
    }
    surface_pixels(&output)
}
/// FX-005/FX-006 CPU oracle chains, matching the WGSL ops one-to-one
/// (ADR-0115). Every intermediate is rounded to binary16 like the surfaces.
fn apply_standard(
    source: Vec<[f32; 4]>,
    size: [u32; 2],
    effect: &PixelEffect,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    let w = luma_weights(working);
    match effect {
        PixelEffect::ChromaKey {
            edge_shrink,
            edge_feather,
            ..
        }
        | PixelEffect::LumaKey {
            edge_shrink,
            edge_feather,
            ..
        } => {
            let mut matte = matte_extract(&source, effect, working)?;
            matte = matte_erode(&matte, size, edge_shrink[0], 0)?;
            matte = matte_erode(&matte, size, edge_shrink[1], 1)?;
            if edge_feather[0] > 0.0 {
                matte = convolve(&matte, size, &kernel(edge_feather[0])?, 0)?;
            }
            if edge_feather[1] > 0.0 {
                matte = convolve(&matte, size, &kernel(edge_feather[1])?, 1)?;
            }
            key_composite(&matte, &source, effect, working)
        }
        PixelEffect::Glow {
            threshold,
            radius,
            intensity,
        } => {
            let mut bloom = vec![[0.0; 4]; source.len()];
            for (i, &p) in source.iter().enumerate() {
                let s = straight(p);
                let l = s[0] * w[0] + s[1] * w[1] + s[2] * w[2];
                if l > *threshold {
                    bloom[i] = p;
                }
            }
            let mut bloom = surface_pixels(&bloom)?;
            if radius[0] > 0.0 {
                bloom = convolve(&bloom, size, &kernel(radius[0])?, 0)?;
            }
            if radius[1] > 0.0 {
                bloom = convolve(&bloom, size, &kernel(radius[1])?, 1)?;
            }
            let mut output = source.clone();
            for (o, b) in output.iter_mut().zip(&bloom) {
                let coverage = (b[3] * intensity).min(1.0);
                for c in 0..3 {
                    o[c] = (o[c] + b[c] * intensity).clamp(-65504.0, 65504.0);
                }
                o[3] = (o[3] + coverage * (1.0 - o[3])).clamp(0.0, 1.0);
            }
            surface_pixels(&output)
        }
        PixelEffect::Sharpen { amount, radius } => {
            let mut blurred = source.clone();
            if radius[0] > 0.0 {
                blurred = convolve(&blurred, size, &kernel(radius[0])?, 0)?;
            }
            if radius[1] > 0.0 {
                blurred = convolve(&blurred, size, &kernel(radius[1])?, 1)?;
            }
            let mut output = source.clone();
            for (o, b) in output.iter_mut().zip(&blurred) {
                for c in 0..3 {
                    o[c] = (o[c] + amount * (o[c] - b[c])).clamp(-65504.0, 65504.0);
                }
                o[3] = (o[3] + amount * (o[3] - b[3])).clamp(0.0, 1.0);
            }
            surface_pixels(&output)
        }
        PixelEffect::Vignette {
            amount,
            midpoint,
            feather,
            roundness,
        } => {
            let mut output = vec![[0.0; 4]; source.len()];
            for y in 0..size[1] as i32 {
                for x in 0..size[0] as i32 {
                    // Normalized signed position over the output surface.
                    let nx = (x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0;
                    let ny = (y as f32 + 0.5) / size[1] as f32 * 2.0 - 1.0;
                    let rect = nx.abs().max(ny.abs());
                    let ellipse = (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2;
                    let d = rect + (ellipse - rect) * roundness;
                    let t = ((d - midpoint) / feather.max(1e-6)).clamp(0.0, 1.0);
                    let s = t * t * (3.0 - 2.0 * t);
                    let f = 1.0 - amount * s;
                    let p = source[(y as u32 * size[0] + x as u32) as usize];
                    output[(y as u32 * size[0] + x as u32) as usize] =
                        [p[0] * f, p[1] * f, p[2] * f, p[3]];
                }
            }
            surface_pixels(&output)
        }
        PixelEffect::CornerPin { pins, source: quad } => {
            let Some(rect) = quad else {
                return Ok(vec![[0.0; 4]; source.len()]);
            };
            if rect.max[0] <= rect.min[0] || rect.max[1] <= rect.min[1] {
                return Ok(vec![[0.0; 4]; source.len()]);
            }
            let m = kronello_render::corner_pin_inverse(*rect, *pins).map_err(|_| {
                GpuError::InvalidInput("corner pin requires a nondegenerate convex quad")
            })?;
            let mut output = vec![[0.0; 4]; source.len()];
            for y in 0..size[1] as i32 {
                for x in 0..size[0] as i32 {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let hx = m[0][0] * fx + m[0][1] * fy + m[0][2];
                    let hy = m[1][0] * fx + m[1][1] * fy + m[1][2];
                    let hw = m[2][0] * fx + m[2][1] * fy + m[2][2];
                    let (ex, ey) = (hx / hw, hy / hw);
                    let inside = hw > 0.0
                        && ex >= rect.min[0] as f32
                        && ex <= rect.max[0] as f32
                        && ey >= rect.min[1] as f32
                        && ey <= rect.max[1] as f32;
                    if inside {
                        output[(y as u32 * size[0] + x as u32) as usize] =
                            bilinear(&source, size, [ex - 0.5, ey - 0.5]);
                    }
                }
            }
            surface_pixels(&output)
        }
        _ => unreachable!("not a standard effect"),
    }
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
    // FX-005/FX-006 multi-pass chains (ADR-0115).
    if effect.is_standard() {
        return apply_standard(source, size, effect, working);
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
