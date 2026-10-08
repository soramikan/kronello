//! Independent CPU reference for ADR-0044. RGB matrices use D65 for both spaces.
use crate::{GpuError, InputSpace, WorkingSpace};
pub const ALPHA_EPSILON: f32 = 1.0 / 65536.0;
pub const REC709_TO_REC2020: [[f32; 3]; 3] = [
    [0.627404, 0.329282, 0.0433136],
    [0.0690973, 0.9195404, 0.0113623],
    [0.0163914, 0.0880133, 0.8955953],
];
pub const REC2020_TO_REC709: [[f32; 3]; 3] = [
    [1.660491, -0.5876411, -0.0728499],
    [-0.1245505, 1.1328999, -0.0083494],
    [-0.0181508, -0.1005789, 1.1187297],
];
pub fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub fn srgb_encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
pub fn convert_primaries(rgb: [f32; 3], from: WorkingSpace, to: WorkingSpace) -> [f32; 3] {
    if from == to {
        return rgb;
    }
    let matrix = if from == WorkingSpace::LinearRec709 {
        REC709_TO_REC2020
    } else {
        REC2020_TO_REC709
    };
    matrix.map(|row| row.iter().zip(rgb).map(|(a, b)| a * b).sum())
}
pub fn validate_straight(pixel: [f32; 4], space: InputSpace) -> Result<(), GpuError> {
    if pixel.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&pixel[3]) {
        return Err(GpuError::InvalidInput(
            "finite RGB and alpha in [0, 1] required",
        ));
    }
    if space == InputSpace::Srgb && pixel[..3].iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err(GpuError::InvalidInput("sRGB input must be in [0, 1]"));
    }
    Ok(())
}
pub fn premultiply(p: [f32; 4]) -> [f32; 4] {
    if p[3] == 0.0 {
        [0.0; 4]
    } else {
        [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
    }
}
/// External boundary only: preserve alpha and discard unstable straight RGB.
pub fn unpremultiply_external(p: [f32; 4]) -> [f32; 4] {
    if p[3] <= ALPHA_EPSILON {
        [0.0, 0.0, 0.0, p[3]]
    } else {
        [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
    }
}
pub fn source_over(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| src[i] + dst[i] * (1.0 - src[3]))
}
pub fn to_working(p: [f32; 4], space: InputSpace, working: WorkingSpace) -> [f32; 4] {
    let mut rgb = [p[0], p[1], p[2]];
    if space == InputSpace::Srgb {
        rgb = rgb.map(srgb_decode);
    }
    let from = if space == InputSpace::LinearRec2020 {
        WorkingSpace::LinearRec2020
    } else {
        WorkingSpace::LinearRec709
    };
    let rgb = convert_primaries(rgb, from, working);
    premultiply([rgb[0], rgb[1], rgb[2], p[3]])
}

// ---- FX-003 W3C separable/non-separable blend functions (ADR-0109) ----
// All functions operate on straight channel values; HDR and negative inputs
// are never clamped. The division guards below define the zero-alpha and
// singular-function behavior shared with scene.wgsl.

/// Dodge/burn/vivid-light divide-by-x contract: negative or non-finite
/// intermediate results map to ±∞ then fold into the guarding comparisons.
fn dodge(b: f32, s: f32) -> f32 {
    if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}
fn burn(b: f32, s: f32) -> f32 {
    if s <= 0.0 {
        0.0
    } else {
        1.0 - (1.0 - b).min(s) / s
    }
}
fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 {
            ((16.0 * b - 12.0) * b + 4.0) * b
        } else {
            b.sqrt()
        };
        b + (2.0 * s - 1.0) * (d - b)
    }
}
/// W3C overlay with the branch on `b`: `if b <= 0.5 { 2bs } else ...`.
/// Overlay(B,C) branches on B; hard-light(B,C) is overlay(C,B).
fn overlay_branch(b: f32, s: f32) -> f32 {
    if b <= 0.5 {
        2.0 * b * s
    } else {
        1.0 - 2.0 * (1.0 - b) * (1.0 - s)
    }
}
/// Separable per-channel function dispatch. Operation ids are the fixed
/// FX-003 table; mode order here must match `blend_rgb`.
fn blend_channel(b: f32, s: f32, mode: kronello_model::BlendMode) -> f32 {
    use kronello_model::BlendMode;
    match mode {
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::ColorDodge => dodge(b, s),
        BlendMode::ColorBurn => burn(b, s),
        BlendMode::HardLight => overlay_branch(s, b),
        BlendMode::SoftLight => soft_light(b, s),
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        BlendMode::Overlay => overlay_branch(b, s),
        BlendMode::LinearDodge => b + s,
        BlendMode::LinearBurn => b + s - 1.0,
        BlendMode::VividLight => {
            if s <= 0.5 {
                burn(b, 2.0 * s)
            } else {
                dodge(b, 2.0 * (s - 0.5))
            }
        }
        BlendMode::LinearLight => b + 2.0 * s - 1.0,
        _ => unreachable!("non-separable and normal modes bypass blend_channel"),
    }
}
/// W3C non-separable helpers. Rec.601 luma, no gamut clipping.
fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    c.map(|v| v + d)
}
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    // Explicit index selection (first minimum, last maximum) avoids a sort and
    // is mirrored instruction-for-instruction in scene.wgsl.
    let mut lo = 0usize;
    let mut hi = 0usize;
    for i in 1..3 {
        if c[i] < c[lo] {
            lo = i;
        }
        if c[i] >= c[hi] {
            hi = i;
        }
    }
    let mid = 3 - lo - hi;
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}
fn sat(c: [f32; 3]) -> f32 {
    let lo = c.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = c.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    hi - lo
}
fn blend_rgb(cb: [f32; 3], cs: [f32; 3], mode: kronello_model::BlendMode) -> [f32; 3] {
    use kronello_model::BlendMode;
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => std::array::from_fn(|i| blend_channel(cb[i], cs[i], mode)),
    }
}
/// Source-over with the FX-003 blend function. Straight RGB values are
/// recovered from premultiplied operands only where alpha is positive; the
/// separable W3C forms apply on the straight domain (ADR-0109). RGB remains
/// extended range; alpha output follows Porter-Duff source-over.
pub fn blend(src: [f32; 4], dst: [f32; 4], mode: kronello_model::BlendMode) -> [f32; 4] {
    use kronello_model::BlendMode;
    if mode == BlendMode::Normal {
        return source_over(src, dst);
    }
    let [sa, da] = [src[3], dst[3]];
    // Straight operands; a zero alpha defines its color as zero so the
    // composite formula degenerates to the surviving term.
    let cs = if sa > 0.0 {
        [src[0] / sa, src[1] / sa, src[2] / sa]
    } else {
        [0.0; 3]
    };
    let cb = if da > 0.0 {
        [dst[0] / da, dst[1] / da, dst[2] / da]
    } else {
        [0.0; 3]
    };
    let b = blend_rgb(cb, cs, mode);
    let mut out = [0.; 4];
    for i in 0..3 {
        out[i] = src[i] * (1. - da) + dst[i] * (1. - sa) + sa * da * b[i];
    }
    out[3] = sa + da * (1. - sa);
    out
}

// ---- COLOR-002 pointwise corrections (ADR-0108) ----
// All ops act on premultiplied working-space RGB and preserve alpha exactly.
// Negative and HDR values are never clamped.

fn levels_channel(
    v: f32,
    in_black: f32,
    in_white: f32,
    gamma: f32,
    out_black: f32,
    out_white: f32,
) -> f32 {
    let n = (v - in_black) / (in_white - in_black);
    // sign-preserving pow keeps the extrapolated domain defined for gamma 1/y.
    let g = if n < 0.0 {
        -(-n).powf(1.0 / gamma)
    } else {
        n.powf(1.0 / gamma)
    };
    out_black + g * (out_white - out_black)
}

/// Fritsch–Carlson tangents for a strictly increasing control table. This is
/// the exact monotone-cubic (PCHIP) rule the WGSL kernel mirrors.
pub fn monotone_cubic_tangents(points: &[[f32; 2]]) -> Vec<f32> {
    let n = points.len();
    debug_assert!(n >= 2);
    let mut d = vec![0.0f32; n];
    let mut h = vec![0.0f32; n - 1];
    let mut s = vec![0.0f32; n - 1];
    for i in 0..n - 1 {
        h[i] = points[i + 1][0] - points[i][0];
        s[i] = (points[i + 1][1] - points[i][1]) / h[i];
    }
    if n == 2 {
        d[0] = s[0];
        d[1] = s[0];
        return d;
    }
    let endpoint = |h0: f32, h1: f32, s0: f32, s1: f32| -> f32 {
        let d = ((2.0 * h0 + h1) * s0 - h0 * s1) / (h0 + h1);
        if d.signum() != s0.signum() {
            0.0
        } else if s0.signum() != s1.signum() && d.abs() > 3.0 * s0.abs() {
            3.0 * s0
        } else {
            d
        }
    };
    d[0] = endpoint(h[0], h[1], s[0], s[1]);
    d[n - 1] = endpoint(h[n - 2], h[n - 3], s[n - 2], s[n - 3]);
    for i in 1..n - 1 {
        if s[i - 1] * s[i] <= 0.0 {
            d[i] = 0.0;
        } else {
            let w1 = 2.0 * h[i] + h[i - 1];
            let w2 = h[i] + 2.0 * h[i - 1];
            d[i] = (w1 + w2) / (w1 / s[i - 1] + w2 / s[i]);
        }
    }
    d
}
/// Evaluate the monotone cubic at x; outside the knot range the endpoint
/// tangent extends linearly.
pub fn monotone_cubic(points: &[[f32; 2]], tangents: &[f32], x: f32) -> f32 {
    let n = points.len();
    let first = points[0];
    let last = points[n - 1];
    if x <= first[0] {
        return first[1] + tangents[0] * (x - first[0]);
    }
    if x >= last[0] {
        return last[1] + tangents[n - 1] * (x - last[0]);
    }
    // Segment i satisfies xs[i] <= x < xs[i+1]; the early returns above pin i.
    let mut i = 0usize;
    for (k, p) in points.iter().enumerate().take(n - 1).skip(1) {
        if x >= p[0] {
            i = k;
        }
    }
    let h = points[i + 1][0] - points[i][0];
    let t = (x - points[i][0]) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    let (a, b, c, e) = (
        2.0 * t3 - 3.0 * t2 + 1.0,
        t3 - 2.0 * t2 + t,
        -2.0 * t3 + 3.0 * t2,
        t3 - t2,
    );
    a * points[i][1] + b * h * tangents[i] + c * points[i + 1][1] + e * h * tangents[i + 1]
}

// Working-space HSL on premultiplied RGB. The canonical S_L denominator
// 1-|2L-1| is used so that the extension to negative and HDR values stays
// self-consistent (saturating to an achromatic mapping only when the
// denominator degenerates at L == 1 or L == 0 with nonzero chroma).
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let lo = r.min(g).min(b);
    let hi = r.max(g).max(b);
    let l = 0.5 * (lo + hi);
    if hi == lo {
        return (0.0, 0.0, l);
    }
    let d = hi - lo;
    let denom = 1.0 - (2.0 * l - 1.0).abs();
    let s = if denom == 0.0 { 0.0 } else { d / denom };
    let h = if hi == r {
        (g - b) / d
    } else if hi == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h.rem_euclid(6.0) * 60.0, s, l)
}
fn hsl_to_rgb(h_degrees: f32, s: f32, l: f32) -> [f32; 3] {
    // No saturation clamp: a negative multiplier deterministically inverts
    // chroma, preserving the unclamped HDR contract.
    let c = s * (1.0 - (2.0 * l - 1.0).abs());
    let m = l - 0.5 * c;
    if c == 0.0 {
        return [l; 3];
    }
    let h2 = (h_degrees / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (h2 % 2.0 - 1.0).abs());
    let (r, g, b) = match h2 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}
/// COLOR-002 pointwise kernel shared by the CPU reference and (semantically)
/// the WGSL pass. Input and output are premultiplied working-space pixels;
/// alpha is returned unchanged.
pub fn apply_color(p: [f32; 4], effect: &kronello_render::PixelEffect) -> [f32; 4] {
    use kronello_render::PixelEffect;
    let rgb = match effect {
        PixelEffect::ColorExposure { exposure, offset } => {
            let k = (*exposure).exp2();
            [p[0] * k + offset, p[1] * k + offset, p[2] * k + offset]
        }
        PixelEffect::ColorLevels {
            in_black,
            in_white,
            gamma,
            out_black,
            out_white,
        } => [
            levels_channel(p[0], *in_black, *in_white, *gamma, *out_black, *out_white),
            levels_channel(p[1], *in_black, *in_white, *gamma, *out_black, *out_white),
            levels_channel(p[2], *in_black, *in_white, *gamma, *out_black, *out_white),
        ],
        PixelEffect::ColorCurves { points } => {
            let tangents = monotone_cubic_tangents(points);
            [
                monotone_cubic(points, &tangents, p[0]),
                monotone_cubic(points, &tangents, p[1]),
                monotone_cubic(points, &tangents, p[2]),
            ]
        }
        PixelEffect::ColorHsl {
            hue_shift,
            saturation,
            lightness,
        } => {
            let (h, s, l) = rgb_to_hsl(p[0], p[1], p[2]);
            hsl_to_rgb(h + hue_shift, s * saturation, l + lightness)
        }
        // COLOR-003 (ADR-0113): the sample position is the straight working
        // RGB; the result blends back into premultiplied space and preserves
        // alpha. Domain-external values clamp inside `CubeLut::sample`.
        PixelEffect::ColorLut { lut, intensity } => {
            let straight = if p[3] > ALPHA_EPSILON {
                [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
            } else {
                [0.0; 3]
            };
            let mapped = lut.sample(straight);
            let mixed: [f32; 3] =
                std::array::from_fn(|i| straight[i] + (mapped[i] - straight[i]) * intensity);
            [mixed[0] * p[3], mixed[1] * p[3], mixed[2] * p[3]]
        }
        _ => unreachable!("not a pointwise color effect"),
    };
    [rgb[0], rgb[1], rgb[2], p[3]]
}
