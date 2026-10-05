//! Backend-independent document types, identifiers, and snapshot contracts.
//!
//! Property schemas are validated at construction and deserialization. Property
//! references must additionally be validated against a [`SchemaRegistry`]; this
//! crate stores source and modifier descriptions but never evaluates them.

mod builtin;
mod composition;
mod curve;
mod error;
mod expression;
mod id;
mod property;
mod schema;
mod value;
mod wire;

pub use builtin::{
    AUDIO_VOLUME_ID, FILL_COLOR_ID, OPACITY_ID, STROKE_WIDTH_ID, TRANSFORM_ANCHOR_ID,
    TRANSFORM_POSITION_ID, TRANSFORM_ROTATION_ID, TRANSFORM_SCALE_ID, TRANSFORM_SKEW_ID,
};
pub use composition::{
    Composition, CompositionError, CompositionInstance, CompositionReference, DesignExtent,
    InstancePath, MediaNode, NodeKind, ParentGraph, PropertyKey, SceneNode, validate_compositions,
};
pub use curve::{
    AnimationCurve, CurveDefinition, CurveError, CurveInterpolation, INTERPOLATION_VERSION,
    Keyframe, TimeBezier,
};
pub use error::{JsonError, ModelError, from_json};
pub use expression::{
    EXPRESSION_VERSION, Expression, ExpressionBudget, ExpressionDependency, ExpressionError,
    ExpressionNode, expression_value_bytes,
};
pub use id::{
    AssetId, ClipId, CompositionId, CompositionInstanceId, ContentId, CurveId, DescriptorId,
    ExpressionId, ModifierId, NodeId, PropertyId, SchemaKey, SequenceId, TrackId,
};
pub use property::{DescriptorRef, Modifier, Property, PropertySource, SourceResolver};
pub use schema::{
    Capabilities, CoordinateSpace, DescriptorDefinition, InterpolationMode, NumericBound,
    NumericRange, PropertyDescriptor, SchemaRegistry, Unit, ValueRange,
};
pub use value::{
    Color, ColorComponents, ColorSpace, DataTable, FiniteF64, Path, PathSegment, Value, ValueType,
};

mod project;
pub use project::{
    DocumentObject, OpaqueObject, PROJECT_SCHEMA_VERSION, PROJECT_SEMANTIC_VERSION, Project,
    ProjectError, project_json_schema,
};

mod shape;
pub use shape::{
    Fill, FillRule, Gradient, GradientGeometry, GradientInterpolation, GradientOptions,
    GradientSpread, GradientStop, GradientUnits, ResolvedFill, ResolvedGeometry, ResolvedGradient,
    ResolvedGradientStop, ResolvedShape, ResolvedStroke, Shape, ShapeError, ShapeGeometry, Stroke,
    StrokeCap, StrokeJoin, shape_descriptors, validate_path, validate_shape_contents,
};

mod text;
pub use text::{
    FontRef, ResolvedText, ResolvedTextStyle, RubyAssociation, TEXT_LAYOUT_VERSION, TextAlignment,
    TextDirection, TextDocument, TextError, TextRange, TextStyleSpan, text_descriptors,
    validate_text_contents,
};

mod effect;
mod template;
pub use effect::*;
pub use template::*;
mod asset;
pub use asset::{Asset, AssetKind, AssetLocator, StreamMetadata};

mod sequence;
pub use sequence::*;
