//! Pure semantic evaluation of immutable composition definitions.
//!
//! Compilation owns no playback state. Each query allocates its own memo table,
//! keyed by placement identity, and maps rational time without frame snapping.
//! This is a semantic scene, not a renderer or the persisted RenderSnapshot.

mod graph;
mod scene;

pub use graph::{DependencyDeclarations, DependencyGraph, EvaluationSnapshot, ReferenceBindings};
pub use scene::{Affine2, EvaluatedNode, EvaluatedScene, NodeKey, TransformValues};

use kronello_animation::AnimationError;
use kronello_model::{
    CompositionError, CompositionId, InstancePath, ModelError, PropertyId, PropertyKey,
};
use kronello_time::TimeError;
use thiserror::Error;

/// Composition inputs have no owning NodeId; node properties reuse the model's
/// exact (InstancePath, NodeId, PropertyId) key without synthesizing identities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimePropertyKey {
    Node(PropertyKey),
    Composition {
        instance_path: InstancePath,
        composition: CompositionId,
        property: PropertyId,
    },
}
impl RuntimePropertyKey {
    pub fn instance_path(&self) -> &InstancePath {
        match self {
            Self::Node(key) => &key.instance_path,
            Self::Composition { instance_path, .. } => instance_path,
        }
    }
}
impl From<PropertyKey> for RuntimePropertyKey {
    fn from(key: PropertyKey) -> Self {
        Self::Node(key)
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum EvaluationError {
    #[error("invalid composition definitions: {0:?}")]
    InvalidCompositions(Vec<CompositionError>),
    #[error("composition not found: {0}")]
    CompositionNotFound(CompositionId),
    #[error("duplicate curve ID: {0}")]
    DuplicateCurveId(kronello_model::CurveId),
    #[error("property not found: {0:?}")]
    PropertyNotFound(RuntimePropertyKey),
    /// A closed path: the final key repeats the first. Each consecutive pair
    /// identifies the dependent property and its responsible upstream property.
    #[error("property dependency cycle: {path:?}")]
    DependencyCycle { path: Vec<RuntimePropertyKey> },
    #[error("reference must override an existing placement input and read its parent scope: {0:?}")]
    InvalidReferenceBinding(RuntimePropertyKey),
    #[error("duplicate descriptor on node {node:?}: {descriptor}")]
    DuplicateNodeDescriptor { node: NodeKey, descriptor: String },
    #[error("property {key:?}: {source}")]
    InvalidValue {
        key: RuntimePropertyKey,
        source: ModelError,
    },
    #[error("property {key:?}: {source}")]
    Animation {
        key: RuntimePropertyKey,
        source: AnimationError,
    },
    #[error("UNSUPPORTED_FEATURE: {feature}, property {key:?}")]
    UnsupportedFeature {
        key: RuntimePropertyKey,
        feature: String,
    },
    #[error("instance path not found: {0:?}")]
    InstancePathNotFound(InstancePath),
    #[error("time mapping for {instance_path:?}: {source}")]
    TimeMapping {
        instance_path: InstancePath,
        source: TimeError,
    },
    #[error("non-finite affine transform on {node:?}")]
    NonFiniteTransform { node: NodeKey },
    #[error("working color space must be linear")]
    InvalidWorkingColorSpace,
}
impl EvaluationError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFeature { .. } => "UNSUPPORTED_FEATURE",
            Self::DependencyCycle { .. } => "PROPERTY_DEPENDENCY_CYCLE",
            _ => "EVALUATION_ERROR",
        }
    }
}
