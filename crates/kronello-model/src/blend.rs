use crate::{ModelError, Property, PropertySource, SchemaRegistry, Value};
use serde::{Deserialize, Serialize};

pub const BLEND_VERSION: u32 = 1;
pub const BLEND_KEY: &str = "kronello.blend_mode";

/// Blending operates in the explicitly selected linear working space. The
/// separable and non-separable modes follow W3C Compositing and Blending
/// (ADR-0109); HDR and negative straight values are never clamped.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Overlay,
    LinearDodge,
    LinearBurn,
    VividLight,
    LinearLight,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
impl BlendMode {
    /// Wire names are snake_case; the set is closed (ADR-0109).
    pub fn from_value(value: &Value) -> Result<Self, ModelError> {
        match value {
            Value::Enum(value) => Ok(match value.as_str() {
                "normal" => Self::Normal,
                "multiply" => Self::Multiply,
                "screen" => Self::Screen,
                "darken" => Self::Darken,
                "lighten" => Self::Lighten,
                "color_dodge" => Self::ColorDodge,
                "color_burn" => Self::ColorBurn,
                "hard_light" => Self::HardLight,
                "soft_light" => Self::SoftLight,
                "difference" => Self::Difference,
                "exclusion" => Self::Exclusion,
                "overlay" => Self::Overlay,
                "linear_dodge" => Self::LinearDodge,
                "linear_burn" => Self::LinearBurn,
                "vivid_light" => Self::VividLight,
                "linear_light" => Self::LinearLight,
                "hue" => Self::Hue,
                "saturation" => Self::Saturation,
                "color" => Self::Color,
                "luminosity" => Self::Luminosity,
                _ => Err(ModelError::InvalidBlendMode)?,
            }),
            _ => Err(ModelError::InvalidBlendMode),
        }
    }
    /// Absence preserves source-over in legacy documents. The mode is authored,
    /// not animated, and exactly one property may select it on each layer.
    pub fn from_properties(properties: &[Property]) -> Result<Self, ModelError> {
        let mut mode = Self::Normal;
        let mut found = false;
        for property in properties
            .iter()
            .filter(|p| p.descriptor().key.as_str() == BLEND_KEY)
        {
            if found {
                return Err(ModelError::InvalidBlendMode);
            }
            found = true;
            property.validate(&SchemaRegistry::with_builtin())?;
            let PropertySource::Constant(value) = property.source() else {
                return Err(ModelError::SourceNotAllowed);
            };
            mode = Self::from_value(value)?;
        }
        Ok(mode)
    }
}
