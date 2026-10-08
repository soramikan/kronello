//! Owned preview runtime resources. Not entries in the stateless request registry.
use crate::ServiceError;
use kronello_audio::{AudioTarget, ChannelSources, DocumentAudioPlan, MAX_AUDIO_FRAMES};
use kronello_model::{DocumentObject, TrackId};
use kronello_render::RenderTarget;
use kronello_store::{ProjectStore, Snapshot};
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use std::{
    collections::btree_map::Entry,
    path::{Path, PathBuf},
};

pub const MAX_PLAYBACK_BLOCK_FRAMES: usize = 4096;

/// AUDIO-009: peak/RMS levels measured by the shared evaluator for one
/// rendered block, serialized to JSON by the FFI for the playback GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockMeters {
    /// Per-channel peak of the summed output within the block.
    pub master_peak: [f32; 2],
    /// Per-channel RMS of the summed output within the block.
    pub master_rms: [f32; 2],
    /// Levels per track with audible contributions within the block.
    pub tracks: Vec<TrackMeterReading>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackMeterReading {
    pub track: TrackId,
    pub peak: [f32; 2],
    pub rms: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPrepareRequest {
    pub project: PathBuf,
    pub target: RenderTarget,
    pub expected_revision: String,
}

/// Immutable project input captured by the shared service on the editing thread.
/// The preparation producer compiles and decodes it without reopening the DB.
pub struct AudioPreparationInput {
    request: AudioPrepareRequest,
    stored: Snapshot,
}
impl AudioPreparationInput {
    pub fn prepare(self) -> Result<PreparedAudio, ServiceError> {
        PreparedAudio::prepare_snapshot(
            &self.request.project,
            self.request.target,
            &self.request.expected_revision,
            self.stored,
        )
    }
}
impl crate::Service<'_> {
    pub fn capture_audio_input(
        &self,
        request: AudioPrepareRequest,
    ) -> Result<AudioPreparationInput, ServiceError> {
        let store = crate::open_existing(&request.project)?;
        let stored = store.snapshot()?;
        store.close()?;
        if stored.revision.to_string() != request.expected_revision {
            return Err(ServiceError::new(
                "REVISION_CONFLICT",
                "audio preparation revision differs",
            ));
        }
        Ok(AudioPreparationInput { request, stored })
    }
}

pub struct PreparedAudio {
    revision: String,
    plan: DocumentAudioPlan,
    /// Decoded sources keep their own speaker layout (ADR-0124); the block
    /// renderer's declared monitor layout is stereo, so multichannel sources
    /// fold down only through the explicit evaluator matrix.
    sources: ChannelSources,
    has_audio: bool,
}

impl PreparedAudio {
    /// Disk reads, decoding and compilation belong on a producer/preparation thread.
    /// The owned plan and verified source bytes never read the project again.
    pub fn prepare(
        project: &Path,
        target: RenderTarget,
        expected_revision: &str,
    ) -> Result<Self, ServiceError> {
        let stored = ProjectStore::read_snapshot(project)?;
        Self::prepare_snapshot(project, target, expected_revision, stored)
    }
    fn prepare_snapshot(
        project: &Path,
        target: RenderTarget,
        expected_revision: &str,
        stored: Snapshot,
    ) -> Result<Self, ServiceError> {
        if stored.revision.to_string() != expected_revision {
            return Err(ServiceError::new(
                "REVISION_CONFLICT",
                "audio preparation revision differs",
            ));
        }
        if stored.document.schema_version != kronello_model::PROJECT_SCHEMA_VERSION {
            return Err(ServiceError::new(
                "UNSUPPORTED_SCHEMA_VERSION",
                "audio project schema version",
            ));
        }
        if stored.document.semantic_version != kronello_model::PROJECT_SEMANTIC_VERSION
            || !stored.document.unknown_fields.is_empty()
        {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "audio project semantic version or unknown fields",
            ));
        }
        let resolved = match target {
            RenderTarget::Source { source } => match source {
                kronello_render::SourcePreviewRef::Composition { .. } => None,
                _ => Some(
                    kronello_render::resolve_source(&stored.document, &source.source_ref())
                        .map_err(crate::ServiceError::from)?,
                ),
            },
            _ => None,
        };
        let target = match target {
            RenderTarget::Composition { composition } => AudioTarget::Composition(composition),
            RenderTarget::Sequence { sequence } => AudioTarget::Sequence(sequence),
            // GUI-011 (ADR-0128): a composition source plays its authored
            // audio plan; media sources decode the resolved angle stream.
            RenderTarget::Source { source } => match source {
                kronello_render::SourcePreviewRef::Composition { composition } => {
                    AudioTarget::Composition(composition)
                }
                _ => {
                    let resolved = resolved.expect("resolved source");
                    match resolved.audio_stream {
                        Some(stream_index) => AudioTarget::Source {
                            asset: resolved.asset,
                            stream_index,
                            offset: resolved.offset,
                        },
                        None => {
                            return Ok(Self {
                                revision: stored.revision.to_string(),
                                plan: DocumentAudioPlan::default(),
                                sources: ChannelSources::new(),
                                has_audio: false,
                            });
                        }
                    }
                }
            },
        };
        let plan =
            DocumentAudioPlan::compile_version(&stored.document, target, 2).map_err(audio_error)?;
        let clips = plan.clips();
        let has_audio = !clips.is_empty() || match target {
            AudioTarget::Sequence(id) => stored.document.sequences.iter().any(|s| {
                matches!(s, DocumentObject::Known(s) if s.id == id && s.tracks.iter().any(|t| t.kind == kronello_model::TrackKind::Audio && t.clips.iter().any(|c| matches!(c.source_ref, kronello_model::SourceRef::Generator { .. }))))
            }),
            AudioTarget::Composition(_) | AudioTarget::Source { .. } => false,
        };
        let mut sources = ChannelSources::new();
        let mut frames = 0;
        if !clips.is_empty() {
            let runtime = kronello_media::MediaRuntime::load()?;
            for clip in clips {
                if let Entry::Vacant(entry) = sources.entry((clip.asset, clip.stream_index)) {
                    let asset = stored
                        .document
                        .assets
                        .iter()
                        .find_map(|asset| match asset {
                            DocumentObject::Known(a) if a.id == clip.asset => Some(a),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            ServiceError::new("ASSET_MISSING", clip.asset.to_string())
                        })?;
                    let decoded = runtime.decode_asset_audio_bounded(
                        asset,
                        project,
                        clip.stream_index,
                        MAX_AUDIO_FRAMES - frames,
                    )?;
                    frames += decoded.buffer.frame_count();
                    entry.insert(decoded.buffer);
                }
            }
        }
        Ok(Self {
            revision: stored.revision.to_string(),
            plan,
            sources,
            has_audio,
        })
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }
    /// Bit-identical to evaluator 2 before export's PCM quantisation.
    /// Caller owns the binary interleaved stereo buffer. No disk/project reads.
    pub fn render_block(&self, start_sample: i64, output: &mut [f32]) -> Result<(), ServiceError> {
        let frames = output.len() / 2;
        if start_sample < 0
            || !output.len().is_multiple_of(2)
            || frames == 0
            || frames > MAX_PLAYBACK_BLOCK_FRAMES
        {
            return Err(ServiceError::new(
                "INVALID_AUDIO_INPUT",
                "expected 1..4096 stereo frames and nonnegative start sample",
            ));
        }
        let end = start_sample
            .checked_add(frames as i64)
            .ok_or_else(|| ServiceError::new("TIME_ERROR", "audio sample overflow"))?;
        let time_error =
            |e: kronello_time::TimeError| ServiceError::new("TIME_ERROR", e.to_string());
        let range = TimeRange::new(
            Time::new(start_sample, 48000).map_err(time_error)?,
            Time::new(end, 48000).map_err(time_error)?,
        )
        .map_err(time_error)?;
        let bus = self
            .plan
            .mix_reader(&self.sources, range)
            .map_err(audio_error)?;
        for (out, frame) in output.chunks_exact_mut(2).zip(bus.buffer().frames()) {
            out.copy_from_slice(frame);
        }
        Ok(())
    }
    /// AUDIO-009: identical render path plus the evaluator-measured meters.
    /// Caller owns the binary interleaved stereo buffer. No disk/project reads.
    pub fn render_block_metered(
        &self,
        start_sample: i64,
        output: &mut [f32],
    ) -> Result<BlockMeters, ServiceError> {
        let frames = output.len() / 2;
        if start_sample < 0
            || !output.len().is_multiple_of(2)
            || frames == 0
            || frames > MAX_PLAYBACK_BLOCK_FRAMES
        {
            return Err(ServiceError::new(
                "INVALID_AUDIO_INPUT",
                "expected 1..4096 stereo frames and nonnegative start sample",
            ));
        }
        let end = start_sample
            .checked_add(frames as i64)
            .ok_or_else(|| ServiceError::new("TIME_ERROR", "audio sample overflow"))?;
        let time_error =
            |e: kronello_time::TimeError| ServiceError::new("TIME_ERROR", e.to_string());
        let range = TimeRange::new(
            Time::new(start_sample, 48000).map_err(time_error)?,
            Time::new(end, 48000).map_err(time_error)?,
        )
        .map_err(time_error)?;
        let (bus, meters) = self
            .plan
            .mix_metered(&self.sources, range)
            .map_err(audio_error)?;
        for (out, frame) in output.chunks_exact_mut(2).zip(bus.buffer().frames()) {
            out.copy_from_slice(frame);
        }
        Ok(BlockMeters {
            master_peak: meters.master.peak,
            master_rms: meters.master.rms,
            tracks: meters
                .tracks
                .iter()
                .map(|meter| TrackMeterReading {
                    track: meter.track,
                    peak: meter.peak,
                    rms: meter.rms,
                })
                .collect(),
        })
    }
}
fn audio_error(error: kronello_audio::AudioError) -> ServiceError {
    ServiceError::new(error.code(), error.to_string())
}
