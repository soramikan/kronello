//! MEDIA-001 operations, shared by all transports. Filesystem/native work is
//! performed by media; project persistence remains in the shared store service.
use crate::{ProjectInfo, ServiceError, info, open_existing, parse_revision};
use kronello_media::{
    CollectedProject, MediaCapabilities, MediaRuntime, collect_project, relink_asset,
};
use kronello_model::{AssetId, DocumentObject};
use kronello_store::{OpenOptions, ProjectStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesRequest {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub asset: AssetId,
    pub search_directory: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectRequest {
    pub project: PathBuf,
    pub output_directory: PathBuf,
}
impl From<kronello_media::MediaError> for ServiceError {
    fn from(error: kronello_media::MediaError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}
pub(crate) fn capabilities(_: CapabilitiesRequest) -> Result<MediaCapabilities, ServiceError> {
    Ok(MediaRuntime::load()?.capabilities().clone())
}
pub(crate) fn relink(request: RelinkRequest) -> Result<ProjectInfo, ServiceError> {
    let revision = parse_revision(&request.base_revision)?;
    let mut store = open_existing(&request.project)?;
    let mut document = store.snapshot()?.document;
    document
        .ensure_editable()
        .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
    let asset = document
        .assets
        .iter_mut()
        .find_map(|object| match object {
            DocumentObject::Known(a) if a.id == request.asset => Some(a),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", "asset ID not present"))?;
    *asset = relink_asset(asset, &request.project, &request.search_directory)?;
    store.import_json(
        revision,
        uuid::Uuid::new_v4(),
        &serde_json::to_string(&document)?,
    )?;
    let result = info(&store)?;
    store.close()?;
    Ok(result)
}
pub(crate) fn collect(request: CollectRequest) -> Result<CollectedProject, ServiceError> {
    let store = open_existing(&request.project)?;
    let document = store.snapshot()?.document;
    store.close()?;
    collect_project(
        &document,
        &request.project,
        &request.output_directory,
        |path, document| {
            let mut store = ProjectStore::open(path, OpenOptions::default())?;
            store.import_json(0, uuid::Uuid::new_v4(), &serde_json::to_string(document)?)?;
            store.close()?;
            Ok(())
        },
    )
}
