//! Shared offline analysis command. Decoding and analysis happen once before storage.
use crate::{ImportRequest, ResultData, ServiceError};
use kronello_audio::{AudioBuffer, MAX_AUDIO_FRAMES};
use kronello_model::{AssetId, AudioAnalysisConfig, AudioAnalysisSource, DocumentObject};
use kronello_render::RenderTarget;
use kronello_time::TimeRange;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioAnalyzeInput {
    Asset {
        asset: AssetId,
        stream_index: u32,
    },
    Bus {
        target: RenderTarget,
        range: TimeRange,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalyzeRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub id: AssetId,
    pub input: AudioAnalyzeInput,
    pub config: AudioAnalysisConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}
impl crate::Service<'_> {
    pub fn analyze_audio(&self, request: AudioAnalyzeRequest) -> Result<ResultData, ServiceError> {
        crate::local_locator(&request.project)?;
        if request
            .idempotency_key
            .as_ref()
            .is_some_and(|k| k.is_empty() || k.len() > 256)
        {
            return Err(ServiceError::invalid(
                "idempotency_key must contain 1..256 UTF-8 bytes",
            ));
        }
        let mut receipt_request = request.clone();
        receipt_request.idempotency_key = None;
        receipt_request.project = request.project.canonicalize()?;
        receipt_request.base_revision = crate::parse_revision(&request.base_revision)?.to_string();
        let payload = serde_json::json!({"operation":"audio.analyze", "request":receipt_request});
        if let Some(key) = &request.idempotency_key
            && let Some(record) =
                kronello_store::ProjectStore::read_idempotency_record(&request.project, key)?
        {
            if record.service_payload.as_ref() != Some(&payload) {
                return Err(kronello_store::StoreError::IdempotencyKeyReused.into());
            }
            let result = record.service_result.ok_or_else(|| {
                ServiceError::new("STORAGE_ERROR", "analysis receipt has no result")
            })?;
            return Ok(ResultData::Project(serde_json::from_value(result)?));
        }
        request
            .config
            .validate()
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let stored = kronello_store::ProjectStore::read_snapshot(&request.project)?;
        if stored.revision.to_string() != request.base_revision {
            return Err(ServiceError::new(
                "REVISION_CONFLICT",
                "analysis revision differs",
            ));
        }
        stored
            .document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        if stored
            .document
            .audio_analyses
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == request.id))
        {
            return Err(ServiceError::invalid("analysis id already exists"));
        }
        let (source, start, buffer) = match &request.input {
            AudioAnalyzeInput::Asset {
                asset,
                stream_index,
            } => {
                let asset = stored
                    .document
                    .assets
                    .iter()
                    .find_map(|a| match a {
                        DocumentObject::Known(a) if a.id == *asset => Some(a),
                        _ => None,
                    })
                    .ok_or_else(|| ServiceError::new("ASSET_MISSING", asset.to_string()))?;
                let decoded = kronello_media::MediaRuntime::load()?.decode_asset_audio_bounded(
                    asset,
                    &request.project,
                    *stream_index,
                    MAX_AUDIO_FRAMES,
                )?;
                (
                    AudioAnalysisSource::Asset {
                        asset: asset.id,
                        stream_index: *stream_index,
                        content_hash: asset.content_hash.clone(),
                    },
                    0,
                    decoded.buffer,
                )
            }
            AudioAnalyzeInput::Bus { target, range } => {
                let samples = kronello_audio::sample_range(*range).map_err(audio_error)?;
                let length = samples
                    .end
                    .checked_sub(samples.start)
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n <= MAX_AUDIO_FRAMES)
                    .ok_or_else(|| {
                        ServiceError::new("AUDIO_BUDGET_EXCEEDED", "analysis bus frames")
                    })?;
                if length.div_ceil(request.config.hop as usize) > 65536
                    || request
                        .config
                        .work_for_samples(length)
                        .is_none_or(|w| w > 100_000_000)
                {
                    return Err(ServiceError::new(
                        "AUDIO_BUDGET_EXCEEDED",
                        "analysis work budget",
                    ));
                }
                if samples.start < 0 {
                    return Err(ServiceError::invalid("negative bus start"));
                }
                let prepared = crate::PreparedAudio::prepare(
                    &request.project,
                    *target,
                    &request.base_revision,
                )?;
                let mut frames = Vec::with_capacity(length);
                let mut offset = 0;
                while offset < length {
                    let n = (length - offset).min(crate::MAX_PLAYBACK_BLOCK_FRAMES);
                    let mut block = vec![0.0; n * 2];
                    prepared.render_block(samples.start + offset as i64, &mut block)?;
                    frames.extend(block.chunks_exact(2).map(|s| [s[0], s[1]]));
                    offset += n;
                }
                let identity = serde_json::to_vec(&(&stored.document, target, range, 2u32))
                    .map_err(|e| ServiceError::invalid(e.to_string()))?;
                (
                    AudioAnalysisSource::Bus {
                        snapshot_hash: format!("{:x}", Sha256::digest(identity)),
                        target: serde_json::to_string(target)
                            .map_err(|e| ServiceError::invalid(e.to_string()))?,
                        evaluator_version: 2,
                    },
                    samples.start,
                    AudioBuffer::new(frames).map_err(audio_error)?,
                )
            }
        };
        let data =
            kronello_audio::analyze_audio(request.id, source, request.config, start, &buffer)
                .map_err(audio_error)?;
        let mut document = stored.document;
        document.audio_analyses.push(DocumentObject::Known(data));
        crate::project::import_with_payload(
            ImportRequest {
                project: request.project,
                base_revision: request.base_revision,
                document,
                idempotency_key: request.idempotency_key,
                plan_hash: None,
            },
            payload,
        )
        .map(ResultData::Project)
    }
}
fn audio_error(e: kronello_audio::AudioError) -> ServiceError {
    ServiceError::new(e.code(), e.to_string())
}
