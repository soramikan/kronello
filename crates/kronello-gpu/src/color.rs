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

/// Source-over with a separable linear working-space blend function. RGB remains
/// extended range; transparent inputs never require division by zero alpha.
pub fn blend(src: [f32; 4], dst: [f32; 4], mode: kronello_model::BlendMode) -> [f32; 4] {
    use kronello_model::BlendMode;
    if mode == BlendMode::Normal {
        return source_over(src, dst);
    }
    let [sa, da] = [src[3], dst[3]];
    let mut out = [0.; 4];
    for i in 0..3 {
        // Algebraically eliminate unpremultiplication. This remains defined at
        // zero alpha and avoids overflow from dividing HDR RGB by tiny alpha.
        out[i] = match mode {
            BlendMode::Multiply => src[i] * (1. - da) + dst[i] * (1. - sa) + src[i] * dst[i],
            BlendMode::Screen => src[i] + dst[i] - src[i] * dst[i],
            BlendMode::Normal => unreachable!(),
        };
    }
    out[3] = sa + da * (1. - sa);
    out
}
