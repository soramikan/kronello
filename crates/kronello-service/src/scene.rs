//! AI-002 (ADR-0125): `scene.detect` fixed-input jobs and the shared
//! `scene.apply` edit. Detection is `kronello_scene` over sequentially
//! decoded RGBA8 frames; results persist as a versioned
//! `SceneBoundaryAsset` through the store's normal import path, and apply
//! reuses `MarkerSet`/`ClipSplit` through `edit::build`/`apply_template`, so
//! undo, revision, idempotency and plan-hash contracts are unchanged.
use std::path::{Path, PathBuf};

use kronello_jobs::{JobRecord, JobStore, Submission};
use kronello_media::MediaRuntime;
use kronello_model::{
    Asset, AssetId, AssetKind, ClipId, DocumentObject, FiniteF64, Marker, MarkerColor, MarkerId,
    SCENE_BOUNDARY_VERSION, SCENE_DETECT_MAX_FRAMES, SceneBoundary, SceneBoundaryAsset,
    SceneDetectionParams, SceneSource, SequenceId, SourceRef, TrackId,
};
use kronello_store::Event;
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{EditApplyRequest, EditCommand, ServiceError, TimelineCommand};

/// Fixed worker input for `scene.detect`. Serialized inside the job
/// envelope; the worker re-validates every identity field against the
/// record exactly like `proxy.generate` (ADR-0119/0125).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneJobInput {
    /// Canonical project path; the worker registers the result through the
    /// shared store contract like any other edit.
    pub project: PathBuf,
    /// SHA-256 of the serialized document at submit (the record snapshot hash).
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub document_hash: String,
    /// Locked source asset object, content-hash verified on decode.
    pub asset: Asset,
    pub stream_index: u32,
    /// Analyzed source-time interval `[start, end)`.
    pub range: TimeRange,
    /// Versioned detector parameters baked into the result hash.
    pub params: SceneDetectionParams,
    /// Pre-allocated `SceneBoundaryAsset` id.
    pub scene_asset_id: AssetId,
    /// Publication destination for the machine-readable boundary receipt;
    /// must equal the job record destination.
    pub destination: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneDetectRequest {
    pub project: PathBuf,
    /// Optional fence against changes since the caller inspected the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    /// Explicit boundary-asset id; absent derives one deterministically from
    /// the locked input, parameters and submission revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<AssetId>,
    pub asset: AssetId,
    pub stream_index: u32,
    /// Source-time interval `[start, end)`; absent covers the locked stream
    /// duration and fails when the stream has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TimeRange>,
    /// Detector parameters; absent uses the versioned defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<SceneDetectionParams>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneApplyMode {
    /// One sequence-level marker per mapped boundary.
    Markers,
    /// One `ClipSplit` per mapped boundary through the shared NLE rules.
    Split,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneApplyRequest {
    pub project: PathBuf,
    /// Decimal revision, independent of JSON number precision.
    pub base_revision: String,
    pub idempotency_key: String,
    pub session_id: Uuid,
    pub scene_asset: AssetId,
    pub sequence: SequenceId,
    pub mode: SceneApplyMode,
    /// Restrict boundary mapping to clips on this track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackId>,
    /// Restrict boundary mapping to this clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<ClipId>,
    /// Boundaries below this confidence are ignored; absent keeps all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f64>,
}

fn document_hash(document: &kronello_model::Project) -> Result<String, ServiceError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(document)?)?)
    ))
}

/// Managed `<stem>.scene/` folder beside the project; the receipt filename is
/// the boundary-asset id so every publication name is collision-free.
fn scene_destination(project: &Path, scene_asset_id: AssetId) -> Result<PathBuf, ServiceError> {
    let stem = project
        .file_stem()
        .ok_or_else(|| ServiceError::invalid("project path has no filename"))?
        .to_string_lossy();
    let directory = project
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}.scene"));
    Ok(directory.join(format!("{scene_asset_id}.json")))
}

fn derived_uuid(domain: &[u8], write: impl FnOnce(&mut Sha256)) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(domain);
    write(&mut digest);
    let hash = digest.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // UUID v8, RFC variant: application-defined deterministic identity.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn push_time(digest: &mut Sha256, time: Time) {
    digest.update(time.numerator().to_be_bytes());
    digest.update(time.denominator().to_be_bytes());
}

/// Deterministic result identity from the fixed semantic input, like the
/// proxy asset derivation: identical submissions on the same revision mint
/// the same asset id and destination.
fn derive_scene_asset_id(
    asset_id: AssetId,
    stream_index: u32,
    range: TimeRange,
    params: &SceneDetectionParams,
    revision: u64,
) -> AssetId {
    AssetId::from_uuid(derived_uuid(b"kronello.scene-asset-v1", |digest| {
        digest.update(asset_id.as_uuid().as_bytes());
        digest.update(stream_index.to_be_bytes());
        push_time(digest, range.start());
        push_time(digest, range.end());
        digest.update(serde_json::to_vec(params).unwrap_or_default());
        digest.update(revision.to_be_bytes());
    }))
}

/// Deterministic marker identity per (result asset, sequence time).
fn scene_marker_id(scene_asset: AssetId, time: Time) -> MarkerId {
    MarkerId::from_uuid(derived_uuid(b"kronello.scene-marker-v1", |digest| {
        digest.update(scene_asset.as_uuid().as_bytes());
        push_time(digest, time);
    }))
}

/// Deterministic right-clip identity per (result asset, source clip, cut).
fn scene_split_right_id(scene_asset: AssetId, clip: ClipId, time: Time) -> ClipId {
    ClipId::from_uuid(derived_uuid(b"kronello.scene-split-right-v1", |digest| {
        digest.update(scene_asset.as_uuid().as_bytes());
        digest.update(clip.as_uuid().as_bytes());
        push_time(digest, time);
    }))
}

/// Source time `t` mapped to sequence time on `clip`; unmapped times
/// (outside the map domain or arithmetic overflow) return `None`.
fn boundary_sequence_time(clip: &kronello_model::Clip, t: Time) -> Option<Time> {
    let local = if clip.reverse_sampling.is_some() {
        clip.source_in.checked_sub(t).ok()?
    } else {
        t.checked_sub(clip.source_in).ok()?
    };
    let parent = clip.time_map.inverse_canonical(local).ok()?;
    clip.timeline_range.start().checked_add(parent).ok()
}

/// Whether `clip` places the analyzed source stream.
fn sources_scene_source(clip: &kronello_model::Clip, scene: &SceneBoundaryAsset) -> bool {
    matches!(
        clip.source_ref,
        SourceRef::Asset { asset, stream_index }
            if asset == scene.source.asset && stream_index == scene.source.stream_index
    )
}

fn analysis_id_taken(document: &kronello_model::Project, id: AssetId) -> bool {
    document
        .scene_boundary_assets
        .iter()
        .any(|a| matches!(a, DocumentObject::Known(a) if a.id == id))
        || document
            .tracking_data_assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == id))
        || document
            .expression_data_assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == id))
        || document
            .audio_analyses
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == id))
        || document
            .assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == id))
}

impl crate::Service<'_> {
    /// `scene.detect`: validate the locked source and submit one fixed-input
    /// detection job. Nothing about the document changes at submit; the
    /// worker registers the `SceneBoundaryAsset` after validated staging.
    pub fn detect_scene(&self, request: SceneDetectRequest) -> Result<JobRecord, ServiceError> {
        crate::local_locator(&request.project)?;
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
        let stored = crate::session::read_snapshot(&project)?;
        crate::jobs::check_expected_revision(
            request.expected_revision.as_deref(),
            stored.revision,
        )?;
        stored
            .document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        let asset = stored
            .document
            .assets
            .iter()
            .find_map(|a| match a {
                DocumentObject::Known(a) if a.id == request.asset => Some(a),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("ASSET_MISSING", request.asset.to_string()))?;
        if asset.kind != AssetKind::Video {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "scene detection requires a video asset",
            ));
        }
        let stream = asset
            .streams
            .iter()
            .find(|s| s.index == request.stream_index)
            .ok_or_else(|| {
                ServiceError::new("INVALID_MEDIA_INPUT", "scene stream index missing")
            })?;
        if stream.width.is_none() || stream.height.is_none() {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "scene stream lacks locked dimensions",
            ));
        }
        let range = match request.range {
            Some(range) => range,
            None => {
                let (Some(start), Some(duration)) = (stream.start_time, stream.duration) else {
                    return Err(ServiceError::new(
                        "INVALID_MEDIA_INPUT",
                        "scene detection requires an explicit range without a locked stream duration",
                    ));
                };
                let end = start
                    .checked_add(duration)
                    .map_err(|e| ServiceError::invalid(e.to_string()))?;
                TimeRange::new(start, end).map_err(|e| ServiceError::invalid(e.to_string()))?
            }
        };
        if range.is_empty() {
            return Err(ServiceError::invalid("scene detect range must be nonempty"));
        }
        let params = request.params.unwrap_or_default();
        params
            .validate()
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        // Early budget check: the worker re-verifies against the decoded
        // count, but submitters should not queue a job that cannot succeed.
        let estimated = range
            .end()
            .checked_sub(range.start())
            .ok()
            .and_then(|d| d.checked_div(stream.time_base).ok())
            .map(|ticks| ticks.floor())
            .unwrap_or(0);
        if estimated > SCENE_DETECT_MAX_FRAMES as i64 {
            return Err(ServiceError::new(
                "SCENE_BUDGET_EXCEEDED",
                "range exceeds the frame cap",
            ));
        }
        let scene_asset_id = request.id.unwrap_or_else(|| {
            derive_scene_asset_id(
                asset.id,
                request.stream_index,
                range,
                &params,
                stored.revision,
            )
        });
        if analysis_id_taken(&stored.document, scene_asset_id) {
            return Err(ServiceError::invalid(
                "scene boundary asset id already exists",
            ));
        }
        let destination = scene_destination(&project, scene_asset_id)?;
        if destination.exists() {
            return Err(ServiceError::new(
                "OUTPUT_EXISTS",
                "scene detection destination exists",
            ));
        }
        let input = SceneJobInput {
            project: project.clone(),
            document_hash: document_hash(&stored.document)?,
            asset: asset.clone(),
            stream_index: request.stream_index,
            range,
            params,
            scene_asset_id,
            destination: destination.clone(),
        };
        let total_frames = estimated.max(1) as u64;
        let fixed = crate::jobs::FixedInput::scene(input.clone());
        let store = self.jobs()?;
        let submission = Submission {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            project_id: stored.document.id.to_string(),
            revision: stored.revision.to_string(),
            snapshot_hash: input.document_hash.clone(),
            output_profile: serde_json::to_value(&input)?,
            destination,
            total_frames,
        };
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

    /// `scene.apply`: map `SceneBoundaryAsset` boundaries onto the locked
    /// source's clip placements and apply them as sequence markers or clip
    /// splits through the shared edit pipeline.
    pub fn apply_scene(&self, request: SceneApplyRequest) -> Result<Event, ServiceError> {
        crate::local_locator(&request.project)?;
        if request.idempotency_key.is_empty() || request.idempotency_key.len() > 256 {
            return Err(ServiceError::invalid(
                "idempotency_key must contain 1..256 UTF-8 bytes",
            ));
        }
        if let Some(min) = request.min_confidence
            && (!min.is_finite() || !(0.0..=1.0).contains(&min))
        {
            return Err(ServiceError::invalid(
                "min_confidence must be finite in [0, 1]",
            ));
        }
        let base = crate::parse_revision(&request.base_revision)?;
        let mut receipt_request = request.clone();
        receipt_request.project = request.project.canonicalize()?;
        receipt_request.base_revision = base.to_string();
        // The session is the editing identity, not part of the semantic
        // request: identical retries from any session must hit the receipt.
        receipt_request.session_id = Uuid::nil();
        let payload = serde_json::json!({
            "operation": "scene.apply",
            "request": receipt_request,
        });
        let store = crate::open_existing(&request.project)?;
        // Identical retries replay the committed event before the revision
        // fence, exactly like `edit::apply_template`.
        if let Some(event) = crate::edit::retry(&store, &request.idempotency_key, &payload)? {
            store.close()?;
            return Ok(event);
        }
        let snapshot = store.snapshot()?;
        store.close()?;
        if base != snapshot.revision {
            return Err(kronello_store::StoreError::RevisionConflict {
                base,
                current: snapshot.revision,
            }
            .into());
        }
        let document = snapshot.document;
        document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        if !document
            .scene_boundary_assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == request.scene_asset))
        {
            return Err(ServiceError::new(
                "ASSET_MISSING",
                request.scene_asset.to_string(),
            ));
        }
        let scene = document
            .scene_boundary_asset(request.scene_asset)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let sequence = document
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) if s.id == request.sequence => Some(s),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))?;
        let content_end = sequence
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(|c| c.timeline_range.end())
            .max()
            .unwrap_or(Time::ZERO);
        let min_confidence = request.min_confidence.unwrap_or(0.0);
        let boundaries: Vec<_> = scene
            .boundaries
            .iter()
            .filter(|b| b.confidence.get() >= min_confidence)
            .collect();
        let mut commands: Vec<EditCommand> = Vec::new();
        match request.mode {
            SceneApplyMode::Markers => {
                let mut mapped: std::collections::BTreeMap<Time, FiniteF64> =
                    std::collections::BTreeMap::new();
                for track in &sequence.tracks {
                    if request.track.is_some_and(|t| t != track.id) {
                        continue;
                    }
                    for clip in &track.clips {
                        if request.clip.is_some_and(|c| c != clip.id)
                            || !sources_scene_source(clip, &scene)
                        {
                            continue;
                        }
                        for boundary in &boundaries {
                            let Some(time) = boundary_sequence_time(clip, boundary.time) else {
                                continue;
                            };
                            if time >= clip.timeline_range.start()
                                && time <= clip.timeline_range.end()
                                && time <= content_end
                            {
                                mapped.entry(time).or_insert(boundary.confidence);
                            }
                        }
                    }
                }
                for (time, confidence) in mapped {
                    commands.push(EditCommand::Timeline(Box::new(
                        TimelineCommand::MarkerSet {
                            sequence: request.sequence,
                            clip: None,
                            marker: Marker {
                                id: scene_marker_id(scene.id, time),
                                time,
                                color: MarkerColor::Red,
                                comment: Some(format!(
                                    "scene boundary {:.0}%",
                                    confidence.get() * 100.0
                                )),
                            },
                        },
                    )));
                }
            }
            SceneApplyMode::Split => {
                for track in &sequence.tracks {
                    if request.track.is_some_and(|t| t != track.id) {
                        continue;
                    }
                    for clip in &track.clips {
                        if request.clip.is_some_and(|c| c != clip.id)
                            || !sources_scene_source(clip, &scene)
                        {
                            continue;
                        }
                        let mut times: Vec<Time> = boundaries
                            .iter()
                            .filter_map(|b| boundary_sequence_time(clip, b.time))
                            .filter(|t| {
                                *t > clip.timeline_range.start() && *t < clip.timeline_range.end()
                            })
                            .collect();
                        times.sort();
                        times.dedup();
                        // Sequential splits: each cut lands inside the right
                        // piece of the previous one, and every right-clip id
                        // is derived from the fixed triple.
                        let mut current = clip.id;
                        for time in times {
                            let right = scene_split_right_id(scene.id, clip.id, time);
                            commands.push(EditCommand::Timeline(Box::new(
                                TimelineCommand::ClipSplit {
                                    sequence: request.sequence,
                                    clip: current,
                                    time,
                                    right_clip: right,
                                },
                            )));
                            current = right;
                        }
                    }
                }
            }
        }
        if commands.is_empty() {
            return Err(ServiceError::invalid(
                "no scene boundary maps inside the apply scope",
            ));
        }
        let plan = crate::edit::build(document, base, commands.clone())?;
        crate::edit::apply_template(
            EditApplyRequest {
                project: request.project.clone(),
                base_revision: request.base_revision.clone(),
                plan_hash: plan.plan_hash,
                idempotency_key: request.idempotency_key.clone(),
                session_id: request.session_id,
                commands,
            },
            payload,
        )
    }
}

/// Register the boundary asset through the store's normal import path.
/// Retried across concurrent edits while the source asset stays unchanged,
/// mirroring `register_proxy_link` (ADR-0119/0125).
fn register_scene_asset(
    input: &SceneJobInput,
    data: &SceneBoundaryAsset,
) -> Result<(), ServiceError> {
    let mut delay_ms = 0u64;
    for attempt in 0..64u32 {
        match register_scene_asset_once(input, data) {
            Ok(()) => return Ok(()),
            Err(error)
                if (error.code == "REVISION_CONFLICT" || error.code == "PROJECT_LOCKED")
                    && attempt < 63 =>
            {
                if error.code == "PROJECT_LOCKED" {
                    delay_ms = (delay_ms + 25).min(250);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(ServiceError::new(
        "PROJECT_LOCKED",
        "scene boundary registration exhausted retries",
    ))
}

fn register_scene_asset_once(
    input: &SceneJobInput,
    data: &SceneBoundaryAsset,
) -> Result<(), ServiceError> {
    let mut store = crate::open_existing(&input.project)?;
    let result = (|| {
        let snapshot = store.snapshot()?;
        let mut document = snapshot.document;
        document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        let source = document
            .assets
            .iter()
            .find_map(|a| match a {
                DocumentObject::Known(a) if a.id == input.asset.id => Some(a),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("SCENE_SOURCE_CHANGED", "source asset was removed"))?;
        if source.content_hash != input.asset.content_hash {
            return Err(ServiceError::new(
                "SCENE_SOURCE_CHANGED",
                "source asset content changed during detection",
            ));
        }
        if let Some(existing) = document.scene_boundary_assets.iter().find_map(|a| match a {
            DocumentObject::Known(a) if a.id == data.id => Some(a),
            _ => None,
        }) {
            if existing == data {
                // Worker resume: the earlier attempt already committed.
                return Ok(());
            }
            return Err(ServiceError::invalid(
                "scene boundary asset id already exists",
            ));
        }
        document
            .scene_boundary_assets
            .push(DocumentObject::Known(data.clone()));
        store.import_json(
            snapshot.revision,
            uuid::Uuid::new_v4(),
            &serde_json::to_string(&document)?,
        )?;
        Ok(())
    })();
    let close = store.close();
    result.and(close.map_err(Into::into))
}

/// Worker half of `scene.detect`: fixed-input validation, sequential decode,
/// deterministic detection, document registration, atomic publication.
/// Called for every attempt, including resumed jobs; every step is
/// idempotent or content-verified.
pub(crate) fn execute_scene_job(
    store: &JobStore,
    record: &JobRecord,
    input: &SceneJobInput,
) -> Result<(), ServiceError> {
    if record.engine_version != env!("CARGO_PKG_VERSION") {
        return Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "job engine version differs",
        ));
    }
    if input.document_hash != record.snapshot_hash
        || input.destination != record.destination
        || serde_json::to_value(input)? != record.output_profile
    {
        return Err(ServiceError::new(
            "JOB_INPUT_HASH_MISMATCH",
            "fixed scene input identity differs",
        ));
    }
    input
        .asset
        .validate()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    input
        .params
        .validate()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    if input.range.is_empty() {
        return Err(ServiceError::invalid("scene detect range must be nonempty"));
    }
    let stream = input
        .asset
        .streams
        .iter()
        .find(|s| s.index == input.stream_index)
        .ok_or_else(|| ServiceError::new("INVALID_MEDIA_INPUT", "scene stream index missing"))?;
    let (Some(width), Some(height)) = (stream.width, stream.height) else {
        return Err(ServiceError::new(
            "INVALID_MEDIA_INPUT",
            "scene stream lacks locked dimensions",
        ));
    };
    let runtime = MediaRuntime::load()?;
    let path = kronello_media::resolve_asset(&input.asset, &input.project)?;
    let mut decoder = runtime.open_video_stream(&path, input.stream_index)?;
    let mut decoded: Vec<(Time, Vec<u8>)> = Vec::new();
    while let Some(frame) = decoder.next_rgba()? {
        if frame.pts >= input.range.end() {
            break;
        }
        if frame.width != width || frame.height != height {
            return Err(ServiceError::new(
                "ASSET_HASH_MISMATCH",
                "decoded scene frame differs from locked dimensions",
            ));
        }
        if input.range.contains(frame.pts) {
            decoded.push((frame.pts, frame.rgba));
            if decoded.len() > SCENE_DETECT_MAX_FRAMES as usize {
                return Err(ServiceError::new(
                    "SCENE_BUDGET_EXCEEDED",
                    "range exceeds the frame cap",
                ));
            }
        }
    }
    if decoded.is_empty() {
        return Err(ServiceError::new(
            "INVALID_MEDIA_INPUT",
            "scene detection range contains no frames",
        ));
    }
    let frames: Vec<kronello_scene::SceneFrame<'_>> = decoded
        .iter()
        .map(|(time, rgba)| kronello_scene::SceneFrame {
            time: *time,
            width,
            height,
            rgba,
        })
        .collect();
    let detected = kronello_scene::detect_boundaries(&frames, &input.params)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let boundaries = detected
        .into_iter()
        .map(|b| {
            Ok(SceneBoundary {
                time: b.time,
                confidence: FiniteF64::new(b.confidence)
                    .map_err(|_| ServiceError::invalid("invalid boundary confidence"))?,
            })
        })
        .collect::<Result<Vec<SceneBoundary>, ServiceError>>()?;
    let mut data = SceneBoundaryAsset {
        id: input.scene_asset_id,
        version: SCENE_BOUNDARY_VERSION,
        source: SceneSource {
            asset: input.asset.id,
            stream_index: input.stream_index,
            content_hash: input.asset.content_hash.clone(),
        },
        params: input.params.clone(),
        range: input.range,
        frames_analyzed: frames.len() as u32,
        boundaries,
        content_hash: String::new(),
    };
    data.content_hash = data
        .computed_hash()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    data.validate()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    // The managed `<stem>.scene/` directory may not exist yet; job staging
    // and atomic publication both require it on the destination volume.
    if let Some(parent) = record.destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = store.staging(record)?;
    let stage_path = staging.output();
    std::fs::write(&stage_path, serde_json::to_vec_pretty(&data)?)?;
    let report = serde_json::json!({
        "scene_asset_id": input.scene_asset_id,
        "asset": input.asset.id,
        "stream_index": input.stream_index,
        "range": input.range,
        "frames_analyzed": data.frames_analyzed,
        "boundaries": data.boundaries.len(),
        "content_hash": data.content_hash,
    });
    let outcome =
        serde_json::json!({"destination": record.destination, "validated": true, "report": report});
    store.prepare_publication(record, &stage_path, outcome.clone())?;
    register_scene_asset(input, &data)?;
    store.publish_attempt(&record.id, record.attempt, outcome, || {
        kronello_jobs::publish_path(&stage_path, &record.destination)
    })?;
    Ok(())
}
