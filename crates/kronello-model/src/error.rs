use crate::{CurveId, ExpressionId, InterpolationMode, SchemaKey, Unit, ValueType};
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Domain errors retain machine-readable categories and offending values.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ModelError {
    #[error("blend mode must be one constant normal, multiply, or screen property")]
    InvalidBlendMode,
    #[error("numeric values must be finite: {value}")]
    NonFinite { value: f64 },
    #[error("invalid stable schema key: {key}")]
    InvalidSchemaKey { key: String },
    #[error("expected {expected:?}, got {actual:?}")]
    ValueTypeMismatch {
        expected: ValueType,
        actual: ValueType,
    },
    #[error("unit {unit:?} is incompatible with {value_type:?}")]
    IncompatibleUnit { value_type: ValueType, unit: Unit },
    #[error("invalid coordinate space for this type or unit")]
    IncompatibleCoordinateSpace,
    #[error("{mode:?} interpolation is incompatible with {value_type:?}")]
    IncompatibleInterpolation {
        value_type: ValueType,
        mode: InterpolationMode,
    },
    #[error("an animatable descriptor needs at least one interpolation mode")]
    MissingInterpolation,
    #[error("non-animatable descriptors cannot allow curve or expression sources")]
    IncompatibleCapabilities,
    #[error("explicit color interpolation space must be linear; other types cannot set it")]
    InvalidColorInterpolationSpace,
    #[error("invalid or empty numeric interval")]
    InvalidRange,
    #[error("range shape does not match {value_type:?}")]
    IncompatibleRange { value_type: ValueType },
    #[error("component {component} is outside its declared range: {value}")]
    OutOfRange { component: usize, value: f64 },
    #[error("descriptor version {version} is unsupported")]
    UnsupportedDescriptorVersion { version: u32 },
    #[error("duplicate schema key: {key}")]
    DuplicateSchemaKey { key: SchemaKey },
    #[error("duplicate descriptor ID")]
    DuplicateDescriptorId,
    #[error("schema key not found: {key}")]
    DescriptorNotFound { key: SchemaKey },
    #[error("descriptor {key} version mismatch: requested {requested}, registered {registered}")]
    DescriptorVersionMismatch {
        key: SchemaKey,
        requested: u32,
        registered: u32,
    },
    #[error("source is not allowed by this descriptor")]
    SourceNotAllowed,
    #[error("modifiers are not allowed by this descriptor")]
    ModifiersNotAllowed,
    #[error("duplicate modifier ID")]
    DuplicateModifierId,
    #[error("modifier version must be nonzero")]
    InvalidModifierVersion,
    #[error("curve not found: {id}")]
    CurveNotFound { id: CurveId },
    #[error("expression not found: {id}")]
    ExpressionNotFound { id: ExpressionId },
    #[error("invalid sRGB hex input")]
    InvalidSrgbHex,
}

/// A rejected document is never returned as a partially decoded success.
/// Callers must retain the original input on failure (ADR-0045).
#[derive(Debug, Error)]
pub enum JsonError {
    #[error("invalid JSON")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("JSON structure cannot be safely represented by this model")]
    IncompatibleStructure {
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Validation(#[from] ModelError),
}

/// Strict JSON boundary. Unknown fields/variants and unsupported structures are
/// typed compatibility failures, rather than being silently discarded.
pub fn from_json<T: DeserializeOwned>(input: &str) -> Result<T, JsonError> {
    serde_json::from_str(input).map_err(|source| {
        if source.is_data() {
            JsonError::IncompatibleStructure { source }
        } else {
            JsonError::InvalidJson { source }
        }
    })
}
