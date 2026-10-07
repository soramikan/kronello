//! Deterministic motion tracking command (ADR-0118). Decoding is sequential
//! `next_rgba`; analysis is `kronello_tracking::analyze`; persistence follows
//! the `audio.analyze` revision/idempotency contract.
use crate::{ImportRequest, ResultData, ServiceError};
use kronello_media::MediaRuntime;
use kronello_model::{
    AssetId, DocumentObject, TRACKING_MAX_FRAMES, TRACKING_MAX_SEEDS, TrackingMode, TrackingSeed,
    TrackingSource,
};
use kronello_time::{Rational, TimeRange};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackAnalyzeRequest {
    pub project: PathBuf,
    /// Decimal revision, independent of JSON number precision.
    pub base_revision: String,
    /// New `TrackingDataAsset` id; also the `DataAssetCell` lookup key.
    pub id: AssetId,
    pub asset: AssetId,
    pub stream_index: u32,
    pub mode: TrackingMode,
    /// `points`: 1..=8 seeds; `plane`: exactly the four corners.
    pub seeds: Vec<TrackingSeed>,
    /// Source-time interval `[start, end)` to sample.
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

fn tracking_error(e: kronello_tracking::TrackingError) -> ServiceError {
    ServiceError::new(e.code(), e.to_string())
}

impl crate::Service<'_> {
    /// `track.analyze`: decode the locked source stream inside `range`, run
    /// deterministic tracking and commit one `TrackingDataAsset` object.
    pub fn analyze_tracking(
        &self,
        request: TrackAnalyzeRequest,
    ) -> Result<ResultData, ServiceError> {
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
        let payload = serde_json::json!({"operation":"track.analyze", "request":receipt_request});
        if let Some(key) = &request.idempotency_key
            && let Some(record) =
                kronello_store::ProjectStore::read_idempotency_record(&request.project, key)?
        {
            if record.service_payload.as_ref() != Some(&payload) {
                return Err(kronello_store::StoreError::IdempotencyKeyReused.into());
            }
            let result = record.service_result.ok_or_else(|| {
                ServiceError::new("STORAGE_ERROR", "tracking receipt has no result")
            })?;
            return Ok(ResultData::Project(serde_json::from_value(result)?));
        }
        let expected = match request.mode {
            TrackingMode::Points => 1..=TRACKING_MAX_SEEDS,
            TrackingMode::Plane => 4..=4,
        };
        if !expected.contains(&request.seeds.len()) || request.range.is_empty() {
            return Err(ServiceError::invalid(
                "invalid tracking mode seed count or empty range",
            ));
        }
        for seed in &request.seeds {
            seed.validate()
                .map_err(|e| ServiceError::invalid(e.to_string()))?;
        }
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
        let taken = stored
            .document
            .tracking_data_assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == request.id))
            || stored
                .document
                .expression_data_assets
                .iter()
                .any(|a| matches!(a, DocumentObject::Known(a) if a.id == request.id));
        if taken {
            return Err(ServiceError::invalid(
                "tracking data asset id already exists",
            ));
        }
        let asset = stored
            .document
            .assets
            .iter()
            .find_map(|a| match a {
                DocumentObject::Known(a) if a.id == request.asset => Some(a),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("ASSET_MISSING", request.asset.to_string()))?;
        if asset.kind != kronello_model::AssetKind::Video {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "tracking requires a video asset",
            ));
        }
        let locked = asset
            .streams
            .iter()
            .find(|s| s.index == request.stream_index)
            .ok_or_else(|| {
                ServiceError::new("INVALID_MEDIA_INPUT", "tracking stream index missing")
            })?;
        let (Some(width), Some(height)) = (locked.width, locked.height) else {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "tracking stream lacks locked dimensions",
            ));
        };
        let runtime = MediaRuntime::load()?;
        let path = kronello_media::resolve_asset(asset, &request.project)?;
        let mut decoder = runtime.open_video_stream(&path, request.stream_index)?;
        let mut decoded: Vec<(Rational, Vec<u8>)> = Vec::new();
        let mut end_of_last = request.range.start();
        while let Some(frame) = decoder.next_rgba()? {
            if frame.pts >= request.range.end() {
                break;
            }
            if frame.width != width || frame.height != height {
                return Err(ServiceError::new(
                    "ASSET_HASH_MISMATCH",
                    "decoded tracking frame differs from locked dimensions",
                ));
            }
            if request.range.contains(frame.pts) {
                end_of_last = frame
                    .pts
                    .checked_add(frame.duration)
                    .map_err(|e| ServiceError::invalid(e.to_string()))?;
                decoded.push((frame.pts, frame.rgba));
                if decoded.len() > TRACKING_MAX_FRAMES as usize {
                    return Err(ServiceError::new(
                        "TRACKING_BUDGET_EXCEEDED",
                        "range exceeds the synchronous frame cap",
                    ));
                }
            }
        }
        if decoded.is_empty() {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "tracking range contains no frames",
            ));
        }
        let frames: Vec<kronello_tracking::TrackingFrame<'_>> = decoded
            .iter()
            .map(|(time, rgba)| kronello_tracking::TrackingFrame {
                time: *time,
                width,
                height,
                rgba,
            })
            .collect();
        let covered = end_of_last
            .checked_sub(request.range.start())
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let sample_rate = Rational::new(frames.len() as i64, 1)
            .and_then(|n| n.checked_div(covered))
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let data = kronello_tracking::analyze(
            request.id,
            TrackingSource {
                asset: asset.id,
                stream_index: request.stream_index,
                content_hash: asset.content_hash.clone(),
            },
            request.mode,
            &request.seeds,
            request.range,
            sample_rate,
            width,
            height,
            &frames,
        )
        .map_err(tracking_error)?;
        let mut document = stored.document;
        document
            .tracking_data_assets
            .push(DocumentObject::Known(data));
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
