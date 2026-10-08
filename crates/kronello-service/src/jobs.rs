//! Fixed-input job orchestration shared by CLI and MCP.
use std::path::{Path, PathBuf};

use kronello_jobs::{JobConfig, JobError, JobRecord, JobStore, Submission};
use kronello_media::{
    AvExportRequest, AvExportSnapshot, DeliveryAudioCodec, MediaRuntime, MovieProfile,
};
use kronello_model::{AssetId, CaptionFormat, DocumentObject, SequenceId};
use kronello_render::{RenderSnapshot, SequenceRequest, frame_samples};
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Backend, BackendSelection, SequenceRenderRequest, Service, ServiceError};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobAudioClip {
    pub asset: AssetId,
    pub stream_index: u32,
    pub placement: TimeRange,
    pub source_in: Time,
    pub gain: f32,
}
impl JobAudioClip {
    fn compile(&self) -> Result<kronello_audio::AudioClip, ServiceError> {
        Ok(kronello_audio::AudioClip {
            asset: self.asset,
            stream_index: self.stream_index,
            placement: self.placement,
            source_in: self.source_in,
            gain: kronello_audio::Gain::new(self.gain)
                .map_err(|e| ServiceError::new(e.code(), e.to_string()))?,
        })
    }
}
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(tag = "format", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobOutput {
    #[default]
    ImageSequence,
    ProResMov {
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        #[serde(default = "movie_profile_v1")]
        profile_version: u32,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    ProResSdrFromHdrMov {
        profile_version: u32,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    ProResHdrMov {
        profile_version: u32,
        transfer: kronello_render::HdrTransfer,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    Av1Mp4 {
        profile_version: u32,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        #[serde(default)]
        audio_codec: DeliveryAudioCodec,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    H264Mov {
        profile_version: u32,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        #[serde(default)]
        audio_codec: DeliveryAudioCodec,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    HevcMov {
        profile_version: u32,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        #[serde(default)]
        audio_codec: DeliveryAudioCodec,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    Av1Webm {
        profile_version: u32,
        #[serde(default)]
        audio: kronello_audio::AudioSourceMode,
        #[serde(default)]
        audio_codec: DeliveryAudioCodec,
        clips: Vec<JobAudioClip>,
        background: [f32; 3],
    },
    /// Subtitle sidecar file (`caption_format` selects srt/vtt/itt). The job
    /// serializes cue documents from the fixed snapshot; no frames render.
    CaptionSidecar {
        sequence: SequenceId,
        caption_format: CaptionFormat,
    },
}
pub(crate) struct MovieSettings<'a> {
    pub profile: MovieProfile,
    pub audio_version: u32,
    pub audio: kronello_audio::AudioSourceMode,
    pub clips: &'a [JobAudioClip],
    pub background: [f32; 3],
}
impl JobOutput {
    pub(crate) fn supported_profile_versions(&self) -> &'static [u32] {
        match self {
            Self::ProResMov { .. } => &[1, 2, 3],
            Self::ProResSdrFromHdrMov { .. }
            | Self::ProResHdrMov { .. }
            | Self::ImageSequence
            | Self::Av1Mp4 { .. }
            | Self::H264Mov { .. }
            | Self::HevcMov { .. }
            | Self::Av1Webm { .. }
            | Self::CaptionSidecar { .. } => &[1],
        }
    }
    pub(crate) fn movie_settings(&self) -> Result<MovieSettings<'_>, ServiceError> {
        let (profile, version, audio, clips, background) = match self {
            Self::ImageSequence | Self::CaptionSidecar { .. } => {
                return Err(ServiceError::invalid(
                    "render.export requires a movie profile",
                ));
            }
            Self::ProResMov {
                profile_version,
                audio,
                clips,
                background,
            } => (
                MovieProfile::ProResPcm24,
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::ProResSdrFromHdrMov {
                profile_version,
                audio,
                clips,
                background,
            } => (
                MovieProfile::ProResSdrFromHdrPcm24V1,
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::ProResHdrMov {
                profile_version,
                transfer,
                audio,
                clips,
                background,
            } => (
                match transfer {
                    kronello_render::HdrTransfer::Pq => MovieProfile::ProResPqPcm24V1,
                    kronello_render::HdrTransfer::Hlg => MovieProfile::ProResHlgPcm24V1,
                },
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::Av1Mp4 {
                profile_version,
                audio,
                audio_codec,
                clips,
                background,
            } => (
                match audio_codec {
                    DeliveryAudioCodec::Alac => MovieProfile::Av1Mp4AlacV1,
                    DeliveryAudioCodec::Aac => MovieProfile::Av1Mp4AacV1,
                    DeliveryAudioCodec::Opus => {
                        return Err(ServiceError::new(
                            "UNSUPPORTED_FEATURE",
                            "Opus in MP4 is not an AUDIO-005 profile; use av1_webm",
                        ));
                    }
                },
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::H264Mov {
                profile_version,
                audio,
                audio_codec,
                clips,
                background,
            } => (
                match audio_codec {
                    DeliveryAudioCodec::Alac => MovieProfile::H264AlacV1,
                    DeliveryAudioCodec::Aac => MovieProfile::H264AacV1,
                    DeliveryAudioCodec::Opus => {
                        return Err(ServiceError::new(
                            "UNSUPPORTED_FEATURE",
                            "Opus in MOV is not an AUDIO-005 profile; use av1_webm",
                        ));
                    }
                },
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::HevcMov {
                profile_version,
                audio,
                audio_codec,
                clips,
                background,
            } => (
                match audio_codec {
                    DeliveryAudioCodec::Alac => MovieProfile::HevcAlacV1,
                    DeliveryAudioCodec::Aac => MovieProfile::HevcAacV1,
                    DeliveryAudioCodec::Opus => {
                        return Err(ServiceError::new(
                            "UNSUPPORTED_FEATURE",
                            "Opus in MOV is not an AUDIO-005 profile; use av1_webm",
                        ));
                    }
                },
                *profile_version,
                *audio,
                clips,
                *background,
            ),
            Self::Av1Webm {
                profile_version,
                audio,
                audio_codec,
                clips,
                background,
            } => (
                match audio_codec {
                    DeliveryAudioCodec::Opus => MovieProfile::Av1WebmOpusV1,
                    DeliveryAudioCodec::Alac | DeliveryAudioCodec::Aac => {
                        return Err(ServiceError::new(
                            "UNSUPPORTED_FEATURE",
                            "av1_webm requires audio_codec \"opus\"",
                        ));
                    }
                },
                *profile_version,
                *audio,
                clips,
                *background,
            ),
        };
        let legacy = profile == MovieProfile::ProResPcm24;
        if (legacy
            && (!self.supported_profile_versions().contains(&version)
                || (version == 1 && audio != kronello_audio::AudioSourceMode::Explicit)))
            || (!legacy && !self.supported_profile_versions().contains(&version))
        {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "unsupported movie version or legacy audio mode",
            ));
        }
        Ok(MovieSettings {
            profile,
            audio_version: if legacy { version } else { 3 },
            audio,
            clips,
            background,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderSubmitRequest {
    /// Optional fence against changes since the caller inspected the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    /// Reuses the synchronous target and time/region request without a job-only
    /// target model. output_directory is the MOV filename for ProResMov.
    pub render: SequenceRenderRequest,
    #[serde(default)]
    pub output: JobOutput,
    #[serde(default)]
    pub required_features: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub job: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobListRequest {}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobPruneRequest {}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobListResult {
    pub jobs: Vec<JobRecord>,
}

/// Immutable job input. Exactly one payload kind is legal: render jobs carry
/// `snapshot` + `request`; `proxy.generate` jobs carry `proxy`;
/// `audio.plugin_process` jobs carry `plugin`. Optional fields keep the
/// render shape byte-compatible with schema_version 1.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FixedInput {
    schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<RenderSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request: Option<RenderSubmitRequest>,
    backend: BackendSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy: Option<crate::proxy::ProxyJobInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin: Option<crate::plugin::PluginJobInput>,
}
impl FixedInput {
    fn render(
        snapshot: RenderSnapshot,
        request: RenderSubmitRequest,
        backend: BackendSelection,
    ) -> Self {
        Self {
            schema_version: 1,
            snapshot: Some(snapshot),
            request: Some(request),
            backend,
            proxy: None,
            plugin: None,
        }
    }
    pub(crate) fn proxy(input: crate::proxy::ProxyJobInput) -> Self {
        Self {
            schema_version: 1,
            snapshot: None,
            request: None,
            // Proxy transcode never touches a render backend.
            backend: BackendSelection::CpuReference,
            proxy: Some(input),
            plugin: None,
        }
    }
    /// AUDIO-011: the plugin payload holds the pinned spec; no render state.
    pub(crate) fn plugin(input: crate::plugin::PluginJobInput) -> Self {
        Self {
            schema_version: 1,
            snapshot: None,
            request: None,
            // Plugin processing never touches a render backend.
            backend: BackendSelection::CpuReference,
            proxy: None,
            plugin: Some(input),
        }
    }
    /// The render payload pair; mutually exclusive with `proxy`/`plugin` by
    /// validation.
    fn render_parts(&self) -> Result<(&RenderSnapshot, &RenderSubmitRequest), ServiceError> {
        match (&self.snapshot, &self.request, &self.proxy, &self.plugin) {
            (Some(snapshot), Some(request), None, None) => Ok((snapshot, request)),
            _ => Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed input is not a render job",
            )),
        }
    }
    pub(crate) fn proxy_parts(&self) -> Result<&crate::proxy::ProxyJobInput, ServiceError> {
        match (&self.snapshot, &self.request, &self.proxy, &self.plugin) {
            (None, None, Some(proxy), None) => Ok(proxy),
            _ => Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed input is not a proxy job",
            )),
        }
    }
    pub(crate) fn plugin_parts(&self) -> Result<&crate::plugin::PluginJobInput, ServiceError> {
        match (&self.snapshot, &self.request, &self.proxy, &self.plugin) {
            (None, None, None, Some(plugin)) => Ok(plugin),
            _ => Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed input is not a plugin job",
            )),
        }
    }
    /// A worker never executes input whose kind disagrees with the job record:
    /// render records carry a content-hashed snapshot; proxy records carry a
    /// `proxy` payload; plugin records carry a `plugin` payload validated
    /// against `record.output_profile`.
    fn kind_matches_record(&self, record: &JobRecord) -> bool {
        match (&self.snapshot, &self.request, &self.proxy, &self.plugin) {
            (Some(_), Some(_), None, None) => record.output_profile.get("render").is_some(),
            (None, None, Some(_), None) => record.output_profile.get("proxy_asset_id").is_some(),
            (None, None, None, Some(_)) => record.output_profile.get("plugin").is_some(),
            _ => false,
        }
    }
}
impl From<JobError> for ServiceError {
    fn from(e: JobError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}
fn job_error(e: ServiceError) -> JobError {
    JobError::new(&e.code, e.message)
}
pub(crate) fn absolute(path: &Path) -> Result<PathBuf, ServiceError> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}
pub(crate) fn features(required: &[String]) -> Result<(), ServiceError> {
    let supported = crate::CapabilitiesResult::current(None).features;
    if let Some(missing) = required.iter().find(|f| !supported.contains(f)) {
        return Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            format!("required feature {missing}"),
        ));
    }
    Ok(())
}
impl Service<'_> {
    /// Embedders supply the same-version executable implementing worker_entry.
    /// CLI and MCP use their own executable by default.
    pub fn with_worker_executable(mut self, executable: PathBuf) -> Self {
        self.worker_executable = Some(executable);
        self
    }
    pub fn with_job_config(mut self, config: JobConfig) -> Self {
        self.job_config = Some(config);
        self
    }
    pub(crate) fn jobs(&self) -> Result<JobStore, ServiceError> {
        Ok(JobStore::open(match &self.job_config {
            Some(config) => config.clone(),
            None => JobConfig::from_env()?,
        })?)
    }
    pub(crate) fn submit_job(
        &self,
        mut request: RenderSubmitRequest,
    ) -> Result<JobRecord, ServiceError> {
        let backend = match self.backend {
            Backend::Selected(selection) => selection,
            Backend::Injected(_) => {
                return Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "in-process injected backend cannot be serialized for worker",
                ));
            }
        };
        request.render.input.region.validate()?;
        features(&request.required_features)?;
        let project_path = request.render.input.project.canonicalize().map_err(|e| {
            ServiceError::new(
                if e.kind() == std::io::ErrorKind::NotFound {
                    "PROJECT_NOT_FOUND"
                } else {
                    "IO_ERROR"
                },
                e.to_string(),
            )
        })?;
        request.render.input.project = project_path.clone();
        request.render.output_directory = absolute(&request.render.output_directory)?;
        if request.render.output_directory.exists() {
            return Err(ServiceError::new(
                "OUTPUT_EXISTS",
                "destination already exists",
            ));
        }
        for font in &mut request.render.input.fonts {
            font.path = absolute(&font.path)?;
        }
        for lut in &mut request.render.input.luts {
            lut.path = absolute(&lut.path)?;
        }
        let stored = crate::session::read_snapshot(&project_path)?;
        check_expected_revision(request.expected_revision.as_deref(), stored.revision)?;
        crate::document_asset_locators(&stored.document)?;
        // COLOR-003 lattices are embedded in the fixed snapshot at submit so
        // worker replay never re-reads a mutable locator (ADR-0113).
        let snapshot = crate::freeze_render_input(&stored, &request.render.input)?
            .with_luts(crate::load_locked_luts(&request.render.input)?);
        // Every fixed job decodes authored originals (ADR-0119), including
        // image-sequence and sidecar outputs that never build a movie snapshot.
        if snapshot.media_proxies() != kronello_render::MediaProxyMode::Off {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "media_proxies is a preview-only switch; jobs use originals",
            ));
        }
        let total_frames =
            frame_samples(request.render.range, request.render.frame_rate)?.len() as u64;
        if total_frames == 0 {
            return Err(ServiceError::invalid(
                "job range must contain at least one frame",
            ));
        }
        match &request.output {
            JobOutput::ImageSequence => (),
            JobOutput::CaptionSidecar {
                sequence,
                caption_format,
            } => {
                validate_sidecar_destination(&request.render.output_directory, *caption_format)?;
                sidecar_content(&snapshot, *sequence, *caption_format)?;
            }
            _ => {
                validate_movie_destination(&request.render.output_directory, &request.output)?;
                movie_snapshot(&snapshot, &request.output)?;
            }
        }
        let store = self.jobs()?;
        let submission = Submission {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            project_id: snapshot.project().id.to_string(),
            revision: snapshot.revision().to_string(),
            snapshot_hash: snapshot.content_hash()?,
            output_profile: serde_json::to_value(&request)?,
            destination: request.render.output_directory.clone(),
            total_frames,
        };
        let fixed = FixedInput::render(snapshot, request, backend);
        let executable = match &self.worker_executable {
            Some(path) => path.clone(),
            None => std::env::current_exe()?,
        };
        let record = store.submit(&serde_json::to_vec(&fixed)?, submission)?;
        if let Err(error) = store.spawn_attempt(&record.id, record.attempt, &executable) {
            store.finish_error_attempt(&record.id, record.attempt, &error)?;
            return Err(error.into());
        }
        Ok(record)
    }
    fn render_fixed_job(
        &self,
        store: &JobStore,
        record: &JobRecord,
        fixed: &FixedInput,
    ) -> Result<(), ServiceError> {
        if !fixed.kind_matches_record(record) {
            return Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed input kind differs from the job record",
            ));
        }
        if let Some(proxy) = &fixed.proxy {
            return crate::proxy::execute_proxy_job(store, record, proxy);
        }
        if let Some(plugin) = &fixed.plugin {
            return crate::plugin::execute_plugin_job(
                store,
                record,
                plugin,
                self.plugin_helper.as_ref(),
            );
        }
        let (snapshot, request) = fixed.render_parts()?;
        self.validate_fixed_job(record, fixed)?;
        self.execute_fixed_job(store, record, snapshot, request)
    }
    pub(crate) fn resume_job(&self, request: JobRequest) -> Result<JobRecord, ServiceError> {
        let store = self.jobs()?;
        let record = store.get(&request.job)?;
        if record.directory_pruned {
            return Err(ServiceError::new(
                "JOB_INPUT_UNAVAILABLE",
                "fixed input was pruned",
            ));
        }
        let fixed: FixedInput = serde_json::from_slice(&store.input(&record)?)?;
        self.validate_fixed_job(&record, &fixed)?;
        if let Ok(proxy) = fixed.proxy_parts() {
            // A published proxy must hash to the receipt bytes exactly.
            if record.destination.exists()
                && let Some(result) = store.publication_result(&record)?
            {
                let report = &result["report"];
                let expected = report["content_hash"].as_str().unwrap_or_default();
                let width = report["width"].as_u64();
                let height = report["height"].as_u64();
                let actual = kronello_media::content_hash(&record.destination)
                    .map_err(|e| ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string()))?;
                let probe = MediaRuntime::load()?
                    .probe(&record.destination)
                    .map_err(|e| ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string()))?;
                let video = probe
                    .streams
                    .iter()
                    .find(|s| s.kind == kronello_media::StreamKind::Video);
                if actual != expected
                    || video.map(|v| (v.width.map(u64::from), v.height.map(u64::from)))
                        != Some((width, height))
                    || report["proxy_asset_id"].as_str()
                        != Some(proxy.proxy_asset_id.to_string().as_str())
                {
                    return Err(ServiceError::new(
                        "OUTPUT_VALIDATION_FAILED",
                        "published proxy differs from the validated receipt",
                    ));
                }
            }
        } else if let Ok(plugin) = fixed.plugin_parts() {
            // A published plugin .mov must hash to the receipt bytes exactly.
            if record.destination.exists()
                && let Some(result) = store.publication_result(&record)?
            {
                let report = &result["report"];
                let expected = report["content_hash"].as_str().unwrap_or_default();
                let actual = kronello_media::content_hash(&record.destination)
                    .map_err(|e| ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string()))?;
                if actual != expected
                    || report["asset"].as_str() != Some(plugin.asset.id.to_string().as_str())
                    || report["plugin"]["component"].as_str()
                        != Some(plugin.plugin.component.as_str())
                {
                    return Err(ServiceError::new(
                        "OUTPUT_VALIDATION_FAILED",
                        "published plugin output differs from the validated receipt",
                    ));
                }
            }
        } else if record.destination.exists()
            && let Some(result) = store.publication_result(&record)?
        {
            let (_, request) = fixed.render_parts()?;
            match &request.output {
                JobOutput::ImageSequence => {
                    let metadata: kronello_render::SequenceMetadata =
                        serde_json::from_value(result["report"].clone())?;
                    validate_sequence(
                        &record.destination,
                        &metadata,
                        record.total_frames,
                        &record.snapshot_hash,
                    )
                    .map_err(|error| {
                        ServiceError::new("OUTPUT_VALIDATION_FAILED", error.to_string())
                    })?;
                }
                JobOutput::CaptionSidecar { .. } => {
                    let bytes = std::fs::read(&record.destination).map_err(|error| {
                        ServiceError::new("OUTPUT_VALIDATION_FAILED", error.to_string())
                    })?;
                    let report = &result["report"];
                    if bytes.len() as u64 != report["bytes"].as_u64().unwrap_or_default()
                        || format!("{:x}", Sha256::digest(&bytes))
                            != report["sha256"].as_str().unwrap_or_default()
                        || report["render_snapshot_hash"].as_str()
                            != Some(record.snapshot_hash.as_str())
                    {
                        return Err(ServiceError::new(
                            "OUTPUT_VALIDATION_FAILED",
                            "published caption sidecar differs",
                        ));
                    }
                }
                output => {
                    let settings = output.movie_settings()?;
                    let probe =
                        MediaRuntime::load()?
                            .probe(&record.destination)
                            .map_err(|error| {
                                ServiceError::new("OUTPUT_VALIDATION_FAILED", error.to_string())
                            })?;
                    probe.verify_movie(settings.profile).map_err(|error| {
                        ServiceError::new("OUTPUT_VALIDATION_FAILED", error.to_string())
                    })?;
                    let (snapshot, _) = fixed.render_parts()?;
                    let expected = movie_snapshot(snapshot, output)?;
                    let report: kronello_media::AvExportReport =
                        serde_json::from_value(result["report"].clone())?;
                    if probe.render_snapshot_hash != record.snapshot_hash
                        || probe.export_snapshot_hash != expected.content_hash()?
                        || report.frames.len() as u64 != record.total_frames
                        || report.render_snapshot_hash != record.snapshot_hash
                        || report
                            .frames
                            .iter()
                            .any(|frame| frame.snapshot_content_hash != record.snapshot_hash)
                    {
                        return Err(ServiceError::new(
                            "OUTPUT_VALIDATION_FAILED",
                            "published movie snapshot differs",
                        ));
                    }
                }
            }
        }
        let resumed = store.resume(&record.id)?;
        if resumed.status == kronello_jobs::JobStatus::Queued {
            let executable = self
                .worker_executable
                .clone()
                .unwrap_or(std::env::current_exe()?);
            if let Err(error) = store.spawn_attempt(&resumed.id, resumed.attempt, &executable) {
                store.finish_error_attempt(&resumed.id, resumed.attempt, &error)?;
                return Err(error.into());
            }
        }
        Ok(resumed)
    }
    fn validate_fixed_job(
        &self,
        record: &JobRecord,
        fixed: &FixedInput,
    ) -> Result<(), ServiceError> {
        if fixed.schema_version != 1 {
            return Err(ServiceError::new(
                "UNSUPPORTED_SCHEMA_VERSION",
                "job input envelope",
            ));
        }
        if record.engine_version != env!("CARGO_PKG_VERSION") {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "job engine version differs",
            ));
        }
        if let Ok(proxy) = fixed.proxy_parts() {
            return self.validate_proxy_job(record, proxy);
        }
        if let Ok(plugin) = fixed.plugin_parts() {
            return crate::plugin::validate_plugin_job(record, plugin);
        }
        let (snapshot, request) = fixed.render_parts()?;
        snapshot.validate()?;
        // Fixed-input file outputs decode authored originals only (ADR-0119);
        // preview proxy mode is legal input but must never reach a worker.
        if snapshot.media_proxies() != kronello_render::MediaProxyMode::Off {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "fixed jobs cannot substitute preview proxies",
            ));
        }
        request.render.input.region.validate()?;
        match &request.output {
            JobOutput::ImageSequence => (),
            JobOutput::CaptionSidecar {
                sequence,
                caption_format,
            } => {
                validate_sidecar_destination(&request.render.output_directory, *caption_format)?;
                sidecar_content(snapshot, *sequence, *caption_format)?;
            }
            output => {
                movie_snapshot(snapshot, output)?;
            }
        }
        features(&request.required_features)?;
        if snapshot.content_hash()? != record.snapshot_hash {
            return Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "snapshot identity differs",
            ));
        }
        // External assets remain references, verified by the worker every time.
        for asset in &snapshot.project().assets {
            match asset {
                DocumentObject::Known(asset) => {
                    kronello_media::resolve_asset(asset, &request.render.input.project)?;
                }
                DocumentObject::Opaque(_) => {
                    return Err(ServiceError::new(
                        "UNSUPPORTED_FEATURE",
                        "opaque asset lock",
                    ));
                }
            }
        }
        // Sidecar serialization reads cue documents only; no fonts are loaded.
        if !matches!(request.output, JobOutput::CaptionSidecar { .. }) {
            let font_bytes = crate::load_locked_fonts(snapshot, &request.render.input)?;
            let _ = font_bytes;
        }
        if request.render.output_directory != record.destination
            || serde_json::to_value(request)? != record.output_profile
            || snapshot.project().id.to_string() != record.project_id
            || snapshot.revision().to_string() != record.revision
            || frame_samples(request.render.range, request.render.frame_rate)?.len() as u64
                != record.total_frames
        {
            return Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed request identity differs",
            ));
        }
        Ok(())
    }
    /// `proxy.generate` identity checks shared by the worker and `job.resume`:
    /// the fixed payload is the whole contract (asset object, dimensions,
    /// destination) and must equal the recorded submission exactly.
    fn validate_proxy_job(
        &self,
        record: &JobRecord,
        input: &crate::proxy::ProxyJobInput,
    ) -> Result<(), ServiceError> {
        if input.document_hash != record.snapshot_hash
            || input.destination != record.destination
            || serde_json::to_value(input)? != record.output_profile
        {
            return Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed proxy input identity differs",
            ));
        }
        input
            .asset
            .validate()
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        // The source asset is an external reference; content verify is live.
        kronello_media::resolve_asset(&input.asset, &input.project)?;
        Ok(())
    }
    fn execute_fixed_job(
        &self,
        store: &JobStore,
        record: &JobRecord,
        snapshot: &RenderSnapshot,
        request: &RenderSubmitRequest,
    ) -> Result<(), ServiceError> {
        let sidecar = matches!(request.output, JobOutput::CaptionSidecar { .. });
        // Sidecar jobs serialize stored cue documents and never rasterize;
        // they do not require locked font inputs.
        let font_bytes = if sidecar {
            Vec::new()
        } else {
            crate::load_locked_fonts(snapshot, &request.render.input)?
        };
        let fonts: Vec<_> = snapshot
            .font_locks()
            .iter()
            .zip(&font_bytes)
            .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
            .collect();
        let destination = &request.render.output_directory;
        let staging = store.staging(record)?;
        let stage_path = staging.output();
        let mut failure = None;
        let mut checkpoint = |completed| {
            #[cfg(all(feature = "test-job-control", debug_assertions))]
            if completed > 0
                && std::env::var_os("KRONELLO_TEST_JOB_DEVICE_LOST").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
            {
                let error = JobError::new(
                    "GPU_DEVICE_LOST",
                    "injected device loss after first completed frame",
                );
                let message = error.to_string();
                failure = Some(error);
                return Err(message);
            }
            #[cfg(all(feature = "test-job-control", debug_assertions))]
            if completed > 0
                && std::env::var_os("KRONELLO_TEST_JOB_DISK_FULL").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
            {
                let error = JobError::Io(std::io::Error::from(std::io::ErrorKind::StorageFull));
                let message = error.to_string();
                failure = Some(error);
                return Err(message);
            }
            #[cfg(all(feature = "test-job-control", debug_assertions))]
            if let Some(gate) = std::env::var_os("KRONELLO_TEST_JOB_GATE") {
                let gate = PathBuf::from(gate);
                let after = std::env::var("KRONELLO_TEST_JOB_GATE_AFTER_FRAME")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                while completed >= after && !gate.exists() {
                    if let Err(error) = store.checkpoint(&record.id, completed) {
                        let message = error.to_string();
                        failure = Some(error);
                        return Err(message);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            store.checkpoint(&record.id, completed).map_err(|e| {
                let message = e.to_string();
                failure = Some(e);
                message
            })
        };
        let rendered = if let JobOutput::CaptionSidecar {
            sequence,
            caption_format,
        } = &request.output
        {
            let content = sidecar_content(snapshot, *sequence, *caption_format)?;
            std::fs::write(&stage_path, content.as_bytes())?;
            Ok(serde_json::json!({
                "caption_format": caption_format,
                "bytes": content.len() as u64,
                "sha256": format!("{:x}", Sha256::digest(content.as_bytes())),
                "render_snapshot_hash": record.snapshot_hash,
            }))
        } else {
            self.with_video_backend(&request.render.input.project, |backend| {
                let render = &request.render;
                let result = match &request.output {
                    JobOutput::ImageSequence => {
                        let metadata = kronello_render::render_sequence_with_checkpoint(
                            snapshot,
                            &fonts,
                            backend,
                            SequenceRequest {
                                range: render.range,
                                frame_rate: render.frame_rate,
                                region: render.input.region,
                            },
                            &stage_path,
                            &mut |n| {
                                checkpoint(n).map_err(kronello_render::RenderError::InvalidInput)
                            },
                        )?;
                        #[cfg(all(feature = "test-job-control", debug_assertions))]
                        if std::env::var_os("KRONELLO_TEST_JOB_CORRUPT_OUTPUT").as_deref()
                            == Some(std::ffi::OsStr::new("1"))
                        {
                            std::fs::write(
                                stage_path.join(&metadata.frames[0].display.name),
                                b"corrupt",
                            )?;
                        }
                        validate_sequence(
                            &stage_path,
                            &metadata,
                            record.total_frames,
                            &record.snapshot_hash,
                        )?;
                        serde_json::to_value(metadata)?
                    }
                    output => {
                        let settings = output.movie_settings()?;
                        let runtime = MediaRuntime::load()?;
                        let av = movie_snapshot(snapshot, &request.output)?;
                        let report = runtime.export_av_with_checkpoint(
                            &av,
                            &render.input.project,
                            &fonts,
                            backend,
                            &AvExportRequest {
                                output: stage_path.clone(),
                                range: render.range,
                                frame_rate: render.frame_rate,
                                region: render.input.region,
                                background: settings.background,
                                clipping: kronello_audio::ClippingPolicy::Reject,
                            },
                            &mut |n| {
                                checkpoint(n).map_err(kronello_media::MediaError::InvalidInput)
                            },
                        )?;
                        #[cfg(all(feature = "test-job-control", debug_assertions))]
                        if std::env::var_os("KRONELLO_TEST_JOB_CORRUPT_OUTPUT").as_deref()
                            == Some(std::ffi::OsStr::new("1"))
                        {
                            std::fs::write(&stage_path, b"corrupt")?;
                        }
                        let probe = runtime.probe(&stage_path).map_err(|e| {
                            ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string())
                        })?;
                        probe.verify_movie(settings.profile)?;
                        if report.frames.len() as u64 != record.total_frames
                            || probe.render_snapshot_hash != record.snapshot_hash
                        {
                            return Err(ServiceError::new(
                                "OUTPUT_VALIDATION_FAILED",
                                "MOV frame count or snapshot identity differs",
                            ));
                        }
                        serde_json::to_value(report)?
                    }
                };
                Ok(result)
            })
        };
        if let Some(error) = failure {
            return Err(error.into());
        }
        let mut result = rendered?;
        // Paths in the report describe the deliverable rather than staging.
        if let Some(request) = result.get_mut("request") {
            request["output"] = serde_json::to_value(destination)?;
        }
        let result =
            serde_json::json!({"destination": destination, "validated": true, "report": result});
        store.prepare_publication(record, &stage_path, result.clone())?;
        store.publish_attempt(&record.id, record.attempt, result, || {
            kronello_jobs::publish_path(&stage_path, destination)?;
            #[cfg(all(feature = "test-job-control", debug_assertions))]
            if let Some(gate) = std::env::var_os("KRONELLO_TEST_JOB_AFTER_RENAME_GATE") {
                while !Path::new(&gate).exists() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            Ok(())
        })?;
        Ok(())
    }
}
fn validate_sequence(
    path: &Path,
    metadata: &kronello_render::SequenceMetadata,
    count: u64,
    snapshot_hash: &str,
) -> Result<(), ServiceError> {
    if metadata.frames.len() as u64 != count {
        return Err(ServiceError::new(
            "OUTPUT_VALIDATION_FAILED",
            "frame count differs",
        ));
    }
    let reread: kronello_render::SequenceMetadata =
        serde_json::from_slice(&std::fs::read(path.join("sequence.json"))?)?;
    if &reread != metadata {
        return Err(ServiceError::new(
            "OUTPUT_VALIDATION_FAILED",
            "manifest differs",
        ));
    }
    for frame in &metadata.frames {
        if frame.metadata.snapshot_content_hash != snapshot_hash {
            return Err(ServiceError::new(
                "OUTPUT_VALIDATION_FAILED",
                "snapshot differs",
            ));
        }
        for artifact in [&frame.numeric, &frame.display] {
            let bytes = std::fs::read(path.join(&artifact.name))?;
            if bytes.len() as u64 != artifact.bytes
                || format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256
            {
                return Err(ServiceError::new(
                    "OUTPUT_VALIDATION_FAILED",
                    "artifact content differs",
                ));
            }
        }
        let stored: kronello_render::FrameMetadata =
            serde_json::from_slice(&std::fs::read(path.join(&frame.metadata_file))?)?;
        if stored != frame.metadata {
            return Err(ServiceError::new(
                "OUTPUT_VALIDATION_FAILED",
                "frame metadata differs",
            ));
        }
    }
    Ok(())
}
/// Called before transport setup by both binaries. A worker never reads stdin.
pub fn worker_entry() -> Option<std::process::ExitCode> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("worker") {
        return None;
    }
    let result = (|| {
        kronello_jobs::detach_worker()?;
        eprintln!(
            "worker startup pid={} at_ms={} args={args:?}",
            std::process::id(),
            kronello_jobs::now_ms()
        );
        if args.len() != 3 || args[1] != "--job" {
            return Err(JobError::new("INVALID_REQUEST", "worker --job <id>"));
        }
        let store = JobStore::open(JobConfig::from_env()?)?;
        let id = &args[2];
        let owned_attempt = store.get(id)?.attempt;
        let heartbeat = kronello_jobs::WorkerHeartbeat::start(store.clone(), id.clone());
        let result = (|| {
            store.wait_for_slot(id)?;
            let record = store.get(id)?;
            let fixed: FixedInput = serde_json::from_slice(&store.input(&record)?)?;
            Service::new(fixed.backend)
                .render_fixed_job(&store, &record, &fixed)
                .map_err(job_error)
        })();
        drop(heartbeat);
        if let Err(error) = &result {
            store.finish_error_attempt(id, owned_attempt, error)?;
        }
        result
    })();
    Some(match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    })
}

pub(crate) fn movie_profile_v1() -> u32 {
    1
}
pub(crate) fn movie_snapshot(
    snapshot: &RenderSnapshot,
    output: &JobOutput,
) -> Result<AvExportSnapshot, ServiceError> {
    let settings = output.movie_settings()?;
    let clips = settings
        .clips
        .iter()
        .map(JobAudioClip::compile)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if settings.profile != MovieProfile::ProResPcm24 {
        AvExportSnapshot::with_movie_profile(snapshot, settings.audio, clips, settings.profile)?
    } else if settings.audio_version == 1 {
        AvExportSnapshot::new(snapshot, clips)?
    } else {
        AvExportSnapshot::with_audio_profile(
            snapshot,
            settings.audio,
            clips,
            settings.audio_version,
        )?
    })
}

/// Serialize one sequence's caption cues from a fixed snapshot for a sidecar
/// job. The same validation runs at submit, worker validation and execution.
pub(crate) fn sidecar_content(
    snapshot: &RenderSnapshot,
    sequence: SequenceId,
    format: CaptionFormat,
) -> Result<String, ServiceError> {
    let sequence = snapshot
        .project()
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))?;
    crate::captions::serialize(snapshot.project(), sequence, format)
}

pub(crate) fn validate_sidecar_destination(
    path: &Path,
    format: CaptionFormat,
) -> Result<(), ServiceError> {
    if path.extension().is_none_or(|e| e != format.extension()) {
        return Err(ServiceError::new(
            "INVALID_MEDIA_INPUT",
            format!(
                "caption sidecar requires .{} destination",
                format.extension()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn validate_movie_destination(
    path: &Path,
    output: &JobOutput,
) -> Result<(), ServiceError> {
    let settings = output.movie_settings()?;
    let extension = settings.profile.container();
    if path.extension().is_none_or(|e| e != extension) {
        if settings.profile == MovieProfile::ProResPcm24 {
            return Err(ServiceError::invalid("ProResMov requires .mov destination"));
        }
        return Err(ServiceError::new(
            "INVALID_MEDIA_INPUT",
            format!("movie profile requires .{extension} destination"),
        ));
    }
    Ok(())
}

pub(crate) fn check_expected_revision(
    expected: Option<&str>,
    revision: u64,
) -> Result<(), ServiceError> {
    if let Some(expected) = expected {
        let base = crate::parse_revision(expected)?;
        if base != revision {
            return Err(kronello_store::StoreError::RevisionConflict {
                base,
                current: revision,
            }
            .into());
        }
    }
    Ok(())
}
