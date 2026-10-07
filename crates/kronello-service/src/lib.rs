//! Shared synchronous Command/Query boundary for headless rendering and edits.
//! Entry points own transport only; storage, fonts and rendering compose here.
mod session;
pub use session::ProjectSession;
mod export_profiles;
pub use export_profiles::{
    DeviceAvailability, ExportAudioCodec, ExportExecution, ExportProfileCapability,
};
mod font_authoring;
pub use font_authoring::FontPinRequest;
mod nle;
mod playback;
pub use kronello_render::RenderTarget;
pub use nle::*;
pub use playback::{
    AudioPreparationInput, AudioPrepareRequest, MAX_PLAYBACK_BLOCK_FRAMES, PreparedAudio,
};
mod jobs;
pub use jobs::*;
mod api;
mod repeater;
mod vector;
pub use vector::*;
mod audio_analysis;
pub use audio_analysis::{AudioAnalyzeInput, AudioAnalyzeRequest};
mod captions;
pub use captions::*;
mod edit;
mod events;
mod inspect;
mod paging;
mod query;
pub use api::*;
pub use events::*;
pub use inspect::*;
pub use query::*;
mod project;
pub use project::{CreatePlanRequest, ImportPlanRequest, ProjectChangeKind, ProjectChangePlan};
mod wire;
pub use edit::{
    EditApplyRequest, EditCommand, EditPlan, HistoryEntry, HistoryRequest, HistoryResult,
    PlanRequest, UndoConflict, UndoRequest,
};

mod template;
pub use template::{
    TemplateChange, TemplateCommand, TemplateDefineRequest, TemplateInstantiateRequest,
    TemplateMigrationPlan, TemplateMigrationPlanRequest, TemplatePreviewNode,
    TemplatePreviewRequest, TemplatePreviewResult, TemplateSetDurationRequest,
    TemplateSetInputRequest,
};
mod media;
pub use media::{CollectRequest, LutImportRequest, RelinkRequest};
mod control;
pub use control::ExecutionControl;

use std::path::{Path, PathBuf};

use kronello_gpu::{GpuContext, GpuError, render_adapter::CpuReferenceBackend};
use kronello_model::{CompositionId, FontRef, Project};
use kronello_render::{
    FrameMetadata, FrameRequest, OutputRegion, RenderBackend, RenderError, RenderProfile,
    RenderSnapshot, SequenceMetadata, SequenceRequest, render_frame,
    render_sequence_with_checkpoint,
};
use kronello_store::{ProjectStore, StoreError};
use kronello_text::FontData;
use kronello_time::{FrameRate, Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum Request {
    #[serde(rename = "font.pin")]
    FontPin(FontPinRequest),
    #[serde(rename = "svg.inspect")]
    SvgInspect(SvgInspectRequest),
    #[serde(rename = "svg.export")]
    SvgExport(SvgExportRequest),
    #[serde(rename = "svg.import_plan")]
    SvgImportPlan(SvgImportPlanRequest),
    #[serde(rename = "audio.analyze")]
    AudioAnalyze(AudioAnalyzeRequest),
    #[serde(rename = "sequence.query")]
    SequenceQuery(SequenceQueryRequest),
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

    #[serde(rename = "render.export")]
    RenderExport(RenderSubmitRequest),
    #[serde(rename = "render.submit")]
    RenderSubmit(RenderSubmitRequest),
    #[serde(rename = "job.get")]
    JobGet(JobRequest),
    #[serde(rename = "job.list")]
    JobList(JobListRequest),
    #[serde(rename = "job.cancel")]
    JobCancel(JobRequest),
    #[serde(rename = "job.resume")]
    JobResume(JobRequest),
    #[serde(rename = "job.prune")]
    JobPrune(JobPruneRequest),
    #[serde(rename = "template.preview")]
    TemplatePreview(TemplatePreviewRequest),
    #[serde(rename = "template.migration_plan")]
    TemplateMigrationPlan(TemplateMigrationPlanRequest),
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
    #[serde(rename = "expression.format")]
    ExpressionFormat(ExpressionFormatRequest),
    #[serde(rename = "history.list")]
    HistoryList(HistoryRequest),
    #[serde(rename = "scene.query")]
    SceneQuery(SceneQueryRequest),
    #[serde(rename = "node.explain")]
    NodeExplain(NodeExplainRequest),
    #[serde(rename = "render.explain")]
    RenderExplain(RenderExplainRequest),
    #[serde(rename = "property.sample")]
    PropertySample(PropertySampleRequest),
    #[serde(rename = "capabilities.get")]
    CapabilitiesGet(CapabilitiesRequest),
    #[serde(rename = "project.import_plan")]
    ProjectImportPlan(ImportPlanRequest),
    #[serde(rename = "project.create_plan")]
    ProjectCreatePlan(CreatePlanRequest),
    #[serde(rename = "captions.import_plan")]
    CaptionsImportPlan(CaptionsImportPlanRequest),
    #[serde(rename = "captions.import")]
    CaptionsImport(CaptionsImportRequest),
    #[serde(rename = "captions.export")]
    CaptionsExport(CaptionsExportRequest),
    #[serde(rename = "lut.import")]
    LutImport(LutImportRequest),
    #[serde(rename = "inspect.scopes")]
    InspectScopes(InspectScopesRequest),
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub project: PathBuf,
    pub document: Project,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
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
/// COLOR-003 explicit `.cube` render input (ADR-0113): `hash` is the
/// lowercase hex SHA-256 of the document bytes an asset records as
/// `content_hash`; `path` is a local locator read, parsed and verified when
/// the snapshot is frozen, then carried inside the snapshot for replay.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LutInput {
    pub hash: String,
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
    /// COLOR-003 explicit `.cube` locators keyed by content hash, like
    /// `fonts`. Supplied lattices are verified; unreferenced ones are not
    /// bound into the scene.
    #[serde(default)]
    pub luts: Vec<LutInput>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendSelection>,
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
    pub open_mode: ProjectOpenMode,
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
    Font(FontRef),
    SvgReport(kronello_vector::SvgReport),
    SvgExport(SvgExportResult),
    Timeline(SequenceQueryResult),
    Movie(Box<kronello_media::AvExportReport>),
    Job(Box<kronello_jobs::JobRecord>),
    Jobs(JobListResult),
    Pruned(kronello_jobs::PruneResult),
    Collected(kronello_media::CollectedProject),
    Project(ProjectInfo),
    Export(Box<ExportResult>),
    Frame(Box<FrameResult>),
    Sequence(SequenceMetadata),
    Plan(Box<EditPlan>),
    ExpressionText(ExpressionFormatResult),
    TemplatePreview(Box<TemplatePreviewResult>),
    TemplateMigrationPlan(Box<TemplateMigrationPlan>),
    Edit(kronello_store::Event),
    History(HistoryResult),
    Scene(SceneQueryResult),
    NodeExplanation(Box<NodeExplainResult>),
    RenderExplanation(Box<RenderExplainResult>),
    Samples(PropertySampleResult),
    Capabilities(Box<CapabilitiesResult>),
    ProjectPlan(Box<ProjectChangePlan>),
    Captions(CaptionsExportResult),
    /// COLOR-004 scope bins over one fixed working-space frame (ADR-0113).
    Scopes(Box<InspectScopesResult>),
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
        if let RenderError::LayoutOverflow {
            node,
            instance_path,
            line,
            advance,
            wrap_width,
        } = &e
        {
            error.details = Some(
                serde_json::json!({"node":node,"instance_path":instance_path,
                "line":line,"advance":advance,"wrap_width":wrap_width}),
            );
        }
        if let RenderError::Evaluation(kronello_eval::EvaluationError::DependencyCycle { path }) =
            &e
        {
            use kronello_eval::RuntimePropertyKey;
            let path: Vec<_> = path
                .iter()
                .map(|k| match k {
                    RuntimePropertyKey::Node(k) => serde_json::json!({"kind":"node",
                    "instance_path":k.instance_path,"node":k.node,"property":k.property}),
                    RuntimePropertyKey::LayoutValue {
                        instance_path,
                        text,
                        consumer,
                    } => serde_json::json!({"kind":"layout","instance_path":instance_path,
                        "text":text,"consumer":consumer}),
                    RuntimePropertyKey::Composition {
                        instance_path,
                        composition,
                        property,
                    } => serde_json::json!({"kind":"composition","instance_path":instance_path,
                        "composition":composition,"property":property}),
                })
                .collect();
            error.details = Some(serde_json::json!({"path":path}));
        }
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
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelection {
    #[default]
    Gpu,
    CpuReference,
    /// Strict VideoToolbox hardware decode and same-device Metal import.
    GpuResidentBgra8,
    GpuResidentNv12,
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
    read_only_inspection: bool,
}
impl Service<'_> {
    pub fn new(selection: BackendSelection) -> Self {
        Self {
            backend: Backend::Selected(selection),
            gpu_factory: create_gpu_context,
            media_capabilities: None,
            job_config: None,
            worker_executable: None,
            read_only_inspection: false,
        }
    }
}
impl<'a> Service<'a> {
    /// Prepare a native preview through the same snapshot/font/compiler policy
    /// as render.frame. Native adapters own presentation, never document edits.
    pub fn preview_dag(
        &self,
        request: &FrameRenderRequest,
    ) -> Result<(String, kronello_render::RenderDag), ServiceError> {
        self.with_render_input(&request.input, None, |snapshot, fonts| {
            if snapshot.profile().hdr.is_some() {
                return Err(RenderError::UnsupportedFeature("native single DAG preview has no HDR display conversion; use render.frame SDR display artifact".into()).into());
            }
            if snapshot.profile().temporal.is_some() {
                return Err(RenderError::UnsupportedFeature(
                    "single DAG native preview cannot integrate temporal samples; use render.frame"
                        .into(),
                )
                .into());
            }
            let scene = kronello_render::build_scene_ir(snapshot, request.time, fonts)?;
            let dag = kronello_render::build_render_dag(
                &scene,
                snapshot.profile(),
                request.input.region,
            )?;
            Ok((snapshot.revision().to_string(), dag))
        })
    }
    pub fn with_backend(backend: &'a dyn RenderBackend) -> Self {
        Self {
            backend: Backend::Injected(backend),
            gpu_factory: create_gpu_context,
            media_capabilities: None,
            job_config: None,
            worker_executable: None,
            read_only_inspection: false,
        }
    }
    pub fn with_media_capabilities(mut self, capabilities: MediaCapabilities) -> Self {
        self.media_capabilities = Some(capabilities);
        self
    }
    /// Snapshot-only inspection for resources/prompts. Existing command tools
    /// retain their normal open/close, exclusive-lock and migration policy.
    /// This policy changes only project.info and project.export, not edits.
    pub fn with_read_only_inspection(mut self) -> Self {
        self.read_only_inspection = true;
        self
    }
    pub fn execute_json(&self, json: &str) -> Response {
        self.execute_json_with_control(json, &())
    }
    /// The same decoder/dispatcher with optional cooperative request control.
    /// Cancellation never implies job.cancel or rollback of committed edits.
    pub fn execute_json_with_control(
        &self,
        json: &str,
        control: &dyn ExecutionControl,
    ) -> Response {
        match serde_json::from_str(json) {
            Ok(request) => match self.dispatch_controlled(request, control) {
                Ok(result) => Response::Success { result },
                Err(error) => Response::Error { error },
            },
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
        self.dispatch_controlled(request, &())
    }
    fn dispatch_controlled(
        &self,
        request: Request,
        control: &dyn ExecutionControl,
    ) -> Result<ResultData, ServiceError> {
        if control.is_cancelled() {
            return Err(ServiceError::new(
                "REQUEST_CANCELLED",
                "Request cancelled before dispatch",
            ));
        }
        validate_request_locators(&request)?;
        match request {
            Request::FontPin(r) => font_authoring::pin(r).map(ResultData::Font),
            Request::SvgInspect(r) => vector::inspect(r).map(ResultData::SvgReport),
            Request::SvgExport(r) => vector::export(r).map(ResultData::SvgExport),
            Request::SvgImportPlan(r) => {
                vector::import_plan(r).map(|r| ResultData::Plan(Box::new(r)))
            }
            Request::AudioAnalyze(r) => self.analyze_audio(r),
            Request::SequenceQuery(r) => nle::sequence_query(r).map(ResultData::Timeline),
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
            Request::JobResume(r) => self.resume_job(r).map(|r| ResultData::Job(Box::new(r))),
            Request::JobList(_) => Ok(ResultData::Jobs(JobListResult {
                jobs: self.jobs()?.list()?,
            })),
            Request::JobPrune(_) => Ok(ResultData::Pruned(self.jobs()?.prune()?)),
            Request::SceneQuery(r) => query::scene(r).map(ResultData::Scene),
            Request::NodeExplain(r) => {
                inspect::node(r).map(|r| ResultData::NodeExplanation(Box::new(r)))
            }
            Request::RenderExplain(r) => {
                let backend = match self.backend {
                    Backend::Selected(BackendSelection::Gpu) => {
                        kronello_render::ExplainBackend::Gpu
                    }
                    Backend::Selected(BackendSelection::GpuResidentBgra8) => {
                        kronello_render::ExplainBackend::GpuResidentBgra8
                    }
                    Backend::Selected(BackendSelection::GpuResidentNv12) => {
                        kronello_render::ExplainBackend::GpuResidentNv12
                    }
                    Backend::Selected(BackendSelection::CpuReference) => {
                        kronello_render::ExplainBackend::CpuReference
                    }
                    Backend::Injected(_) => kronello_render::ExplainBackend::Unknown,
                };
                inspect::render(r, backend).map(|r| ResultData::RenderExplanation(Box::new(r)))
            }
            Request::PropertySample(r) => query::sample(r).map(ResultData::Samples),
            Request::CapabilitiesGet(_) => Ok(ResultData::Capabilities(Box::new(
                CapabilitiesResult::current(Some(match &self.media_capabilities {
                    Some(capabilities) => capabilities.clone(),
                    None => media::capabilities()?,
                })),
            ))),
            Request::TemplatePreview(r) => {
                template::preview(r, self).map(|r| ResultData::TemplatePreview(Box::new(r)))
            }
            Request::TemplateMigrationPlan(r) => template::migration_plan(r, self)
                .map(|r| ResultData::TemplateMigrationPlan(Box::new(r))),
            Request::TemplateSetDuration(r) => template::set_duration(r).map(ResultData::Edit),
            Request::TemplateDefine(r) => template::define(r).map(ResultData::Edit),
            Request::TemplateInstantiate(r) => template::instantiate(r).map(ResultData::Edit),
            Request::TemplateSetInput(r) => template::set_input(r).map(ResultData::Edit),
            Request::EditPlan(r) => edit::plan(r).map(|p| ResultData::Plan(Box::new(p))),
            Request::EditApply(r) => edit::apply(r).map(ResultData::Edit),
            Request::EditUndo(r) => edit::undo(r).map(ResultData::Edit),
            Request::ExpressionFormat(r) => {
                query::expression_format(r).map(ResultData::ExpressionText)
            }
            Request::HistoryList(r) => edit::history(r).map(ResultData::History),
            Request::AssetRelink(r) => media::relink(r).map(ResultData::Project),
            Request::ProjectCollect(r) => media::collect(r).map(ResultData::Collected),
            Request::ProjectCreatePlan(r) => {
                project::create_plan(r).map(|p| ResultData::ProjectPlan(Box::new(p)))
            }
            Request::ProjectImportPlan(r) => {
                project::import_plan(r).map(|p| ResultData::ProjectPlan(Box::new(p)))
            }
            Request::ProjectCreate(r) => project::create(r).map(ResultData::Project),
            Request::ProjectImport(r) => project::import(r).map(ResultData::Project),
            Request::CaptionsImportPlan(r) => {
                captions::import_plan(r).map(|p| ResultData::Plan(Box::new(p)))
            }
            Request::CaptionsImport(r) => captions::import(r).map(ResultData::Edit),
            Request::CaptionsExport(r) => captions::export(r).map(ResultData::Captions),
            Request::LutImport(r) => media::lut_import(r).map(ResultData::Edit),
            Request::InspectScopes(r) => {
                inspect::scopes(r, self).map(|result| ResultData::Scopes(Box::new(result)))
            }
            Request::ProjectInfo(r) => {
                if self.read_only_inspection {
                    Ok(ResultData::Project(snapshot_info(
                        read_project_snapshot(&r.project)?,
                        ProjectOpenMode::ReadOnlySnapshot,
                    )?))
                } else {
                    let store = open_existing(&r.project)?;
                    let info = info(&store)?;
                    store.close()?;
                    Ok(ResultData::Project(info))
                }
            }
            Request::ProjectExport(r) => {
                let snapshot = if self.read_only_inspection {
                    read_project_snapshot(&r.project)?
                } else {
                    let store = open_existing(&r.project)?;
                    let snapshot = store.snapshot()?;
                    store.close()?;
                    snapshot
                };
                Ok(ResultData::Export(Box::new(ExportResult {
                    revision: snapshot.revision.to_string(),
                    document: snapshot.document,
                })))
            }
            Request::RenderFrame(r) => {
                let frame = self.render_requested_frame(&r)?;
                Ok(ResultData::Frame(Box::new(FrameResult {
                    metadata: frame.metadata,
                    linear: frame.pixels.linear,
                    display: frame.pixels.display,
                })))
            }
            Request::RenderExport(r) => {
                jobs::features(&r.required_features)?;
                self.with_render_input(
                    &r.render.input,
                    r.expected_revision.as_deref(),
                    |snapshot, fonts| {
                        self.with_video_backend(&r.render.input.project, |backend| {
                            jobs::validate_movie_destination(
                                &r.render.output_directory,
                                &r.output,
                            )?;
                            let av = jobs::movie_snapshot(snapshot, &r.output)?;
                            let settings = r.output.movie_settings()?;
                            let runtime = kronello_media::MediaRuntime::load()?;
                            let report = runtime.export_av(
                                &av,
                                &r.render.input.project,
                                fonts,
                                backend,
                                &kronello_media::AvExportRequest {
                                    output: r.render.output_directory.clone(),
                                    range: r.render.range,
                                    frame_rate: r.render.frame_rate,
                                    region: r.render.input.region,
                                    background: settings.background,
                                    clipping: kronello_audio::ClippingPolicy::Reject,
                                },
                            )?;
                            Ok(ResultData::Movie(Box::new(report)))
                        })
                    },
                )
            }
            Request::RenderSequence(r) => self.render(&r.input, |snapshot, fonts, backend| {
                let total = kronello_render::frame_samples(r.range, r.frame_rate)?.len() as u64;
                Ok(ResultData::Sequence(render_sequence_with_checkpoint(
                    snapshot,
                    fonts,
                    backend,
                    SequenceRequest {
                        range: r.range,
                        frame_rate: r.frame_rate,
                        region: r.input.region,
                    },
                    &r.output_directory,
                    &mut |completed| {
                        if control.is_cancelled() {
                            return Err(RenderError::Backend {
                                code: "REQUEST_CANCELLED",
                                message: "Request cancelled at frame boundary".into(),
                            });
                        }
                        control.progress(completed, total);
                        Ok(())
                    },
                )?))
            }),
        }
    }
    fn render<T>(
        &self,
        input: &RenderInput,
        run: impl FnOnce(
            &RenderSnapshot,
            &[FontData<'_>],
            &dyn RenderBackend,
        ) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        self.with_render_input(input, None, |snapshot, fonts| {
            self.with_video_backend(&input.project, |backend| run(snapshot, fonts, backend))
        })
    }
    /// Shared current-frame rendering, including the explicit media decode adapter.
    /// Native presentation consumes these same pixels without a separate renderer.
    pub fn render_requested_frame(
        &self,
        request: &FrameRenderRequest,
    ) -> Result<kronello_render::RenderedFrame, ServiceError> {
        let run =
            |snapshot: &RenderSnapshot, fonts: &[FontData<'_>], backend: &dyn RenderBackend| {
                Ok(render_frame(
                    snapshot,
                    fonts,
                    backend,
                    FrameRequest {
                        time: request.time,
                        region: request.input.region,
                    },
                )?)
            };
        match request.backend {
            None => self.render(&request.input, run),
            Some(selection) => {
                let mut selected = Service::new(selection);
                selected.gpu_factory = self.gpu_factory;
                selected.render(&request.input, run)
            }
        }
    }
    fn with_render_input<T>(
        &self,
        input: &RenderInput,
        expected_revision: Option<&str>,
        run: impl FnOnce(&RenderSnapshot, &[FontData<'_>]) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        input.region.validate()?;
        let store = open_existing(&input.project)?;
        let stored = store.snapshot()?;
        store.close()?;
        jobs::check_expected_revision(expected_revision, stored.revision)?;
        let snapshot = freeze_render_input(&stored, input)?.with_luts(load_locked_luts(input)?);
        let bytes = load_locked_fonts(&snapshot, input)?;
        let fonts: Vec<_> = snapshot
            .font_locks()
            .iter()
            .zip(&bytes)
            .map(|(identity, bytes)| FontData { identity, bytes })
            .collect();
        run(&snapshot, &fonts)
    }
    pub(crate) fn with_video_backend<T>(
        &self,
        project_path: &Path,
        run: impl FnOnce(&dyn RenderBackend) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        if let Backend::Selected(
            selection @ (BackendSelection::GpuResidentBgra8 | BackendSelection::GpuResidentNv12),
        ) = self.backend
        {
            let gpu =
                (self.gpu_factory)().map_err(|e| ServiceError::new("GPU_ERROR", e.to_string()))?;
            let backend = kronello_media::ResidentVideoRenderBackend::new(
                &gpu,
                project_path,
                if selection == BackendSelection::GpuResidentBgra8 {
                    kronello_media::ResidentVideoFormat::Bgra8
                } else {
                    kronello_media::ResidentVideoFormat::Nv12VideoRange
                },
            );
            run(&backend)
        } else {
            self.with_selected_backend(project_path, |backend| {
                let runtime = kronello_media::MediaRuntime::load();
                run(&kronello_media::SequentialVideoRenderBackend::new(
                    backend,
                    project_path,
                    runtime.as_ref(),
                ))
            })
        }
    }
    fn with_selected_backend<T>(
        &self,
        project_path: &Path,
        run: impl FnOnce(&dyn RenderBackend) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        match self.backend {
            Backend::Injected(backend) => run(backend),
            Backend::Selected(BackendSelection::CpuReference) => run(&CpuReferenceBackend),
            Backend::Selected(
                BackendSelection::Gpu
                | BackendSelection::GpuResidentBgra8
                | BackendSelection::GpuResidentNv12,
            ) => {
                let gpu = (self.gpu_factory)().map_err(|e| {
                    let code = match e {
                        GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
                        GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
                        _ => "GPU_ERROR",
                    };
                    ServiceError::new(code, e.to_string())
                })?;
                if matches!(self.backend, Backend::Selected(BackendSelection::Gpu)) {
                    configure_external_raster_cache(&gpu, project_path)?;
                }
                run(&gpu)
            }
        }
    }
}
fn configure_external_raster_cache(gpu: &GpuContext, project: &Path) -> Result<(), ServiceError> {
    if std::env::var("KRONELLO_RASTER_CACHE_DISABLE").as_deref() == Ok("1") {
        gpu.record_memory_only_policy(
            kronello_render::PersistentRasterCachePolicy::ExplicitlyDisabled,
        )
        .map_err(|e| ServiceError::new("CACHE_CONFIGURATION", e.to_string()))?;
        return Ok(());
    }
    let explicit = std::env::var_os("KRONELLO_RASTER_CACHE_ROOT").is_some();
    let directory =
        if let Some(path) = std::env::var_os("KRONELLO_RASTER_CACHE_ROOT") {
            std::path::PathBuf::from(path)
        } else if cfg!(target_os = "macos") {
            std::path::PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or_else(|| ServiceError::new("CACHE_CONFIGURATION", "HOME unavailable"))?,
            )
            .join("Library/Caches/kronello/raster")
        } else if cfg!(target_os = "windows") {
            std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
                ServiceError::new("CACHE_CONFIGURATION", "LOCALAPPDATA unavailable")
            })?)
            .join("kronello/raster")
        } else if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
            std::path::PathBuf::from(path).join("kronello/raster")
        } else {
            std::path::PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or_else(|| ServiceError::new("CACHE_CONFIGURATION", "HOME unavailable"))?,
            )
            .join(".cache/kronello/raster")
        };
    if !directory.is_absolute() {
        return Err(ServiceError::new(
            "CACHE_CONFIGURATION",
            "raster cache directory must be absolute",
        ));
    }
    let parent = project.parent().unwrap_or(Path::new("."));
    let project_directory = parent
        .canonicalize()
        .map_err(|e| ServiceError::new("CACHE_CONFIGURATION", e.to_string()))?;
    // Resolve existing ancestors before creating the directory, including
    // symlinks. A cache may never be published within the project directory.
    let mut ancestor = directory.as_path();
    let mut tail = vec![];
    while !ancestor.exists() {
        tail.push(
            ancestor
                .file_name()
                .ok_or_else(|| ServiceError::new("CACHE_CONFIGURATION", "invalid cache ancestor"))?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| ServiceError::new("CACHE_CONFIGURATION", "invalid cache ancestor"))?;
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(|e| ServiceError::new("CACHE_CONFIGURATION", e.to_string()))?;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    if resolved.starts_with(project_directory) {
        if !explicit {
            gpu.record_memory_only_policy(
                kronello_render::PersistentRasterCachePolicy::DefaultDirectoryOverlapsProject,
            )
            .map_err(|e| ServiceError::new("CACHE_CONFIGURATION", e.to_string()))?;
            return Ok(());
        }
        return Err(ServiceError::new(
            "CACHE_CONFIGURATION",
            "raster cache must be outside the project directory",
        ));
    }
    gpu.configure_cache(kronello_gpu::GpuCacheConfig {
        disk: Some(kronello_gpu::DiskRasterConfig {
            directory: resolved,
            capacity: kronello_render::CacheCapacity::default(),
        }),
        ..Default::default()
    })
    .map_err(|e| ServiceError::new("CACHE_CONFIGURATION", e.to_string()))
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
        let font = matches.first().ok_or_else(|| {
            let mut error = ServiceError::new(
                "FONT_MISSING",
                format!(
                    "missing locked font locator: {} ({})",
                    identity.family, identity.postscript_name
                ),
            );
            error.details = Some(serde_json::json!({"font": identity}));
            error
        })?;
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
/// COLOR-003 locked-lattice loading (ADR-0113). Each supplied locator is a
/// local path whose bytes must hash to `LutInput.hash`, parse as a supported
/// `.cube` document, and respect the document lattice ceiling. Verification
/// happens once here; the normalized data then travels inside the snapshot
/// so replayed fixed input never re-reads a locator.
pub(crate) fn load_locked_luts(
    input: &RenderInput,
) -> Result<std::collections::BTreeMap<String, kronello_model::CubeLut>, ServiceError> {
    let mut luts = std::collections::BTreeMap::new();
    for lut in &input.luts {
        if lut.hash.len() != 64
            || !lut
                .hash
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        {
            return Err(ServiceError::invalid(
                "lut input hash must be a lowercase hex sha256",
            ));
        }
        if luts.contains_key(&lut.hash) {
            return Err(ServiceError::invalid("duplicate lut input"));
        }
        let bytes = std::fs::read(&lut.path).map_err(|e| {
            ServiceError::new(
                if e.kind() == std::io::ErrorKind::NotFound {
                    "LUT_MISSING"
                } else {
                    "IO_ERROR"
                },
                e.to_string(),
            )
        })?;
        if format!("{:x}", Sha256::digest(&bytes)) != lut.hash {
            return Err(ServiceError::new(
                "ASSET_HASH_MISMATCH",
                "lut input hash differs",
            ));
        }
        let parsed = kronello_model::CubeLut::parse(&bytes)
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        parsed
            .validate_document_size()
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        luts.insert(lut.hash.clone(), parsed);
    }
    Ok(luts)
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
fn open_existing(path: &Path) -> Result<session::StoreLease, ServiceError> {
    session::open_existing(path)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectOpenMode {
    Normal,
    Safe,
    /// Read-only inspection (ADR-0064) read a snapshot without opening a store.
    ReadOnlySnapshot,
}

fn info(store: &ProjectStore) -> Result<ProjectInfo, ServiceError> {
    let mode = if store.safe_mode() {
        ProjectOpenMode::Safe
    } else {
        ProjectOpenMode::Normal
    };
    snapshot_info(store.snapshot()?, mode)
}
fn read_project_snapshot(path: &Path) -> Result<kronello_store::Snapshot, ServiceError> {
    if !path.is_file() {
        return Err(ServiceError::new(
            "PROJECT_NOT_FOUND",
            "project file does not exist",
        ));
    }
    Ok(ProjectStore::read_snapshot(path)?)
}
fn snapshot_info(
    snapshot: kronello_store::Snapshot,
    open_mode: ProjectOpenMode,
) -> Result<ProjectInfo, ServiceError> {
    let content_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(
            &snapshot.document
        )?)?)
    );
    Ok(ProjectInfo {
        open_mode,
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
        Request::FontPin(r) => local_locator(&r.path),
        Request::SequenceCreate(r) => local_locator(&r.project),
        Request::SequenceQuery(r) => local_locator(&r.project),
        Request::ClipPlace(r) => local_locator(&r.project),
        Request::ClipTrim(r) => local_locator(&r.project),
        Request::ClipStretch(r) => local_locator(&r.project),
        Request::InstanceRetime(r) => local_locator(&r.project),
        Request::TemplateInstanceRetime(r) => local_locator(&r.project),

        Request::RenderSubmit(r) | Request::RenderExport(r) => {
            local_locator(&r.render.output_directory)?;
            render_locators(&r.render.input)
        }
        Request::JobGet(_)
        | Request::JobCancel(_)
        | Request::JobResume(_)
        | Request::JobList(_)
        | Request::JobPrune(_) => Ok(()),
        Request::ProjectCreatePlan(r) => {
            local_locator(&r.project)?;
            document_asset_locators(&r.document)
        }
        Request::ProjectImportPlan(r) => {
            local_locator(&r.project)?;
            document_asset_locators(&r.document)
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
        Request::TemplatePreview(r) => {
            local_locator(&r.project)?;
            for font in &r.fonts {
                local_locator(&font.path)?;
            }
            Ok(())
        }
        Request::TemplateMigrationPlan(r) => {
            local_locator(&r.project)?;
            for font in &r.fonts {
                local_locator(&font.path)?;
            }
            Ok(())
        }
        Request::TemplateDefine(r) => local_locator(&r.project),
        Request::TemplateInstantiate(r) => local_locator(&r.project),
        Request::TemplateSetInput(r) => local_locator(&r.project),
        Request::TemplateSetDuration(r) => local_locator(&r.project),
        Request::EditPlan(r) => local_locator(&r.project),
        Request::EditApply(r) => local_locator(&r.project),
        Request::EditUndo(r) => local_locator(&r.project),
        Request::ExpressionFormat(r) => local_locator(&r.project),
        Request::HistoryList(r) => local_locator(&r.project),
        Request::SceneQuery(r) => {
            local_locator(&r.project)?;
            if let Some(evaluation) = &r.evaluation {
                for font in &evaluation.fonts {
                    local_locator(&font.path)?;
                }
                for lut in &evaluation.luts {
                    local_locator(&lut.path)?;
                }
            }
            Ok(())
        }
        Request::NodeExplain(r) => {
            local_locator(&r.project)?;
            for font in &r.fonts {
                local_locator(&font.path)?;
            }
            for lut in &r.luts {
                local_locator(&lut.path)?;
            }
            Ok(())
        }
        Request::RenderExplain(r) => render_locators(&r.input),
        Request::PropertySample(r) => {
            local_locator(&r.project)?;
            if let Some(fonts) = &r.fonts {
                for font in fonts {
                    local_locator(&font.path)?;
                }
            }
            if let Some(luts) = &r.luts {
                for lut in luts {
                    local_locator(&lut.path)?;
                }
            }
            Ok(())
        }
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
        Request::SvgInspect(_) | Request::SvgExport(_) | Request::CapabilitiesGet(_) => Ok(()),
        Request::SvgImportPlan(r) => local_locator(&r.project),
        Request::AudioAnalyze(r) => local_locator(&r.project),
        Request::CaptionsImportPlan(r) => local_locator(&r.project),
        Request::CaptionsImport(r) => local_locator(&r.plan.project),
        Request::CaptionsExport(r) => local_locator(&r.project),
        Request::LutImport(r) => {
            local_locator(&r.project)?;
            local_locator(&r.path)
        }
        Request::InspectScopes(r) => render_locators(&r.input),
    }
}
fn render_locators(input: &RenderInput) -> Result<(), ServiceError> {
    local_locator(&input.project)?;
    for font in &input.fonts {
        local_locator(&font.path)?;
    }
    for lut in &input.luts {
        local_locator(&lut.path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn project_info_reports_actual_normal_and_safe_open_mode() {
        let folder = tempfile::tempdir().unwrap();
        for (mode, expected) in [
            (
                kronello_store::OpenMode::ForceNormal,
                super::ProjectOpenMode::Normal,
            ),
            (
                kronello_store::OpenMode::ForceSafe,
                super::ProjectOpenMode::Safe,
            ),
        ] {
            let path = folder.path().join(format!("{expected:?}.kronello"));
            let store =
                kronello_store::ProjectStore::open(&path, kronello_store::OpenOptions { mode })
                    .unwrap();
            assert_eq!(super::info(&store).unwrap().open_mode, expected);
            store.close().unwrap();
        }
    }
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
                plan_hash: None,
                idempotency_key: None,
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
                luts: Vec::new(),
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
