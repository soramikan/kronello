//! Shared synchronous Command/Query boundary for the M1 headless workflow.
//! Entry points own transport only; storage, fonts and rendering compose here.
mod wire;

use std::path::{Path, PathBuf};

use kronello_gpu::{GpuContext, GpuError, render_adapter::CpuReferenceBackend};
use kronello_model::{CompositionId, FontRef, Project};
use kronello_render::{
    FrameMetadata, FrameRequest, OutputRegion, RenderBackend, RenderError, RenderProfile,
    RenderSnapshot, SequenceMetadata, SequenceRequest, render_frame, render_sequence,
};
use kronello_store::{OpenOptions, ProjectStore, StoreError};
use kronello_text::FontData;
use kronello_time::{FrameRate, Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Request {
    #[serde(rename = "project.create")]
    ProjectCreate(CreateRequest),
    #[serde(rename = "project.import")]
    ProjectImport(ImportRequest),
    #[serde(rename = "project.export")]
    ProjectExport(ProjectRequest),
    #[serde(rename = "project.info")]
    ProjectInfo(ProjectRequest),
    #[serde(rename = "render.frame")]
    RenderFrame(FrameRenderRequest),
    #[serde(rename = "render.sequence")]
    RenderSequence(SequenceRenderRequest),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub project: PathBuf,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub project: PathBuf,
    /// Decimal revision, independent of JSON number precision.
    pub base_revision: String,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRequest {
    pub project: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontInput {
    pub identity: FontRef,
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderInput {
    pub project: PathBuf,
    pub composition: CompositionId,
    pub region: OutputRegion,
    #[serde(default)]
    pub profile: RenderProfile,
    #[serde(default)]
    pub fonts: Vec<FontInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRenderRequest {
    pub input: RenderInput,
    pub time: Time,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceRenderRequest {
    pub input: RenderInput,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub output_directory: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    pub project_id: String,
    pub name: String,
    pub revision: String,
    pub schema_version: u32,
    pub semantic_version: u32,
    pub content_hash: String,
    pub compositions: Vec<CompositionId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportResult {
    pub revision: String,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameResult {
    pub metadata: FrameMetadata,
    pub linear: Vec<[f32; 4]>,
    pub display: Vec<[f32; 4]>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResultData {
    Project(ProjectInfo),
    Export(ExportResult),
    Frame(Box<FrameResult>),
    Sequence(SequenceMetadata),
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Success { result: ResultData },
    Error { error: ServiceError },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
}
impl ServiceError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_REQUEST", message)
    }
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ServiceError {}
impl From<StoreError> for ServiceError {
    fn from(e: StoreError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}
impl From<RenderError> for ServiceError {
    fn from(e: RenderError) -> Self {
        let code = if matches!(
            e,
            RenderError::Layout(kronello_text::LayoutError::MissingFont { .. })
        ) {
            "FONT_MISSING"
        } else {
            e.code()
        };
        Self::new(code, e.to_string())
    }
}
impl From<serde_json::Error> for ServiceError {
    fn from(e: serde_json::Error) -> Self {
        Self::invalid(e.to_string())
    }
}
impl From<std::io::Error> for ServiceError {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", e.to_string())
    }
}

/// Explicit execution choice. GPU initialization is lazy and never falls back.
#[derive(Debug, Clone, Copy, Default)]
pub enum BackendSelection {
    #[default]
    Gpu,
    CpuReference,
}
enum Backend<'a> {
    Injected(&'a dyn RenderBackend),
    Selected(BackendSelection),
}
pub struct Service<'a> {
    backend: Backend<'a>,
}
impl Service<'_> {
    pub fn new(selection: BackendSelection) -> Self {
        Self {
            backend: Backend::Selected(selection),
        }
    }
}
impl<'a> Service<'a> {
    pub fn with_backend(backend: &'a dyn RenderBackend) -> Self {
        Self {
            backend: Backend::Injected(backend),
        }
    }
    pub fn execute(&self, request: Request) -> Response {
        match self.dispatch(request) {
            Ok(result) => Response::Success { result },
            Err(error) => Response::Error { error },
        }
    }
    pub fn dispatch(&self, request: Request) -> Result<ResultData, ServiceError> {
        match request {
            Request::ProjectCreate(r) => create(r).map(ResultData::Project),
            Request::ProjectImport(r) => {
                let revision = parse_revision(&r.base_revision)?;
                let json = serde_json::to_string(&r.document)?;
                let mut store = open_existing(&r.project)?;
                store.import_json(revision, uuid::Uuid::new_v4(), &json)?;
                let info = info(&store)?;
                store.close()?;
                Ok(ResultData::Project(info))
            }
            Request::ProjectInfo(r) => {
                let store = open_existing(&r.project)?;
                let info = info(&store)?;
                store.close()?;
                Ok(ResultData::Project(info))
            }
            Request::ProjectExport(r) => {
                let store = open_existing(&r.project)?;
                let snapshot = store.snapshot()?;
                store.close()?;
                Ok(ResultData::Export(ExportResult {
                    revision: snapshot.revision.to_string(),
                    document: snapshot.document,
                }))
            }
            Request::RenderFrame(r) => self.render(&r.input, |snapshot, fonts, backend| {
                let frame = render_frame(
                    snapshot,
                    fonts,
                    backend,
                    FrameRequest {
                        time: r.time,
                        region: r.input.region,
                    },
                )?;
                Ok(ResultData::Frame(Box::new(FrameResult {
                    metadata: frame.metadata,
                    linear: frame.pixels.linear,
                    display: frame.pixels.display,
                })))
            }),
            Request::RenderSequence(r) => self.render(&r.input, |snapshot, fonts, backend| {
                Ok(ResultData::Sequence(render_sequence(
                    snapshot,
                    fonts,
                    backend,
                    SequenceRequest {
                        range: r.range,
                        frame_rate: r.frame_rate,
                        region: r.input.region,
                    },
                    &r.output_directory,
                )?))
            }),
        }
    }
    fn render(
        &self,
        input: &RenderInput,
        run: impl FnOnce(
            &RenderSnapshot,
            &[FontData<'_>],
            &dyn RenderBackend,
        ) -> Result<ResultData, ServiceError>,
    ) -> Result<ResultData, ServiceError> {
        input.region.validate()?;
        let store = open_existing(&input.project)?;
        let stored = store.snapshot()?;
        store.close()?;
        let snapshot = RenderSnapshot::new(
            &stored.document,
            input.composition,
            stored.revision,
            input.profile,
        )?;
        // File locators are explicit request inputs. Locked identity comes from
        // the snapshot; never discover system fonts or silently re-pin bytes.
        for font in &input.fonts {
            if !snapshot.font_locks().contains(&font.identity) {
                return Err(ServiceError::invalid("font input is not a snapshot lock"));
            }
        }
        let mut bytes = Vec::new();
        for identity in snapshot.font_locks() {
            let matches: Vec<_> = input
                .fonts
                .iter()
                .filter(|font| &font.identity == identity)
                .collect();
            if matches.len() > 1 {
                return Err(ServiceError::invalid("duplicate font input"));
            }
            let font = matches
                .first()
                .ok_or_else(|| ServiceError::new("FONT_MISSING", "missing locked font locator"))?;
            bytes.push(std::fs::read(&font.path).map_err(|e| {
                ServiceError::new(
                    if e.kind() == std::io::ErrorKind::NotFound {
                        "FONT_MISSING"
                    } else {
                        "IO_ERROR"
                    },
                    e.to_string(),
                )
            })?);
        }
        for (identity, bytes) in snapshot.font_locks().iter().zip(&bytes) {
            if format!("{:x}", Sha256::digest(bytes)) != identity.sha256 {
                return Err(ServiceError::new(
                    "ASSET_HASH_MISMATCH",
                    "locked font hash differs",
                ));
            }
            let actual =
                kronello_text::pin_font(bytes, identity.face_index).map_err(RenderError::from)?;
            if &actual != identity {
                return Err(ServiceError::new(
                    "ASSET_HASH_MISMATCH",
                    "locked font identity differs",
                ));
            }
        }
        let fonts: Vec<_> = snapshot
            .font_locks()
            .iter()
            .zip(&bytes)
            .map(|(identity, bytes)| FontData { identity, bytes })
            .collect();
        match self.backend {
            Backend::Injected(backend) => run(&snapshot, &fonts, backend),
            Backend::Selected(BackendSelection::CpuReference) => {
                run(&snapshot, &fonts, &CpuReferenceBackend)
            }
            Backend::Selected(BackendSelection::Gpu) => {
                let gpu = GpuContext::new().map_err(|e| {
                    let code = match e {
                        GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
                        GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
                        _ => "GPU_ERROR",
                    };
                    ServiceError::new(code, e.to_string())
                })?;
                run(&snapshot, &fonts, &gpu)
            }
        }
    }
}
fn parse_revision(value: &str) -> Result<u64, ServiceError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ServiceError::invalid(
            "base_revision must be an unsigned decimal string",
        ));
    }
    value
        .parse()
        .map_err(|_| ServiceError::invalid("base_revision overflow"))
}
fn open_existing(path: &Path) -> Result<ProjectStore, ServiceError> {
    if !path.is_file() {
        return Err(ServiceError::new(
            "PROJECT_NOT_FOUND",
            "project file does not exist",
        ));
    }
    Ok(ProjectStore::open(path, OpenOptions::default())?)
}
fn info(store: &ProjectStore) -> Result<ProjectInfo, ServiceError> {
    let snapshot = store.snapshot()?;
    let content_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(
            &snapshot.document
        )?)?)
    );
    Ok(ProjectInfo {
        project_id: snapshot.document.id.to_string(),
        name: snapshot.document.name,
        revision: snapshot.revision.to_string(),
        schema_version: snapshot.document.schema_version,
        semantic_version: snapshot.document.semantic_version,
        content_hash,
        compositions: snapshot
            .document
            .compositions
            .iter()
            .filter_map(|c| match c {
                kronello_model::DocumentObject::Known(c) => Some(c.id),
                _ => None,
            })
            .collect(),
    })
}
fn create(request: CreateRequest) -> Result<ProjectInfo, ServiceError> {
    // Validate before creating anything. Exclusive publication prevents overwrite.
    request
        .document
        .validate_storage()
        .map_err(StoreError::from)?;
    if request
        .project
        .extension()
        .is_none_or(|ext| ext != "kronello")
    {
        return Err(ServiceError::invalid(
            "project must use .kronello extension",
        ));
    }
    let json = serde_json::to_string(&request.document)?;
    let parent = request
        .project
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Publish a fully initialized, closed SQLite file. A failed import leaves
    // no partially initialized target for another process to adopt.
    let staging = tempfile::Builder::new()
        .prefix(".kronello-create-")
        .suffix(".kronello")
        .tempfile_in(parent)?;
    let mut store = ProjectStore::open(staging.path(), OpenOptions::default())?;
    store.import_json(0, uuid::Uuid::new_v4(), &json)?;
    let info = info(&store)?;
    store.close()?;
    staging.persist_noclobber(&request.project).map_err(|e| {
        ServiceError::new(
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                "PROJECT_EXISTS"
            } else {
                "IO_ERROR"
            },
            e.error.to_string(),
        )
    })?;
    Ok(info)
}
