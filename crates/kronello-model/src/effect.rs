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
        let sigma_id = match self.parameters {
            // Audio effects are executed only by the audio evaluator.
            EffectParameters::AudioGain { .. } => return Err(EffectError::UnsupportedFeature),
            EffectParameters::GaussianBlur { sigma }
            | EffectParameters::DropShadow { sigma, .. } => sigma,
            EffectParameters::ColorExposure { .. }
            | EffectParameters::ColorLevels { .. }
            | EffectParameters::ColorCurves { .. }
            | EffectParameters::ColorHsl { .. } => unreachable!("handled above"),
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
            EffectParameters::ColorExposure { .. }
            | EffectParameters::ColorLevels { .. }
            | EffectParameters::ColorCurves { .. }
            | EffectParameters::ColorHsl { .. } => unreachable!("handled above"),
        })
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
        if name == "sigma" || name == "opacity" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, if name == "opacity" { 1.0 } else { 1_000_000.0 })
                    .expect("effect range"),
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
