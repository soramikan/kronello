//! Output-space effect contract, kernel and backward region requests.
use crate::RenderError;
use kronello_model::{Color, ResolvedEffect};
use serde::{Deserialize, Serialize};

pub const EFFECT_KERNEL_VERSION: &str = "fx001-separable-gaussian-transparent-v1";
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
    GaussianBlur {
        sigma: [f32; 2],
    },
    DropShadow {
        sigma: [f32; 2],
        offset: [f32; 2],
        color: Color,
        opacity: f32,
    },
}
impl PixelEffect {
    pub fn from_design(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
        let sigma = match effect {
            ResolvedEffect::GaussianBlur { sigma } | ResolvedEffect::DropShadow { sigma, .. } => {
                *sigma
            }
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
        };
        result.validate()?;
        Ok(result)
    }
    pub fn sigma(&self) -> [f32; 2] {
        match self {
            Self::GaussianBlur { sigma } | Self::DropShadow { sigma, .. } => *sigma,
        }
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        for sigma in self.sigma() {
            gaussian_kernel(sigma)?;
        }
        if let Self::DropShadow {
            offset, opacity, ..
        } = self
            && (offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(opacity))
        {
            return Err(RenderError::InvalidInput(
                "invalid shadow offset/opacity".into(),
            ));
        }
        Ok(())
    }
    /// Bilinear translation requires floor/ceil source taps in addition to blur.
    pub fn required_input(&self, output: PixelBounds) -> PixelBounds {
        let halo = self.sigma().map(|s| f64::from((3.0 * s).ceil()));
        match self {
            Self::GaussianBlur { .. } => output.expand(halo),
            Self::DropShadow { offset, .. } => {
                let shifted = output.translate(offset.map(|v| -f64::from(v)));
                let shifted = PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                };
                output.union(shifted.expand(halo))
            }
        }
    }
    pub fn output_bounds(&self, input: PixelBounds) -> PixelBounds {
        let halo = self.sigma().map(|s| f64::from((3.0 * s).ceil()));
        match self {
            Self::GaussianBlur { .. } => input.expand(halo),
            Self::DropShadow { offset, .. } => {
                let shifted = input.expand(halo).translate(offset.map(f64::from));
                input.union(PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                })
            }
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
