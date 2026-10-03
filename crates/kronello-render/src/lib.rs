//! Immutable scene compilation and image-sequence export. Concrete execution
//! is injected through RenderBackend; this crate imports no GPU or store API.
mod cache;
mod dag;
mod output;
mod snapshot;
mod template;

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
    Shape(#[from] ShapeError),
    #[error(transparent)]
    Text(#[from] TextError),
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
impl RenderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Template(e) => e.code(),
            Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            Self::UnsupportedSchema(_) => "UNSUPPORTED_SCHEMA",
            Self::Evaluation(e) => e.code(),
            Self::Layout(LayoutError::MissingFont { .. }) => "ASSET_MISSING",
            Self::Layout(
                LayoutError::FontHashMismatch { .. } | LayoutError::FontIdentityMismatch { .. },
            ) => "ASSET_HASH_MISMATCH",
            Self::Layout(LayoutError::MissingGlyphs { .. }) => "GLYPH_MISSING",
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

pub trait RenderBackend {
    fn name(&self) -> &str;
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError>;
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
