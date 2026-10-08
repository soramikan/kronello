use std::path::Path;

use kronello_audio::{Bus, ChannelBuffer, ChannelBus, ClippingPolicy, MAX_AUDIO_FRAMES};
use kronello_model::{Asset, AssetKind, ChannelMask};
use kronello_time::{Rational, Time};
use serde::{Deserialize, Serialize};

use crate::{MediaError, MediaRuntime, content_hash, ffi, resolve_asset};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryAudioCodec {
    #[default]
    Alac,
    Aac,
    Opus,
}
impl DeliveryAudioCodec {
    pub(crate) fn encoder_kind(self) -> ffi::AudioEncoderKind {
        match self {
            Self::Alac => ffi::AudioEncoderKind::Alac,
            Self::Aac => ffi::AudioEncoderKind::Aac,
            Self::Opus => ffi::AudioEncoderKind::Opus,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    /// Decoded samples in the source's own speaker layout (ADR-0124: layout
    /// is preserved through decode; conversion is always an explicit choice).
    pub buffer: ChannelBuffer,
    /// Timestamp of the first decoded source sample, before placement/trim.
    pub source_start: Time,
    pub source_rate: u32,
    pub source_channels: u32,
    pub stream_index: u32,
}
/// Synchronous sink for drained decode output: interleaved f32 frames in the
/// given [`ChannelMask`] order. The mask is constant within a stream.
pub type AudioChunkSink<'a> = dyn FnMut(&[f32], ChannelMask) -> Result<(), MediaError> + 'a;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioDecodeReport {
    pub source_start: Time,
    pub source_rate: u32,
    pub source_channels: u32,
    /// Speaker layout the samples were delivered in (the source's own mask).
    pub mask: ChannelMask,
    pub frames: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AudioEncodeReport {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: usize,
    pub clipped_samples: usize,
}
impl MediaRuntime {
    /// Decode a caller-selected local audio stream and drain swresample to 48 kHz.
    /// Public project exports use decode_asset_audio for hash verification.
    pub fn decode_audio(&self, path: &Path, stream_index: u32) -> Result<DecodedAudio, MediaError> {
        self.decode_audio_bounded(path, stream_index, MAX_AUDIO_FRAMES)
    }
    fn decode_audio_bounded(
        &self,
        path: &Path,
        stream_index: u32,
        budget: usize,
    ) -> Result<DecodedAudio, MediaError> {
        let mut samples = Vec::new();
        let mut frames = 0_usize;
        let report = self.decode_audio_stream(path, stream_index, &mut |chunk, chunk_mask| {
            let chunk_frames = chunk.len() / chunk_mask.channels();
            if frames
                .checked_add(chunk_frames)
                .is_none_or(|n| n > budget.min(MAX_AUDIO_FRAMES))
            {
                return Err(kronello_audio::AudioError::BudgetExceeded(
                    "decoded audio sample budget exceeded".into(),
                )
                .into());
            }
            frames += chunk_frames;
            samples.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(DecodedAudio {
            buffer: ChannelBuffer::new(report.mask, samples)?,
            source_start: report.source_start,
            source_rate: report.source_rate,
            source_channels: report.source_channels,
            stream_index,
        })
    }
    /// Decode a local selected stream with bounded native chunks and synchronous
    /// backpressure. Validate timestamp continuity and finite samples, drain the
    /// resampler, and stop immediately on sink failure. Asset callers must verify
    /// their hash before and after this operation.
    pub fn decode_audio_stream(
        &self,
        path: &Path,
        stream_index: u32,
        sink: &mut AudioChunkSink<'_>,
    ) -> Result<AudioDecodeReport, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput("expected local audio file".into()));
        }
        let mut decoder = ffi::NativeAudioDecoder::open(&self.native, &path, stream_index)?;
        let mut start = None;
        let mut input_count = 0_i64;
        let mut source_rate = 0;
        let mut source_channels = 0;
        let mut mask = None;
        let mut frames = 0_usize;
        while let Some(chunk) = decoder.next()? {
            match mask {
                None => mask = Some(chunk.mask),
                // The native boundary already rejects mid-stream format
                // changes; this keeps the Rust contract equally explicit.
                Some(m) if m != chunk.mask => {
                    return Err(MediaError::Audio(
                        kronello_audio::AudioError::UnsupportedChannelLayout(
                            "audio layout changes within stream".into(),
                        ),
                    ));
                }
                _ => {}
            }
            if chunk.input_samples != 0 {
                let pts = chunk
                    .pts
                    .ok_or_else(|| MediaError::Decode("audio frame has no PTS".into()))?;
                // A negative first PTS is codec priming (e.g. Opus pre-skip):
                // delivered content starts at zero, so anchor the timeline
                // there instead of at the pre-content packet timestamp.
                let first = start.is_none();
                let origin = *start.get_or_insert(if pts < Rational::ZERO {
                    Rational::ZERO
                } else {
                    pts
                });
                let expected =
                    origin.checked_add(Rational::new(input_count, i64::from(chunk.rate))?)?;
                let delta = pts.checked_sub(expected)?;
                // WebM quantizes primed packet times to 1 ms; allow two stream
                // ticks so codec-delay rounding never trips the check while
                // whole-frame gaps or overlaps still fail.
                let slack = decoder.time_base.checked_mul(Rational::from_integer(2))?;
                if !(first && pts < Rational::ZERO)
                    && (delta >= slack || delta <= slack.checked_neg()?)
                {
                    return Err(MediaError::UnsupportedFeature(
                        "discontinuous audio timestamps".into(),
                    ));
                }
                source_rate = chunk.rate;
                source_channels = chunk.channels;
                input_count = input_count
                    .checked_add(chunk.input_samples as i64)
                    .ok_or_else(|| MediaError::Decode("audio input sample overflow".into()))?;
            }
            if chunk.samples.iter().any(|v| !v.is_finite()) {
                return Err(MediaError::Decode("non-finite audio sample".into()));
            }
            frames = frames
                .checked_add(chunk.samples.len() / chunk.mask.channels())
                .ok_or_else(|| MediaError::InvalidInput("decoded audio size overflow".into()))?;
            sink(&chunk.samples, chunk.mask)?;
        }
        if frames == 0 {
            return Err(MediaError::Decode("resampler produced no samples".into()));
        }
        Ok(AudioDecodeReport {
            source_start: start.ok_or_else(|| MediaError::Decode("empty audio stream".into()))?,
            source_rate,
            source_channels,
            mask: mask.ok_or_else(|| MediaError::Decode("audio stream without layout".into()))?,
            frames,
        })
    }
    /// Resolve and verify before decode and verify again before using its bytes.
    pub fn decode_asset_audio(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
    ) -> Result<DecodedAudio, MediaError> {
        self.decode_asset_audio_bounded(asset, project_path, stream_index, MAX_AUDIO_FRAMES)
    }
    /// Decode with a remaining aggregate source budget, checked before append.
    pub fn decode_asset_audio_bounded(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        budget: usize,
    ) -> Result<DecodedAudio, MediaError> {
        if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
            return Err(MediaError::InvalidInput("asset is not audio/video".into()));
        }
        let path = resolve_asset(asset, project_path)?;
        let decoded = self.decode_audio_bounded(&path, stream_index, budget)?;
        if content_hash(&path)? != asset.content_hash {
            return Err(MediaError::AssetHashMismatch(path.display().to_string()));
        }
        Ok(decoded)
    }
    pub fn encode_audio(
        &self,
        bus: &Bus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        let bus = ChannelBus::from(bus.clone());
        self.encode_audio_channels(&bus, policy, output)
    }
    /// PCM24 MOV stage at the bus's own layout (mono through 7.1, ADR-0124).
    pub fn encode_audio_channels(
        &self,
        bus: &ChannelBus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        self.encode_audio_kind(bus, policy, output, ffi::AudioEncoderKind::Pcm24)
    }
    /// Lossless MP4 audio stage, using exactly the existing PCM24 quantizer.
    /// Movie profiles remux these packets to their declared MOV/MP4 container.
    pub fn encode_alac(
        &self,
        bus: &Bus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        let bus = ChannelBus::from(bus.clone());
        self.encode_alac_channels(&bus, policy, output)
    }
    /// Multichannel form of [`encode_alac`](Self::encode_alac).
    pub fn encode_alac_channels(
        &self,
        bus: &ChannelBus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        self.encode_audio_kind(bus, policy, output, DeliveryAudioCodec::Alac.encoder_kind())
    }
    /// Lossy delivery audio stage for the closed AUDIO-005 profiles. AAC-LC
    /// writes an MP4 intermediate, Opus a WebM intermediate; movie profiles
    /// remux these packets to their declared container.
    pub fn encode_delivery_audio(
        &self,
        codec: DeliveryAudioCodec,
        bus: &Bus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        let bus = ChannelBus::from(bus.clone());
        self.encode_delivery_audio_channels(codec, &bus, policy, output)
    }
    /// Multichannel form of [`encode_delivery_audio`](Self::encode_delivery_audio).
    pub fn encode_delivery_audio_channels(
        &self,
        codec: DeliveryAudioCodec,
        bus: &ChannelBus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        self.encode_audio_kind(bus, policy, output, codec.encoder_kind())
    }
    fn encode_audio_kind(
        &self,
        bus: &ChannelBus,
        policy: ClippingPolicy,
        output: &Path,
        kind: ffi::AudioEncoderKind,
    ) -> Result<AudioEncodeReport, MediaError> {
        let name = kind.encoder_name();
        if !self
            .capabilities
            .codecs
            .iter()
            .any(|c| c.encoder && c.name == name)
        {
            return Err(MediaError::EncoderUnavailable {
                encoder: name.into(),
                reason: "native audio encoder is missing".into(),
                ffmpeg: None,
            });
        }
        let mask = bus.buffer().mask();
        let quantized = bus.quantize_pcm24(policy)?;
        if quantized.samples.is_empty() {
            return Err(MediaError::InvalidInput("empty audio output".into()));
        }
        let temp = stage_file(output)?;
        self.native
            .encode_audio(temp.path(), &quantized.samples, kind, mask)?;
        temp.as_file().sync_all()?;
        publish_file(temp, output)?;
        Ok(AudioEncodeReport {
            codec: kind.codec_name().into(),
            sample_rate: 48_000,
            channels: mask.channels() as u32,
            frames: bus.buffer().frame_count(),
            clipped_samples: quantized.clipped_samples,
        })
    }
}
pub(crate) fn stage_file(output: &Path) -> Result<tempfile::NamedTempFile, MediaError> {
    if output.exists() {
        return Err(MediaError::OutputExists(output.into()));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(tempfile::NamedTempFile::new_in(parent)?)
}
pub(crate) fn publish_file(temp: tempfile::NamedTempFile, output: &Path) -> Result<(), MediaError> {
    temp.persist_noclobber(output).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            MediaError::OutputExists(output.into())
        } else {
            MediaError::Io(e.error)
        }
    })?;
    Ok(())
}
