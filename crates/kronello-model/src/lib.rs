//! Backend-independent document types, identifiers, and snapshot contracts.
//!
//! Property schemas are validated at construction and deserialization. Property
//! references must additionally be validated against a [`SchemaRegistry`]; this
//! crate stores source and modifier descriptions but never evaluates them.

mod builtin;
mod error;
mod id;
mod property;
mod schema;
mod value;

pub use builtin::{
    FILL_COLOR_ID, OPACITY_ID, STROKE_WIDTH_ID, TRANSFORM_ANCHOR_ID, TRANSFORM_POSITION_ID,
    TRANSFORM_ROTATION_ID, TRANSFORM_SCALE_ID, TRANSFORM_SKEW_ID,
};
pub use error::{JsonError, ModelError, from_json};
pub use id::{AssetId, CurveId, DescriptorId, ExpressionId, ModifierId, PropertyId, SchemaKey};
pub use property::{DescriptorRef, Modifier, Property, PropertySource, SourceResolver};
pub use schema::{
    Capabilities, CoordinateSpace, DescriptorDefinition, InterpolationMode, NumericBound,
    NumericRange, PropertyDescriptor, SchemaRegistry, Unit, ValueRange,
};
pub use value::{
    Color, ColorComponents, ColorSpace, FiniteF64, Path, PathSegment, Value, ValueType,
};
