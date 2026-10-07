//! Output-space effect contract, kernel and backward region requests.
use crate::RenderError;
use kronello_eval::Affine2;
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
    /// FX-005 chroma key (ADR-0115): alpha becomes the keyed Cb/Cr matte,
    /// eroded by `edge_shrink` output pixels then Gaussian-feathered by
    /// `edge_feather`. `key_color` retains its straight tag until execution.
    ChromaKey {
        key_color: Color,
        similarity: f32,
        edge_shrink: [f32; 2],
        edge_feather: [f32; 2],
        spill: f32,
    },
    /// FX-005 luma key (ADR-0115): straight working-space luminance distance
    /// to `key_luma`, normalized by `tolerance`, drives the alpha matte.
    LumaKey {
        key_luma: f32,
        tolerance: f32,
        edge_shrink: [f32; 2],
        edge_feather: [f32; 2],
    },
    /// FX-006 glow (ADR-0115): straight luminance above `threshold` is
    /// extracted, blurred by a Gaussian of sigma `radius` in output pixels,
    /// and added back at `intensity` strength.
    Glow {
        threshold: f32,
        radius: [f32; 2],
        intensity: f32,
    },
    /// FX-006 unsharp mask (ADR-0115): out = source + amount * (source -
    /// blur(source)) on all four premultiplied channels; alpha clamps to [0,1].
    Sharpen {
        amount: f32,
        radius: [f32; 2],
    },
    /// FX-006 vignette (ADR-0115): multiplies premultiplied RGB by a
    /// smoothstep corner falloff; alpha is preserved.
    Vignette {
        amount: f32,
        midpoint: f32,
        feather: f32,
        roundness: f32,
    },
    /// FX-006 corner pin (ADR-0115): inverse-mapped quad warp. `pins` are the
    /// destination corners in output-pixel edge coordinates, ordered
    /// top-left, top-right, bottom-right, bottom-left. `source` is the input
    /// content's bounds rectangle on the same lattice; the DAG builder fills
    /// it once input bounds are known, so a directly constructed value must
    /// carry it explicitly.
    CornerPin {
        pins: [[f32; 2]; 4],
        source: Option<PixelBounds>,
    },
}
/// Kernel tag shared by all COLOR-002 v1 pointwise passes.
pub const COLOR002_KERNEL_VERSION: &str = "color002-pointwise-f16-v1";
/// Kernel tag shared by all FX-005/FX-006 v1 passes (ADR-0115).
pub const STANDARD_KERNEL_VERSION: &str = "fx005006-keying-standard-f16-v1";
/// Maximum corner-pin / edge-adjust kernel support, matching the Gaussian
/// budget so CPU and GPU rejects stay identical.
const STANDARD_KERNEL_MAX_RADIUS: f32 = 1024.0;
impl PixelEffect {
    /// COLOR-002 corrections are pointwise: no kernel, no neighborhood input.
    pub fn is_pointwise_color(&self) -> bool {
        matches!(
            self,
            Self::ColorExposure { .. }
                | Self::ColorLevels { .. }
                | Self::ColorCurves { .. }
                | Self::ColorHsl { .. }
        )
    }
    /// FX-005/FX-006 keying and standard effects (ADR-0115). They carry their
    /// own multi-pass execution path rather than the Gaussian/shadow one.
    pub fn is_standard(&self) -> bool {
        matches!(
            self,
            Self::ChromaKey { .. }
                | Self::LumaKey { .. }
                | Self::Glow { .. }
                | Self::Sharpen { .. }
                | Self::Vignette { .. }
                | Self::CornerPin { .. }
        )
    }
    /// Scale-only lowering retains the historic meaning: the map is a pure
    /// diagonal transform with no translation. `from_design_mapped` covers
    /// effects with absolute position parameters.
    pub fn from_design(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
        Self::from_design_mapped(
            effect,
            Affine2([[scale[0], 0.0, 0.0], [0.0, scale[1], 0.0]]),
        )
    }
    /// Lower a design-space resolved effect through the region's
    /// design-to-output-pixel map. Positions (corner pins) use the full map;
    /// lengths use the diagonal scale only.
    pub fn from_design_mapped(
        effect: &ResolvedEffect,
        design_to_pixel: Affine2,
    ) -> Result<Self, RenderError> {
        let scale = [design_to_pixel.0[0][0], design_to_pixel.0[1][1]];
        if scale.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(RenderError::InvalidInput(
                "invalid effect output scale".into(),
            ));
        }
        // FX-005/FX-006 variants: resolution-checked in the model crate; the
        // pixel form converts precision and maps length/position parameters.
        let standard = match effect {
            ResolvedEffect::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => Some(Self::ChromaKey {
                key_color: *key_color,
                similarity: *similarity as f32,
                edge_shrink: [0, 1].map(|i| (*edge_shrink * scale[i]) as f32),
                edge_feather: [0, 1].map(|i| (*edge_feather * scale[i]) as f32),
                spill: *spill as f32,
            }),
            ResolvedEffect::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => Some(Self::LumaKey {
                key_luma: *key_luma as f32,
                tolerance: *tolerance as f32,
                edge_shrink: [0, 1].map(|i| (*edge_shrink * scale[i]) as f32),
                edge_feather: [0, 1].map(|i| (*edge_feather * scale[i]) as f32),
            }),
            ResolvedEffect::Glow {
                threshold,
                radius,
                intensity,
            } => Some(Self::Glow {
                threshold: *threshold as f32,
                radius: [0, 1].map(|i| (*radius * scale[i]) as f32),
                intensity: *intensity as f32,
            }),
            ResolvedEffect::Sharpen { amount, radius } => Some(Self::Sharpen {
                amount: *amount as f32,
                radius: [0, 1].map(|i| (*radius * scale[i]) as f32),
            }),
            ResolvedEffect::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => Some(Self::Vignette {
                amount: *amount as f32,
                midpoint: *midpoint as f32,
                feather: *feather as f32,
                roundness: *roundness as f32,
            }),
            ResolvedEffect::CornerPin { corners } => {
                let pins = corners.map(|p| design_to_pixel.transform_point(p));
                if pins
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Err(RenderError::InvalidInput(
                        "corner pin outside finite output range".into(),
                    ));
                }
                Some(Self::CornerPin {
                    pins: pins.map(|p| p.map(|v| v as f32)),
                    source: None,
                })
            }
            _ => None,
        };
        if let Some(result) = standard {
            result.validate()?;
            return Ok(result);
        }
        Self::from_design_spatial(effect, scale)
    }
    fn from_design_spatial(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
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
        if self.is_pointwise_color() {
            COLOR002_KERNEL_VERSION
        } else if self.is_standard() {
            STANDARD_KERNEL_VERSION
        } else if self.covariance().is_some() {
            AFFINE_EFFECT_KERNEL_VERSION
        } else {
            EFFECT_KERNEL_VERSION
        }
    }
    pub fn semantic_version(&self) -> u32 {
        if self.is_pointwise_color() {
            kronello_model::COLOR_EFFECT_VERSION
        } else if self.is_standard() {
            kronello_model::STANDARD_EFFECT_VERSION
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
    /// Per-axis read support in output pixels: Gaussian 3-sigma plus matte
    /// erosion for keying; zero for pointwise and warp effects.
    pub fn halo(&self) -> [f64; 2] {
        if let Some(c) = self.covariance() {
            return [c[0], c[2]].map(|v| (3.0 * v.sqrt()).ceil());
        }
        let mut halo: [f64; 2] = [0.0; 2];
        for sigma in self.kernel_sigmas() {
            for (i, s) in sigma.iter().enumerate() {
                halo[i] = halo[i].max((3.0 * f64::from(*s)).ceil());
            }
        }
        if let Self::ChromaKey { edge_shrink, .. } | Self::LumaKey { edge_shrink, .. } = self {
            for (h, s) in halo.iter_mut().zip(edge_shrink) {
                *h += f64::from(s.ceil());
            }
        }
        halo
    }
    /// Gaussian sigma equivalents for the separable kernel paths: blur sigma
    /// for blur/shadow, `radius` for glow/sharpen, `edge_feather` for keying.
    fn kernel_sigmas(&self) -> Vec<[f32; 2]> {
        match self {
            Self::Glow { radius, .. } | Self::Sharpen { radius, .. } => vec![*radius],
            Self::ChromaKey { edge_feather, .. } | Self::LumaKey { edge_feather, .. } => {
                vec![*edge_feather]
            }
            _ => vec![self.sigma()],
        }
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.is_pointwise_color() {
            return self.validate_color();
        }
        if self.is_standard() {
            return self.validate_standard();
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
    /// FX-005/FX-006 parameter contract mirrors the model-side checks and the
    /// shared 1024-output-pixel kernel budget (ADR-0115).
    fn validate_standard(&self) -> Result<(), RenderError> {
        let invalid = || RenderError::InvalidInput("invalid standard effect parameters".into());
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        let nonnegative = |v: f32| v.is_finite() && v >= 0.0;
        let lengths = |v: &[f32; 2]| v.iter().all(|v| nonnegative(*v));
        match self {
            Self::ChromaKey {
                similarity,
                edge_shrink,
                edge_feather,
                spill,
                ..
            } => {
                if !unit(*similarity)
                    || !unit(*spill)
                    || !lengths(edge_shrink)
                    || !lengths(edge_feather)
                {
                    return Err(invalid());
                }
            }
            Self::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => {
                if !unit(*key_luma)
                    || !unit(*tolerance)
                    || !lengths(edge_shrink)
                    || !lengths(edge_feather)
                {
                    return Err(invalid());
                }
            }
            Self::Glow {
                threshold,
                radius,
                intensity,
            } => {
                if !nonnegative(*threshold) || !nonnegative(*intensity) || !lengths(radius) {
                    return Err(invalid());
                }
            }
            Self::Sharpen { amount, radius } => {
                if !nonnegative(*amount) || !lengths(radius) {
                    return Err(invalid());
                }
            }
            Self::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => {
                if !unit(*amount) || !unit(*midpoint) || !unit(*roundness) || !nonnegative(*feather)
                {
                    return Err(invalid());
                }
            }
            Self::CornerPin { pins, .. } => {
                if pins
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Err(invalid());
                }
                // Pin quad validity is independent of the (possibly empty)
                // source bounds: always check convexity and nondegeneracy.
                corner_pin_inverse(
                    PixelBounds {
                        min: [0.0; 2],
                        max: [1.0; 2],
                    },
                    *pins,
                )?;
            }
            _ => unreachable!("not a standard effect"),
        }
        // Kernel budgets: erosions cap at the Gaussian radius; feathers and
        // glow/sharpen radii share the separable Gaussian budget.
        for sigma in self.kernel_sigmas() {
            for s in sigma {
                gaussian_kernel(s)?;
            }
        }
        let shrink = match self {
            Self::ChromaKey { edge_shrink, .. } | Self::LumaKey { edge_shrink, .. } => {
                Some(*edge_shrink)
            }
            _ => None,
        };
        if let Some(shrink) = shrink
            && shrink.iter().any(|v| v.ceil() > STANDARD_KERNEL_MAX_RADIUS)
        {
            return Err(RenderError::UnsupportedFeature(
                "effect kernel radius exceeds 1024 output pixels".into(),
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
            _ => unreachable!(),
        }
        Ok(())
    }
    /// Bilinear translation requires floor/ceil source taps in addition to
    /// blur. Keying/glow/sharpen expand by their kernel halo; corner pin
    /// conservatively requests the whole incoming source quad since any
    /// destination pixel may sample it (ADR-0115).
    pub fn required_input(&self, output: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. }
            | Self::AffineGaussianBlur { .. }
            | Self::ChromaKey { .. }
            | Self::LumaKey { .. }
            | Self::Glow { .. }
            | Self::Sharpen { .. } => output.expand(halo),
            Self::CornerPin { source, .. } => source.unwrap_or(output),
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
    /// FX-005 keying and vignette keep the input extent: the matte can only
    /// shrink coverage. Glow/sharpen expand by their kernel; corner pin
    /// reports the destination quad hull (ADR-0115).
    pub fn output_bounds(&self, input: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. }
            | Self::AffineGaussianBlur { .. }
            | Self::Glow { .. }
            | Self::Sharpen { .. } => input.expand(halo),
            Self::ChromaKey { .. } | Self::LumaKey { .. } | Self::Vignette { .. } => input,
            Self::CornerPin { pins, .. } => corner_pin_hull(*pins),
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
/// Conservative coverage hull of the corner-pin destination quad on the
/// output pixel lattice.
pub fn corner_pin_hull(pins: [[f32; 2]; 4]) -> PixelBounds {
    PixelBounds {
        min: [0, 1].map(|i| {
            pins.iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::INFINITY, f64::min)
                .floor()
        }),
        max: [0, 1].map(|i| {
            pins.iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
        }),
    }
}
/// Inverse homography from destination output-pixel edge coordinates to
/// source edge coordinates over `source`, as a row-major 3x3 matrix. Quad
/// corners map TL→pins[0], TR→pins[1], BR→pins[2], BL→pins[3]. The quad must
/// be strictly convex; a degenerate or self-intersecting quad is a typed
/// error.
pub fn corner_pin_inverse(
    source: PixelBounds,
    pins: [[f32; 2]; 4],
) -> Result<[[f32; 3]; 3], RenderError> {
    let degenerate =
        || RenderError::InvalidInput("corner pin requires a nondegenerate convex quad".into());
    if !source.min.iter().chain(&source.max).all(|v| v.is_finite())
        || source.max[0] <= source.min[0]
        || source.max[1] <= source.min[1]
    {
        return Err(degenerate());
    }
    let d = pins.map(|p| p.map(f64::from));
    let e = [0, 1, 2, 3].map(|i| {
        let (a, b) = (d[i], d[(i + 1) % 4]);
        [b[0] - a[0], b[1] - a[1]]
    });
    // Strict convexity: all four cross products share one nonzero sign.
    let crosses = [0, 1, 2, 3].map(|i| e[i][0] * e[(i + 1) % 4][1] - e[i][1] * e[(i + 1) % 4][0]);
    let scale = e
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        .max(f64::EPSILON);
    if crosses
        .iter()
        .any(|c| !c.is_finite() || c.abs() <= 1e-6 * scale * scale)
    {
        return Err(degenerate());
    }
    if !(crosses.iter().all(|c| *c > 0.0) || crosses.iter().all(|c| *c < 0.0)) {
        return Err(degenerate());
    }
    // Unit-square to quad homography (Heckbert): the source rectangle is
    // normalized to the unit square so sampling bounds stay [0,1].
    let sx = d[1][0] - d[0][0] - d[2][0] + d[3][0];
    let sy = d[1][1] - d[0][1] - d[2][1] + d[3][1];
    let dx1 = d[1][0] - d[2][0];
    let dx2 = d[3][0] - d[2][0];
    let dy1 = d[1][1] - d[2][1];
    let dy2 = d[3][1] - d[2][1];
    let denominator = dx1 * dy2 - dx2 * dy1;
    if !denominator.is_finite() || denominator.abs() <= 1e-6 * scale * scale {
        return Err(degenerate());
    }
    let g = (sx * dy2 - dx2 * sy) / denominator;
    let h = (dx1 * sy - sx * dy1) / denominator;
    // Forward map: u,v in the unit square to output-pixel x,y.
    let forward = [
        [
            d[1][0] - d[0][0] + g * d[1][0],
            d[3][0] - d[0][0] + h * d[3][0],
            d[0][0],
        ],
        [
            d[1][1] - d[0][1] + g * d[1][1],
            d[3][1] - d[0][1] + h * d[3][1],
            d[0][1],
        ],
        [g, h, 1.0],
    ];
    let det = forward[0][0] * (forward[1][1] * forward[2][2] - forward[1][2] * forward[2][1])
        - forward[0][1] * (forward[1][0] * forward[2][2] - forward[1][2] * forward[2][0])
        + forward[0][2] * (forward[1][0] * forward[2][1] - forward[1][1] * forward[2][0]);
    let norm = forward
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0, f64::max);
    if !det.is_finite() || det.abs() <= 1e-12 * norm * norm * norm {
        return Err(degenerate());
    }
    let m = |r: usize, c: usize| forward[r][c];
    let cofactor = [
        [
            m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1),
            m(0, 2) * m(2, 1) - m(0, 1) * m(2, 2),
            m(0, 1) * m(1, 2) - m(0, 2) * m(1, 1),
        ],
        [
            m(1, 2) * m(2, 0) - m(1, 0) * m(2, 2),
            m(0, 0) * m(2, 2) - m(0, 2) * m(2, 0),
            m(0, 2) * m(1, 0) - m(0, 0) * m(1, 2),
        ],
        [
            m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0),
            m(0, 1) * m(2, 0) - m(0, 0) * m(2, 1),
            m(0, 0) * m(1, 1) - m(0, 1) * m(1, 0),
        ],
    ];
    let inverse = cofactor.map(|row| row.map(|v| v / det));
    if inverse.iter().flatten().any(|v| !v.is_finite()) {
        return Err(degenerate());
    }
    // Compose with the unit-square to source-rectangle map so the result maps
    // destination edge coordinates directly to source edge coordinates.
    let w = source.max[0] - source.min[0];
    let h = source.max[1] - source.min[1];
    let total = [
        [
            w * inverse[0][0],
            w * inverse[0][1],
            w * inverse[0][2] + source.min[0],
        ],
        [
            h * inverse[1][0],
            h * inverse[1][1],
            h * inverse[1][2] + source.min[1],
        ],
        inverse[2],
    ];
    if total.iter().flatten().any(|v| !v.is_finite()) {
        return Err(degenerate());
    }
    Ok(total.map(|row| row.map(|v| v as f32)))
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
