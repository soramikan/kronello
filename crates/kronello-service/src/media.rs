//! MEDIA-001 operations, shared by all transports. Filesystem/native work is
//! performed by media; project persistence remains in the shared store service.
use crate::{ProjectInfo, ServiceError, info, open_existing, parse_revision};
use kronello_media::{CollectedProject, MediaRuntime, collect_project, relink_asset};
use kronello_model::{AssetId, DocumentObject};
use kronello_store::{OpenOptions, ProjectStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// COLOR-003 `.cube` import request (ADR-0113). The caller-allocated asset id
/// names the record; the content hash is computed from file bytes here and
/// recorded as `content_hash`, never trusted from the request.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LutImportRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: uuid::Uuid,
    pub idempotency_key: String,
    /// Local `.cube` locator; stored relative to the project directory when it
    /// lives inside it, otherwise absolute (the relink/search convention).
    pub path: PathBuf,
    /// Id of the `AssetKind::Data` record the import upserts.
    pub asset: AssetId,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelinkRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub asset: AssetId,
    pub search_directory: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
pub(crate) fn capabilities() -> Result<crate::MediaCapabilities, ServiceError> {
    Ok(MediaRuntime::load()?.capabilities().clone().into())
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
/// `lut.import` is an edit operation: the file is parsed and hash-verified
/// once, then persisted as an external `AssetKind::Data` record through the
/// shared plan/apply path so undo, revision checks and idempotency apply.
/// The document never stores expanded lattice bytes.
pub(crate) fn lut_import(request: LutImportRequest) -> Result<kronello_store::Event, ServiceError> {
    let bytes = std::fs::read(&request.path).map_err(|e| {
        ServiceError::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                "LUT_MISSING"
            } else {
                "IO_ERROR"
            },
            e.to_string(),
        )
    })?;
    let lut = kronello_model::CubeLut::parse(&bytes)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    lut.validate_document_size()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let canonical = request
        .path
        .canonicalize()
        .map_err(|e| ServiceError::new("IO_ERROR", e.to_string()))?;
    let hash = kronello_media::content_hash(&canonical)?;
    let base = request
        .project
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| ServiceError::new("IO_ERROR", e.to_string()))?;
    let relative = canonical
        .strip_prefix(&base)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let asset = kronello_model::Asset {
        id: request.asset,
        content_hash: hash,
        kind: kronello_model::AssetKind::Data,
        streams: vec![],
        locator: kronello_model::AssetLocator {
            relative,
            absolute: Some(canonical.to_string_lossy().into_owned()),
        },
    };
    asset
        .validate()
        .map_err(|e| ServiceError::new("INVALID_DOCUMENT", e.to_string()))?;
    let commands = vec![crate::EditCommand::AssetSet { asset }];
    let plan = crate::edit::plan(crate::PlanRequest {
        project: request.project.clone(),
        base_revision: request.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(crate::EditApplyRequest {
        project: request.project,
        base_revision: request.base_revision,
        session_id: request.session_id,
        idempotency_key: request.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
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
