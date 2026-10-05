use std::path::{Path, PathBuf};

use kronello_audio::{
    AudioClip, AudioSourceMode, AudioTarget, ClippingPolicy, DocumentAudioPlan, SAMPLE_RATE,
    mix_reader, sample_range,
};
use kronello_model::{ColorSpace, DocumentObject};
use kronello_render::{
    FrameMetadata, FrameRequest, OutputRegion, RenderBackend, RenderSnapshot, frame_samples,
    render_frame_tiles,
};
use kronello_text::FontData;
use kronello_time::{FrameRate, Rational, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::audio::{publish_file, stage_file};
use crate::{
    AudioEncodeReport, EncodeCodec, EncodeRequest, MediaError, MediaPathReport, MediaRuntime,
};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MovieProfile {
    #[default]
    ProResPcm24,
    Av1Mp4AlacV1,
    H264AlacV1,
    HevcAlacV1,
}
impl MovieProfile {
    pub fn video_codec(self) -> EncodeCodec {
        match self {
            Self::ProResPcm24 => EncodeCodec::ProRes,
            Self::Av1Mp4AlacV1 => EncodeCodec::Av1,
            Self::H264AlacV1 => EncodeCodec::H264,
            Self::HevcAlacV1 => EncodeCodec::Hevc,
        }
    }
    pub(crate) fn native_id(self) -> i32 {
        match self {
            Self::ProResPcm24 => 0,
            Self::Av1Mp4AlacV1 => 1,
            Self::H264AlacV1 => 2,
            Self::HevcAlacV1 => 3,
        }
    }
    fn codecs(self) -> (&'static str, &'static str) {
        match self {
            Self::ProResPcm24 => ("prores", "pcm_s24le"),
            Self::Av1Mp4AlacV1 => ("av1", "alac"),
            Self::H264AlacV1 => ("h264", "alac"),
            Self::HevcAlacV1 => ("hevc", "alac"),
        }
    }
}

/// Owned export envelope. Version 1 retains explicit M2 audio; version 2
/// pins source selection and document-compiled placements.
/// Both inputs are owned, immutable through this API, and hashed together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvExportSnapshot {
    schema_version: u32,
    render: RenderSnapshot,
    clips: Vec<AudioClip>,
    #[serde(default, skip_serializing_if = "is_explicit")]
    audio: AudioSourceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    movie_profile: Option<MovieProfile>,
}
impl AvExportSnapshot {
    pub fn new(render: &RenderSnapshot, clips: Vec<AudioClip>) -> Result<Self, MediaError> {
        let snapshot = Self {
            schema_version: 1,
            render: render.clone(),
            clips,
            audio: AudioSourceMode::Explicit,
            movie_profile: None,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    /// Version 2 pins the explicit source selection and document-derived placements.
    pub fn with_audio(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
    ) -> Result<Self, MediaError> {
        Self::with_audio_profile(render, audio, clips, 2)
    }
    /// Profile 3 pins AUDIO-004 evaluator 2; profiles 1/2 retain their meaning.
    pub fn with_audio_profile(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
        profile: u32,
    ) -> Result<Self, MediaError> {
        if !matches!(profile, 2 | 3) {
            return Err(MediaError::UnsupportedFeature(
                "audio profile version".into(),
            ));
        }
        if audio != AudioSourceMode::Explicit && !clips.is_empty() {
            return Err(MediaError::InvalidInput(
                "document/silence audio cannot contain explicit clips".into(),
            ));
        }
        let clips = if audio == AudioSourceMode::Document {
            document_plan(render, profile)?.clips()
        } else {
            clips
        };
        let snapshot = Self {
            schema_version: profile,
            render: render.clone(),
            clips,
            audio,
            movie_profile: None,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    /// Additional movie profiles always pin evaluator 2 (audio envelope 3).
    pub fn with_movie_profile(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
        profile: MovieProfile,
    ) -> Result<Self, MediaError> {
        if profile == MovieProfile::ProResPcm24 {
            return Err(MediaError::UnsupportedFeature(
                "use legacy audio profile constructor".into(),
            ));
        }
        let mut snapshot = Self::with_audio_profile(render, audio, clips, 3)?;
        snapshot.movie_profile = Some(profile);
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn movie_profile(&self) -> MovieProfile {
        self.movie_profile.unwrap_or_default()
    }
    pub fn audio(&self) -> AudioSourceMode {
        self.audio
    }
    pub fn render(&self) -> &RenderSnapshot {
        &self.render
    }
    pub fn clips(&self) -> &[AudioClip] {
        &self.clips
    }
    pub fn content_hash(&self) -> Result<String, MediaError> {
        let value = serde_json::to_value(self)?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
    }
    pub fn validate(&self) -> Result<(), MediaError> {
        if self
            .movie_profile
            .is_some_and(|p| p == MovieProfile::ProResPcm24 || self.schema_version != 3)
        {
            return Err(MediaError::UnsupportedFeature(
                "delivery profiles require audio envelope 3".into(),
            ));
        }
        if !matches!(self.schema_version, 1..=3)
            || (self.schema_version == 1 && self.audio != AudioSourceMode::Explicit)
        {
            return Err(MediaError::UnsupportedFeature(
                "audio export snapshot schema".into(),
            ));
        }
        self.render.validate()?;
        if self.audio == AudioSourceMode::Document
            && document_plan(&self.render, self.schema_version)?.clips() != self.clips
        {
            return Err(MediaError::InvalidInput(
                "document audio placements differ from fixed snapshot".into(),
            ));
        }
        if self.audio == AudioSourceMode::Silence && !self.clips.is_empty() {
            return Err(MediaError::InvalidInput(
                "silence audio contains clips".into(),
            ));
        }
        if self.clips.len() > 1024 {
            return Err(MediaError::InvalidInput(
                "audio clip budget exceeded".into(),
            ));
        }
        for clip in &self.clips {
            clip.validate()?;
            let object = self
                .render
                .project()
                .assets
                .iter()
                .find(|a| match a {
                    DocumentObject::Known(a) => a.id == clip.asset,
                    DocumentObject::Opaque(a) => a.id == clip.asset.as_uuid(),
                })
                .ok_or_else(|| MediaError::AssetMissing(clip.asset.to_string()))?;
            let DocumentObject::Known(asset) = object else {
                return Err(MediaError::UnsupportedFeature("opaque audio asset".into()));
            };
            asset.validate()?;
            if !matches!(
                asset.kind,
                kronello_model::AssetKind::Audio | kronello_model::AssetKind::Video
            ) {
                return Err(MediaError::InvalidInput(
                    "audio clip references non-audio asset".into(),
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Video,
    Audio,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaStream {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    pub time_base: Rational,
    pub start: Option<Rational>,
    pub duration: Option<Rational>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaProbe {
    pub streams: Vec<MediaStream>,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
}
impl MediaProbe {
    /// Require zero-origin ProRes + stereo 48 kHz PCM24, within one audio sample.
    pub fn verify_av(&self) -> Result<(), MediaError> {
        self.verify_movie(MovieProfile::ProResPcm24)
    }
    pub fn verify_movie(&self, profile: MovieProfile) -> Result<(), MediaError> {
        if self.streams.len() != 2 {
            return Err(MediaError::Encode(
                "expected exactly two output streams".into(),
            ));
        }
        let video = self
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Video)
            .ok_or_else(|| MediaError::Encode("missing video stream".into()))?;
        let audio = self
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .ok_or_else(|| MediaError::Encode("missing audio stream".into()))?;
        let (video_codec, audio_codec) = profile.codecs();
        if video.codec != video_codec
            || audio.codec != audio_codec
            || audio.sample_rate != Some(48_000)
            || audio.channels != Some(2)
            || video.start != Some(Rational::ZERO)
            || audio.start != Some(Rational::ZERO)
        {
            return Err(MediaError::Encode(
                "invalid A/V codec, sample format or start PTS".into(),
            ));
        }
        let video_duration = video
            .duration
            .ok_or_else(|| MediaError::Encode("missing video duration".into()))?;
        let audio_duration = audio
            .duration
            .ok_or_else(|| MediaError::Encode("missing audio duration".into()))?;
        let difference = video_duration.checked_sub(audio_duration)?;
        let tolerance = Rational::new(1, 48_000)?;
        if video_duration <= Rational::ZERO
            || audio_duration <= Rational::ZERO
            || difference >= tolerance
            || difference <= tolerance.checked_neg()?
        {
            return Err(MediaError::Encode(
                "A/V durations differ by at least one sample".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AvExportRequest {
    pub output: PathBuf,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub region: OutputRegion,
    /// Explicit opaque background, linear Rec.709 SDR, before BT.709 encoding.
    pub background: [f32; 3],
    pub clipping: ClippingPolicy,
}
/// Logical I/O observed after successful operations. Stage byte lengths come
/// from file metadata; they are not physical device I/O or process RSS estimates.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StreamingExportReport {
    pub audio_spool_write_bytes: u64,
    pub audio_window_read_bytes: u64,
    pub audio_stage_bytes: u64,
    pub video_stage_bytes: u64,
    pub published_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AvExportReport {
    pub schema_version: u32,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
    pub audio_render_snapshot_hash: String,
    #[serde(default)]
    pub audio_source: AudioSourceMode,
    #[serde(default = "legacy_audio_profile")]
    pub audio_profile_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movie_profile: Option<MovieProfile>,
    pub request: AvExportRequest,
    pub sample_range: std::ops::Range<i64>,
    pub frames: Vec<FrameMetadata>,
    pub video: MediaPathReport,
    pub audio: AudioEncodeReport,
    pub probe: MediaProbe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<StreamingExportReport>,
}
impl MediaRuntime {
    /// Read the stream's container codec tag (little-endian FourCC when textual).
    pub fn probe_codec_tag(&self, path: &Path, stream_index: u32) -> Result<u32, MediaError> {
        self.native.probe_codec_tag(path, stream_index)
    }
    pub fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput("expected local file".into()));
        }
        self.native.probe(&path)
    }
    /// Packet-copy the selected codecs with rational PTS; publish only after probe.
    pub fn mux_av(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
    ) -> Result<MediaProbe, MediaError> {
        self.mux_movie(
            video,
            audio,
            output,
            render_hash,
            export_hash,
            MovieProfile::ProResPcm24,
        )
    }
    #[allow(clippy::too_many_arguments)] // Explicit codec contract accompanies both snapshot identities.
    pub fn mux_movie(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
    ) -> Result<MediaProbe, MediaError> {
        for hash in [render_hash, export_hash] {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(MediaError::InvalidInput(
                    "expected SHA-256 snapshot identity".into(),
                ));
            }
        }
        let video = video.canonicalize()?;
        let audio = audio.canonicalize()?;
        let video_input = self.probe(&video)?;
        let audio_input = self.probe(&audio)?;
        let video_stream = video_input
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Video)
            .ok_or_else(|| MediaError::InvalidInput("mux input has no video".into()))?;
        let audio_stream = audio_input
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .ok_or_else(|| MediaError::InvalidInput("mux input has no audio".into()))?;
        MediaProbe {
            streams: vec![video_stream.clone(), audio_stream.clone()],
            render_snapshot_hash: String::new(),
            export_snapshot_hash: String::new(),
        }
        .verify_movie(profile)?;
        let temp = stage_file(output)?;
        self.native.mux_av(
            &video,
            &audio,
            temp.path(),
            render_hash,
            export_hash,
            profile,
        )?;
        let probe = self.probe(temp.path())?;
        probe.verify_movie(profile)?;
        if profile == MovieProfile::HevcAlacV1 {
            let video = probe
                .streams
                .iter()
                .find(|s| s.kind == StreamKind::Video)
                .unwrap();
            if self.probe_codec_tag(temp.path(), video.index)? != u32::from_le_bytes(*b"hvc1") {
                return Err(MediaError::Encode(
                    "HEVC delivery requires hvc1 sample entry".into(),
                ));
            }
        }
        for input in [video_stream, audio_stream] {
            let output_stream = probe
                .streams
                .iter()
                .find(|s| s.kind == input.kind)
                .ok_or_else(|| MediaError::Encode("mux lost stream".into()))?;
            if output_stream.duration != input.duration || output_stream.start != input.start {
                return Err(MediaError::Encode("mux changed exact stream timing".into()));
            }
        }
        if probe.render_snapshot_hash != render_hash || probe.export_snapshot_hash != export_hash {
            return Err(MediaError::Encode(
                "muxed snapshot identity mismatch".into(),
            ));
        }
        temp.as_file().sync_all()?;
        publish_file(temp, output)?;
        Ok(probe)
    }
    /// Render and mix only the owned fixed snapshot. No latest-project lookup,
    /// implicit backend fallback, codec substitution or shell subprocess.
    pub fn export_av(
        &self,
        snapshot: &AvExportSnapshot,
        project_path: &Path,
        fonts: &[FontData<'_>],
        backend: &dyn RenderBackend,
        request: &AvExportRequest,
    ) -> Result<AvExportReport, MediaError> {
        self.export_av_with_checkpoint(snapshot, project_path, fonts, backend, request, &mut |_| {
            Ok(())
        })
    }
    /// Cooperatively cancel at frame boundaries and before final muxing.
    pub fn export_av_with_checkpoint(
        &self,
        snapshot: &AvExportSnapshot,
        project_path: &Path,
        fonts: &[FontData<'_>],
        backend: &dyn RenderBackend,
        request: &AvExportRequest,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<AvExportReport, MediaError> {
        checkpoint(0)?;
        snapshot.validate()?;
        let profile = snapshot.movie_profile();
        if profile != MovieProfile::ProResPcm24 {
            let encoder = self.capabilities.select_encoder(profile.video_codec())?;
            if profile == MovieProfile::Av1Mp4AlacV1 && encoder.name != "libsvtav1" {
                return Err(MediaError::EncoderUnavailable {
                    encoder: "libsvtav1".into(),
                    reason: "AV1 MP4 version 1 requires SVT-AV1".into(),
                    ffmpeg: None,
                });
            }
        }
        request.region.validate()?;
        if snapshot.render.profile().working_space != ColorSpace::LinearRec709 {
            return Err(MediaError::UnsupportedFeature(
                "A/V export requires SDR linear Rec.709".into(),
            ));
        }
        if request
            .background
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(MediaError::InvalidInput(
                "background must be finite linear Rec.709 SDR".into(),
            ));
        }
        for time in [request.range.start(), request.range.end()] {
            if request.frame_rate.time_to_frame(time)?.denominator() != 1 {
                return Err(MediaError::InvalidInput(
                    "export range must use exact frame boundaries".into(),
                ));
            }
        }
        let times = frame_samples(request.range, request.frame_rate)?;
        if times.is_empty() {
            return Err(MediaError::InvalidInput("empty export".into()));
        }
        if request.output.exists() {
            return Err(MediaError::OutputExists(request.output.clone()));
        }
        let render_hash = snapshot.render.content_hash()?;
        let export_hash = snapshot.content_hash()?;
        let parent = request
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Every intermediate is on the destination volume and owned by RAII.
        let stage = tempfile::tempdir_in(parent)?;
        let video_file = stage.path().join("video.mov");
        let audio_file = stage.path().join("audio.mov");
        let mut sources = crate::streaming::SpoolSources::new();
        for clip in &snapshot.clips {
            if !sources.contains(&(clip.asset, clip.stream_index)) {
                let asset = snapshot
                    .render
                    .project()
                    .assets
                    .iter()
                    .find_map(|a| match a {
                        DocumentObject::Known(a) if a.id == clip.asset => Some(a),
                        _ => None,
                    })
                    .ok_or_else(|| MediaError::AssetMissing(clip.asset.to_string()))?;
                sources.decode(
                    self,
                    asset,
                    project_path,
                    clip.stream_index,
                    stage.path(),
                    checkpoint,
                )?;
            }
        }
        let plan = if snapshot.audio == AudioSourceMode::Document {
            Some(document_plan(&snapshot.render, snapshot.schema_version)?)
        } else {
            None
        };
        let samples = sample_range(request.range)?;
        if samples.is_empty() {
            return Err(MediaError::InvalidInput("empty audio output".into()));
        }
        let mut audio_encoder = crate::ffi::NativeAudioEncoder::open(
            &self.native,
            &audio_file,
            profile != MovieProfile::ProResPcm24,
        )?;
        let mut clipped_samples = 0_usize;
        let mut start = samples.start;
        while start < samples.end {
            checkpoint(0)?;
            let end = samples.end.min(
                start
                    .checked_add(audio_encoder.block as i64)
                    .ok_or_else(|| MediaError::InvalidInput("audio batch overflow".into()))?,
            );
            let range = TimeRange::new(
                SAMPLE_RATE.sample_to_time(start)?,
                SAMPLE_RATE.sample_to_time(end)?,
            )?;
            let bus = match &plan {
                Some(plan) => plan.mix_reader(&sources, range)?,
                None => mix_reader(&snapshot.clips, &sources, range)?,
            };
            let quantized = bus.quantize_pcm24(request.clipping)?;
            clipped_samples = clipped_samples
                .checked_add(quantized.clipped_samples)
                .ok_or_else(|| MediaError::InvalidInput("clipping count overflow".into()))?;
            audio_encoder.frame(&quantized.samples)?;
            start = end;
        }
        audio_encoder.finish()?;
        drop(audio_encoder);
        let audio_spool_write_bytes = sources.decoded_bytes;
        let audio_window_read_bytes = sources.read_bytes();
        drop(sources);
        let audio = AudioEncodeReport {
            codec: if profile == MovieProfile::ProResPcm24 {
                "pcm_s24le"
            } else {
                "alac"
            }
            .into(),
            sample_rate: 48_000,
            channels: 2,
            frames: usize::try_from(samples.end - samples.start)
                .map_err(|_| MediaError::InvalidInput("audio count overflow".into()))?,
            clipped_samples,
        };
        let encode_request = EncodeRequest {
            output: video_file.clone(),
            codec: profile.video_codec(),
            width: request.region.pixels[0],
            height: request.region.pixels[1],
            time_base: request.frame_rate.frame_to_time(1)?,
        };
        let mut metadata = Vec::new();
        let video = self.encode_video_stream(&encode_request, times.len(), &mut |number| {
            checkpoint(number as u64)?;
            let (index, time) = times[number];
            let mut rgba =
                vec![
                    0_u8;
                    request.region.pixels[0] as usize * request.region.pixels[1] as usize * 4
                ];
            let mut frame_metadata = render_frame_tiles(
                &snapshot.render,
                fonts,
                backend,
                FrameRequest {
                    time,
                    region: request.region,
                },
                &mut |[x, y], tile, output| {
                    // The sole full-frame buffer is RGBA8. No full-frame linear
                    // or display surface is allocated by this movie path.
                    let bytes = bt709_rgba(&output.linear, request.background).map_err(|e| {
                        kronello_render::RenderError::UnsupportedFeature(e.to_string())
                    })?;
                    for row in 0..tile.pixels[1] as usize {
                        let destination = ((y as usize + row) * request.region.pixels[0] as usize
                            + x as usize)
                            * 4;
                        let source = row * tile.pixels[0] as usize * 4;
                        let width = tile.pixels[0] as usize * 4;
                        rgba[destination..destination + width]
                            .copy_from_slice(&bytes[source..source + width]);
                    }
                    Ok(())
                },
            )?;
            if frame_metadata.snapshot_content_hash != render_hash {
                return Err(MediaError::Encode(
                    "video snapshot identity mismatch".into(),
                ));
            }
            frame_metadata.frame_index = Some(index.to_string());
            frame_metadata.sequence_number = Some(number as u64);
            metadata.push(frame_metadata);
            Ok(crate::EncodeFrame {
                pts: time.checked_sub(request.range.start())?,
                rgba,
            })
        })?;
        let audio_stage_bytes = std::fs::metadata(&audio_file)?.len();
        let video_stage_bytes = std::fs::metadata(&video_file)?.len();
        let video_probe = self.probe(&video_file)?;
        let audio_probe = self.probe(&audio_file)?;
        let expected_video = request.range.end().checked_sub(request.range.start())?;
        let expected_audio = SAMPLE_RATE.sample_to_time(audio.frames as i64)?;
        if !video_probe
            .streams
            .iter()
            .any(|s| s.kind == StreamKind::Video && s.duration == Some(expected_video))
            || !audio_probe
                .streams
                .iter()
                .any(|s| s.kind == StreamKind::Audio && s.duration == Some(expected_audio))
        {
            return Err(MediaError::Encode(format!(
                "encoded streams differ from requested duration: video {:?} expected {expected_video:?}, audio {:?} expected {expected_audio:?}",
                video_probe.streams, audio_probe.streams
            )));
        }
        checkpoint(metadata.len() as u64)?;
        let probe = self.mux_movie(
            &video_file,
            &audio_file,
            &request.output,
            &render_hash,
            &export_hash,
            profile,
        )?;
        Ok(AvExportReport {
            schema_version: 1,
            render_snapshot_hash: render_hash.clone(),
            export_snapshot_hash: export_hash,
            audio_render_snapshot_hash: render_hash,
            audio_source: snapshot.audio,
            audio_profile_version: snapshot.schema_version,
            movie_profile: snapshot.movie_profile,
            request: request.clone(),
            sample_range: sample_range(request.range)?,
            frames: metadata,
            video,
            audio,
            probe,
            streaming: Some(StreamingExportReport {
                audio_spool_write_bytes,
                audio_window_read_bytes,
                audio_stage_bytes,
                video_stage_bytes,
                published_bytes: std::fs::metadata(&request.output).ok().map(|m| m.len()),
            }),
        })
    }
}
fn bt709_rgba(pixels: &[[f32; 4]], background: [f32; 3]) -> Result<Vec<u8>, MediaError> {
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        for channel in 0..3 {
            let value = pixel[channel] + background[channel] * (1.0 - pixel[3]);
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(MediaError::UnsupportedFeature(
                    "SDR export cannot encode out-of-gamut/HDR samples".into(),
                ));
            }
            let encoded = if value < 0.018 {
                value * 4.5
            } else {
                1.099 * value.powf(0.45) - 0.099
            };
            rgba.push((encoded * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        rgba.push(255);
    }
    Ok(rgba)
}

fn is_explicit(mode: &AudioSourceMode) -> bool {
    *mode == AudioSourceMode::Explicit
}
fn document_plan(render: &RenderSnapshot, profile: u32) -> Result<DocumentAudioPlan, MediaError> {
    let target = match render.target() {
        kronello_render::RenderTarget::Composition { composition } => {
            AudioTarget::Composition(composition)
        }
        kronello_render::RenderTarget::Sequence { sequence } => AudioTarget::Sequence(sequence),
    };
    Ok(DocumentAudioPlan::compile_version(
        render.project(),
        target,
        if profile == 3 { 2 } else { 1 },
    )?)
}

fn legacy_audio_profile() -> u32 {
    1
}
