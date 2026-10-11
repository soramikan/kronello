//! Owned preview runtime resources. Not entries in the stateless request registry.
use crate::ServiceError;
use kronello_audio::{
    AudioError, AudioPlaybackSession, AudioTarget, ChannelSourceReader, DocumentAudioPlan,
    MAX_AUDIO_FRAMES,
};
use kronello_model::{AssetId, ChannelMask, DocumentObject, TrackId};
use kronello_render::RenderTarget;
use kronello_store::{ProjectStore, Snapshot};
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use std::{
    collections::btree_map::Entry,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
    /// Optional playback resume position: stateful chains pre-evaluate up to
    /// this sample in bounded blocks during preparation so the first
    /// streaming render does not replay a whole placement in one call.
    #[serde(default)]
    pub resume_sample: Option<i64>,
}

/// Immutable project input captured by the shared service on the editing thread.
/// The preparation producer compiles and decodes it without reopening the DB.
pub struct AudioPreparationInput {
    request: AudioPrepareRequest,
    stored: AudioPreparationSnapshot,
    sources: Arc<Mutex<kronello_media::AudioSourceCache>>,
    checkpoints: Arc<Mutex<crate::audio_checkpoints::AudioCheckpointCache>>,
}
struct AudioPreparationSnapshot {
    stored: Snapshot,
    canonical_project: PathBuf,
}
impl AudioPreparationInput {
    pub fn prepare(self) -> Result<PreparedAudio, ServiceError> {
        PreparedAudio::prepare_snapshot(
            &self.request.project,
            self.request.target,
            &self.request.expected_revision,
            self.stored,
            self.request.resume_sample,
            &self.sources,
            self.checkpoints,
        )
    }
}
impl crate::Service<'_> {
    pub fn with_audio_checkpoint_cache_limits(
        mut self,
        limits: crate::AudioCheckpointCacheLimits,
    ) -> Result<Self, ServiceError> {
        self.audio_checkpoints = Arc::new(Mutex::new(
            crate::audio_checkpoints::AudioCheckpointCache::new(limits)?,
        ));
        Ok(self)
    }
    pub fn audio_checkpoint_cache_stats(&self) -> crate::AudioCheckpointCacheStats {
        self.audio_checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats()
    }
    /// Configure the fallback service's derived PCM retention. An active native
    /// ProjectSession owns its separate cache and default budget.
    pub fn with_audio_source_cache_limits(
        mut self,
        limits: kronello_media::AudioSourceCacheLimits,
    ) -> Self {
        self.audio_sources = Arc::new(Mutex::new(kronello_media::AudioSourceCache::new(limits)));
        self
    }
    /// Fallback service-owned cache; native sessions expose their own stats.
    pub fn audio_source_cache_stats(&self) -> kronello_media::AudioSourceCacheStats {
        self.audio_sources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats()
    }
    pub fn capture_audio_input(
        &self,
        request: AudioPrepareRequest,
    ) -> Result<AudioPreparationInput, ServiceError> {
        let canonical_project = request
            .project
            .canonicalize()
            .map_err(kronello_store::StoreError::from)?;
        let sources = crate::session::audio_source_cache(&request.project)?
            .unwrap_or_else(|| self.audio_sources.clone());
        let checkpoints = crate::session::audio_checkpoint_cache(&request.project)?
            .unwrap_or_else(|| self.audio_checkpoints.clone());
        let store = crate::open_existing(&request.project)?;
        let stored = store.snapshot()?;
        store.close()?;
        if stored.revision.to_string() != request.expected_revision {
            return Err(ServiceError::new(
                "REVISION_CONFLICT",
                "audio preparation revision differs",
            ));
        }
        Ok(AudioPreparationInput {
            request,
            stored: AudioPreparationSnapshot {
                stored,
                canonical_project,
            },
            sources,
            checkpoints,
        })
    }
}

/// Immutable PCM is shared; every prepared plan owns its DSP session.
#[derive(Default)]
pub(crate) struct SharedChannelSources(
    pub(crate) std::collections::BTreeMap<(AssetId, u32), Arc<kronello_media::DecodedAudio>>,
);
impl SharedChannelSources {
    fn new() -> Self {
        Self::default()
    }
    fn entry(
        &mut self,
        key: (AssetId, u32),
    ) -> Entry<'_, (AssetId, u32), Arc<kronello_media::DecodedAudio>> {
        self.0.entry(key)
    }
    fn buffer(
        &self,
        asset: AssetId,
        stream: u32,
    ) -> Result<&kronello_audio::ChannelBuffer, AudioError> {
        self.0
            .get(&(asset, stream))
            .map(|source| &source.buffer)
            .ok_or(AudioError::AssetMissing(asset))
    }
}
impl ChannelSourceReader for SharedChannelSources {
    fn layout(&self, asset: AssetId, stream: u32) -> Result<ChannelMask, AudioError> {
        Ok(self.buffer(asset, stream)?.mask())
    }
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError> {
        Ok(self.buffer(asset, stream)?.frame_count())
    }
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        let buffer = self.buffer(asset, stream)?;
        if out.len() != buffer.channels() {
            return Err(AudioError::UnsupportedChannelLayout(
                "source frame channel count mismatch".into(),
            ));
        }
        out.copy_from_slice(
            buffer
                .frame(index)
                .ok_or(AudioError::SourceTooShort(asset))?,
        );
        Ok(())
    }
}

pub struct PreparedAudio {
    revision: String,
    plan: Arc<DocumentAudioPlan>,
    /// Decoded sources keep their own speaker layout (ADR-0124); the block
    /// renderer's declared monitor layout is stereo, so multichannel sources
    /// fold down only through the explicit evaluator matrix.
    sources: SharedChannelSources,
    has_audio: bool,
    /// Stateful DSP/WSOLA cursors for sequential block renders: later blocks
    /// resume instead of replaying each placement from its start (ADR-0076).
    /// The mutex only serializes the producer's own calls; the plan and
    /// emitted samples stay bit-identical to the stateless evaluator.
    session: Mutex<AudioPlaybackSession>,
    checkpoints: Arc<Mutex<crate::audio_checkpoints::AudioCheckpointCache>>,
    checkpoint_generation: Option<Arc<()>>,
    warm_start_sample: i64,
    warm_evaluated_frames: i64,
}

impl PreparedAudio {
    /// Disk reads, decoding and compilation belong on a producer/preparation thread.
    /// The owned plan and verified source bytes never read the project again.
    pub fn prepare(
        project: &Path,
        target: RenderTarget,
        expected_revision: &str,
        resume_sample: Option<i64>,
    ) -> Result<Self, ServiceError> {
        let canonical_project = project
            .canonicalize()
            .map_err(kronello_store::StoreError::from)?;
        let stored = ProjectStore::read_snapshot(project)?;
        let uncached = Mutex::new(kronello_media::AudioSourceCache::new(
            kronello_media::AudioSourceCacheLimits {
                entries: 0,
                pcm_bytes: 0,
            },
        ));
        Self::prepare_snapshot(
            project,
            target,
            expected_revision,
            AudioPreparationSnapshot {
                stored,
                canonical_project,
            },
            resume_sample,
            &uncached,
            Arc::new(Mutex::new(
                crate::audio_checkpoints::AudioCheckpointCache::new(
                    crate::AudioCheckpointCacheLimits {
                        entries: 0,
                        state_bytes: 0,
                        ..Default::default()
                    },
                )?,
            )),
        )
    }
    fn prepare_snapshot(
        project: &Path,
        target: RenderTarget,
        expected_revision: &str,
        stored: AudioPreparationSnapshot,
        resume_sample: Option<i64>,
        source_cache: &Mutex<kronello_media::AudioSourceCache>,
        checkpoints: Arc<Mutex<crate::audio_checkpoints::AudioCheckpointCache>>,
    ) -> Result<Self, ServiceError> {
        let AudioPreparationSnapshot {
            stored,
            canonical_project,
        } = stored;
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
        let checkpoint_key = crate::audio_checkpoints::AudioCheckpointKey {
            project: canonical_project,
            revision: expected_revision.to_owned(),
            target,
        };
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
                            let plan = Arc::new(DocumentAudioPlan::default());
                            let session = Mutex::new(plan.playback_session());
                            return Ok(Self {
                                revision: stored.revision.to_string(),
                                plan,
                                sources: SharedChannelSources::new(),
                                has_audio: false,
                                session,
                                checkpoints,
                                checkpoint_generation: None,
                                warm_start_sample: 0,
                                warm_evaluated_frames: 0,
                            });
                        }
                    }
                }
            },
        };
        let cached_plan = checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .plan(&checkpoint_key);
        let plan = match cached_plan {
            Some(plan) => plan,
            None => Arc::new(
                DocumentAudioPlan::compile_version(&stored.document, target, 2)
                    .map_err(audio_error)?,
            ),
        };
        let clips = plan.clips();
        let has_audio = !clips.is_empty() || match target {
            AudioTarget::Sequence(id) => stored.document.sequences.iter().any(|s| {
                matches!(s, DocumentObject::Known(s) if s.id == id && s.tracks.iter().any(|t| t.kind == kronello_model::TrackKind::Audio && t.clips.iter().any(|c| matches!(c.source_ref, kronello_model::SourceRef::Generator { .. }))))
            }),
            AudioTarget::Composition(_) | AudioTarget::Source { .. } => false,
        };
        let mut sources = SharedChannelSources::new();
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
                    let decoded = source_cache
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .decode(
                            &runtime,
                            asset,
                            project,
                            clip.stream_index,
                            MAX_AUDIO_FRAMES - frames,
                        )?;
                    frames += decoded.buffer.frame_count();
                    entry.insert(decoded);
                }
            }
        }
        let limit = plan
            .streaming_warm_limit()
            .map_err(audio_error)?
            .min(resume_sample.unwrap_or(0))
            .max(0);
        let resume = checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .resume(checkpoint_key, plan.clone(), &sources, limit);
        let session = Mutex::new(resume.session);
        let mut prepared = Self {
            revision: stored.revision.to_string(),
            plan,
            sources,
            has_audio,
            session,
            checkpoints,
            checkpoint_generation: resume.generation,
            warm_start_sample: resume.start_sample,
            warm_evaluated_frames: 0,
        };
        if let Some(sample) = resume_sample.filter(|sample| *sample > 0) {
            prepared.warm(sample)?;
        }
        Ok(prepared)
    }
    /// Advance stateful chains to `sample` in bounded blocks, on the
    /// preparation/producer side — never the device callback. Keeps the
    /// first block after a seek or snapshot swap from replaying a whole
    /// placement inside one render call.
    fn warm(&mut self, sample: i64) -> Result<(), ServiceError> {
        let limit = self
            .plan
            .streaming_warm_limit()
            .map_err(audio_error)?
            .min(sample)
            .max(0);
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let interval = self
            .checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .interval_samples();
        let mut start = self.warm_start_sample;
        while start < limit {
            let boundary = start
                .checked_div(interval)
                .and_then(|bucket| bucket.checked_add(1))
                .and_then(|bucket| bucket.checked_mul(interval))
                .unwrap_or(i64::MAX);
            let end = limit
                .min(start.saturating_add(MAX_PLAYBACK_BLOCK_FRAMES as i64))
                .min(boundary);
            let range = TimeRange::new(
                Time::new(start, 48_000)
                    .map_err(|e| ServiceError::new("TIME_ERROR", e.to_string()))?,
                Time::new(end, 48_000)
                    .map_err(|e| ServiceError::new("TIME_ERROR", e.to_string()))?,
            )
            .map_err(|e| ServiceError::new("TIME_ERROR", e.to_string()))?;
            self.plan
                .mix_streaming(&self.sources, range, &mut session)
                .map_err(audio_error)?;
            self.warm_evaluated_frames += end - start;
            if end == boundary || end == limit {
                self.checkpoints
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .save(self.checkpoint_generation.as_ref(), end, &session);
            }
            start = end;
        }
        Ok(())
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }
    pub fn warm_start_sample(&self) -> i64 {
        self.warm_start_sample
    }
    pub fn warm_evaluated_frames(&self) -> i64 {
        self.warm_evaluated_frames
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
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let bus = self
            .plan
            .mix_streaming(&self.sources, range, &mut session)
            .map_err(audio_error)?;
        self.checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .save(self.checkpoint_generation.as_ref(), end, &session);
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
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let (bus, meters) = self
            .plan
            .mix_metered_streaming(&self.sources, range, &mut session)
            .map_err(audio_error)?;
        self.checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .save(self.checkpoint_generation.as_ref(), end, &session);
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
