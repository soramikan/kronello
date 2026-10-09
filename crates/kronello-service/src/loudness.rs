//! AUDIO-008: BS.1770-4 loudness query and loudness normalization (ADR-0117).
//! Measurement renders through the shared audio plan used by playback and
//! export; normalization mutates only through the shared edit pipeline, so
//! the gain event is undoable and idempotent like every other timeline edit.
use std::collections::btree_map::Entry;
use std::path::{Path, PathBuf};

use kronello_audio::{
    AudioTarget, ChannelBuffer, ChannelSources, DocumentAudioPlan, MAX_AUDIO_FRAMES, sample_range,
};
use kronello_model::{
    AUDIO_GAIN_ID, AssetId, ChannelMask, ClipId, DescriptorRef, DocumentObject, Effect,
    EffectDefinition, EffectParameters, FiniteF64, Project, Property, PropertyId, PropertySource,
    SchemaKey, SequenceId, TrackState, Value,
};
use kronello_store::Event;
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{EditApplyRequest, EditCommand, ServiceError, TimelineCommand, edit};

/// AUDIO-008 measurement target. Clip and Sequence render the shared
/// DocumentAudioPlan; Asset decodes the verified stream directly.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioLoudnessInput {
    /// One clip on an audio track, measured through its authored chain.
    Clip { sequence: SequenceId, clip: ClipId },
    /// Whole sequence extent or an explicit range within it.
    Sequence {
        sequence: SequenceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<TimeRange>,
    },
    /// GUI-012 (ADR-0138): one audio track measured in place — every other
    /// track is muted in the measurement snapshot, so the result is the
    /// track's program contribution, not an isolated clip. The measured
    /// range is the target track's own clip extent.
    Track {
        sequence: SequenceId,
        track: kronello_model::TrackId,
    },
    /// A verified asset stream, optionally limited to a source range.
    Asset {
        asset: AssetId,
        stream_index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<TimeRange>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioLoudnessRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub input: AudioLoudnessInput,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioLoudnessResult {
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrated_lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub momentary_lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_term_lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_peak_dbtp: Option<f64>,
    /// Measured 48 kHz frame count.
    pub frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioNormalizeRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub sequence: SequenceId,
    pub clip: ClipId,
    /// Integrated LUFS target, within (-70, 0].
    pub target_lufs: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioNormalizeResult {
    pub event: Event,
    pub measured_lufs: f64,
    pub gain_db: f64,
    pub gain_linear: f64,
}

fn audio_error(e: kronello_audio::AudioError) -> ServiceError {
    ServiceError::new(e.code(), e.to_string())
}
fn snapshot(path: &Path, base_revision: &str) -> Result<kronello_store::Snapshot, ServiceError> {
    crate::local_locator(path)?;
    let base = crate::parse_revision(base_revision)?;
    let stored = kronello_store::ProjectStore::read_snapshot(path)?;
    if stored.revision != base {
        return Err(ServiceError::new(
            "REVISION_CONFLICT",
            "audio loudness revision differs",
        ));
    }
    Ok(stored)
}
/// Locate a clip on an audio track, returning the track index and clip.
fn audio_clip(
    document: &Project,
    sequence: SequenceId,
    clip_id: ClipId,
) -> Result<(usize, &kronello_model::Clip), ServiceError> {
    let seq = document
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", format!("sequence {sequence}")))?;
    seq.tracks
        .iter()
        .enumerate()
        .find_map(|(index, track)| {
            if track.kind != kronello_model::TrackKind::Audio {
                return None;
            }
            track
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| (index, clip))
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", format!("audio clip {clip_id}")))
}
/// Isolate one clip for measurement: other tracks muted, the target track
/// keeps only the selected clip, and its transitions are removed (they
/// referenced the dropped clips). Muted tracks keep valid structure.
fn solo_clip_document(document: &Project, sequence: SequenceId, clip: ClipId) -> Project {
    let mut document = document.clone();
    for s in document.sequences.iter_mut() {
        let DocumentObject::Known(seq) = s else {
            continue;
        };
        if seq.id != sequence {
            continue;
        }
        let mut target_clip_ids = std::collections::BTreeSet::new();
        for track in seq.tracks.iter_mut() {
            if track.kind != kronello_model::TrackKind::Audio {
                track.state = Some(TrackState {
                    visible: true,
                    muted: true,
                    locked: false,
                });
                continue;
            }
            if track.clips.iter().any(|c| c.id == clip) {
                target_clip_ids.extend(track.clips.iter().map(|c| c.id));
                track.clips.retain(|c| c.id == clip);
            } else {
                track.state = Some(TrackState {
                    visible: true,
                    muted: true,
                    locked: false,
                });
            }
        }
        // Transitions touching the target track referenced dropped clips.
        seq.transitions.retain(|transition| {
            !target_clip_ids.contains(&transition.outgoing)
                && !target_clip_ids.contains(&transition.incoming)
        });
    }
    document
}
/// GUI-012: isolate one audio track for measurement — every other track is
/// muted, the target keeps all of its clips, and transitions that touched
/// dropped clips are removed (same rule as `solo_clip_document`). Returns
/// the measurement snapshot plus the track's own clip extent.
fn solo_track_document(
    document: &Project,
    sequence: SequenceId,
    track_id: kronello_model::TrackId,
) -> Result<(Project, TimeRange), ServiceError> {
    let seq = document
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", format!("sequence {sequence}")))?;
    let target = seq
        .tracks
        .iter()
        .find(|t| t.id == track_id)
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", format!("track {track_id}")))?;
    if target.kind != kronello_model::TrackKind::Audio {
        return Err(ServiceError::new(
            "INVALID_AUDIO_INPUT",
            "track loudness requires an audio track",
        ));
    }
    let end = target
        .clips
        .iter()
        .map(|c| c.timeline_range.end())
        .max()
        .unwrap_or(Time::ZERO);
    let range = TimeRange::new(Time::ZERO, end)
        .map_err(|e| ServiceError::invalid(format!("track extent: {e}")))?;
    let target_clip_ids: std::collections::BTreeSet<_> =
        target.clips.iter().map(|c| c.id).collect();
    let mut document = document.clone();
    for s in document.sequences.iter_mut() {
        let DocumentObject::Known(seq) = s else {
            continue;
        };
        if seq.id != sequence {
            continue;
        }
        for track in seq.tracks.iter_mut() {
            if track.id != track_id {
                track.state = Some(TrackState {
                    visible: true,
                    muted: true,
                    locked: false,
                });
            }
        }
        seq.transitions.retain(|transition| {
            target_clip_ids.contains(&transition.outgoing)
                && target_clip_ids.contains(&transition.incoming)
        });
    }
    Ok((document, range))
}
/// Compile the shared plan for one document and decode its declared sources.
/// The loudness measurement target is the stereo bus (the AUDIO-008 contract);
/// multichannel sources fold down through the explicit ADR-0124 matrix.
fn render_plan(
    project_path: &Path,
    document: &Project,
    target: AudioTarget,
    range: TimeRange,
) -> Result<ChannelBuffer, ServiceError> {
    let samples = sample_range(range).map_err(audio_error)?;
    let length = samples
        .end
        .checked_sub(samples.start)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= MAX_AUDIO_FRAMES)
        .ok_or_else(|| ServiceError::new("AUDIO_BUDGET_EXCEEDED", "loudness bus frames"))?;
    if samples.start < 0 {
        return Err(ServiceError::invalid("negative loudness range"));
    }
    let _ = length;
    let plan = DocumentAudioPlan::compile_version(document, target, 2).map_err(audio_error)?;
    let mut sources = ChannelSources::new();
    let mut decoded_frames = 0_usize;
    let clips = plan.clips();
    if !clips.is_empty() {
        let runtime = kronello_media::MediaRuntime::load()?;
        for clip in clips {
            if let Entry::Vacant(entry) = sources.entry((clip.asset, clip.stream_index)) {
                let asset = document
                    .assets
                    .iter()
                    .find_map(|asset| match asset {
                        DocumentObject::Known(a) if a.id == clip.asset => Some(a),
                        _ => None,
                    })
                    .ok_or_else(|| ServiceError::new("ASSET_MISSING", clip.asset.to_string()))?;
                let decoded = runtime.decode_asset_audio_bounded(
                    asset,
                    project_path,
                    clip.stream_index,
                    MAX_AUDIO_FRAMES - decoded_frames,
                )?;
                decoded_frames += decoded.buffer.frame_count();
                entry.insert(decoded.buffer);
            }
        }
    }
    let bus = plan
        .mix_channels(&sources, range, ChannelMask::STEREO)
        .map_err(audio_error)?;
    Ok(bus.buffer().clone())
}
impl crate::Service<'_> {
    /// AUDIO-008 deterministic BS.1770-4 measurement (read-only query).
    pub fn loudness(&self, r: AudioLoudnessRequest) -> Result<AudioLoudnessResult, ServiceError> {
        let stored = snapshot(&r.project, &r.base_revision)?;
        let buffer = match &r.input {
            AudioLoudnessInput::Clip { sequence, clip } => {
                let (_, clip) = audio_clip(&stored.document, *sequence, *clip)?;
                let document = solo_clip_document(&stored.document, *sequence, clip.id);
                render_plan(
                    &r.project,
                    &document,
                    AudioTarget::Sequence(*sequence),
                    clip.timeline_range,
                )?
            }
            AudioLoudnessInput::Sequence { sequence, range } => {
                let range = match range {
                    Some(range) => *range,
                    None => {
                        let seq = stored
                            .document
                            .sequences
                            .iter()
                            .find_map(|s| match s {
                                DocumentObject::Known(s) if s.id == *sequence => Some(s),
                                _ => None,
                            })
                            .ok_or_else(|| {
                                ServiceError::new("ASSET_MISSING", format!("sequence {sequence}"))
                            })?;
                        let end = seq
                            .tracks
                            .iter()
                            .flat_map(|t| &t.clips)
                            .map(|c| c.timeline_range.end())
                            .max()
                            .unwrap_or(Time::ZERO);
                        TimeRange::new(Time::ZERO, end)
                            .map_err(|e| ServiceError::invalid(format!("sequence extent: {e}")))?
                    }
                };
                render_plan(
                    &r.project,
                    &stored.document,
                    AudioTarget::Sequence(*sequence),
                    range,
                )?
            }
            AudioLoudnessInput::Track { sequence, track } => {
                let (document, range) = solo_track_document(&stored.document, *sequence, *track)?;
                render_plan(
                    &r.project,
                    &document,
                    AudioTarget::Sequence(*sequence),
                    range,
                )?
            }
            AudioLoudnessInput::Asset {
                asset,
                stream_index,
                range,
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
                    &r.project,
                    *stream_index,
                    MAX_AUDIO_FRAMES,
                )?;
                match range {
                    Some(range) => {
                        let samples = sample_range(*range).map_err(audio_error)?;
                        let channels = decoded.buffer.channels();
                        if samples.start < 0
                            || usize::try_from(samples.end).unwrap_or(usize::MAX)
                                > decoded.buffer.frame_count()
                        {
                            return Err(ServiceError::new(
                                "AUDIO_SOURCE_TOO_SHORT",
                                "asset loudness range exceeds decoded stream",
                            ));
                        }
                        let begin = samples.start as usize * channels;
                        let end = samples.end as usize * channels;
                        ChannelBuffer::new(
                            decoded.buffer.mask(),
                            decoded.buffer.samples()[begin..end].to_vec(),
                        )
                        .map_err(audio_error)?
                    }
                    None => decoded.buffer,
                }
            }
        };
        let report = kronello_audio::loudness_channels(&buffer).map_err(audio_error)?;
        Ok(AudioLoudnessResult {
            revision: stored.revision.to_string(),
            integrated_lufs: report.integrated_lufs,
            momentary_lufs: report.momentary_lufs,
            short_term_lufs: report.short_term_lufs,
            true_peak_dbtp: report.true_peak_dbtp,
            frames: buffer.frame_count() as u64,
        })
    }
    /// AUDIO-008 normalize: measure integrated loudness of the clip through
    /// the shared plan, then append one `kronello.audio.gain` effect via the
    /// same edit.plan/edit.apply pipeline every timeline mutation uses.
    pub fn normalize_audio(
        &self,
        r: AudioNormalizeRequest,
    ) -> Result<AudioNormalizeResult, ServiceError> {
        if !r.target_lufs.is_finite() || !(-70.0..=0.0).contains(&r.target_lufs) {
            return Err(ServiceError::invalid(
                "target_lufs must be finite within [-70, 0]",
            ));
        }
        if r.idempotency_key.is_empty() || r.idempotency_key.len() > 256 {
            return Err(ServiceError::invalid(
                "idempotency_key must contain 1..256 UTF-8 bytes",
            ));
        }
        let stored = snapshot(&r.project, &r.base_revision)?;
        stored
            .document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        let (_, clip) = audio_clip(&stored.document, r.sequence, r.clip)?;
        let clip = clip.clone();
        let document = solo_clip_document(&stored.document, r.sequence, clip.id);
        let rendered = render_plan(
            &r.project,
            &document,
            AudioTarget::Sequence(r.sequence),
            clip.timeline_range,
        )?;
        let measured = kronello_audio::loudness_channels(&rendered)
            .map_err(audio_error)?
            .integrated_lufs
            .ok_or_else(|| {
                ServiceError::new(
                    "INVALID_AUDIO_INPUT",
                    "clip is too short or too quiet for BS.1770 integrated loudness",
                )
            })?;
        let gain_db = r.target_lufs - measured;
        let gain_linear = 10_f64.powf(gain_db / 20.0);
        if !(gain_linear.is_finite() && gain_linear <= f64::from(f32::MAX)) {
            return Err(ServiceError::new(
                "INVALID_AUDIO_INPUT",
                "normalization gain exceeds the audio volume range",
            ));
        }
        // The appended gain effect multiplies the measured chain output by
        // exactly the gain needed to reach the target. Its identity is
        // derived from the clip and current property count so CLI and MCP
        // runs of the same command stream mint identical documents; the
        // derivation never uses array positions or display names.
        let property_id = PropertyId::from_uuid({
            let mut digest = Sha256::new();
            digest.update(b"kronello.audio-normalize-gain-v1");
            digest.update(clip.id.as_uuid().as_bytes());
            digest.update(clip.properties.len().to_be_bytes());
            let hash = digest.finalize();
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&hash[..16]);
            // UUID v8, RFC variant: application-defined deterministic identity.
            bytes[6] = (bytes[6] & 0x0f) | 0x80;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            Uuid::from_bytes(bytes)
        });
        let property = Property::new(
            property_id,
            DescriptorRef {
                key: SchemaKey::new("kronello.audio.volume")
                    .map_err(|e| ServiceError::invalid(e.to_string()))?,
                version: 1,
            },
            PropertySource::Constant(Value::Scalar(
                FiniteF64::new(gain_linear)
                    .map_err(|_| ServiceError::invalid("non-finite normalization gain"))?,
            )),
            vec![],
            &crate::edit::registry(),
        )
        .map_err(|e| ServiceError::invalid(format!("normalization property: {e}")))?;
        let mut properties = clip.properties.clone();
        properties.push(property);
        let mut effects = clip.effects.clone();
        effects.push(Effect::Known(EffectDefinition {
            effect_id: AUDIO_GAIN_ID.to_string(),
            version: 1,
            parameters: EffectParameters::AudioGain { gain: property_id },
        }));
        let commands = vec![EditCommand::Timeline(Box::new(
            TimelineCommand::ClipSetEffects {
                sequence: r.sequence,
                clip: r.clip,
                properties,
                effects,
            },
        ))];
        let plan = edit::build(stored.document.clone(), stored.revision, commands)?;
        let mut payload_request = r.clone();
        payload_request.idempotency_key = String::new();
        payload_request.project = r
            .project
            .canonicalize()
            .unwrap_or_else(|_| r.project.clone());
        payload_request.base_revision = stored.revision.to_string();
        let payload = serde_json::json!({
            "operation": "audio.normalize",
            "request": payload_request,
        });
        let event = edit::apply_template(
            EditApplyRequest {
                project: r.project,
                base_revision: stored.revision.to_string(),
                session_id: r.session_id,
                idempotency_key: r.idempotency_key,
                plan_hash: plan.plan_hash,
                commands: plan.commands,
            },
            payload,
        )?;
        Ok(AudioNormalizeResult {
            event,
            measured_lufs: measured,
            gain_db,
            gain_linear,
        })
    }
}
