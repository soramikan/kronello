//! Backend-independent document types, identifiers, and snapshot contracts.
//!
//! Property schemas are validated at construction and deserialization. Property
//! references must additionally be validated against a [`SchemaRegistry`]; this
//! crate stores source and modifier descriptions but never evaluates them.

mod builtin;
mod composition;
mod curve;
mod error;
mod id;
mod property;
mod schema;
mod value;

pub use builtin::{
    FILL_COLOR_ID, OPACITY_ID, STROKE_WIDTH_ID, TRANSFORM_ANCHOR_ID, TRANSFORM_POSITION_ID,
    TRANSFORM_ROTATION_ID, TRANSFORM_SCALE_ID, TRANSFORM_SKEW_ID,
};
pub use composition::{
    Composition, CompositionError, CompositionInstance, CompositionReference, DesignExtent,
    InstancePath, NodeKind, ParentGraph, PropertyKey, SceneNode, validate_compositions,
};
pub use curve::{
    AnimationCurve, CurveDefinition, CurveError, CurveInterpolation, INTERPOLATION_VERSION,
    Keyframe, TimeBezier,
};
pub use error::{JsonError, ModelError, from_json};
pub use id::{
    AssetId, CompositionId, CompositionInstanceId, ContentId, CurveId, DescriptorId, ExpressionId,
    ModifierId, NodeId, PropertyId, SchemaKey,
};
pub use property::{DescriptorRef, Modifier, Property, PropertySource, SourceResolver};
pub use schema::{
    Capabilities, CoordinateSpace, DescriptorDefinition, InterpolationMode, NumericBound,
    NumericRange, PropertyDescriptor, SchemaRegistry, Unit, ValueRange,
};
pub use value::{
    Color, ColorComponents, ColorSpace, FiniteF64, Path, PathSegment, Value, ValueType,
};

mod project;
pub use project::{
    DocumentObject, OpaqueObject, PROJECT_SCHEMA_VERSION, PROJECT_SEMANTIC_VERSION, Project,
    ProjectError, project_json_schema,
};
