//! Pure offline channel-masked mixing on the absolute 48 kHz sample grid.
//! No codecs, filesystem, device handles, or mutable project lookups.
//! Channel layouts follow ADR-0124: an explicit versioned [`ChannelMask`]
//! rides every buffer and bus; layout conversion is explicit and LFE is
//! excluded unless requested.
use std::collections::BTreeMap;
use std::ops::Range;

use kronello_model::AssetId;
use kronello_time::{SampleRate, Time, TimeRange};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod analysis;
pub use analysis::{analyze_audio, analyze_audio_channels};
mod advanced;
mod channels;
mod document;
mod dsp;
mod loudness;
mod sync;
mod wsola;
pub use advanced::{AUDIO_EVALUATION_VERSION, AUDIO_GENERATOR_SILENCE, AUDIO_GENERATOR_TONE};
pub use channels::{
    ChannelBuffer, ChannelBus, ChannelBusMeters, ChannelMeter, ChannelSourceReader, ChannelSources,
    ChannelTrackMeter, ITU_CENTER_COEFFICIENT, ITU_SURROUND_MONO_COEFFICIENT, LayoutMatrix,
    MAX_CHANNELS, channel_meter, convert_layout, layout_matrix,
};
pub use document::{AudioSourceMode, AudioTarget, DocumentAudioPlan};
pub use loudness::{LoudnessReport, loudness, loudness_channels};
pub use sync::{MAX_SYNC_LAG, estimate_sync_lag, max_search_lag};

pub const SAMPLE_RATE: SampleRate = SampleRate::HZ_48000;
/// Conservative offline memory limit: ten minutes of frames per source/bus.
/// Multichannel buses additionally bound total samples by channels.
pub const MAX_AUDIO_FRAMES: usize = 48_000 * 600;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("INVALID_AUDIO_INPUT: {0}")]
    InvalidInput(String),
    #[error("AUDIO_BUDGET_EXCEEDED: {0}")]
    BudgetExceeded(String),
    #[error("ASSET_MISSING: audio {0}")]
    AssetMissing(AssetId),
    #[error("AUDIO_SOURCE_READ: {0}")]
    SourceRead(String),
    #[error("AUDIO_SOURCE_TOO_SHORT: {0}")]
    SourceTooShort(AssetId),
    #[error(transparent)]
    Sequence(#[from] kronello_model::SequenceError),
    #[error("UNSUPPORTED_FEATURE: {0}")]
    Unsupported(String),
    /// ADR-0124: masks outside the closed set, layout/count mismatches, and
    /// frames sized for a different layout are all typed rejections; the
    /// pipeline never falls back to a guessed layout.
    #[error("UNSUPPORTED_CHANNEL_LAYOUT: {0}")]
    UnsupportedChannelLayout(String),
    #[error("AUDIO_OVERFLOW: non-finite mixing result")]
    Overflow,
    #[error("AUDIO_CLIPPING: {samples} channel samples exceed full scale")]
    Clipping { samples: usize },
    /// NLE-007 multicam audio sync estimation failure (ADR-0127): silent,
    /// too-short, ambiguous or weakly correlated input.
    #[error("MULTICAM_SYNC_FAILED: {0}")]
    SyncFailed(String),
    #[error(transparent)]
    Time(#[from] kronello_time::TimeError),
}
impl AudioError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_AUDIO_INPUT",
            Self::BudgetExceeded(_) => "AUDIO_BUDGET_EXCEEDED",
            Self::AssetMissing(_) => "ASSET_MISSING",
            Self::SourceRead(_) => "AUDIO_SOURCE_READ",
            Self::SourceTooShort(_) => "AUDIO_SOURCE_TOO_SHORT",
            Self::Sequence(e) => e.code(),
            Self::Unsupported(_) => "UNSUPPORTED_FEATURE",
            Self::UnsupportedChannelLayout(_) => "UNSUPPORTED_CHANNEL_LAYOUT",
            Self::Overflow => "AUDIO_OVERFLOW",
            Self::Clipping { .. } => "AUDIO_CLIPPING",
            Self::SyncFailed(_) => "MULTICAM_SYNC_FAILED",
            Self::Time(_) => "TIME_ERROR",
        }
    }
}
impl From<kronello_model::ChannelLayoutError> for AudioError {
    fn from(error: kronello_model::ChannelLayoutError) -> Self {
        let message = error.to_string();
        Self::UnsupportedChannelLayout(
            message
                .strip_prefix("UNSUPPORTED_CHANNEL_LAYOUT: ")
                .unwrap_or(&message)
                .to_string(),
        )
    }
}

/// Dimensionless nonnegative linear amplitude. 0 mutes; 1 is unity.
/// UI decibels must be explicitly converted before constructing this value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct Gain(f32);
impl Gain {
    pub const UNITY: Self = Self(1.0);
    pub fn new(linear: f32) -> Result<Self, AudioError> {
        if !linear.is_finite() || linear < 0.0 {
            return Err(AudioError::InvalidInput(
                "gain must be finite and nonnegative".into(),
            ));
        }
        Ok(Self(linear))
    }
    pub fn linear(self) -> f32 {
        self.0
    }
}
impl TryFrom<f32> for Gain {
    type Error = AudioError;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<Gain> for f32 {
    fn from(value: Gain) -> Self {
        value.0
    }
}

/// Source trim is relative to the first decoded sample, at unity playback speed.
/// Placement order is also the deterministic float summation order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClip {
    pub asset: AssetId,
    pub stream_index: u32,
    pub placement: TimeRange,
    pub source_in: Time,
    pub gain: Gain,
}
impl AudioClip {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.source_in < Time::ZERO {
            return Err(AudioError::InvalidInput(
                "source_in must be nonnegative".into(),
            ));
        }
        sample_range(self.placement)?;
        sample_index(self.source_in)?;
        Ok(())
    }
}

pub fn sample_index(time: Time) -> Result<i64, AudioError> {
    Ok(SAMPLE_RATE.sample_floor(time)?)
}
pub fn sample_range(range: TimeRange) -> Result<Range<i64>, AudioError> {
    Ok(SAMPLE_RATE.samples_for_range(range)?)
}

/// Interleaved logical frames in left/right order, finite f32, 48 kHz.
/// Values outside [-1,1] are retained through decoding and mixing.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    frames: Vec<[f32; 2]>,
}
impl AudioBuffer {
    pub fn new(frames: Vec<[f32; 2]>) -> Result<Self, AudioError> {
        if frames.len() > MAX_AUDIO_FRAMES || frames.iter().flatten().any(|v| !v.is_finite()) {
            return Err(AudioError::InvalidInput(
                "audio budget or finite sample violation".into(),
            ));
        }
        Ok(Self { frames })
    }
    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }
}
pub fn apply_gain(source: &AudioBuffer, gain: Gain) -> Result<AudioBuffer, AudioError> {
    let frames: Vec<_> = source
        .frames
        .iter()
        .map(|f| f.map(|v| v * gain.0))
        .collect();
    if frames.iter().flatten().any(|v| !v.is_finite()) {
        return Err(AudioError::Overflow);
    }
    AudioBuffer::new(frames)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bus {
    start_sample: i64,
    buffer: AudioBuffer,
}
impl Bus {
    /// One finished offline bus starting at `start_sample`. `AudioBuffer::new`
    /// already enforces the frame budget and finite-sample invariant.
    pub fn new(start_sample: i64, buffer: AudioBuffer) -> Self {
        Self {
            start_sample,
            buffer,
        }
    }
    pub fn start_sample(&self) -> i64 {
        self.start_sample
    }
    pub fn buffer(&self) -> &AudioBuffer {
        &self.buffer
    }
    pub(crate) fn from_frames(
        start_sample: i64,
        buffer: ChannelBuffer,
    ) -> Result<Self, AudioError> {
        let frames = buffer.stereo_frames().ok_or_else(|| {
            AudioError::UnsupportedChannelLayout(
                "a stereo Bus requires a stereo ChannelBuffer".into(),
            )
        })?;
        Ok(Self {
            start_sample,
            buffer: AudioBuffer::new(frames)?,
        })
    }
    /// Signed PCM24 in the high 24 bits of S32, for FFmpeg's native pcm_s24le.
    /// Round to nearest, ties away from zero; no dither. Full-scale +1 maps to
    /// 8388607 and -1 maps to -8388608. Saturation must be explicitly requested.
    pub fn quantize_pcm24(&self, policy: ClippingPolicy) -> Result<QuantizedAudio, AudioError> {
        quantize_samples(self.buffer.frames().as_flattened(), policy)
    }
}
/// Shared PCM24 quantizer for stereo and multichannel buses: interleaved
/// samples in, signed S32 samples out (`sample * 2^24 << 8`, ties away).
pub fn quantize_samples(
    samples: &[f32],
    policy: ClippingPolicy,
) -> Result<QuantizedAudio, AudioError> {
    let clipped_samples = samples.iter().filter(|v| v.abs() > 1.0).count();
    if policy == ClippingPolicy::Reject && clipped_samples != 0 {
        return Err(AudioError::Clipping {
            samples: clipped_samples,
        });
    }
    let samples = samples
        .iter()
        .map(|v| {
            let q = (f64::from(v.clamp(-1.0, 1.0)) * 8_388_608.0).round();
            (q.clamp(-8_388_608.0, 8_388_607.0) as i32) * 256
        })
        .collect();
    Ok(QuantizedAudio {
        samples,
        clipped_samples,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClippingPolicy {
    Reject,
    Saturate,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizedAudio {
    pub samples: Vec<i32>,
    pub clipped_samples: usize,
}

/// AUDIO-009: per-rendered-range peak/RMS levels produced by the shared
/// evaluator, so meters describe exactly the samples that were rendered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StereoMeter {
    /// Maximum absolute per-channel sample within the rendered range.
    pub peak: [f32; 2],
    /// Root-mean-square per-channel level within the rendered range.
    pub rms: [f32; 2],
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackMeter {
    pub track: kronello_model::TrackId,
    /// Maximum absolute per-channel sample contributed by this track.
    pub peak: [f32; 2],
    /// Root-mean-square per-channel level contributed by this track.
    pub rms: [f32; 2],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusMeters {
    /// Tracks with audible contributions in the rendered range.
    pub tracks: Vec<TrackMeter>,
    /// Summed output levels in the rendered range.
    pub master: StereoMeter,
}
/// Compute peak/RMS over one finite interleaved stereo range.
pub fn stereo_meter(frames: &[[f32; 2]]) -> StereoMeter {
    let mut peak = [0.0_f32; 2];
    let mut energy = [0.0_f64; 2];
    for frame in frames {
        for channel in 0..2 {
            peak[channel] = peak[channel].max(frame[channel].abs());
            energy[channel] += f64::from(frame[channel]) * f64::from(frame[channel]);
        }
    }
    let count = frames.len().max(1) as f64;
    StereoMeter {
        peak,
        rms: [
            (energy[0] / count).sqrt() as f32,
            (energy[1] / count).sqrt() as f32,
        ],
    }
}

/// Each source key selects the exact authored stream of a verified asset.
/// `AudioSources` stores stereo frames; [`ChannelSources`] carries any
/// supported layout.
pub type AudioSources = BTreeMap<(AssetId, u32), AudioBuffer>;

/// Calculate every boundary from absolute rational time. Adjacent requests use
/// identical floor boundaries; rounded frame lengths are never accumulated.
pub fn mix(
    clips: &[AudioClip],
    sources: &AudioSources,
    range: TimeRange,
) -> Result<Bus, AudioError> {
    mix_reader(clips, sources, range)
}

/// Mix a bounded stereo Bus from indexed immutable sources. Multichannel
/// sources are explicitly downmixed to stereo (ADR-0124, LFE excluded).
pub fn mix_reader(
    clips: &[AudioClip],
    sources: &dyn ChannelSourceReader,
    range: TimeRange,
) -> Result<Bus, AudioError> {
    mix_with_gain_reader(clips, sources, range, &mut |_, _| Ok(Gain::UNITY))
}

/// Stateless per-sample gain hook. Sample indices are absolute, never accumulated.
pub fn mix_with_gain(
    clips: &[AudioClip],
    sources: &AudioSources,
    range: TimeRange,
    gain: &mut dyn FnMut(usize, i64) -> Result<Gain, AudioError>,
) -> Result<Bus, AudioError> {
    mix_with_gain_reader(clips, sources, range, gain)
}

/// Mix at an explicit target layout. `target` must come from the closed
/// ADR-0124 set; every source converts through [`layout_matrix`] — there is
/// no implicit fold-down, the caller picks the output mask.
pub fn mix_channels(
    clips: &[AudioClip],
    sources: &dyn ChannelSourceReader,
    range: TimeRange,
    target: kronello_model::ChannelMask,
) -> Result<ChannelBus, AudioError> {
    mix_with_gain_channels(clips, sources, range, &mut |_, _| Ok(Gain::UNITY), target)
}

pub(crate) fn mix_with_gain_reader(
    clips: &[AudioClip],
    sources: &dyn ChannelSourceReader,
    range: TimeRange,
    gain: &mut dyn FnMut(usize, i64) -> Result<Gain, AudioError>,
) -> Result<Bus, AudioError> {
    let bus = mix_with_gain_channels(
        clips,
        sources,
        range,
        gain,
        kronello_model::ChannelMask::STEREO,
    )?;
    bus.into_stereo_bus()
}

pub(crate) fn mix_with_gain_channels(
    clips: &[AudioClip],
    sources: &dyn ChannelSourceReader,
    range: TimeRange,
    gain: &mut dyn FnMut(usize, i64) -> Result<Gain, AudioError>,
    target: kronello_model::ChannelMask,
) -> Result<ChannelBus, AudioError> {
    let output = sample_range(range)?;
    let length = output
        .end
        .checked_sub(output.start)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|v| *v <= MAX_AUDIO_FRAMES)
        .ok_or_else(|| AudioError::InvalidInput("bus sample budget exceeded".into()))?;
    let channels = target.channels();
    let mut frames = vec![0.0_f32; length * channels];
    for (clip_index, clip) in clips.iter().enumerate() {
        clip.validate()?;
        let placement = sample_range(clip.placement)?;
        let source_in = sample_index(clip.source_in)?;
        let source_length = sources.frame_count(clip.asset, clip.stream_index)?;
        let source_mask = sources.layout(clip.asset, clip.stream_index)?;
        let source_channels = source_mask.channels();
        let matrix = layout_matrix(source_mask, target, false)?;
        let source_end = source_in
            .checked_add(
                placement
                    .end
                    .checked_sub(placement.start)
                    .ok_or(AudioError::Overflow)?,
            )
            .ok_or(AudioError::Overflow)?;
        if usize::try_from(source_end)
            .ok()
            .is_none_or(|end| end > source_length)
        {
            return Err(AudioError::SourceTooShort(clip.asset));
        }
        let start = output.start.max(placement.start);
        let end = output.end.min(placement.end);
        if start >= end {
            continue;
        }
        // All differences are bounded by the validated source and bus lengths.
        let dst = usize::try_from(start - output.start).map_err(|_| AudioError::Overflow)?;
        let src = usize::try_from(source_in + (start - placement.start))
            .map_err(|_| AudioError::Overflow)?;
        let len = usize::try_from(end - start).map_err(|_| AudioError::Overflow)?;
        let mut input = [0.0_f32; MAX_CHANNELS];
        let mut converted = [0.0_f32; MAX_CHANNELS];
        for offset in 0..len {
            sources.read_frame(
                clip.asset,
                clip.stream_index,
                src + offset,
                &mut input[..source_channels],
            )?;
            matrix.apply(&input[..source_channels], &mut converted[..channels])?;
            let linear = gain(clip_index, start + offset as i64)?.linear() * clip.gain.0;
            for channel in 0..channels {
                let out = &mut frames[(dst + offset) * channels + channel];
                *out += converted[channel] * linear;
                if !out.is_finite() {
                    return Err(AudioError::Overflow);
                }
            }
        }
    }
    Ok(ChannelBus {
        start_sample: output.start,
        buffer: ChannelBuffer::new(target, frames)?,
    })
}
