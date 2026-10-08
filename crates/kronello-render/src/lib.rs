//! Immutable scene compilation and image-sequence export. Concrete execution
//! is injected through RenderBackend; this crate imports no GPU or store API.
mod hdr;
pub use hdr::*;
mod bounds;
mod cache;
pub use bounds::{DesignBounds, LayoutValue};
mod dag;
mod effect;
mod inspect;
pub use inspect::*;
mod output;
mod temporal;
pub use temporal::*;
mod scopes;
mod simulation;
mod snapshot;
mod template;
pub use effect::*;
pub use scopes::*;

pub use cache::*;
pub use dag::*;
pub use output::*;
pub use snapshot::*;

use kronello_eval::EvaluationError;
use kronello_model::{ShapeError, TextError};
use kronello_text::LayoutError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("text {node} line {line} advances {advance} beyond wrap width {wrap_width}")]
    LayoutOverflow {
        node: kronello_model::NodeId,
        instance_path: kronello_model::InstancePath,
        line: usize,
        advance: f64,
        wrap_width: f64,
    },
    #[error("visual bounds follower requires an invertible parent transform")]
    SingularLayoutTransform,
    #[error(transparent)]
    Sequence(#[from] kronello_model::SequenceError),
    #[error(transparent)]
    Template(#[from] kronello_template::TemplateError),
    #[error("UNSUPPORTED_FEATURE: {0}")]
    UnsupportedFeature(String),
    #[error("unsupported snapshot structure: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid render input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Evaluation(#[from] EvaluationError),
    #[error(transparent)]
    Effect(#[from] kronello_model::EffectError),
    #[error(transparent)]
    Shape(#[from] ShapeError),
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Caption(#[from] kronello_model::CaptionError),
    #[error(transparent)]
    Layout(#[from] LayoutError),
    #[error(transparent)]
    Vector(#[from] kronello_vector::VectorError),
    #[error(transparent)]
    Time(#[from] kronello_time::TimeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Png(#[from] png::EncodingError),
    #[error("{code}: {message}")]
    Backend { code: &'static str, message: String },
}
impl From<kronello_model::LutError> for RenderError {
    fn from(e: kronello_model::LutError) -> Self {
        match e {
            kronello_model::LutError::Invalid(message) => Self::Backend {
                code: "INVALID_LUT",
                message,
            },
            kronello_model::LutError::UnsupportedFeature(message) => {
                Self::UnsupportedFeature(message)
            }
        }
    }
}
impl RenderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LayoutOverflow { .. } => "LAYOUT_OVERFLOW",
            Self::SingularLayoutTransform => "LAYOUT_SINGULAR_TRANSFORM",
            Self::Sequence(e) => e.code(),
            Self::Template(e) => e.code(),
            Self::Shape(ShapeError::UnsupportedGradientVersion)
            | Self::Shape(ShapeError::UnsupportedStrokeVersion)
            | Self::Text(TextError::Gradient(ShapeError::UnsupportedGradientVersion))
            | Self::Effect(kronello_model::EffectError::UnsupportedFeature)
            | Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            Self::UnsupportedSchema(_) => "UNSUPPORTED_SCHEMA",
            Self::Shape(ShapeError::MorphCorrespondence { .. }) => "PATH_MORPH_CORRESPONDENCE",
            Self::Shape(ShapeError::InvalidTrimRange) => "PATH_TRIM_RANGE",
            Self::Shape(ShapeError::InvalidDashArray) => "STROKE_INVALID_DASH",
            Self::Shape(ShapeError::StrokeBudgetExceeded) => "STROKE_BUDGET_EXCEEDED",
            Self::Shape(ShapeError::OpenStrokeAlignment) => "STROKE_OPEN_ALIGNMENT",
            Self::Evaluation(e) => e.code(),
            Self::Layout(LayoutError::MissingFont { .. }) => "ASSET_MISSING",
            Self::Layout(
                LayoutError::FontHashMismatch { .. } | LayoutError::FontIdentityMismatch { .. },
            ) => "ASSET_HASH_MISMATCH",
            Self::Layout(LayoutError::MissingGlyphs { .. }) => "GLYPH_MISSING",
            Self::Caption(e) => e.code(),
            Self::Layout(
                LayoutError::UnsupportedFeature { .. }
                | LayoutError::UnsupportedVersion { .. }
                | LayoutError::UnsupportedGlyphOutline { .. },
            ) => "UNSUPPORTED_FEATURE",
            Self::Shape(ShapeError::MissingContent { .. })
            | Self::Text(TextError::MissingContent { .. }) => "ASSET_MISSING",
            Self::Backend { code, .. } => code,
            Self::Io(_) => "OUTPUT_IO_ERROR",
            _ => "RENDER_ERROR",
        }
    }
}

/// Both buffers have output pixel dimensions. Linear is working-space
/// premultiplied numeric truth; display is encoded straight sRGB. Neither is
/// gamut-clipped here. CPU reference execution must be explicitly selected.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendFrame {
    pub linear: Vec<[f32; 4]>,
    pub display: Vec<[f32; 4]>,
}

/// Actual execution transfers; GPU compute writes do not count as copies.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct RenderTransferStats {
    pub cpu_upload_pixel_bytes: u64,
    pub cpu_upload_pixel_operations: u64,
    pub cpu_upload_control_bytes: u64,
    pub cpu_upload_control_operations: u64,
    pub gpu_copy_bytes: u64,
    pub gpu_copy_operations: u64,
    pub gpu_readback_bytes: u64,
    pub gpu_readback_operations: u64,
    #[serde(default)]
    pub gpu_wait_operations: u64,
    #[serde(default)]
    pub gpu_compute_dispatches: u64,
}
impl RenderTransferStats {
    /// Difference between monotonically accumulated execution observations.
    pub fn since(&self, before: &Self) -> Self {
        Self {
            cpu_upload_pixel_bytes: self
                .cpu_upload_pixel_bytes
                .saturating_sub(before.cpu_upload_pixel_bytes),
            cpu_upload_pixel_operations: self
                .cpu_upload_pixel_operations
                .saturating_sub(before.cpu_upload_pixel_operations),
            cpu_upload_control_bytes: self
                .cpu_upload_control_bytes
                .saturating_sub(before.cpu_upload_control_bytes),
            cpu_upload_control_operations: self
                .cpu_upload_control_operations
                .saturating_sub(before.cpu_upload_control_operations),
            gpu_copy_bytes: self.gpu_copy_bytes.saturating_sub(before.gpu_copy_bytes),
            gpu_copy_operations: self
                .gpu_copy_operations
                .saturating_sub(before.gpu_copy_operations),
            gpu_readback_bytes: self
                .gpu_readback_bytes
                .saturating_sub(before.gpu_readback_bytes),
            gpu_readback_operations: self
                .gpu_readback_operations
                .saturating_sub(before.gpu_readback_operations),
            gpu_wait_operations: self
                .gpu_wait_operations
                .saturating_sub(before.gpu_wait_operations),
            gpu_compute_dispatches: self
                .gpu_compute_dispatches
                .saturating_sub(before.gpu_compute_dispatches),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PersistentRasterCachePolicy {
    #[default]
    MemoryOnly,
    Enabled,
    ExplicitlyDisabled,
    DefaultDirectoryOverlapsProject,
    DisabledForGpuResident,
}
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RenderResourceCacheStats {
    pub gpu_textures: CacheStats,
    pub gpu_pool: CacheStats,
    pub disk_raster: CacheStats,
    pub rejected_disk_entries: u64,
    #[serde(default)]
    pub persistent_disk_policy: PersistentRasterCachePolicy,
}
/// RAII observation ownership; platform locks remain inside the concrete backend.
pub trait RenderObservationScope {}
pub trait RenderBackend {
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn RenderObservationScope + '_>>, RenderError> {
        Ok(None)
    }
    fn requires_gpu_resident(&self) -> bool {
        false
    }
    fn transfer_stats(&self) -> Option<RenderTransferStats> {
        None
    }
    /// Monotonic execution totals for request-local tile/temporal deltas.
    /// None keeps older injected backends' explicit last-execution semantics.
    fn transfer_stats_total(&self) -> Option<RenderTransferStats> {
        None
    }
    fn resource_cache_stats(&self) -> Option<RenderResourceCacheStats> {
        None
    }
    fn name(&self) -> &str;
    /// Opt in with a stable numeric implementation and device/driver identity.
    fn cache_namespace(&self) -> Option<String> {
        None
    }
    fn input_path(&self) -> &str {
        "semantic_scene"
    }
    fn image_input_path(&self) -> &str {
        self.input_path()
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError>;
    /// Convert the accumulated working-space premultiplied image once.
    fn display_from_linear(
        &self,
        _linear: &[[f32; 4]],
        _working: kronello_model::ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        Err(RenderError::UnsupportedFeature(
            "temporal output transform".into(),
        ))
    }
    /// Backends opt in only with a stable execution namespace/fingerprint.
    /// The default deliberately does not cache device results.
    fn execute_with_cache(
        &self,
        dag: &RenderDag,
        _cache: &mut RenderCache,
    ) -> Result<BackendFrame, RenderError> {
        self.execute(dag)
    }
}

mod media;
pub use media::{DecodedVideoFrame, VideoDecodeBackend, VideoImage};

mod sequence;
pub use sequence::{RenderTarget, lower_sequence};

mod source;
pub use source::{ResolvedSource, SourcePreviewRef, lower_source, resolve_source};
