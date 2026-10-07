//! Output-space effect contract, kernel and backward region requests.
use crate::RenderError;
use kronello_model::{Color, ResolvedEffect};
use serde::{Deserialize, Serialize};

pub const AFFINE_EFFECT_KERNEL_VERSION: &str = "fx002-affine-ellipse-lattice-rne16-v2";
pub const AFFINE_KERNEL_MAX_CANDIDATES: usize = 65_536;
pub const EFFECT_KERNEL_VERSION: &str = "fx001-separable-gaussian-transparent-rne16-v1";
/// Half-open rectangle on the original requested output pixel lattice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}
impl PixelBounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
    pub fn expand(self, halo: [f64; 2]) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i] - halo[i]),
            max: std::array::from_fn(|i| self.max[i] + halo[i]),
        }
    }
    pub fn translate(self, offset: [f64; 2]) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i] + offset[i]),
            max: std::array::from_fn(|i| self.max[i] + offset[i]),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NodeBounds {
    pub ink_bounds: Option<PixelBounds>,
    pub visual_bounds: Option<PixelBounds>,
}
/// Sigma/offset in output pixels. Colors retain straight tags until execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PixelEffect {
    /// Symmetric covariance [xx, xy, yy] in output pixels squared.
    AffineGaussianBlur {
        covariance: [f64; 3],
    },
    AffineDropShadow {
        covariance: [f64; 3],
        offset: [f32; 2],
        color: Color,
        opacity: f32,
    },
    GaussianBlur {
        sigma: [f32; 2],
    },
    DropShadow {
        sigma: [f32; 2],
        offset: [f32; 2],
        color: Color,
        opacity: f32,
    },
    /// COLOR-002 pointwise correction in premultiplied working space:
    /// rgb = rgb * 2^exposure + offset. Alpha is preserved; HDR and
    /// negative values are never clamped (ADR-0108).
    ColorExposure {
        exposure: f32,
        offset: f32,
    },
    /// in_white > in_black and gamma > 0 are enforced at resolution and
    /// re-checked in validate.
    ColorLevels {
        in_black: f32,
        in_white: f32,
        gamma: f32,
        out_black: f32,
        out_white: f32,
    },
    /// Monotone-cubic control points, x strictly increasing within [0,1].
    ColorCurves {
        points: Vec<[f32; 2]>,
    },
    /// Working-space HSL: hue_shift in degrees, saturation multiplier,
    /// additive lightness.
    ColorHsl {
        hue_shift: f32,
        saturation: f32,
        lightness: f32,
    },
    /// COLOR-003 pointwise `.cube` LUT in working space (ADR-0113). The
    /// lattice was content-verified before reaching the DAG: `size` is
    /// document-bounded and `data` keeps red-fastest row order. Alpha is
    /// preserved; domain-normalized samples clamp to lattice endpoints.
    ColorLut {
        lut: kronello_model::CubeLut,
        intensity: f32,
    },
}
/// Kernel tag shared by all COLOR-002 v1 pointwise passes.
pub const COLOR002_KERNEL_VERSION: &str = "color002-pointwise-f16-v1";
/// Kernel tag for the COLOR-003 tetrahedral LUT pass.
pub const COLOR003_KERNEL_VERSION: &str = "color003-tetrahedral-f16-v1";
impl PixelEffect {
    /// COLOR-002 corrections are pointwise: no kernel, no neighborhood input.
    pub fn is_pointwise_color(&self) -> bool {
        matches!(
            self,
            Self::ColorExposure { .. }
                | Self::ColorLevels { .. }
                | Self::ColorCurves { .. }
                | Self::ColorHsl { .. }
                | Self::ColorLut { .. }
        )
    }
    pub fn from_design(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
        // COLOR-003 carries an asset reference, not lattice bytes; the DAG
        // builder resolves it against the snapshot luts input.
        if let ResolvedEffect::ColorLut { .. } = effect {
            return Err(RenderError::InvalidInput(
                "color lut effects require scene-resolved lattice data".into(),
            ));
        }
        // COLOR-002 parameters are resolution-checked in the model crate; the
        // pixel-space form only converts precision since nothing is spatial.
        let pointwise = match effect {
            ResolvedEffect::ColorExposure { exposure, offset } => Some(Self::ColorExposure {
                exposure: *exposure as f32,
                offset: *offset as f32,
            }),
            ResolvedEffect::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => Some(Self::ColorLevels {
                in_black: *in_black as f32,
                in_white: *in_white as f32,
                gamma: *gamma as f32,
                out_black: *out_black as f32,
                out_white: *out_white as f32,
            }),
            ResolvedEffect::ColorCurves { curve } => Some(Self::ColorCurves {
                points: curve.iter().map(|p| p.map(|v| v as f32)).collect(),
            }),
            ResolvedEffect::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => Some(Self::ColorHsl {
                hue_shift: *hue_shift as f32,
                saturation: *saturation as f32,
                lightness: *lightness as f32,
            }),
            _ => None,
        };
        if let Some(result) = pointwise {
            result.validate()?;
            return Ok(result);
        }
        if let ResolvedEffect::AffineGaussianBlur { sigma, linear }
        | ResolvedEffect::AffineDropShadow { sigma, linear, .. } = effect
        {
            validate_affine_linear(*linear)?;
            if !sigma.is_finite()
                || *sigma < 0.0
                || scale.iter().any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err(RenderError::InvalidInput(
                    "invalid affine effect scale/sigma".into(),
                ));
            }
            let rows = [0, 1].map(|i| linear[i].map(|v| v * scale[i] * sigma));
            let covariance = [
                rows[0][0] * rows[0][0] + rows[0][1] * rows[0][1],
                rows[0][0] * rows[1][0] + rows[0][1] * rows[1][1],
                rows[1][0] * rows[1][0] + rows[1][1] * rows[1][1],
            ];
            if *sigma > 0.0 && covariance == [0.0; 3] {
                return Err(RenderError::UnsupportedFeature(
                    "affine Gaussian covariance underflow".into(),
                ));
            }
            let result = match effect {
                ResolvedEffect::AffineGaussianBlur { .. } => {
                    Self::AffineGaussianBlur { covariance }
                }
                ResolvedEffect::AffineDropShadow {
                    offset,
                    color,
                    opacity,
                    ..
                } => Self::AffineDropShadow {
                    covariance,
                    offset: [0, 1].map(|i| (offset[i] * scale[i]) as f32),
                    color: *color,
                    opacity: *opacity as f32,
                },
                _ => unreachable!(),
            };
            result.validate()?;
            return Ok(result);
        }
        let sigma = match effect {
            ResolvedEffect::GaussianBlur { sigma } | ResolvedEffect::DropShadow { sigma, .. } => {
                *sigma
            }
            _ => unreachable!(),
        };
        if !sigma.is_finite() || sigma < 0.0 || scale.iter().any(|s| !s.is_finite() || *s <= 0.0) {
            return Err(RenderError::InvalidInput(
                "invalid effect scale/sigma".into(),
            ));
        }
        let sigma = scale.map(|s| (sigma * s) as f32);
        let result = match effect {
            ResolvedEffect::GaussianBlur { .. } => Self::GaussianBlur { sigma },
            ResolvedEffect::DropShadow {
                offset,
                color,
                opacity,
                ..
            } => Self::DropShadow {
                sigma,
                offset: std::array::from_fn(|i| (offset[i] * scale[i]) as f32),
                color: *color,
                opacity: *opacity as f32,
            },
            _ => unreachable!(),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn sigma(&self) -> [f32; 2] {
        match self {
            Self::GaussianBlur { sigma } | Self::DropShadow { sigma, .. } => *sigma,
            Self::AffineGaussianBlur { covariance } | Self::AffineDropShadow { covariance, .. } => {
                [covariance[0].sqrt() as f32, covariance[2].sqrt() as f32]
            }
            _ => [0.0; 2],
        }
    }
    pub fn covariance(&self) -> Option<[f64; 3]> {
        match self {
            Self::AffineGaussianBlur { covariance } | Self::AffineDropShadow { covariance, .. } => {
                Some(*covariance)
            }
            _ => None,
        }
    }
    pub fn kernel_version(&self) -> &'static str {
        if matches!(self, Self::ColorLut { .. }) {
            COLOR003_KERNEL_VERSION
        } else if self.is_pointwise_color() {
            COLOR002_KERNEL_VERSION
        } else if self.covariance().is_some() {
            AFFINE_EFFECT_KERNEL_VERSION
        } else {
            EFFECT_KERNEL_VERSION
        }
    }
    pub fn semantic_version(&self) -> u32 {
        if self.is_pointwise_color() {
            kronello_model::COLOR_EFFECT_VERSION
        } else if self.covariance().is_some() {
            kronello_model::AFFINE_EFFECT_VERSION
        } else {
            kronello_model::EFFECT_VERSION
        }
    }
    pub fn shadow(&self) -> Option<([f32; 2], Color, f32)> {
        match self {
            Self::DropShadow {
                offset,
                color,
                opacity,
                ..
            }
            | Self::AffineDropShadow {
                offset,
                color,
                opacity,
                ..
            } => Some((*offset, *color, *opacity)),
            _ => None,
        }
    }
    pub fn halo(&self) -> [f64; 2] {
        if let Some(c) = self.covariance() {
            [c[0], c[2]].map(|v| (3.0 * v.sqrt()).ceil())
        } else {
            self.sigma().map(|s| f64::from((3.0 * s).ceil()))
        }
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.is_pointwise_color() {
            return self.validate_color();
        }
        if let Some(covariance) = self.covariance() {
            affine_gaussian_kernel(covariance)?;
        } else {
            for sigma in self.sigma() {
                gaussian_kernel(sigma)?;
            }
        }
        if let Some((offset, _, opacity)) = self.shadow()
            && (offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(&opacity))
        {
            return Err(RenderError::InvalidInput(
                "invalid shadow offset/opacity".into(),
            ));
        }
        Ok(())
    }
    /// COLOR-002 parameter contract mirrors the model-side checks.
    fn validate_color(&self) -> Result<(), RenderError> {
        let invalid = || RenderError::InvalidInput("invalid color effect parameters".into());
        match self {
            Self::ColorExposure { exposure, offset } => {
                if !exposure.is_finite() || !offset.is_finite() {
                    return Err(invalid());
                }
            }
            Self::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                if ![in_black, in_white, gamma, out_black, out_white]
                    .into_iter()
                    .all(|v| v.is_finite())
                    || in_white <= in_black
                    || *gamma <= 0.0
                {
                    return Err(invalid());
                }
            }
            Self::ColorCurves { points } => {
                if !(2..=kronello_model::CURVES_MAX_POINTS).contains(&points.len())
                    || !points
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    || !points.windows(2).all(|w| w[0][0] < w[1][0])
                {
                    return Err(invalid());
                }
            }
            Self::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => {
                if ![hue_shift, saturation, lightness]
                    .into_iter()
                    .all(|v| v.is_finite())
                {
                    return Err(invalid());
                }
            }
            Self::ColorLut { lut, intensity } => {
                if !intensity.is_finite() || !(0.0..=1.0).contains(intensity) {
                    return Err(invalid());
                }
                lut.validate()
                    .and_then(|()| lut.validate_document_size())
                    .map_err(RenderError::from)?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    /// Bilinear translation requires floor/ceil source taps in addition to blur.
    pub fn required_input(&self, output: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. } | Self::AffineGaussianBlur { .. } => output.expand(halo),
            Self::DropShadow { offset, .. } | Self::AffineDropShadow { offset, .. } => {
                let shifted = output.translate(offset.map(|v| -f64::from(v)));
                let shifted = PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                };
                output.union(shifted.expand(halo))
            }
            _ => output,
        }
    }
    pub fn output_bounds(&self, input: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. } | Self::AffineGaussianBlur { .. } => input.expand(halo),
            Self::DropShadow { offset, .. } | Self::AffineDropShadow { offset, .. } => {
                let shifted = input.expand(halo).translate(offset.map(f64::from));
                input.union(PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                })
            }
            _ => input,
        }
    }
}
/// Symmetric discrete Gaussian, normalized over [-ceil(3*sigma), +ceil(3*sigma)].
/// CPU and GPU consume these exact float32 weights; sigma zero is identity.
pub fn gaussian_kernel(sigma: f32) -> Result<Vec<f32>, RenderError> {
    if !sigma.is_finite() || sigma < 0.0 {
        return Err(RenderError::InvalidInput("invalid gaussian sigma".into()));
    }
    let radius = (3.0 * sigma).ceil();
    if radius > 1024.0 {
        return Err(RenderError::UnsupportedFeature(
            "effect kernel radius exceeds 1024 output pixels".into(),
        ));
    }
    if sigma == 0.0 {
        return Ok(vec![1.0]);
    }
    let radius = radius as i32;
    let mut weights: Vec<_> = (-radius..=radius)
        .map(|i| {
            let x = f64::from(i) / f64::from(sigma);
            (-0.5 * x * x).exp()
        })
        .collect();
    let sum: f64 = weights.iter().sum();
    for w in &mut weights {
        *w /= sum;
    }
    Ok(weights.into_iter().map(|w| w as f32).collect())
}

/// Finite nonsingular affine range. The normalized determinant cutoff is part
/// of v2 semantics, including sigma zero; it is never repaired by clamping.
pub fn validate_affine_linear(linear: [[f64; 2]; 2]) -> Result<(), RenderError> {
    let m = linear
        .into_iter()
        .flatten()
        .map(f64::abs)
        .fold(0.0, f64::max);
    if !linear.into_iter().flatten().all(f64::is_finite) || m == 0.0 {
        return Err(RenderError::UnsupportedFeature(
            "degenerate affine effect transform".into(),
        ));
    }
    let n = linear.map(|r| r.map(|v| v / m));
    if (n[0][0] * n[1][1] - n[0][1] * n[1][0]).abs() <= 1e-6 {
        return Err(RenderError::UnsupportedFeature(
            "degenerate affine effect transform".into(),
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaussianTap {
    pub offset: [i32; 2],
    pub weight: f32,
}
/// v2 samples exp(-q/2) at integer output offsets, retaining q<=9. The
/// transformed circular 3-sigma support is elliptical, not an axis-aligned
/// separable approximation. CPU/GPU consume identical row-major f32 weights.
pub fn affine_gaussian_kernel(c: [f64; 3]) -> Result<Vec<GaussianTap>, RenderError> {
    let unsupported =
        || RenderError::UnsupportedFeature("affine Gaussian covariance or kernel budget".into());
    if c == [0.0; 3] {
        return Ok(vec![GaussianTap {
            offset: [0; 2],
            weight: 1.0,
        }]);
    }
    if !c.into_iter().all(f64::is_finite) || c[0] <= 0.0 || c[2] <= 0.0 {
        return Err(unsupported());
    }
    let m = c[0].max(c[2]);
    let n = c.map(|v| v / m);
    let det = n[0] * n[2] - n[1] * n[1];
    if !det.is_finite() || det <= 1e-12 {
        return Err(unsupported());
    }
    let determinant = c[0] * c[2] - c[1] * c[1];
    if !determinant.is_normal() || determinant <= 0.0 {
        return Err(unsupported());
    }
    let radius = [c[0], c[2]].map(|v| (3.0 * v.sqrt()).ceil());
    if radius.iter().any(|r| *r > 1024.0) {
        return Err(unsupported());
    }
    let [rx, ry] = radius.map(|v| v as i32);
    if (2 * rx + 1) as usize * (2 * ry + 1) as usize > AFFINE_KERNEL_MAX_CANDIDATES {
        return Err(unsupported());
    }
    let mut taps = Vec::new();
    let mut sum = 0.0;
    for y in -ry..=ry {
        for x in -rx..=rx {
            let (x0, y0) = (f64::from(x), f64::from(y));
            // The central tap is exact without a determinant division.
            let q = if x == 0 && y == 0 {
                0.0
            } else {
                (c[2] * x0 * x0 - 2.0 * c[1] * x0 * y0 + c[0] * y0 * y0) / determinant
            };
            if q <= 9.0 {
                let weight = (-0.5 * q).exp();
                sum += weight;
                taps.push((
                    GaussianTap {
                        offset: [x, y],
                        weight: 0.0,
                    },
                    weight,
                ));
            }
        }
    }
    Ok(taps
        .into_iter()
        .map(|(mut tap, w)| {
            tap.weight = (w / sum) as f32;
            tap
        })
        .collect())
}

#[cfg(test)]
mod affine_tests {
    use super::*;
    #[test]
    fn fx002_covariance_kernel_and_fractional_shadow_halo() {
        let design = ResolvedEffect::AffineDropShadow {
            sigma: 1.0,
            linear: [[2.0, 1.0], [0.0, 1.0]],
            offset: [3.0, -0.5],
            color: Color::from_srgb8([0; 3], None),
            opacity: 0.5,
        };
        let effect = PixelEffect::from_design(&design, [0.5, 2.0]).unwrap();
        assert_eq!(effect.covariance(), Some([1.25, 1.0, 4.0]));
        assert_eq!(effect.shadow().unwrap().0, [1.5, -1.0]);
        assert_eq!(effect.halo(), [4.0, 6.0]);
        let roi = PixelBounds {
            min: [10.0; 2],
            max: [20.0; 2],
        };
        assert_eq!(
            effect.required_input(roi),
            PixelBounds {
                min: [4.0, 5.0],
                max: [23.0, 27.0]
            }
        );
        assert_eq!(
            effect.output_bounds(roi),
            PixelBounds {
                min: [7.0, 3.0],
                max: [26.0, 25.0]
            }
        );
        let taps = affine_gaussian_kernel([5.0, 1.0, 1.0]).unwrap();
        let center = taps.iter().find(|t| t.offset == [0, 0]).unwrap().weight;
        // q(1,1)=1, q(1,-1)=2: the cross term cannot be discarded.
        for (offset, q) in [([1, 1], 1.0_f64), ([1, -1], 2.0)] {
            let w = taps.iter().find(|t| t.offset == offset).unwrap().weight;
            assert!((f64::from(w / center) - (-0.5 * q).exp()).abs() < 1e-7);
        }
        assert!((taps.iter().map(|t| t.weight).sum::<f32>() - 1.0).abs() < 1e-6);
        for tap in &taps {
            let opposite = taps
                .iter()
                .find(|t| t.offset == tap.offset.map(|v| -v))
                .unwrap();
            assert_eq!(tap.weight, opposite.weight);
        }
    }
    #[test]
    fn fx002_degenerate_transforms_and_finite_budgets_are_typed() {
        for linear in [
            [[0.0; 2]; 2],
            [[1.0, 1.0], [1.0, 1.0]],
            [[1.0, 0.0], [0.0, 1e-7]],
            [[f64::INFINITY, 0.0], [0.0, 1.0]],
        ] {
            assert_eq!(
                validate_affine_linear(linear).unwrap_err().code(),
                "UNSUPPORTED_FEATURE"
            );
        }
        for covariance in [
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1e-13],
            [1e6, 0.0, 1.0],
            [43.0 * 43.0, 0.0, 43.0 * 43.0],
            [f64::INFINITY, 0.0, 1.0],
            [1e-160, 0.0, 1e-160],
        ] {
            assert_eq!(
                affine_gaussian_kernel(covariance).unwrap_err().code(),
                "UNSUPPORTED_FEATURE"
            );
        }
        // Square candidate budget: 253*253 candidates is accepted; 259*259 is not.
        assert!(affine_gaussian_kernel([42.0 * 42.0, 0.0, 42.0 * 42.0]).is_ok());
        assert_eq!(
            affine_gaussian_kernel([0.0; 3]).unwrap(),
            vec![GaussianTap {
                offset: [0; 2],
                weight: 1.0
            }]
        );
    }
}
