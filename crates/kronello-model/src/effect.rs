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
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolvedEffect {
    GaussianBlur {
        sigma: f64,
    },
    DropShadow {
        sigma: f64,
        offset: [f64; 2],
        color: Color,
        opacity: f64,
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
        let id = match self.parameters {
            EffectParameters::GaussianBlur { .. } => GAUSSIAN_BLUR_ID,
            EffectParameters::DropShadow { .. } => DROP_SHADOW_ID,
        };
        if self.effect_id != id || self.version != EFFECT_VERSION {
            return Err(EffectError::UnsupportedFeature);
        }
        Ok(())
    }
    fn references(&self) -> Vec<(PropertyId, ValueType, Unit)> {
        match self.parameters {
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
        let sigma_id = match self.parameters {
            EffectParameters::GaussianBlur { sigma }
            | EffectParameters::DropShadow { sigma, .. } => sigma,
        };
        let sigma = scalar(sigma_id)?;
        if !(0.0..=1_000_000.0).contains(&sigma) {
            return Err(EffectError::InvalidParameter(sigma_id));
        }
        Ok(match self.parameters {
            EffectParameters::GaussianBlur { .. } => ResolvedEffect::GaussianBlur { sigma },
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
                ResolvedEffect::DropShadow {
                    sigma,
                    offset: offset_value,
                    color: color_value,
                    opacity: opacity_value,
                }
            }
        })
    }
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
        if name == "offset" {
            d.coordinate_space = Some(crate::CoordinateSpace::LocalDesign);
        }
        if name == "sigma" || name == "opacity" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, if name == "opacity" { 1.0 } else { 1_000_000.0 })
                    .expect("effect range"),
            ));
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
