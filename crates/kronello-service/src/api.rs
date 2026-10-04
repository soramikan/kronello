//! Versioned discovery for transports. Capabilities describe compiled support;
//! they do not claim physical device availability. Media reports the loaded runtime.
use serde::{Deserialize, Serialize};

use crate::*;

pub const API_SCHEMA_VERSION: u32 = 1;
pub const API_SCHEMA_ID: &str = "https://kronello.dev/schemas/api-v1.schema.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesRequest {}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandDescriptor {
    pub name: String,
    /// Read-only refers to the source project: render.sequence and
    /// project.collect write output files without changing that project.
    pub read_only: bool,
    /// Payload schema (without transport operation tag).
    pub request_schema: String,
    /// Successful result value schema (without status/kind envelope).
    pub response_schema: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaCapabilities {
    pub runtime_version: String,
    pub decoders: Vec<String>,
    pub encoders: Vec<String>,
    pub hwaccels: Vec<String>,
    pub schema_version: u32,
    pub ffmpeg_version: String,
    pub library_directory: std::path::PathBuf,
    pub substituted: bool,
    pub libraries: Vec<kronello_media::LibraryCapability>,
    pub distribution_eligible: bool,
    pub development_only: bool,
    pub codecs: Vec<kronello_media::CodecCapability>,
}
impl From<kronello_media::MediaCapabilities> for MediaCapabilities {
    fn from(runtime: kronello_media::MediaCapabilities) -> Self {
        Self {
            runtime_version: runtime.ffmpeg_version.clone(),
            decoders: runtime
                .codecs
                .iter()
                .filter(|c| c.decoder)
                .map(|c| c.name.clone())
                .collect(),
            encoders: runtime
                .codecs
                .iter()
                .filter(|c| c.encoder)
                .map(|c| c.name.clone())
                .collect(),
            hwaccels: runtime.hwaccels,
            schema_version: runtime.schema_version,
            ffmpeg_version: runtime.ffmpeg_version,
            library_directory: runtime.library_directory,
            substituted: runtime.substituted,
            libraries: runtime.libraries,
            distribution_eligible: runtime.distribution_eligible,
            development_only: runtime.development_only,
            codecs: runtime.codecs,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesResult {
    pub api_schema_version: u32,
    pub engine_version: String,
    pub semantic_versions: kronello_render::SemanticVersions,
    pub commands: Vec<CommandDescriptor>,
    pub features: Vec<String>,
    pub effects: Vec<String>,
    pub backends: Vec<String>,
    /// Loaded FFmpeg capabilities; discovery failures return a typed error.
    /// An explicitly supplied report avoids runtime initialization.
    pub media: Option<MediaCapabilities>,
}

macro_rules! commands {
    ($emit:ident) => {
        $emit! {
            ("sequence.query", true, SequenceQueryRequest, SequenceQueryResult),
            ("sequence.create", false, SequenceCreateRequest, kronello_store::Event),
            ("clip.place", false, ClipPlaceRequest, kronello_store::Event),
            ("clip.trim", false, ClipTrimRequest, kronello_store::Event),
            ("clip.stretch", false, ClipStretchRequest, kronello_store::Event),
            ("instance.retime", false, InstanceRetimeRequest, kronello_store::Event),
            ("template_instance.retime", false, TemplateInstanceRetimeRequest, kronello_store::Event),

            ("render.submit", true, RenderSubmitRequest, kronello_jobs::JobRecord),
            ("job.get", true, JobRequest, kronello_jobs::JobRecord),
            ("job.list", true, JobListRequest, JobListResult),
            ("job.cancel", true, JobRequest, kronello_jobs::JobRecord),
            ("job.prune", true, JobPruneRequest, kronello_jobs::PruneResult),
            ("project.create", false, CreateRequest, ProjectInfo),
            ("project.import", false, ImportRequest, ProjectInfo),
            ("project.export", true, ProjectRequest, ExportResult),
            ("project.info", true, ProjectRequest, ProjectInfo),
            ("asset.relink", false, RelinkRequest, ProjectInfo),
            ("project.collect", true, CollectRequest, kronello_media::CollectedProject),
            ("render.frame", true, FrameRenderRequest, FrameResult),
            ("render.sequence", true, SequenceRenderRequest, kronello_render::SequenceMetadata),
            ("edit.plan", true, PlanRequest, EditPlan),
            ("edit.apply", false, EditApplyRequest, kronello_store::Event),
            ("edit.undo", false, UndoRequest, kronello_store::Event),
            ("history.list", true, HistoryRequest, HistoryResult),
            ("scene.query", true, SceneQueryRequest, SceneQueryResult),
            ("property.sample", true, PropertySampleRequest, PropertySampleResult),
            ("capabilities.get", true, CapabilitiesRequest, CapabilitiesResult),
            ("template.define", false, TemplateDefineRequest, kronello_store::Event),
            ("template.instantiate", false, TemplateInstantiateRequest, kronello_store::Event),
            ("template.set_input", false, TemplateSetInputRequest, kronello_store::Event),
            ("template.set_duration", false, TemplateSetDurationRequest, kronello_store::Event)
        }
    };
}
/// One registry supplies capability discovery and schema references to MCP/FFI.
pub fn command_registry() -> Vec<CommandDescriptor> {
    macro_rules! registry {
        ($(($name:literal, $read:literal, $input:ty, $output:ty)),*) => {
            vec![$(CommandDescriptor { name: $name.into(), read_only: $read,
                request_schema: format!("{API_SCHEMA_ID}#/$defs/{}", <$input as schemars::JsonSchema>::schema_name()),
                response_schema: format!("{API_SCHEMA_ID}#/$defs/{}", <$output as schemars::JsonSchema>::schema_name()),
            }),*]
        };
    }
    commands!(registry)
}
impl CapabilitiesResult {
    pub fn current(media: Option<MediaCapabilities>) -> Self {
        Self {
            api_schema_version: API_SCHEMA_VERSION,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            semantic_versions: kronello_render::SemanticVersions::current(
                kronello_model::PROJECT_SEMANTIC_VERSION,
            ),
            commands: command_registry(),
            features: [
                "sequence",
                "composition_clip",
                "video_clip",
                "generator_clip",
                "clip_effects",
                "crossfade",
                "ripple",
                "linked_move",
                "composition",
                "shape",
                "text",
                "group",
                "null",
                "composition_instance",
                "constant",
                "curve",
                "expression",
                "selective_undo",
                "template",
            ]
            .map(String::from)
            .to_vec(),
            effects: vec![
                kronello_model::GAUSSIAN_BLUR_ID.into(),
                kronello_model::DROP_SHADOW_ID.into(),
            ],
            backends: ["wgpu_rgba16f", "cpu_reference_float32"]
                .map(String::from)
                .to_vec(),
            media,
        }
    }
}
#[derive(schemars::JsonSchema)]
#[schemars(untagged)]
#[allow(dead_code)]
enum ApiEnvelope {
    Request(Box<Request>),
    Response(Response),
}

/// Draft 2020-12 generated from the same types used by every transport. Every
/// registry payload/result has a stable definition, even if used only once.
pub fn api_json_schema() -> serde_json::Value {
    let mut generator = schemars::SchemaGenerator::default();
    macro_rules! schemas {
        ($(($name:literal, $read:literal, $input:ty, $output:ty)),*) => {
            $(let _ = generator.subschema_for::<$input>();
              let _ = generator.subschema_for::<$output>();)*
        };
    }
    commands!(schemas);
    let _ = generator.subschema_for::<Request>();
    let _ = generator.subschema_for::<Response>();
    let schema = generator.into_root_schema_for::<ApiEnvelope>();
    let mut json = serde_json::to_value(schema).expect("schema serialization");
    json["$id"] = API_SCHEMA_ID.into();
    json["x-api-schema-version"] = API_SCHEMA_VERSION.into();
    json
}
