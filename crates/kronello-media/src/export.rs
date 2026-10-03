use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kronello_audio::{AudioClip, AudioSources, ClippingPolicy, SAMPLE_RATE, mix, sample_range};
use kronello_model::{ColorSpace, DocumentObject};
use kronello_render::{
    FrameMetadata, FrameRequest, OutputRegion, RenderBackend, RenderSnapshot, frame_samples,
    render_frame,
};
use kronello_text::FontData;
use kronello_time::{FrameRate, Rational, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::audio::{publish_file, stage_file};
use crate::{
    AudioEncodeReport, EncodeCodec, EncodeFrame, EncodeRequest, MediaError, MediaPathReport,
    MediaRuntime,
};

/// AUDIO-000 export envelope. RenderSnapshot schema remains unchanged. Audio
/// placements are explicit until the shared document gains its timeline model.
/// Both inputs are owned, immutable through this API, and hashed together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvExportSnapshot {
    schema_version: u32,
    render: RenderSnapshot,
    clips: Vec<AudioClip>,
}
impl AvExportSnapshot {
    pub fn new(render: &RenderSnapshot, clips: Vec<AudioClip>) -> Result<Self, MediaError> {
        let snapshot = Self {
            schema_version: 1,
            render: render.clone(),
            clips,
        };
        snapshot.validate()?;
        Ok(snapshot)
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
        if self.schema_version != 1 {
            return Err(MediaError::UnsupportedFeature(
                "audio export snapshot schema".into(),
            ));
        }
        self.render.validate()?;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Video,
    Audio,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaProbe {
    pub streams: Vec<MediaStream>,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
}
impl MediaProbe {
    /// Require zero-origin ProRes + stereo 48 kHz PCM24, within one audio sample.
    pub fn verify_av(&self) -> Result<(), MediaError> {
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
        if video.codec != "prores"
            || audio.codec != "pcm_s24le"
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvExportRequest {
    pub output: PathBuf,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub region: OutputRegion,
    /// Explicit opaque background, linear Rec.709 SDR, before BT.709 encoding.
    pub background: [f32; 3],
    pub clipping: ClippingPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvExportReport {
    pub schema_version: u32,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
    pub audio_render_snapshot_hash: String,
    pub request: AvExportRequest,
    pub sample_range: std::ops::Range<i64>,
    pub frames: Vec<FrameMetadata>,
    pub video: MediaPathReport,
    pub audio: AudioEncodeReport,
    pub probe: MediaProbe,
}
impl MediaRuntime {
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
        .verify_av()?;
        let temp = stage_file(output)?;
        self.native
            .mux_av(&video, &audio, temp.path(), render_hash, export_hash)?;
        let probe = self.probe(temp.path())?;
        probe.verify_av()?;
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
        snapshot.validate()?;
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
        let size = u64::from(request.region.pixels[0]) * u64::from(request.region.pixels[1]) * 4;
        if times.is_empty()
            || size
                .checked_mul(times.len() as u64)
                .is_none_or(|v| v > 256 * 1024 * 1024)
        {
            return Err(MediaError::InvalidInput(
                "empty export or video payload budget exceeded".into(),
            ));
        }
        if request.output.exists() {
            return Err(MediaError::OutputExists(request.output.clone()));
        }
        let render_hash = snapshot.render.content_hash()?;
        let export_hash = snapshot.content_hash()?;
        let mut sources: AudioSources = BTreeMap::new();
        let mut source_frames = 0_usize;
        for clip in &snapshot.clips {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                sources.entry((clip.asset, clip.stream_index))
            {
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
                let decoded = self
                    .decode_asset_audio(asset, project_path, clip.stream_index)?
                    .buffer;
                source_frames = source_frames
                    .checked_add(decoded.frames().len())
                    .filter(|v| *v <= kronello_audio::MAX_AUDIO_FRAMES)
                    .ok_or_else(|| {
                        MediaError::InvalidInput("aggregate audio source budget exceeded".into())
                    })?;
                entry.insert(decoded);
            }
        }
        let bus = mix(&snapshot.clips, &sources, request.range)?;
        // Reject clipping before expensive frame rendering; explicit Saturate
        // still records every changed channel sample in the encode report.
        bus.quantize_pcm24(request.clipping)?;
        let mut frames = Vec::new();
        let mut metadata = Vec::new();
        for (index, time) in times {
            let mut rendered = render_frame(
                &snapshot.render,
                fonts,
                backend,
                FrameRequest {
                    time,
                    region: request.region,
                },
            )?;
            if rendered.metadata.snapshot_content_hash != render_hash {
                return Err(MediaError::Encode(
                    "video snapshot identity mismatch".into(),
                ));
            }
            let rgba = bt709_rgba(&rendered.pixels.linear, request.background)?;
            rendered.metadata.frame_index = Some(index.to_string());
            rendered.metadata.sequence_number = Some(frames.len() as u64);
            metadata.push(rendered.metadata);
            frames.push(EncodeFrame {
                pts: time.checked_sub(request.range.start())?,
                rgba,
            });
        }
        let parent = request
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stage = tempfile::tempdir_in(parent)?;
        let video_file = stage.path().join("video.mov");
        let audio_file = stage.path().join("audio.mov");
        let video = self.encode_video(
            &EncodeRequest {
                output: video_file.clone(),
                codec: EncodeCodec::ProRes,
                width: request.region.pixels[0],
                height: request.region.pixels[1],
                time_base: request.frame_rate.frame_to_time(1)?,
            },
            &frames,
        )?;
        let audio = self.encode_audio(&bus, request.clipping, &audio_file)?;
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
        let probe = self.mux_av(
            &video_file,
            &audio_file,
            &request.output,
            &render_hash,
            &export_hash,
        )?;
        Ok(AvExportReport {
            schema_version: 1,
            render_snapshot_hash: render_hash.clone(),
            export_snapshot_hash: export_hash,
            audio_render_snapshot_hash: render_hash,
            request: request.clone(),
            sample_range: sample_range(request.range)?,
            frames: metadata,
            video,
            audio,
            probe,
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
