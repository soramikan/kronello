//! Ordered, versioned effects. Parameters reference the owning node's Properties.
use crate::{
    Color, DescriptorDefinition, DescriptorId, FiniteF64, NumericRange, Property,
    PropertyDescriptor, PropertyId, SchemaKey, SchemaRegistry, Unit, Value, ValueRange, ValueType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GAUSSIAN_BLUR_ID: &str = "kronello.gaussian_blur";
pub const DROP_SHADOW_ID: &str = "kronello.drop_shadow";
pub const EFFECT_VERSION: u32 = 1;
pub const AUDIO_GAIN_ID: &str = "kronello.audio.gain";
pub const AFFINE_EFFECT_VERSION: u32 = 2;
/// COLOR-002 pointwise color correction effect ids (ADR-0108).
pub const COLOR_EXPOSURE_ID: &str = "kronello.color.exposure";
pub const COLOR_LEVELS_ID: &str = "kronello.color.levels";
pub const COLOR_CURVES_ID: &str = "kronello.color.curves";
pub const COLOR_HSL_ID: &str = "kronello.color.hsl";
/// COLOR-002 supported version is EFFECT_VERSION (1).
pub const COLOR_EFFECT_VERSION: u32 = EFFECT_VERSION;
/// Maximum accepted COLOR-002 curves control-point count.
pub const CURVES_MAX_POINTS: usize = 64;
/// FX-005 keying effect ids (ADR-0115). Both are matte-producing effects:
/// they rewrite alpha and are the only effects allowed to create or destroy
/// coverage inside the input bounds.
pub const KEYING_CHROMA_ID: &str = "kronello.keying.chroma";
pub const KEYING_LUMA_ID: &str = "kronello.keying.luma";
/// FX-006 standard effect ids (ADR-0115).
pub const GLOW_ID: &str = "kronello.glow";
pub const SHARPEN_ID: &str = "kronello.sharpen";
pub const VIGNETTE_ID: &str = "kronello.vignette";
pub const CORNER_PIN_ID: &str = "kronello.corner_pin";
/// FX-005/FX-006 supported version is EFFECT_VERSION (1).
pub const STANDARD_EFFECT_VERSION: u32 = EFFECT_VERSION;

/// Unknown ids, parameters, fields and variants are retained verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Effect {
    Known(EffectDefinition),
    Opaque(serde_json::Value),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectDefinition {
    pub effect_id: String,
    pub version: u32,
    pub parameters: EffectParameters,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectParameters {
    GaussianBlur {
        sigma: PropertyId,
    },
    DropShadow {
        sigma: PropertyId,
        offset: PropertyId,
        color: PropertyId,
        opacity: PropertyId,
    },
    AudioGain {
        gain: PropertyId,
    },
    /// COLOR-002 exposure: premultiplied working RGB is scaled by
    /// 2^exposure and shifted by offset; alpha is preserved (ADR-0108).
    ColorExposure {
        /// EV stops; finite scalar.
        exposure: PropertyId,
        /// Additive premultiplied-space offset; finite scalar.
        offset: PropertyId,
    },
    /// COLOR-002 levels with gamma, applied per premultiplied RGB channel.
    ColorLevels {
        in_black: PropertyId,
        in_white: PropertyId,
        /// Positive finite scalar; 1.0 is linear.
        gamma: PropertyId,
        out_black: PropertyId,
        out_white: PropertyId,
    },
    /// COLOR-002 monotone-cubic RGB channel curve. The property is a data
    /// table with scalar `x` and `y` columns, `x` strictly increasing, both
    /// in [0,1], and at most CURVES_MAX_POINTS rows.
    ColorCurves {
        curve: PropertyId,
    },
    /// COLOR-002 hue/saturation/lightness in working HSL; alpha preserved.
    ColorHsl {
        /// Shift in degrees; finite angle.
        hue_shift: PropertyId,
        /// Multiplier where 1.0 leaves saturation unchanged; finite scalar.
        saturation: PropertyId,
        /// Additive lightness term; finite scalar.
        lightness: PropertyId,
    },
    /// FX-005 chroma key (ADR-0115): alpha = keyed Cb/Cr matte, then edge
    /// shrink/feather adjust the matte and `spill` suppresses the key hue.
    ChromaKey {
        /// Screen color being removed.
        key_color: PropertyId,
        /// Cb/Cr distance that maps to full transparency; 0..=1.
        similarity: PropertyId,
        /// Matte erosion radius in design_px.
        edge_shrink: PropertyId,
        /// Matte Gaussian feather sigma in design_px.
        edge_feather: PropertyId,
        /// Spill suppression amount; 0..=1.
        spill: PropertyId,
    },
    /// FX-005 luma key (ADR-0115): working-space luminance distance drives
    /// the alpha matte, then edge shrink/feather adjust it.
    LumaKey {
        /// Straight-color luminance that becomes fully transparent; 0..=1.
        key_luma: PropertyId,
        /// Luminance distance that maps to full opacity; 0..=1.
        tolerance: PropertyId,
        /// Matte erosion radius in design_px.
        edge_shrink: PropertyId,
        /// Matte Gaussian feather sigma in design_px.
        edge_feather: PropertyId,
    },
    /// FX-006 glow (ADR-0115): straight luminance above `threshold` is
    /// extracted, blurred by a Gaussian of sigma `radius`, and added back.
    Glow {
        /// Straight luminance cutoff; nonnegative scalar.
        threshold: PropertyId,
        /// Gaussian sigma in design_px.
        radius: PropertyId,
        /// Additive contribution of the blurred bloom; nonnegative scalar.
        intensity: PropertyId,
    },
    /// FX-006 unsharp mask (ADR-0115): out = source + amount * (source -
    /// blur(source)); the alpha channel participates and stays in [0,1].
    Sharpen {
        /// Unsharp strength; nonnegative scalar.
        amount: PropertyId,
        /// Gaussian sigma in design_px.
        radius: PropertyId,
    },
    /// FX-006 vignette (ADR-0115): darkens RGB toward the image corners;
    /// alpha is preserved.
    Vignette {
        /// Maximum darkening factor; 0..=1.
        amount: PropertyId,
        /// Normalized distance where darkening starts; 0..=1.
        midpoint: PropertyId,
        /// Smoothstep width of the falloff; nonnegative scalar.
        feather: PropertyId,
        /// Rectangle-to-ellipse shape blend; 0..=1.
        roundness: PropertyId,
    },
    /// FX-006 corner pin (ADR-0115): the four corners of the incoming
    /// surface's bounds are moved to these absolute Composition design_px
    /// positions, in order top-left, top-right, bottom-right, bottom-left.
    CornerPin {
        top_left: PropertyId,
        top_right: PropertyId,
        bottom_right: PropertyId,
        bottom_left: PropertyId,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ResolvedEffect {
    AffineGaussianBlur {
        sigma: f64,
        linear: [[f64; 2]; 2],
    },
    AffineDropShadow {
        sigma: f64,
        linear: [[f64; 2]; 2],
        offset: [f64; 2],
        color: Color,
        opacity: f64,
    },
    GaussianBlur {
        sigma: f64,
    },
    DropShadow {
        sigma: f64,
        offset: [f64; 2],
        color: Color,
        opacity: f64,
    },
    /// COLOR-002 resolved pointwise operations keep f64 authoring precision.
    ColorExposure {
        exposure: f64,
        offset: f64,
    },
    ColorLevels {
        in_black: f64,
        in_white: f64,
        gamma: f64,
        out_black: f64,
        out_white: f64,
    },
    /// Monotonically increasing `(x, y)` control points, already validated.
    ColorCurves {
        curve: Vec<[f64; 2]>,
    },
    ColorHsl {
        hue_shift: f64,
        saturation: f64,
        lightness: f64,
    },
    /// FX-005 resolved forms; lengths stay in design_px until DAG lowering.
    ChromaKey {
        key_color: Color,
        similarity: f64,
        edge_shrink: f64,
        edge_feather: f64,
        spill: f64,
    },
    LumaKey {
        key_luma: f64,
        tolerance: f64,
        edge_shrink: f64,
        edge_feather: f64,
    },
    /// FX-006 resolved forms. `radius` is the Gaussian sigma in design_px;
    /// corner-pin positions are absolute Composition design_px points in
    /// top-left, top-right, bottom-right, bottom-left order.
    Glow {
        threshold: f64,
        radius: f64,
        intensity: f64,
    },
    Sharpen {
        amount: f64,
        radius: f64,
    },
    Vignette {
        amount: f64,
        midpoint: f64,
        feather: f64,
        roundness: f64,
    },
    CornerPin {
        corners: [[f64; 2]; 4],
    },
}
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("UNSUPPORTED_FEATURE: unknown effect or semantic version")]
    UnsupportedFeature,
    #[error("invalid effect parameter {0}")]
    InvalidParameter(PropertyId),
    #[error("effect stack exceeds 16 entries")]
    StackBudget,
}
impl EffectDefinition {
    pub fn ensure_supported(&self) -> Result<(), EffectError> {
        let (id, latest) = match self.parameters {
            EffectParameters::AudioGain { .. } => (AUDIO_GAIN_ID, EFFECT_VERSION),
            EffectParameters::GaussianBlur { .. } => (GAUSSIAN_BLUR_ID, AFFINE_EFFECT_VERSION),
            EffectParameters::DropShadow { .. } => (DROP_SHADOW_ID, AFFINE_EFFECT_VERSION),
            EffectParameters::ColorExposure { .. } => (COLOR_EXPOSURE_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorLevels { .. } => (COLOR_LEVELS_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorCurves { .. } => (COLOR_CURVES_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorHsl { .. } => (COLOR_HSL_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ChromaKey { .. } => (KEYING_CHROMA_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::LumaKey { .. } => (KEYING_LUMA_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Glow { .. } => (GLOW_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Sharpen { .. } => (SHARPEN_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Vignette { .. } => (VIGNETTE_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::CornerPin { .. } => (CORNER_PIN_ID, STANDARD_EFFECT_VERSION),
        };
        if self.effect_id != id || !(EFFECT_VERSION..=latest).contains(&self.version) {
            return Err(EffectError::UnsupportedFeature);
        }
        Ok(())
    }
    fn references(&self) -> Vec<(PropertyId, ValueType, Unit)> {
        match self.parameters {
            EffectParameters::AudioGain { gain } => {
                vec![(gain, ValueType::Scalar, Unit::Dimensionless)]
            }
            EffectParameters::GaussianBlur { sigma } => {
                vec![(sigma, ValueType::Scalar, Unit::DesignPx)]
            }
            EffectParameters::DropShadow {
                sigma,
                offset,
                color,
                opacity,
            } => vec![
                (sigma, ValueType::Scalar, Unit::DesignPx),
                (offset, ValueType::Vec2, Unit::DesignPx),
                (color, ValueType::Color, Unit::Dimensionless),
                (opacity, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ColorExposure { exposure, offset } => vec![
                (exposure, ValueType::Scalar, Unit::Dimensionless),
                (offset, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => [in_black, in_white, gamma, out_black, out_white]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::ColorCurves { curve } => {
                vec![(curve, ValueType::DataTable, Unit::Dimensionless)]
            }
            EffectParameters::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => vec![
                (hue_shift, ValueType::Angle, Unit::Degrees),
                (saturation, ValueType::Scalar, Unit::Dimensionless),
                (lightness, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => vec![
                (key_color, ValueType::Color, Unit::Dimensionless),
                (similarity, ValueType::Scalar, Unit::Dimensionless),
                (edge_shrink, ValueType::Scalar, Unit::DesignPx),
                (edge_feather, ValueType::Scalar, Unit::DesignPx),
                (spill, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => vec![
                (key_luma, ValueType::Scalar, Unit::Dimensionless),
                (tolerance, ValueType::Scalar, Unit::Dimensionless),
                (edge_shrink, ValueType::Scalar, Unit::DesignPx),
                (edge_feather, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::Glow {
                threshold,
                radius,
                intensity,
            } => vec![
                (threshold, ValueType::Scalar, Unit::Dimensionless),
                (radius, ValueType::Scalar, Unit::DesignPx),
                (intensity, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::Sharpen { amount, radius } => vec![
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (radius, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => vec![
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (midpoint, ValueType::Scalar, Unit::Dimensionless),
                (feather, ValueType::Scalar, Unit::Dimensionless),
                (roundness, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::CornerPin {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => [top_left, top_right, bottom_right, bottom_left]
                .into_iter()
                .map(|id| (id, ValueType::Vec2, Unit::DesignPx))
                .collect(),
        }
    }
    pub fn validate(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), EffectError> {
        self.ensure_supported()?;
        for (id, ty, unit) in self.references() {
            let p = properties
                .iter()
                .find(|p| p.id() == id)
                .ok_or(EffectError::InvalidParameter(id))?;
            let d = registry
                .lookup(&p.descriptor().key)
                .map_err(|_| EffectError::InvalidParameter(id))?;
            if d.definition().value_type != ty || d.definition().unit != unit {
                return Err(EffectError::InvalidParameter(id));
            }
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        self.ensure_supported()?;
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        // COLOR-002 pointwise corrections validate parameter magnitudes and
        // table shape at resolution; alpha/HDR handling is a renderer contract.
        if matches!(
            self.parameters,
            EffectParameters::ColorExposure { .. }
                | EffectParameters::ColorLevels { .. }
                | EffectParameters::ColorCurves { .. }
                | EffectParameters::ColorHsl { .. }
        ) {
            return self.resolve_color(values);
        }
        // FX-005/FX-006 keying and standard effects (ADR-0115).
        if matches!(
            self.parameters,
            EffectParameters::ChromaKey { .. }
                | EffectParameters::LumaKey { .. }
                | EffectParameters::Glow { .. }
                | EffectParameters::Sharpen { .. }
                | EffectParameters::Vignette { .. }
                | EffectParameters::CornerPin { .. }
        ) {
            return self.resolve_standard(values);
        }
        let sigma_id = match self.parameters {
            // Audio effects are executed only by the audio evaluator.
            EffectParameters::AudioGain { .. } => return Err(EffectError::UnsupportedFeature),
            EffectParameters::GaussianBlur { sigma }
            | EffectParameters::DropShadow { sigma, .. } => sigma,
            _ => unreachable!("handled above"),
        };
        let sigma = scalar(sigma_id)?;
        if !(0.0..=1_000_000.0).contains(&sigma) {
            return Err(EffectError::InvalidParameter(sigma_id));
        }
        Ok(match self.parameters {
            EffectParameters::AudioGain { .. } => return Err(EffectError::UnsupportedFeature),
            EffectParameters::GaussianBlur { .. } => {
                if self.version == AFFINE_EFFECT_VERSION {
                    ResolvedEffect::AffineGaussianBlur {
                        sigma,
                        linear: [[1.0, 0.0], [0.0, 1.0]],
                    }
                } else {
                    ResolvedEffect::GaussianBlur { sigma }
                }
            }
            EffectParameters::DropShadow {
                offset,
                color,
                opacity,
                ..
            } => {
                let offset_value = match values.get(&offset) {
                    Some(Value::Vec2(v)) => v.map(FiniteF64::get),
                    _ => return Err(EffectError::InvalidParameter(offset)),
                };
                if offset_value.iter().any(|v| v.abs() > 1_000_000.0) {
                    return Err(EffectError::InvalidParameter(offset));
                }
                let color_value = match values.get(&color) {
                    Some(Value::Color(v)) => *v,
                    _ => return Err(EffectError::InvalidParameter(color)),
                };
                let opacity_value = scalar(opacity)?;
                if !(0.0..=1.0).contains(&opacity_value) {
                    return Err(EffectError::InvalidParameter(opacity));
                }
                if self.version == AFFINE_EFFECT_VERSION {
                    ResolvedEffect::AffineDropShadow {
                        sigma,
                        linear: [[1.0, 0.0], [0.0, 1.0]],
                        offset: offset_value,
                        color: color_value,
                        opacity: opacity_value,
                    }
                } else {
                    ResolvedEffect::DropShadow {
                        sigma,
                        offset: offset_value,
                        color: color_value,
                        opacity: opacity_value,
                    }
                }
            }
            _ => unreachable!("handled above"),
        })
    }
    /// FX-005/FX-006 parameter validation (ADR-0115). Probability-like
    /// parameters are range-checked; lengths and strengths share the 1e6
    /// scalar budget; corner pins are absolute Composition design_px points.
    fn resolve_standard(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let unit_interval = |id| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (0.0..=1.0).contains(&value) {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let nonnegative = |id| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (0.0..=1_000_000.0).contains(&value) {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let point = |id| -> Result<[f64; 2], EffectError> {
            match values.get(&id) {
                Some(Value::Vec2(v)) => {
                    let v = v.map(FiniteF64::get);
                    if v.iter().all(|c| c.abs() <= 1_000_000.0) {
                        Ok(v)
                    } else {
                        Err(EffectError::InvalidParameter(id))
                    }
                }
                _ => Err(EffectError::InvalidParameter(id)),
            }
        };
        match self.parameters {
            EffectParameters::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => {
                let key_color = match values.get(&key_color) {
                    Some(Value::Color(v)) => *v,
                    _ => return Err(EffectError::InvalidParameter(key_color)),
                };
                Ok(ResolvedEffect::ChromaKey {
                    key_color,
                    similarity: unit_interval(similarity)?,
                    edge_shrink: nonnegative(edge_shrink)?,
                    edge_feather: nonnegative(edge_feather)?,
                    spill: unit_interval(spill)?,
                })
            }
            EffectParameters::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => Ok(ResolvedEffect::LumaKey {
                key_luma: unit_interval(key_luma)?,
                tolerance: unit_interval(tolerance)?,
                edge_shrink: nonnegative(edge_shrink)?,
                edge_feather: nonnegative(edge_feather)?,
            }),
            EffectParameters::Glow {
                threshold,
                radius,
                intensity,
            } => Ok(ResolvedEffect::Glow {
                threshold: nonnegative(threshold)?,
                radius: nonnegative(radius)?,
                intensity: nonnegative(intensity)?,
            }),
            EffectParameters::Sharpen { amount, radius } => Ok(ResolvedEffect::Sharpen {
                amount: nonnegative(amount)?,
                radius: nonnegative(radius)?,
            }),
            EffectParameters::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => Ok(ResolvedEffect::Vignette {
                amount: unit_interval(amount)?,
                midpoint: unit_interval(midpoint)?,
                feather: nonnegative(feather)?,
                roundness: unit_interval(roundness)?,
            }),
            EffectParameters::CornerPin {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => Ok(ResolvedEffect::CornerPin {
                corners: [
                    point(top_left)?,
                    point(top_right)?,
                    point(bottom_right)?,
                    point(bottom_left)?,
                ],
            }),
            _ => unreachable!("standard resolution is only invoked for FX-005/006 variants"),
        }
    }
    /// COLOR-002 parameter validation happens here because ranges are
    /// cross-parameter (levels) or structural (curve table). Magnitude bounds
    /// match the existing 1e6 scalar budget.
    fn resolve_color(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let bounded = |id, limit: f64| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if value.abs() <= limit {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        match self.parameters {
            EffectParameters::ColorExposure { exposure, offset } => {
                Ok(ResolvedEffect::ColorExposure {
                    exposure: bounded(exposure, 1_024.0)?,
                    offset: bounded(offset, 1_000_000.0)?,
                })
            }
            EffectParameters::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                let in_black_v = bounded(in_black, 1_000_000.0)?;
                let in_white_v = bounded(in_white, 1_000_000.0)?;
                if in_white_v <= in_black_v {
                    return Err(EffectError::InvalidParameter(in_white));
                }
                let gamma_v = scalar(gamma)?;
                if !(gamma_v > 0.0 && gamma_v <= 1_000_000.0) {
                    return Err(EffectError::InvalidParameter(gamma));
                }
                Ok(ResolvedEffect::ColorLevels {
                    in_black: in_black_v,
                    in_white: in_white_v,
                    gamma: gamma_v,
                    out_black: bounded(out_black, 1_000_000.0)?,
                    out_white: bounded(out_white, 1_000_000.0)?,
                })
            }
            EffectParameters::ColorCurves { curve } => {
                let table = match values.get(&curve) {
                    Some(Value::DataTable(t)) => t,
                    _ => return Err(EffectError::InvalidParameter(curve)),
                };
                let points = curve_points(table).ok_or(EffectError::InvalidParameter(curve))?;
                Ok(ResolvedEffect::ColorCurves { curve: points })
            }
            EffectParameters::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => {
                let hue = match values.get(&hue_shift) {
                    Some(Value::Angle(v)) => v.get(),
                    _ => return Err(EffectError::InvalidParameter(hue_shift)),
                };
                if hue.abs() > 1_000_000.0 {
                    return Err(EffectError::InvalidParameter(hue_shift));
                }
                Ok(ResolvedEffect::ColorHsl {
                    hue_shift: hue,
                    saturation: bounded(saturation, 1_000_000.0)?,
                    lightness: bounded(lightness, 1_000_000.0)?,
                })
            }
            _ => unreachable!("color resolution is only invoked for color variants"),
        }
    }
}
/// Validate and extract the COLOR-002 curve table: exactly two scalar columns
/// named `x` and `y`, 2..=CURVES_MAX_POINTS rows, strictly increasing `x`,
/// both coordinates in [0,1].
fn curve_points(table: &crate::DataTable) -> Option<Vec<[f64; 2]>> {
    if table.columns.len() != 2
        || table.columns.get("x") != Some(&ValueType::Scalar)
        || table.columns.get("y") != Some(&ValueType::Scalar)
        || !(2..=CURVES_MAX_POINTS).contains(&table.rows.len())
    {
        return None;
    }
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        if row.len() != 2 {
            return None;
        }
        let (Some(Value::Scalar(x)), Some(Value::Scalar(y))) = (row.get("x"), row.get("y")) else {
            return None;
        };
        let (x, y) = (x.get(), y.get());
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return None;
        }
        if let Some(last) = points.last()
            && last[0] >= x
        {
            return None;
        }
        points.push([x, y]);
    }
    Some(points)
}
impl Effect {
    pub fn definition(&self) -> Result<&EffectDefinition, EffectError> {
        match self {
            Self::Known(d) => {
                d.ensure_supported()?;
                Ok(d)
            }
            Self::Opaque(_) => Err(EffectError::UnsupportedFeature),
        }
    }
}
/// Identity two-point curve table used as the COLOR-002 curves default.
fn curve_default() -> Value {
    let f = |v| FiniteF64::new(v).expect("finite curve default");
    let columns = BTreeMap::from([
        ("x".to_string(), ValueType::Scalar),
        ("y".to_string(), ValueType::Scalar),
    ]);
    let row = |x, y| {
        BTreeMap::from([
            ("x".to_string(), Value::Scalar(f(x))),
            ("y".to_string(), Value::Scalar(f(y))),
        ])
    };
    Value::DataTable(crate::DataTable {
        columns,
        rows: vec![row(0.0, 0.0), row(1.0, 1.0)],
    })
}
pub fn effect_descriptors() -> Vec<PropertyDescriptor> {
    let f = |v| FiniteF64::new(v).expect("finite effect default");
    [
        (
            0xf0000000_0010_4100_8000_000000000001,
            "sigma",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4100_8000_000000000002,
            "offset",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4100_8000_000000000003,
            "color",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000004,
            "opacity",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000005,
            "exposure",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000006,
            "exposure_offset",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000007,
            "in_black",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000008,
            "in_white",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000009,
            "gamma",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000a,
            "out_black",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000b,
            "out_white",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000c,
            "curve",
            curve_default(),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000d,
            "hue_shift",
            Value::Angle(f(0.0)),
            Unit::Degrees,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000e,
            "saturation",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000f,
            "lightness",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        // FX-005 keying descriptors (ADR-0115).
        (
            0xf0000000_0010_4300_8000_000000000001,
            "key_color",
            Value::Color(Color::from_srgb8([0, 177, 64], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000002,
            "key_luma",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000003,
            "similarity",
            Value::Scalar(f(0.4)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000004,
            "tolerance",
            Value::Scalar(f(0.1)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000005,
            "edge_shrink",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000006,
            "edge_feather",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000007,
            "spill",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        // FX-006 standard effect descriptors (ADR-0115).
        (
            0xf0000000_0010_4300_8000_000000000008,
            "threshold",
            Value::Scalar(f(0.8)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000009,
            "radius",
            Value::Scalar(f(8.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000a,
            "intensity",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000b,
            "amount",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000c,
            "midpoint",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000d,
            "feather",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000e,
            "roundness",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000f,
            "top_left",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000010,
            "top_right",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000011,
            "bottom_right",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000012,
            "bottom_left",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
    ]
    .into_iter()
    .map(|(id, name, value, unit)| {
        let mut d = DescriptorDefinition::new(
            DescriptorId::from_uuid(uuid::Uuid::from_u128(id)),
            SchemaKey::new(format!("kronello.effect.{name}")).expect("effect key"),
            name,
            value.value_type(),
            unit,
            value,
        );
        d.repeatable = true;
        if name == "offset" {
            d.coordinate_space = Some(crate::CoordinateSpace::LocalDesign);
        }
        // Corner pins are absolute Composition design_px positions.
        if matches!(
            name,
            "top_left" | "top_right" | "bottom_right" | "bottom_left"
        ) {
            d.coordinate_space = Some(crate::CoordinateSpace::CompositionDesign);
        }
        if name == "sigma" || name == "opacity" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, if name == "opacity" { 1.0 } else { 1_000_000.0 })
                    .expect("effect range"),
            ));
        }
        if matches!(
            name,
            "edge_shrink"
                | "edge_feather"
                | "radius"
                | "threshold"
                | "intensity"
                | "amount"
                | "feather"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, 1_000_000.0).expect("effect range"),
            ));
        }
        if matches!(
            name,
            "similarity" | "spill" | "key_luma" | "tolerance" | "midpoint" | "roundness"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, 1.0).expect("effect range"),
            ));
        }
        if name == "gamma" {
            let mut bound = NumericRange::inclusive(0.0, 1_000_000.0).expect("gamma range");
            bound.min.as_mut().expect("min").inclusive = false;
            d.range = Some(ValueRange::Scalar(bound));
        }
        PropertyDescriptor::new(d).expect("effect descriptor")
    })
    .collect()
}

// Decode JSON directly, avoiding serde's untagged Content buffer so future
// parameters keep arbitrary-precision numbers just like DocumentObject.
impl<'de> Deserialize<'de> for Effect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(deserializer)?;
        let definition = serde_json::from_str::<EffectDefinition>(&raw.to_string());
        Ok(match definition {
            Ok(d) => Self::Known(d),
            Err(_) => Self::Opaque(raw),
        })
    }
}
