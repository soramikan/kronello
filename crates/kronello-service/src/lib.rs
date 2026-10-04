//! Shared synchronous Command/Query boundary for headless rendering and edits.
//! Entry points own transport only; storage, fonts and rendering compose here.
mod nle;
pub use kronello_render::RenderTarget;
pub use nle::*;
mod jobs;
pub use jobs::*;
mod api;
mod edit;
mod query;
pub use api::*;
pub use query::*;
mod wire;
pub use edit::{
    EditApplyRequest, EditCommand, EditPlan, HistoryEntry, HistoryRequest, HistoryResult,
    PlanRequest, UndoConflict, UndoRequest,
};

mod template;
pub use template::{
    TemplateCommand, TemplateDefineRequest, TemplateInstantiateRequest, TemplateSetDurationRequest,
    TemplateSetInputRequest,
};
mod media;
pub use media::{CollectRequest, RelinkRequest};

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

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Request {
    #[serde(rename = "sequence.create")]
    SequenceCreate(SequenceCreateRequest),
    #[serde(rename = "clip.place")]
    ClipPlace(ClipPlaceRequest),
    #[serde(rename = "clip.trim")]
    ClipTrim(ClipTrimRequest),
    #[serde(rename = "clip.stretch")]
    ClipStretch(ClipStretchRequest),
    #[serde(rename = "instance.retime")]
    InstanceRetime(InstanceRetimeRequest),
    #[serde(rename = "template_instance.retime")]
    TemplateInstanceRetime(TemplateInstanceRetimeRequest),

    #[serde(rename = "render.submit")]
    RenderSubmit(RenderSubmitRequest),
    #[serde(rename = "job.get")]
    JobGet(JobRequest),
    #[serde(rename = "job.list")]
    JobList(JobListRequest),
    #[serde(rename = "job.cancel")]
    JobCancel(JobRequest),
    #[serde(rename = "job.prune")]
    JobPrune(JobPruneRequest),
    #[serde(rename = "template.set_duration")]
    TemplateSetDuration(TemplateSetDurationRequest),
    #[serde(rename = "template.define")]
    TemplateDefine(TemplateDefineRequest),
    #[serde(rename = "template.instantiate")]
    TemplateInstantiate(TemplateInstantiateRequest),
    #[serde(rename = "template.set_input")]
    TemplateSetInput(TemplateSetInputRequest),
    #[serde(rename = "asset.relink")]
    AssetRelink(RelinkRequest),
    #[serde(rename = "project.collect")]
    ProjectCollect(CollectRequest),
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
    #[serde(rename = "edit.plan")]
    EditPlan(PlanRequest),
    #[serde(rename = "edit.apply")]
    EditApply(EditApplyRequest),
    #[serde(rename = "edit.undo")]
    EditUndo(UndoRequest),
    #[serde(rename = "history.list")]
    HistoryList(HistoryRequest),
    #[serde(rename = "scene.query")]
    SceneQuery(SceneQueryRequest),
    #[serde(rename = "property.sample")]
    PropertySample(PropertySampleRequest),
    #[serde(rename = "capabilities.get")]
    CapabilitiesGet(CapabilitiesRequest),
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub project: PathBuf,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub project: PathBuf,
    /// Decimal revision, independent of JSON number precision.
    pub base_revision: String,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectRequest {
    pub project: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FontInput {
    pub identity: FontRef,
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = render_input_schema)]
pub struct RenderInput {
    pub project: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<CompositionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<RenderTarget>,
    pub region: OutputRegion,
    #[serde(default)]
    pub profile: RenderProfile,
    #[serde(default)]
    pub fonts: Vec<FontInput>,
}
fn render_input_schema(schema: &mut schemars::Schema) {
    schema.insert("oneOf".into(), serde_json::json!([
        {"required":["composition"], "properties":{"composition":{"type":"string"}}, "not":{"required":["target"]}},
        {"required":["target"], "properties":{"target":{"type":"object"}}, "not":{"required":["composition"]}}
    ]));
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameRenderRequest {
    pub input: RenderInput,
    pub time: Time,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceRenderRequest {
    pub input: RenderInput,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub output_directory: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportResult {
    pub revision: String,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameResult {
    pub metadata: FrameMetadata,
    pub linear: Vec<[f32; 4]>,
    pub display: Vec<[f32; 4]>,
}
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResultData {
    Job(Box<kronello_jobs::JobRecord>),
    Jobs(JobListResult),
    Pruned(kronello_jobs::PruneResult),
    Collected(kronello_media::CollectedProject),
    Project(ProjectInfo),
    Export(Box<ExportResult>),
    Frame(Box<FrameResult>),
    Sequence(SequenceMetadata),
    Plan(Box<EditPlan>),
    Edit(kronello_store::Event),
    History(HistoryResult),
    Scene(SceneQueryResult),
    Samples(PropertySampleResult),
    Capabilities(Box<CapabilitiesResult>),
}
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Success { result: ResultData },
    Error { error: ServiceError },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
impl ServiceError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
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
        let mut error = Self::new(code, e.to_string());
        if let RenderError::Template(kronello_template::TemplateError::Overflow {
            node,
            actual,
            maximum,
        }) = e
        {
            error.details =
                Some(serde_json::json!({"node":node,"actual_lines":actual,"max_lines":maximum}));
        }
        error
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    gpu_factory: fn() -> Result<GpuContext, GpuError>,
    media_capabilities: Option<MediaCapabilities>,
    job_config: Option<kronello_jobs::JobConfig>,
    worker_executable: Option<PathBuf>,
}
impl Service<'_> {
    pub fn new(selection: BackendSelection) -> Self {
        Self {
            backend: Backend::Selected(selection),
            gpu_factory: create_gpu_context,
            media_capabilities: None,
            job_config: None,
            worker_executable: None,
        }
    }
}
impl<'a> Service<'a> {
    pub fn with_backend(backend: &'a dyn RenderBackend) -> Self {
        Self {
            backend: Backend::Injected(backend),
            gpu_factory: create_gpu_context,
            media_capabilities: None,
            job_config: None,
            worker_executable: None,
        }
    }
    pub fn with_media_capabilities(mut self, capabilities: MediaCapabilities) -> Self {
        self.media_capabilities = Some(capabilities);
        self
    }
    pub fn execute_json(&self, json: &str) -> Response {
        match serde_json::from_str(json) {
            Ok(request) => self.execute(request),
            Err(error) => Response::Error {
                error: error.into(),
            },
        }
    }
    pub fn execute(&self, request: Request) -> Response {
        match self.dispatch(request) {
            Ok(result) => Response::Success { result },
            Err(error) => Response::Error { error },
        }
    }
    pub fn dispatch(&self, request: Request) -> Result<ResultData, ServiceError> {
        validate_request_locators(&request)?;
        match request {
            Request::SequenceCreate(r) => nle::sequence_create(r).map(ResultData::Edit),
            Request::ClipPlace(r) => nle::clip_place(r).map(ResultData::Edit),
            Request::ClipTrim(r) => nle::clip_trim(r).map(ResultData::Edit),
            Request::ClipStretch(r) => nle::clip_stretch(r).map(ResultData::Edit),
            Request::InstanceRetime(r) => nle::instance_retime(r).map(ResultData::Edit),
            Request::TemplateInstanceRetime(r) => {
                nle::template_instance_retime(r).map(ResultData::Edit)
            }
            Request::RenderSubmit(r) => self.submit_job(r).map(|r| ResultData::Job(Box::new(r))),
            Request::JobGet(r) => self
                .jobs()?
                .get(&r.job)
                .map(|r| ResultData::Job(Box::new(r)))
                .map_err(Into::into),
            Request::JobCancel(r) => self
                .jobs()?
                .cancel(&r.job)
                .map(|r| ResultData::Job(Box::new(r)))
                .map_err(Into::into),
            Request::JobList(_) => Ok(ResultData::Jobs(JobListResult {
                jobs: self.jobs()?.list()?,
            })),
            Request::JobPrune(_) => Ok(ResultData::Pruned(self.jobs()?.prune()?)),
            Request::SceneQuery(r) => query::scene(r).map(ResultData::Scene),
            Request::PropertySample(r) => query::sample(r).map(ResultData::Samples),
            Request::CapabilitiesGet(_) => Ok(ResultData::Capabilities(Box::new(
                CapabilitiesResult::current(Some(match &self.media_capabilities {
                    Some(capabilities) => capabilities.clone(),
                    None => media::capabilities()?,
                })),
            ))),
            Request::TemplateSetDuration(r) => template::set_duration(r).map(ResultData::Edit),
            Request::TemplateDefine(r) => template::define(r).map(ResultData::Edit),
            Request::TemplateInstantiate(r) => template::instantiate(r).map(ResultData::Edit),
            Request::TemplateSetInput(r) => template::set_input(r).map(ResultData::Edit),
            Request::EditPlan(r) => edit::plan(r).map(|p| ResultData::Plan(Box::new(p))),
            Request::EditApply(r) => edit::apply(r).map(ResultData::Edit),
            Request::EditUndo(r) => edit::undo(r).map(ResultData::Edit),
            Request::HistoryList(r) => edit::history(r).map(ResultData::History),
            Request::AssetRelink(r) => media::relink(r).map(ResultData::Project),
            Request::ProjectCollect(r) => media::collect(r).map(ResultData::Collected),
            Request::ProjectCreate(r) => create(r).map(ResultData::Project),
            Request::ProjectImport(r) => {
                let revision = parse_revision(&r.base_revision)?;
                let json = serde_json::to_string(&r.document)?;
                let mut store = open_existing(&r.project)?;
                let previous = store.snapshot()?;
                kronello_template::validate_stored_transition(&previous.document, &r.document)?;
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
                Ok(ResultData::Export(Box::new(ExportResult {
                    revision: snapshot.revision.to_string(),
                    document: snapshot.document,
                })))
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
        let snapshot = freeze_render_input(&stored, input)?;
        let bytes = load_locked_fonts(&snapshot, input)?;
        let fonts: Vec<_> = snapshot
            .font_locks()
            .iter()
            .zip(&bytes)
            .map(|(identity, bytes)| FontData { identity, bytes })
            .collect();
        self.with_selected_backend(|backend| run(&snapshot, &fonts, backend))
    }
    fn with_selected_backend<T>(
        &self,
        run: impl FnOnce(&dyn RenderBackend) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        match self.backend {
            Backend::Injected(backend) => run(backend),
            Backend::Selected(BackendSelection::CpuReference) => run(&CpuReferenceBackend),
            Backend::Selected(BackendSelection::Gpu) => {
                let gpu = (self.gpu_factory)().map_err(|e| {
                    let code = match e {
                        GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
                        GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
                        _ => "GPU_ERROR",
                    };
                    ServiceError::new(code, e.to_string())
                })?;
                run(&gpu)
            }
        }
    }
}
/// One target compiler for synchronous rendering and fixed asynchronous input.
/// New render target variants belong here, never in a separate job target model.
fn freeze_render_input(
    stored: &kronello_store::Snapshot,
    input: &RenderInput,
) -> Result<RenderSnapshot, ServiceError> {
    let target = match (input.composition, input.target) {
        (Some(composition), None) => composition.into(),
        (None, Some(target)) => target,
        _ => {
            return Err(ServiceError::invalid(
                "specify exactly one of composition or target",
            ));
        }
    };
    Ok(RenderSnapshot::for_target(
        &stored.document,
        target,
        stored.revision,
        input.profile,
    )?)
}
fn load_locked_fonts(
    snapshot: &RenderSnapshot,
    input: &RenderInput,
) -> Result<Vec<Vec<u8>>, ServiceError> {
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
    Ok(bytes)
}
fn create_gpu_context() -> Result<GpuContext, GpuError> {
    // Never honor fault injection in a release-profile build, even if a
    // dependency enables the test feature through Cargo feature unification.
    #[cfg(all(feature = "test-adapter-unavailable", debug_assertions))]
    if std::env::var_os("KRONELLO_TEST_ADAPTER_UNAVAILABLE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        return Err(GpuError::AdapterUnavailable(
            "test-only injected adapter unavailability".into(),
        ));
    }
    GpuContext::new()
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
    kronello_template::validate_stored_project(&request.document)?;
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

/// Reject URI schemes at local-file boundaries, before any storage/font/output
/// access. Material text and other opaque document strings remain inert data.
fn local_locator(path: &Path) -> Result<(), ServiceError> {
    local_locator_text(&path.to_string_lossy())
}
fn local_locator_text(value: &str) -> Result<(), ServiceError> {
    if let Some((scheme, tail)) = value.split_once(':') {
        let windows_drive = scheme.len() == 1
            && scheme.as_bytes()[0].is_ascii_alphabetic()
            && (tail.starts_with('/') || tail.starts_with('\\'));
        if !windows_drive
            && !scheme.is_empty()
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
        {
            return Err(ServiceError::invalid(
                "locators must be local filesystem paths, not URIs",
            ));
        }
    }
    Ok(())
}
fn document_asset_locators(document: &Project) -> Result<(), ServiceError> {
    // Asset objects are an additive document extension owned by MEDIA-001.
    // Only locator slots are interpreted here; captions/names are never code.
    fn visit(value: &serde_json::Value) -> Result<(), ServiceError> {
        match value {
            serde_json::Value::Object(object) => {
                for (name, value) in object {
                    if matches!(
                        name.as_str(),
                        "relative" | "absolute" | "relative_path" | "absolute_path" | "locator"
                    ) && let Some(text) = value.as_str()
                    {
                        local_locator_text(text)?;
                    }
                    visit(value)?;
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    if let Some(assets) = serde_json::to_value(document)?.get("assets") {
        visit(assets)?;
    }
    Ok(())
}
fn validate_request_locators(request: &Request) -> Result<(), ServiceError> {
    match request {
        Request::SequenceCreate(r) => local_locator(&r.project),
        Request::ClipPlace(r) => local_locator(&r.project),
        Request::ClipTrim(r) => local_locator(&r.project),
        Request::ClipStretch(r) => local_locator(&r.project),
        Request::InstanceRetime(r) => local_locator(&r.project),
        Request::TemplateInstanceRetime(r) => local_locator(&r.project),

        Request::RenderSubmit(r) => {
            local_locator(&r.render.output_directory)?;
            render_locators(&r.render.input)
        }
        Request::JobGet(_) | Request::JobCancel(_) | Request::JobList(_) | Request::JobPrune(_) => {
            Ok(())
        }
        Request::ProjectCreate(r) => {
            local_locator(&r.project)?;
            document_asset_locators(&r.document)
        }
        Request::ProjectImport(r) => {
            local_locator(&r.project)?;
            document_asset_locators(&r.document)
        }
        Request::ProjectExport(r) | Request::ProjectInfo(r) => local_locator(&r.project),
        Request::TemplateDefine(r) => local_locator(&r.project),
        Request::TemplateInstantiate(r) => local_locator(&r.project),
        Request::TemplateSetInput(r) => local_locator(&r.project),
        Request::TemplateSetDuration(r) => local_locator(&r.project),
        Request::EditPlan(r) => local_locator(&r.project),
        Request::EditApply(r) => local_locator(&r.project),
        Request::EditUndo(r) => local_locator(&r.project),
        Request::HistoryList(r) => local_locator(&r.project),
        Request::SceneQuery(r) => local_locator(&r.project),
        Request::PropertySample(r) => local_locator(&r.project),
        Request::RenderFrame(r) => render_locators(&r.input),
        Request::RenderSequence(r) => {
            local_locator(&r.output_directory)?;
            render_locators(&r.input)
        }
        Request::AssetRelink(r) => {
            local_locator(&r.project)?;
            local_locator(&r.search_directory)
        }
        Request::ProjectCollect(r) => {
            local_locator(&r.project)?;
            local_locator(&r.output_directory)
        }
        Request::CapabilitiesGet(_) => Ok(()),
    }
}
fn render_locators(input: &RenderInput) -> Result<(), ServiceError> {
    local_locator(&input.project)?;
    for font in &input.fonts {
        local_locator(&font.path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_backend_adapter_unavailable_never_falls_back() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("empty.kronello");
        let output_directory = directory.path().join("frames");
        // An empty composition needs neither external fonts nor a GPU fixture.
        let mut document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        document["compositions"][0]["root_nodes"] = serde_json::json!([]);
        document["compositions"][0]["nodes"] = serde_json::json!([]);
        document["texts"] = serde_json::json!([]);
        let document: Project = serde_json::from_value(document).unwrap();
        let composition = match &document.compositions[0] {
            kronello_model::DocumentObject::Known(composition) => composition.id,
            _ => panic!("known composition required"),
        };
        let mut service = Service::new(BackendSelection::default());
        service
            .dispatch(Request::ProjectCreate(CreateRequest {
                project: project.clone(),
                document,
            }))
            .unwrap();
        // Inject at construction, before a RenderBackend exists. This runs the
        // production GPU selection/error mapping without global env mutation.
        service.gpu_factory = || Err(GpuError::AdapterUnavailable("unit-test factory".into()));
        let request = Request::RenderSequence(SequenceRenderRequest {
            input: RenderInput {
                project,
                composition: Some(composition),
                target: None,
                region: OutputRegion {
                    origin: [0.0, 0.0],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
                profile: RenderProfile::default(),
                fonts: Vec::new(),
            },
            range: TimeRange::new(Time::ZERO, Time::new(1, 1).unwrap()).unwrap(),
            frame_rate: FrameRate::new(1, 1).unwrap(),
            output_directory: output_directory.clone(),
        });
        let Response::Error { error } = service.execute(request.clone()) else {
            panic!("default GPU must propagate adapter failure")
        };
        assert_eq!(error.code, "ADAPTER_UNAVAILABLE");
        assert!(error.message.contains("unit-test factory"));
        assert!(!output_directory.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);

        service.backend = Backend::Selected(BackendSelection::CpuReference);
        let ResultData::Sequence(metadata) = service.dispatch(request).unwrap() else {
            panic!("explicit CPU selection must render the positive control")
        };
        assert_eq!(metadata.frames.len(), 1);
        assert_eq!(metadata.frames[0].metadata.backend, "cpu_reference_float32");
        assert!(output_directory.join("sequence.json").is_file());
    }
}
