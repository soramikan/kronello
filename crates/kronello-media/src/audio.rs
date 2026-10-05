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
/// Metadata from a fully drained stream. Samples have been delivered in order
/// to the synchronous sink and are never retained by the runtime.
pub type AudioChunkSink<'a> = dyn FnMut(&[[f32; 2]]) -> Result<(), MediaError> + 'a;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioDecodeReport {
    pub source_start: Time,
    pub source_rate: u32,
    pub source_channels: u32,
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
        let mut frames = Vec::new();
        let report = self.decode_audio_stream(path, stream_index, &mut |chunk| {
            if frames
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > MAX_AUDIO_FRAMES)
            {
                return Err(MediaError::InvalidInput(
                    "decoded audio sample budget exceeded".into(),
                ));
            }
            frames.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(DecodedAudio {
            buffer: AudioBuffer::new(frames)?,
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
        let mut frames = 0_usize;
        while let Some(chunk) = decoder.next()? {
            if chunk.input_samples != 0 {
                let pts = chunk
                    .pts
                    .ok_or_else(|| MediaError::Decode("audio frame has no PTS".into()))?;
                let origin = *start.get_or_insert(pts);
                let expected =
                    origin.checked_add(Rational::new(input_count, i64::from(chunk.rate))?)?;
                let delta = pts.checked_sub(expected)?;
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
            if chunk.frames.iter().flatten().any(|v| !v.is_finite()) {
                return Err(MediaError::Decode("non-finite audio sample".into()));
            }
            frames = frames
                .checked_add(chunk.frames.len())
                .ok_or_else(|| MediaError::InvalidInput("decoded audio size overflow".into()))?;
            sink(&chunk.frames)?;
        }
        if frames == 0 {
            return Err(MediaError::Decode("resampler produced no samples".into()));
        }
        Ok(AudioDecodeReport {
            source_start: start.ok_or_else(|| MediaError::Decode("empty audio stream".into()))?,
            source_rate,
            source_channels,
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
