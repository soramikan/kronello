use crate::{ModelError, Property, PropertySource, SchemaRegistry, Value};
use serde::{Deserialize, Serialize};

pub const BLEND_VERSION: u32 = 1;
pub const BLEND_KEY: &str = "kronello.blend_mode";

/// Blending operates in the explicitly selected linear working space.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
}
impl BlendMode {
    pub fn from_value(value: &Value) -> Result<Self, ModelError> {
        match value {
            Value::Enum(value) => match value.as_str() {
                "normal" => Ok(Self::Normal),
                "multiply" => Ok(Self::Multiply),
                "screen" => Ok(Self::Screen),
                _ => Err(ModelError::InvalidBlendMode),
            },
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
