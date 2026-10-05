//! Fixed-input job orchestration shared by CLI and MCP.
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use kronello_jobs::{JobConfig, JobError, JobRecord, JobStore, Submission};
use kronello_media::{
    AvExportRequest, AvExportSnapshot, DeliveryAudioCodec, MediaRuntime, MovieProfile,
};
use kronello_model::{AssetId, DocumentObject};
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
            Self::ImageSequence
            | Self::Av1Mp4 { .. }
            | Self::H264Mov { .. }
            | Self::HevcMov { .. } => &[1],
        }
    }
    pub(crate) fn movie_settings(&self) -> Result<MovieSettings<'_>, ServiceError> {
        let (profile, version, audio, audio_codec, clips, background) = match self {
            Self::ImageSequence => {
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
                DeliveryAudioCodec::Alac,
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
                MovieProfile::Av1Mp4AlacV1,
                *profile_version,
                *audio,
                *audio_codec,
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
                MovieProfile::H264AlacV1,
                *profile_version,
                *audio,
                *audio_codec,
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
                MovieProfile::HevcAlacV1,
                *profile_version,
                *audio,
                *audio_codec,
                clips,
                *background,
            ),
        };
        let legacy = profile == MovieProfile::ProResPcm24;
        if (legacy
            && (!self.supported_profile_versions().contains(&version)
                || (version == 1 && audio != kronello_audio::AudioSourceMode::Explicit)))
            || (!legacy
                && (!self.supported_profile_versions().contains(&version)
                    || audio_codec != DeliveryAudioCodec::Alac))
        {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "unsupported movie version/audio codec or legacy audio mode; AAC adoption is deferred",
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixedInput {
    schema_version: u32,
    snapshot: RenderSnapshot,
    request: RenderSubmitRequest,
    backend: BackendSelection,
}
impl From<JobError> for ServiceError {
    fn from(e: JobError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}
fn job_error(e: ServiceError) -> JobError {
    JobError::new(&e.code, e.message)
}
fn absolute(path: &Path) -> Result<PathBuf, ServiceError> {
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
        let stored = kronello_store::ProjectStore::read_snapshot(&project_path)?;
        check_expected_revision(request.expected_revision.as_deref(), stored.revision)?;
        crate::document_asset_locators(&stored.document)?;
        let snapshot = crate::freeze_render_input(&stored, &request.render.input)?;
        let total_frames =
            frame_samples(request.render.range, request.render.frame_rate)?.len() as u64;
        if total_frames == 0 {
            return Err(ServiceError::invalid(
                "job range must contain at least one frame",
            ));
        }
        if !matches!(request.output, JobOutput::ImageSequence) {
            validate_movie_destination(&request.render.output_directory, &request.output)?;
            movie_snapshot(&snapshot, &request.output)?;
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
        let fixed = FixedInput {
            schema_version: 1,
            snapshot,
            request,
            backend,
        };
        let executable = match &self.worker_executable {
            Some(path) => path.clone(),
            None => std::env::current_exe()?,
        };
        let record = store.submit(&serde_json::to_vec(&fixed)?, submission)?;
        if let Err(error) = store.spawn(&record.id, &executable) {
            store.finish_error(&record.id, &error)?;
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
        fixed.snapshot.validate()?;
        features(&fixed.request.required_features)?;
        if fixed.snapshot.content_hash()? != record.snapshot_hash {
            return Err(ServiceError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "snapshot identity differs",
            ));
        }
        // External assets remain references, verified by the worker every time.
        for asset in &fixed.snapshot.project().assets {
            match asset {
                DocumentObject::Known(asset) => {
                    kronello_media::resolve_asset(asset, &fixed.request.render.input.project)?;
                }
                DocumentObject::Opaque(_) => {
                    return Err(ServiceError::new(
                        "UNSUPPORTED_FEATURE",
                        "opaque asset lock",
                    ));
                }
            }
        }
        let font_bytes = crate::load_locked_fonts(&fixed.snapshot, &fixed.request.render.input)?;
        let fonts: Vec<_> = fixed
            .snapshot
            .font_locks()
            .iter()
            .zip(&font_bytes)
            .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
            .collect();
        let destination = &fixed.request.render.output_directory;
        let parent = destination
            .parent()
            .ok_or_else(|| ServiceError::invalid("destination has no parent"))?;
        let staging = tempfile::Builder::new()
            .prefix(".kronello-job-")
            .tempdir_in(parent)?;
        let stage_path = staging.path().join("output");
        let mut failure = None;
        let mut checkpoint = |completed| {
            #[cfg(all(feature = "test-job-control", debug_assertions))]
            if let Some(gate) = std::env::var_os("KRONELLO_TEST_JOB_GATE") {
                let gate = PathBuf::from(gate);
                while !gate.exists() {
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
        let rendered = self.with_selected_backend(|backend| {
            let video_backend = kronello_media::VideoRenderBackend {
                backend,
                project_path: &fixed.request.render.input.project,
            };
            let backend = &video_backend;
            let request = &fixed.request.render;
            let result = match &fixed.request.output {
                JobOutput::ImageSequence => {
                    let metadata = kronello_render::render_sequence_with_checkpoint(
                        &fixed.snapshot,
                        &fonts,
                        backend,
                        SequenceRequest {
                            range: request.range,
                            frame_rate: request.frame_rate,
                            region: request.input.region,
                        },
                        &stage_path,
                        &mut |n| checkpoint(n).map_err(kronello_render::RenderError::InvalidInput),
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
                    let av = movie_snapshot(&fixed.snapshot, &fixed.request.output)?;
                    let report = runtime.export_av_with_checkpoint(
                        &av,
                        &request.input.project,
                        &fonts,
                        backend,
                        &AvExportRequest {
                            output: stage_path.clone(),
                            range: request.range,
                            frame_rate: request.frame_rate,
                            region: request.input.region,
                            background: settings.background,
                            clipping: kronello_audio::ClippingPolicy::Reject,
                        },
                        &mut |n| checkpoint(n).map_err(kronello_media::MediaError::InvalidInput),
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
        });
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
        store.publish(&record.id, result, || {
            kronello_jobs::publish_path(&stage_path, destination)
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
    eprintln!(
        "worker startup pid={} at_ms={} args={args:?}",
        std::process::id(),
        kronello_jobs::now_ms()
    );
    let result = (|| {
        kronello_jobs::detach_worker()?;
        if args.len() != 3 || args[1] != "--job" {
            return Err(JobError::new("INVALID_REQUEST", "worker --job <id>"));
        }
        let store = JobStore::open(JobConfig::from_env()?)?;
        let id = &args[2];
        store.get(id)?;
        let (stop, receive) = mpsc::channel();
        let pulse_store = store.clone();
        let pulse_id = id.clone();
        let heartbeat = std::thread::spawn(move || {
            eprintln!(
                "worker heartbeat started job={pulse_id} at_ms={}",
                kronello_jobs::now_ms()
            );
            let mut missed = false;
            while receive.recv_timeout(pulse_store.config().heartbeat_interval)
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                match pulse_store.heartbeat(&pulse_id) {
                    Ok(()) => {
                        if missed {
                            eprintln!(
                                "worker heartbeat recovered job={pulse_id} at_ms={}",
                                kronello_jobs::now_ms()
                            );
                            missed = false;
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "worker heartbeat failed job={pulse_id} at_ms={}: {error}",
                            kronello_jobs::now_ms()
                        );
                        if !error.is_retryable_heartbeat() {
                            break;
                        }
                        missed = true;
                    }
                }
            }
        });
        let result = (|| {
            loop {
                if store.claim(id)? {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let record = store.get(id)?;
            let fixed: FixedInput = serde_json::from_slice(&store.input(&record)?)?;
            Service::new(fixed.backend)
                .render_fixed_job(&store, &record, &fixed)
                .map_err(job_error)
        })();
        let _ = stop.send(());
        if heartbeat.join().is_err() {
            eprintln!("worker heartbeat thread panicked job={id}");
        }
        if let Err(error) = &result {
            store.finish_error(id, error)?;
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

pub(crate) fn validate_movie_destination(
    path: &Path,
    output: &JobOutput,
) -> Result<(), ServiceError> {
    let settings = output.movie_settings()?;
    let extension = if settings.profile == MovieProfile::Av1Mp4AlacV1 {
        "mp4"
    } else {
        "mov"
    };
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
