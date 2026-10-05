use std::path::Path;

use kronello_audio::{AudioBuffer, Bus, ClippingPolicy, MAX_AUDIO_FRAMES};
use kronello_model::{Asset, AssetKind};
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub buffer: AudioBuffer,
    /// Timestamp of the first decoded source sample, before placement/trim.
    pub source_start: Time,
    pub source_rate: u32,
    pub source_channels: u32,
    pub stream_index: u32,
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
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput("expected local audio file".into()));
        }
        let mut decoder = ffi::NativeAudioDecoder::open(&self.native, &path, stream_index)?;
        let mut frames = Vec::new();
        let mut start = None;
        let mut input_count = 0_i64;
        let mut source_rate = 0;
        let mut source_channels = 0;
        while let Some(chunk) = decoder.next()? {
            if chunk.input_samples != 0 {
                let pts = chunk
                    .pts
                    .ok_or_else(|| MediaError::Decode("audio frame has no PTS".into()))?;
                let origin = *start.get_or_insert(pts);
                let expected =
                    origin.checked_add(Rational::new(input_count, i64::from(chunk.rate))?)?;
                let delta = pts.checked_sub(expected)?;
                // A demuxer's timestamp grid may be coarser than one sample.
                // Tolerate only its rounding tick; gaps/discontinuities fail.
                if delta >= decoder.time_base || delta <= decoder.time_base.checked_neg()? {
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
            if frames
                .len()
                .checked_add(chunk.frames.len())
                .is_none_or(|n| n > MAX_AUDIO_FRAMES)
            {
                return Err(MediaError::InvalidInput(
                    "decoded audio sample budget exceeded".into(),
                ));
            }
            frames.extend(chunk.frames);
        }
        if frames.is_empty() {
            return Err(MediaError::Decode("resampler produced no samples".into()));
        }
        let source_start = start.ok_or_else(|| MediaError::Decode("empty audio stream".into()))?;
        Ok(DecodedAudio {
            buffer: AudioBuffer::new(frames)?,
            source_start,
            source_rate,
            source_channels,
            stream_index,
        })
    }
    /// Resolve and verify before decode and verify again before using its bytes.
    pub fn decode_asset_audio(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
    ) -> Result<DecodedAudio, MediaError> {
        if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
            return Err(MediaError::InvalidInput("asset is not audio/video".into()));
        }
        let path = resolve_asset(asset, project_path)?;
        let decoded = self.decode_audio(&path, stream_index)?;
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
        self.encode_audio_codec(bus, policy, output, false)
    }
    /// Lossless MP4 audio stage, using exactly the existing PCM24 quantizer.
    /// Movie profiles remux these packets to their declared MOV/MP4 container.
    pub fn encode_alac(
        &self,
        bus: &Bus,
        policy: ClippingPolicy,
        output: &Path,
    ) -> Result<AudioEncodeReport, MediaError> {
        self.encode_audio_codec(bus, policy, output, true)
    }
    fn encode_audio_codec(
        &self,
        bus: &Bus,
        policy: ClippingPolicy,
        output: &Path,
        alac: bool,
    ) -> Result<AudioEncodeReport, MediaError> {
        let name = if alac { "alac" } else { "pcm_s24le" };
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
        let quantized = bus.quantize_pcm24(policy)?;
        if quantized.samples.is_empty() {
            return Err(MediaError::InvalidInput("empty audio output".into()));
        }
        let temp = stage_file(output)?;
        self.native
            .encode_audio(temp.path(), &quantized.samples, alac)?;
        temp.as_file().sync_all()?;
        publish_file(temp, output)?;
        Ok(AudioEncodeReport {
            codec: name.into(),
            sample_rate: 48_000,
            channels: 2,
            frames: bus.buffer().frames().len(),
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
