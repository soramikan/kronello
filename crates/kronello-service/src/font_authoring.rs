//! Explicit local font import identity; no discovery, URL fetch, or document edit.
use crate::ServiceError;
use kronello_model::FontRef;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FontPinRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub face_index: u32,
}

pub(crate) fn pin(request: FontPinRequest) -> Result<FontRef, ServiceError> {
    let metadata = std::fs::metadata(&request.path)
        .map_err(|e| ServiceError::new("FONT_MISSING", e.to_string()))?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
        return Err(ServiceError::new(
            "FONT_INVALID",
            "font must be a regular local file no larger than 64 MiB",
        ));
    }
    let bytes = std::fs::read(&request.path)
        .map_err(|e| ServiceError::new("FONT_MISSING", e.to_string()))?;
    kronello_text::pin_font(&bytes, request.face_index)
        .map_err(|e| ServiceError::new("FONT_INVALID", e.to_string()))
}
