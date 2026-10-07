//! Backend-independent document types, identifiers, and snapshot contracts.
//!
//! Property schemas are validated at construction and deserialization. Property
//! references must additionally be validated against a [`SchemaRegistry`]; this
//! crate stores source and modifier descriptions but never evaluates them.

mod blend;
mod builtin;
pub use blend::{BLEND_KEY, BLEND_VERSION, BlendMode};
mod composition;
mod curve;
mod error;
mod expression;
mod expression_data;
mod expression_syntax;
pub use expression_data::{EXPRESSION_DATA_VERSION, ExpressionDataAsset};
pub use expression_syntax::{
    EXPRESSION_TEXT_MAX_BYTES, EXPRESSION_TEXT_MAX_DEPTH, EXPRESSION_TEXT_MAX_TOKENS,
    ExpressionDiagnostic, ExpressionFormatError, ExpressionMetadata, ExpressionSyntaxError,
    ExpressionTextError, format_expression, parse_expression,
};
mod id;
mod property;
mod schema;
mod value;
mod wire;

pub use builtin::{
    AUDIO_VOLUME_ID, BLEND_MODE_ID, FILL_COLOR_ID, OPACITY_ID, STROKE_WIDTH_ID,
    TRANSFORM_ANCHOR_ID, TRANSFORM_POSITION_ID, TRANSFORM_ROTATION_ID, TRANSFORM_SCALE_ID,
    TRANSFORM_SKEW_ID,
};
pub use composition::{
    Composition, CompositionError, CompositionInstance, CompositionReference, DesignExtent,
    InstancePath, MediaNode, NodeKind, ParentGraph, PropertyKey, SceneNode, normalize_search_tags,
    valid_node_tags, validate_compositions,
};
pub use curve::{
    AnimationCurve, CurveDefinition, CurveError, CurveInterpolation, INTERPOLATION_VERSION,
    Keyframe, TimeBezier,
};
pub use error::{JsonError, ModelError, from_json};
pub use expression::{
    EXPRESSION_SUPPORTED_VERSION, EXPRESSION_VERSION, Expression, ExpressionBudget,
    ExpressionDependency, ExpressionError, ExpressionNode, expression_value_bytes,
};
pub use id::{
    AssetId, CaptionId, ClipId, CompositionId, CompositionInstanceId, ContentId, CurveId,
    DescriptorId, ExpressionId, MarkerId, MaskId, ModifierId, NodeId, PropertyId, SchemaKey,
    SequenceId, TrackId,
};
pub use property::{DescriptorRef, Modifier, Property, PropertySource, SourceResolver};
pub use schema::{
    Capabilities, CoordinateSpace, DescriptorDefinition, InterpolationMode, NumericBound,
    NumericRange, PropertyDescriptor, SchemaRegistry, Unit, ValueRange,
};
pub use value::{
    Color, ColorComponents, ColorSpace, DataTable, FiniteF64, Path, PathSegment, Value, ValueType,
};

mod simulation;
pub use simulation::{
    ParticlePropertyInputs, ParticleSimulation, SIMULATION_VERSION, SimulationModelError,
    simulation_descriptors,
};
mod repeater;
pub use repeater::{
    ExpandedRepeatSource, REPEATER_INSTANCE_LIMIT, REPEATER_VERSION, RepeatInstance, RepeatSource,
    Repeater, RepeaterError,
};
mod project;
pub use project::{
    DocumentObject, OpaqueObject, PROJECT_SCHEMA_VERSION, PROJECT_SEMANTIC_VERSION, Project,
    ProjectError, project_json_schema,
};

mod shape;
pub use shape::{
    EXTENDED_STROKE_VERSION, Fill, FillRule, Gradient, GradientGeometry, GradientInterpolation,
    GradientOptions, GradientSpread, GradientStop, GradientUnits, LEGACY_STROKE_VERSION,
    ResolvedFill, ResolvedGeometry, ResolvedGradient, ResolvedGradientStop, ResolvedShape,
    ResolvedStroke, ResolvedStrokeOptions, Shape, ShapeError, ShapeGeometry, Stroke,
    StrokeAlignment, StrokeCap, StrokeJoin, StrokeOptions, morph_paths, shape_descriptors,
    validate_dash_array, validate_path, validate_shape_contents,
};

mod text;
pub use text::{
    CharacterAnimation, FontRef, ResolvedCharacterAnimation, ResolvedText, ResolvedTextStyle,
    RubyAssociation, TEXT_ADVANCED_LAYOUT_VERSION, TEXT_LAYOUT_VERSION, TextAlignment,
    TextDirection, TextDocument, TextError, TextRange, TextStyleSpan, text_descriptors,
    validate_text_contents,
};

mod caption;
pub use caption::{
    CAPTION_ANCHOR_ID, CAPTION_BACKGROUND_ID, CAPTION_BOLD_WIDTH_RATIO, CAPTION_COLOR_ID,
    CAPTION_FONT_ID, CAPTION_FONT_SIZE_ID, CAPTION_ITALIC_SHEAR, CAPTION_LINE_HEIGHT_RATIO,
    CAPTION_OFFSET_ID, CAPTION_OUTLINE_COLOR_ID, CAPTION_OUTLINE_WIDTH_ID,
    CAPTION_SAFE_AREA_INSET_ID, CAPTION_VERSION, CaptionAnchor, CaptionDocument, CaptionError,
    CaptionFormat, CaptionOutline, CaptionPlacement, CaptionSpan, CaptionSpanFlags, CaptionStyle,
    ResolvedCaption, ResolvedCaptionPlacement, caption_descriptors, validate_caption_contents,
};

mod effect;
mod lut;
mod template;
pub use effect::*;
pub use lut::{CubeLut, LUT_3D_DOCUMENT_MAX_SIZE, LUT_3D_MAX_SIZE, LUT_3D_MIN_SIZE, LutError};
pub use template::*;
mod asset;
pub use asset::{Asset, AssetKind, AssetLocator, StreamMetadata};

mod sequence;
pub use sequence::*;

mod audio_analysis;
pub use audio_analysis::*;

mod proxy;
pub use proxy::*;

mod tracking;
pub use tracking::*;

mod matte;
pub use matte::*;
mod mask;
pub use mask::*;
