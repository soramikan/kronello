//! NLE-007 multicam group creation (ADR-0127). `multicam.create` resolves
//! every angle's `sync_offset` before planning — deterministic timecode-start
//! arithmetic, bounded audio cross-correlation, or explicit manual values —
//! and the finished `MulticamAsset` travels through the ordinary
//! plan/apply path as one `EditCommand::MulticamSet` mutation with shared
//! undo, idempotency and revision-conflict rules.
use crate::*;
use kronello_model::*;
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MulticamSync {
    /// Align each angle's declared `start_time` origin with the reference's:
    /// `sync_offset = start_time(angle) - start_time(reference)`, with absent
    /// declarations counting as zero.
    Timecode,
    /// Deterministic normalized cross-correlation over the decoded 48 kHz
    /// audio of each angle's `audio_stream_index` (or its first audio
    /// stream). Failure to estimate yields `MULTICAM_SYNC_FAILED`.
    Audio,
    /// Explicit per-angle `sync_offset` values from `offsets`; every listed
    /// angle must be covered and extra keys are rejected.
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MulticamAngleSpec {
    /// Stable UUID identity chosen by the caller; never an index or name.
    pub id: AngleId,
    /// Document asset carrying this angle's media.
    pub asset: AssetId,
    /// Stream inside `asset` this angle samples.
    pub stream_index: u32,
    /// Audio stream used for `audio` sync estimation; absent defaults to the
    /// asset's first stream without visual dimensions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_stream_index: Option<u32>,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MulticamCreateRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    /// Stable multicam identity chosen by the caller.
    pub multicam: MulticamId,
    #[serde(default)]
    pub name: String,
    pub sync: MulticamSync,
    /// Angles in display order; every angle needs a stable caller id.
    pub angles: Vec<MulticamAngleSpec>,
    /// Reference angle for `timecode`/`audio`; defaults to the first spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<AngleId>,
    /// `manual`: explicit `sync_offset` per angle id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub offsets: BTreeMap<AngleId, Time>,
}

pub(crate) fn multicam_create(
    r: MulticamCreateRequest,
) -> Result<kronello_store::Event, ServiceError> {
    if r.angles.is_empty() || r.angles.len() > 64 {
        return Err(ServiceError::new(
            "INVALID_MULTICAM",
            "multicam requires 1..=64 angles",
        ));
    }
    let reference = r.reference.unwrap_or(r.angles[0].id);
    let reference_index = r
        .angles
        .iter()
        .position(|a| a.id == reference)
        .ok_or_else(|| ServiceError::new("INVALID_MULTICAM", "reference angle is not in angles"))?;
    // Read the stored document once to resolve assets, streams and offsets.
    // The authoritative revision check still happens inside plan/apply.
    let store = crate::open_existing(&r.project)?;
    let stored = store.snapshot()?;
    store.close()?;
    if stored.revision != crate::parse_revision(&r.base_revision)? {
        return Err(ServiceError::new(
            "REVISION_CONFLICT",
            "project revision changed before multicam resolution",
        ));
    }
    let offsets = resolve_sync_offsets(&stored.document, &r, reference_index)?;
    let multicam = MulticamAsset {
        id: r.multicam,
        name: r.name.clone(),
        angles: r
            .angles
            .iter()
            .map(|spec| MulticamAngle {
                id: spec.id,
                asset: spec.asset,
                stream_index: spec.stream_index,
                sync_offset: offsets
                    .get(&spec.id)
                    .copied()
                    .expect("offset resolved for every angle"),
                name: spec.name.clone(),
            })
            .collect(),
    };
    multicam
        .validate()
        .map_err(|e| ServiceError::new("INVALID_MULTICAM", e.to_string()))?;
    let commands = vec![EditCommand::MulticamSet { multicam }];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

/// Resolved angle wiring: the sampled stream plus the audio stream and the
/// stream start-times used by `timecode`/`audio` sync.
struct AngleWiring<'a> {
    asset: &'a Asset,
    spec: &'a MulticamAngleSpec,
    /// Start-time of the sampled stream (absent counts as zero).
    stream_start: Time,
    /// `(stream index, start_time)` of the audio stream used for sync.
    audio: Option<(u32, Time)>,
}
fn wire_angle<'a>(
    document: &'a Project,
    spec: &'a MulticamAngleSpec,
) -> Result<AngleWiring<'a>, ServiceError> {
    let asset = document
        .assets
        .iter()
        .find_map(|a| match a {
            DocumentObject::Known(a) if a.id == spec.asset => Some(a),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", "multicam angle asset missing"))?;
    let stream = asset
        .streams
        .iter()
        .find(|s| s.index == spec.stream_index)
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "multicam angle stream missing"))?;
    if stream.width.is_none() || stream.height.is_none() {
        return Err(ServiceError::new(
            "INVALID_MULTICAM",
            "multicam angle requires a visual stream",
        ));
    }
    let audio = match spec.audio_stream_index {
        Some(index) => {
            let stream = asset
                .streams
                .iter()
                .find(|s| s.index == index)
                .ok_or_else(|| {
                    ServiceError::new("SOURCE_MISSING", "multicam audio stream missing")
                })?;
            if stream.width.is_some() || stream.height.is_some() {
                return Err(ServiceError::new(
                    "INVALID_MULTICAM",
                    "multicam audio stream is not audio",
                ));
            }
            Some((index, stream.start_time.unwrap_or(Time::ZERO)))
        }
        None => asset
            .streams
            .iter()
            .find(|s| s.width.is_none() && s.height.is_none())
            .map(|s| (s.index, s.start_time.unwrap_or(Time::ZERO))),
    };
    Ok(AngleWiring {
        asset,
        spec,
        stream_start: stream.start_time.unwrap_or(Time::ZERO),
        audio,
    })
}
fn resolve_sync_offsets(
    document: &Project,
    r: &MulticamCreateRequest,
    reference_index: usize,
) -> Result<BTreeMap<AngleId, Time>, ServiceError> {
    match r.sync {
        MulticamSync::Manual => {
            if r.offsets.len() != r.angles.len()
                || r.angles
                    .iter()
                    .any(|spec| !r.offsets.contains_key(&spec.id))
            {
                return Err(ServiceError::new(
                    "INVALID_MULTICAM",
                    "manual sync requires exactly one explicit offset per angle",
                ));
            }
            // Asset/stream wiring is still verified so a malformed angle
            // fails typed rather than persisting a broken group.
            for spec in &r.angles {
                wire_angle(document, spec)?;
            }
            Ok(r.offsets.clone())
        }
        MulticamSync::Timecode => {
            let wiring: Vec<_> = r
                .angles
                .iter()
                .map(|spec| wire_angle(document, spec))
                .collect::<Result<_, _>>()?;
            let reference_start = wiring[reference_index].stream_start;
            wiring
                .iter()
                .map(|w| {
                    Ok((
                        w.spec.id,
                        w.stream_start
                            .checked_sub(reference_start)
                            .map_err(|e| ServiceError::new("INVALID_MULTICAM", e.to_string()))?,
                    ))
                })
                .collect()
        }
        MulticamSync::Audio => audio_sync_offsets(document, r, reference_index),
    }
}
fn audio_sync_offsets(
    document: &Project,
    r: &MulticamCreateRequest,
    reference_index: usize,
) -> Result<BTreeMap<AngleId, Time>, ServiceError> {
    let wiring: Vec<_> = r
        .angles
        .iter()
        .map(|spec| wire_angle(document, spec))
        .collect::<Result<_, _>>()?;
    if wiring.iter().any(|w| w.audio.is_none()) {
        return Err(ServiceError::new(
            "MULTICAM_SYNC_FAILED",
            "angle has no audio stream for sync estimation",
        ));
    }
    let runtime = kronello_media::MediaRuntime::load()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let decode = |index: usize| -> Result<(kronello_audio::AudioBuffer, Time), ServiceError> {
        let w = &wiring[index];
        let (audio_index, audio_start) = w.audio.expect("audio checked");
        runtime
            .decode_asset_audio_bounded(
                w.asset,
                &r.project,
                audio_index,
                kronello_audio::MAX_AUDIO_FRAMES,
            )
            .map(|decoded| (decoded.buffer, audio_start))
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))
    };
    let (reference_buffer, reference_start) = decode(reference_index)?;
    let mut offsets = BTreeMap::new();
    for (index, w) in wiring.iter().enumerate() {
        if index == reference_index {
            offsets.insert(w.spec.id, Time::ZERO);
            continue;
        }
        let (buffer, audio_start) = decode(index)?;
        let max_lag = kronello_audio::max_search_lag(
            reference_buffer.frames().len() as i64,
            buffer.frames().len() as i64,
        );
        let lag = kronello_audio::estimate_sync_lag(&reference_buffer, &buffer, max_lag)
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        // candidate[m] ~= reference[m + lag] ⇒ the angle's media clock leads
        // the reference by `lag`; a positive sync_offset delays it back.
        let lag_time = Time::new(lag, 48_000)
            .map_err(|e| ServiceError::new("INVALID_MULTICAM", e.to_string()))?;
        offsets.insert(
            w.spec.id,
            audio_start
                .checked_sub(reference_start)
                .and_then(|delta| delta.checked_sub(lag_time))
                .map_err(|e| ServiceError::new("INVALID_MULTICAM", e.to_string()))?,
        );
    }
    Ok(offsets)
}
