//! AUDIO-011 plugin boundary (ADR-0131). `audio.plugin_probe` runs a bounded
//! describe exchange through the detached plugin helper; `audio.plugin_process`
//! submits a fixed-input job whose worker decodes a locked asset stream, passes
//! it through the detached helper, and publishes a PCM24 `.mov`. Plugin code is
//! never loaded into this process — every failure is a typed error.
use std::path::PathBuf;
use std::time::Duration;

use kronello_audio::{AudioBuffer, Bus, ClippingPolicy, MAX_AUDIO_FRAMES};
use kronello_jobs::{JobRecord, JobStore, Submission};
use kronello_media::MediaRuntime;
use kronello_model::{Asset, AssetId, AssetKind, DocumentObject};
use kronello_plugin::{
    HelperCommand, HelperIo, HelperRequest, PluginError, PluginReport, PluginSpec, run_helper,
    verify_spec_pin,
};
use kronello_time::Rational;
use serde::{Deserialize, Serialize};

use crate::ServiceError;

/// Hard bounds for one helper exchange. The default covers the largest
/// supported decode (MAX_AUDIO_FRAMES at 48 kHz) with headroom.
const MIN_DEADLINE_MS: u64 = 1_000;
const MAX_DEADLINE_MS: u64 = 600_000;
/// The parent-side whole-exchange deadline stays above the helper watchdog so
/// the helper's typed error lands before a hard kill.
const HELPER_TIMEOUT_MARGIN_MS: u64 = 30_000;

fn plugin_error(error: PluginError) -> ServiceError {
    ServiceError::new(error.code(), error.to_string())
}

fn deadline(requested: Option<u64>) -> Result<u64, ServiceError> {
    let ms = requested.unwrap_or(kronello_plugin::DEFAULT_HELPER_DEADLINE_MS);
    if !(MIN_DEADLINE_MS..=MAX_DEADLINE_MS).contains(&ms) {
        return Err(ServiceError::invalid(format!(
            "plugin deadline_ms must be within {MIN_DEADLINE_MS}..={MAX_DEADLINE_MS}"
        )));
    }
    Ok(ms)
}

fn helper_timeout(deadline_ms: u64) -> Duration {
    Duration::from_millis(deadline_ms.saturating_add(HELPER_TIMEOUT_MARGIN_MS))
}

fn resolve_helper(explicit: Option<&HelperCommand>) -> Result<HelperCommand, ServiceError> {
    match explicit {
        Some(command) => Ok(command.clone()),
        None => HelperCommand::resolve().map_err(plugin_error),
    }
}

/// `audio.plugin_probe`: describe the pinned plugin through the detached
/// helper. Plugin code runs only inside the helper; this process receives
/// one bounded JSON response.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginProbeRequest {
    /// Hash-pinned plugin binding; `path` must be a local filesystem locator.
    pub plugin: PluginSpec,
    /// Helper watchdog budget in milliseconds (1000..=600000, default 120000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginProbeResult {
    pub report: PluginReport,
}

/// `audio.plugin_process`: submit one fixed-input plugin processing job.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginProcessRequest {
    pub project: PathBuf,
    /// Optional fence against changes since the caller inspected the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    /// Locked source asset; must be audio or video carrying an audio stream.
    pub asset: AssetId,
    /// Source stream; defaults to the asset's first non-video stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_index: Option<u32>,
    /// Hash-pinned plugin binding carried into the fixed job input.
    pub plugin: PluginSpec,
    /// Publication destination; the worker writes 48 kHz stereo PCM24 `.mov`.
    pub destination: PathBuf,
    /// Helper watchdog budget for the single processing exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}

/// Fixed worker input for `audio.plugin_process`, serialized inside the job
/// envelope. Identity mirrors `proxy.generate`: `document_hash` is the record
/// snapshot hash and the serialized input is the record output profile.
/// Plugin bundle bytes never appear here — only the pinned spec.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginJobInput {
    /// Canonical project path for asset resolution.
    pub project: PathBuf,
    /// SHA-256 of the serialized document at submit (record snapshot hash).
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub document_hash: String,
    /// Locked source asset object, content-hash verified on decode.
    pub asset: Asset,
    pub stream_index: u32,
    /// Hash-pinned plugin identity; verified at submit, at worker start, and
    /// inside the helper immediately before any `dlopen`.
    pub plugin: PluginSpec,
    /// Publication destination; must equal the job record destination.
    pub destination: PathBuf,
    /// Helper watchdog budget for the single processing exchange.
    pub deadline_ms: u64,
}

/// Identity + pin checks shared by the worker entry and `job.resume`: the
/// fixed payload is the whole contract and must equal the recorded
/// submission exactly. The helper re-verifies the same pin before loading.
pub(crate) fn validate_plugin_job(
    record: &JobRecord,
    input: &PluginJobInput,
) -> Result<(), ServiceError> {
    if input.document_hash != record.snapshot_hash
        || input.destination != record.destination
        || serde_json::to_value(input)? != record.output_profile
    {
        return Err(ServiceError::new(
            "JOB_INPUT_HASH_MISMATCH",
            "fixed plugin input identity differs",
        ));
    }
    input
        .asset
        .validate()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    input.plugin.validate().map_err(plugin_error)?;
    verify_spec_pin(&input.plugin).map_err(plugin_error)?;
    // The source asset is an external reference; content verify is live.
    kronello_media::resolve_asset(&input.asset, &input.project)?;
    Ok(())
}

impl crate::Service<'_> {
    /// `audio.plugin_probe`: verify the pin, then run the bounded describe
    /// exchange through the detached helper. No bundle bytes are read by this
    /// process beyond the documented manifest hash walk.
    pub fn plugin_probe(
        &self,
        request: PluginProbeRequest,
    ) -> Result<PluginProbeResult, ServiceError> {
        if let Some(path) = &request.plugin.path {
            crate::local_locator(path)?;
        }
        request.plugin.validate().map_err(plugin_error)?;
        // Hash mismatches fail before any helper process is spawned.
        verify_spec_pin(&request.plugin).map_err(plugin_error)?;
        let deadline_ms = deadline(request.deadline_ms)?;
        let command = resolve_helper(self.plugin_helper.as_ref())?;
        let helper = HelperRequest::describe(request.plugin, deadline_ms);
        let response =
            run_helper(&command, &helper, helper_timeout(deadline_ms)).map_err(plugin_error)?;
        let report = response.report.ok_or_else(|| {
            ServiceError::new("PLUGIN_PROTOCOL", "probe response carried no report")
        })?;
        Ok(PluginProbeResult { report })
    }

    /// `audio.plugin_process`: freeze the request into a fixed-input job and
    /// spawn the worker. Nothing about the document changes at submit; the
    /// worker publishes the processed `.mov` through the staging contract.
    pub fn plugin_process(&self, request: PluginProcessRequest) -> Result<JobRecord, ServiceError> {
        crate::local_locator(&request.project)?;
        crate::local_locator(&request.destination)?;
        if let Some(path) = &request.plugin.path {
            crate::local_locator(path)?;
        }
        request.plugin.validate().map_err(plugin_error)?;
        verify_spec_pin(&request.plugin).map_err(plugin_error)?;
        let deadline_ms = deadline(request.deadline_ms)?;
        if request.destination.extension().is_none_or(|e| e != "mov") {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "audio.plugin_process requires a .mov destination (PCM24 audio)",
            ));
        }
        let project = request.project.canonicalize().map_err(|e| {
            ServiceError::new(
                if e.kind() == std::io::ErrorKind::NotFound {
                    "PROJECT_NOT_FOUND"
                } else {
                    "IO_ERROR"
                },
                e.to_string(),
            )
        })?;
        let destination = crate::jobs::absolute(&request.destination)?;
        if destination.exists() {
            return Err(ServiceError::new(
                "OUTPUT_EXISTS",
                "destination already exists",
            ));
        }
        let stored = crate::session::read_snapshot(&project)?;
        crate::jobs::check_expected_revision(
            request.expected_revision.as_deref(),
            stored.revision,
        )?;
        let document = &stored.document;
        let asset = document
            .assets
            .iter()
            .find_map(|a| match a {
                DocumentObject::Known(a) if a.id == request.asset => Some(a),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("ASSET_MISSING", request.asset.to_string()))?;
        if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "plugin processing requires an audio or video asset",
            ));
        }
        let stream_index = match request.stream_index {
            Some(index) => index,
            None => asset
                .streams
                .iter()
                .find(|s| s.width.is_none() && s.height.is_none())
                .map(|s| s.index)
                .ok_or_else(|| {
                    ServiceError::new("INVALID_MEDIA_INPUT", "asset has no audio stream")
                })?,
        };
        let stream = asset
            .streams
            .iter()
            .find(|s| s.index == stream_index)
            .ok_or_else(|| {
                ServiceError::new("INVALID_MEDIA_INPUT", "plugin stream index missing")
            })?;
        if stream.width.is_some() || stream.height.is_some() {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "plugin source stream must be audio",
            ));
        }
        let document_hash = crate::proxy::document_hash(document)?;
        // Progress denominator: estimated 48 kHz frames of the locked stream.
        let total_frames = stream
            .duration
            .and_then(|d| d.checked_mul(Rational::from_integer(48_000)).ok())
            .map(|frames| frames.floor().max(1) as u64)
            .unwrap_or(1);
        let input = PluginJobInput {
            project,
            document_hash: document_hash.clone(),
            asset: asset.clone(),
            stream_index,
            plugin: request.plugin,
            destination: destination.clone(),
            deadline_ms,
        };
        let fixed = crate::jobs::FixedInput::plugin(input.clone());
        let submission = Submission {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            project_id: document.id.to_string(),
            revision: stored.revision.to_string(),
            snapshot_hash: document_hash,
            output_profile: serde_json::to_value(&input)?,
            destination,
            total_frames,
        };
        let store = self.jobs()?;
        let record = store.submit(&serde_json::to_vec(&fixed)?, submission)?;
        let executable = match &self.worker_executable {
            Some(path) => path.clone(),
            None => std::env::current_exe()?,
        };
        if let Err(error) = store.spawn_attempt(&record.id, record.attempt, &executable) {
            store.finish_error_attempt(&record.id, record.attempt, &error)?;
            return Err(error.into());
        }
        Ok(record)
    }
}

/// Worker half of `audio.plugin_process`: fixed-input validation, decode →
/// detached helper → encode, then receipted atomic publication. Called for
/// every attempt, including resumed jobs; every step is idempotent or
/// content-verified.
pub(crate) fn execute_plugin_job(
    store: &JobStore,
    record: &JobRecord,
    input: &PluginJobInput,
    helper: Option<&HelperCommand>,
) -> Result<(), ServiceError> {
    if record.engine_version != env!("CARGO_PKG_VERSION") {
        return Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "job engine version differs",
        ));
    }
    validate_plugin_job(record, input)?;
    let stream = input
        .asset
        .streams
        .iter()
        .find(|s| s.index == input.stream_index)
        .ok_or_else(|| {
            ServiceError::new("INVALID_MEDIA_INPUT", "plugin source stream index missing")
        })?;
    if stream.width.is_some() || stream.height.is_some() {
        return Err(ServiceError::new(
            "INVALID_MEDIA_INPUT",
            "plugin source stream must be audio",
        ));
    }
    if let Some(parent) = record.destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = store.staging(record)?;
    let stage_out = staging.output();
    let stage_dir = stage_out
        .parent()
        .ok_or_else(|| ServiceError::new("INVALID_REQUEST", "staging output has no parent"))?
        .to_path_buf();
    let runtime = MediaRuntime::load()?;
    let decoded = runtime.decode_asset_audio_bounded(
        &input.asset,
        &input.project,
        input.stream_index,
        MAX_AUDIO_FRAMES,
    )?;
    // Interleaved little-endian f32 file transport per the helper contract.
    let samples = decoded.buffer.frames();
    let mut bytes = Vec::with_capacity(samples.len() * 8);
    for frame in samples {
        for sample in frame {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    let io = HelperIo {
        input: stage_dir.join("plugin-input.f32le"),
        output: stage_dir.join("plugin-output.f32le"),
        frames: samples.len() as u64,
        sample_rate: 48_000,
        channels: 2,
    };
    std::fs::write(&io.input, &bytes)?;
    let request = HelperRequest::process(input.plugin.clone(), io.clone(), input.deadline_ms);
    let command = resolve_helper(helper)?;
    let response =
        run_helper(&command, &request, helper_timeout(input.deadline_ms)).map_err(plugin_error)?;
    // The helper is a single exchange; the lease/cancel fence lands here and
    // again inside publish_attempt's transaction.
    store.checkpoint(&record.id, (samples.len() as u64).min(record.total_frames))?;
    if response.frames != Some(io.frames) {
        return Err(ServiceError::new(
            "PLUGIN_PROTOCOL",
            "helper frame count differs from the io contract",
        ));
    }
    let output = std::fs::read(&io.output)?;
    if output.len() != bytes.len() {
        return Err(ServiceError::new(
            "PLUGIN_PROTOCOL",
            "helper output length differs from the io contract",
        ));
    }
    let mut processed = Vec::with_capacity(samples.len());
    for pair in output.chunks_exact(8) {
        processed.push([
            f32::from_le_bytes(pair[..4].try_into().expect("4 bytes")),
            f32::from_le_bytes(pair[4..].try_into().expect("4 bytes")),
        ]);
    }
    let buffer =
        AudioBuffer::new(processed).map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let encode = runtime.encode_audio(&Bus::new(0, buffer), ClippingPolicy::Reject, &stage_out)?;
    let report = response.report.ok_or_else(|| {
        ServiceError::new("PLUGIN_PROTOCOL", "process response carried no report")
    })?;
    let outcome = serde_json::json!({
        "destination": record.destination,
        "validated": true,
        "report": {
            "asset": input.asset.id,
            "stream_index": input.stream_index,
            "plugin": {
                "format": input.plugin.format,
                "component": input.plugin.component,
                "name": report.name,
                "version": report.version,
                "latency_samples": report.latency_samples,
            },
            "frames": samples.len() as u64,
            "sample_rate": 48_000,
            "channels": 2,
            "content_hash": kronello_media::content_hash(&stage_out)?,
            "encode": encode,
        }
    });
    store.prepare_publication(record, &stage_out, outcome.clone())?;
    store.publish_attempt(&record.id, record.attempt, outcome, || {
        kronello_jobs::publish_path(&stage_out, &record.destination)
    })?;
    Ok(())
}
