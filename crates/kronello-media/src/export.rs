use std::path::{Path, PathBuf};

use kronello_audio::{
    AudioClip, AudioSourceMode, AudioTarget, ClippingPolicy, DocumentAudioPlan, SAMPLE_RATE,
    sample_range,
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
    ProResPqPcm24V1,
    ProResHlgPcm24V1,
    ProResSdrFromHdrPcm24V1,
    H264AacV1,
    HevcAacV1,
    Av1Mp4AacV1,
    Av1WebmOpusV1,
}
impl MovieProfile {
    pub fn hdr_transfer(self) -> Option<kronello_render::HdrTransfer> {
        match self {
            Self::ProResPqPcm24V1 => Some(kronello_render::HdrTransfer::Pq),
            Self::ProResHlgPcm24V1 => Some(kronello_render::HdrTransfer::Hlg),
            _ => None,
        }
    }
    pub fn is_prores(self) -> bool {
        self.video_codec() == EncodeCodec::ProRes
    }
    pub fn video_codec(self) -> EncodeCodec {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => EncodeCodec::ProRes,
            Self::Av1Mp4AlacV1 | Self::Av1Mp4AacV1 | Self::Av1WebmOpusV1 => EncodeCodec::Av1,
            Self::H264AlacV1 | Self::H264AacV1 => EncodeCodec::H264,
            Self::HevcAlacV1 | Self::HevcAacV1 => EncodeCodec::Hevc,
        }
    }
    /// Closed delivery audio codec for this profile's intermediate stage.
    pub(crate) fn audio_kind(self) -> crate::ffi::AudioEncoderKind {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => crate::ffi::AudioEncoderKind::Pcm24,
            Self::Av1Mp4AlacV1 | Self::H264AlacV1 | Self::HevcAlacV1 => {
                crate::ffi::AudioEncoderKind::Alac
            }
            Self::H264AacV1 | Self::HevcAacV1 | Self::Av1Mp4AacV1 => {
                crate::ffi::AudioEncoderKind::Aac
            }
            Self::Av1WebmOpusV1 => crate::ffi::AudioEncoderKind::Opus,
        }
    }
    /// Container suffix the destination path must use.
    pub fn container(self) -> &'static str {
        match self {
            Self::Av1Mp4AlacV1 | Self::Av1Mp4AacV1 => "mp4",
            Self::Av1WebmOpusV1 => "webm",
            _ => "mov",
        }
    }
    /// Lossy codecs signal encoder delay/padding in container metadata; the
    /// stream duration may exceed the input by up to one codec frame.
    fn audio_frame_slack(self) -> i64 {
        match self {
            Self::H264AacV1 | Self::HevcAacV1 | Self::Av1Mp4AacV1 => 1024,
            Self::Av1WebmOpusV1 => 960,
            _ => 0,
        }
    }
    pub(crate) fn native_id(self) -> i32 {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => 0,
            Self::Av1Mp4AlacV1 => 1,
            Self::H264AlacV1 => 2,
            Self::HevcAlacV1 => 3,
            Self::H264AacV1 => 4,
            Self::HevcAacV1 => 5,
            Self::Av1Mp4AacV1 => 6,
            Self::Av1WebmOpusV1 => 7,
        }
    }
    fn codecs(self) -> (&'static str, &'static str) {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => ("prores", "pcm_s24le"),
            Self::Av1Mp4AlacV1 => ("av1", "alac"),
            Self::H264AlacV1 => ("h264", "alac"),
            Self::HevcAlacV1 => ("hevc", "alac"),
            Self::H264AacV1 => ("h264", "aac"),
            Self::HevcAacV1 => ("hevc", "aac"),
            Self::Av1Mp4AacV1 => ("av1", "aac"),
            Self::Av1WebmOpusV1 => ("av1", "opus"),
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
    /// AUDIO-010: the declared output speaker layout. Stereo is the default
    /// and keeps the pre-AUDIO-010 envelope bytes identical.
    #[serde(
        default = "stereo_audio_layout",
        skip_serializing_if = "is_stereo_layout"
    )]
    audio_layout: kronello_model::ChannelMask,
}
impl AvExportSnapshot {
    pub fn new(render: &RenderSnapshot, clips: Vec<AudioClip>) -> Result<Self, MediaError> {
        let snapshot = Self {
            schema_version: 1,
            render: render.clone(),
            clips,
            audio: AudioSourceMode::Explicit,
            movie_profile: None,
            audio_layout: kronello_model::ChannelMask::STEREO,
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
        Self::with_audio_movie_profile(render, audio, clips, profile, None)
    }
    /// AUDIO-010: an explicit non-stereo output layout. It pins audio
    /// envelope 3 (evaluator 2) because pitch-preserved retime and
    /// multichannel mixing are defined there. `movie_profile == None` selects
    /// the PCM24 MOV audio stage; `Some(profile)` selects its codec.
    pub fn with_audio_layout(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
        movie_profile: Option<MovieProfile>,
        audio_layout: kronello_model::ChannelMask,
    ) -> Result<Self, MediaError> {
        if audio_layout == kronello_model::ChannelMask::STEREO {
            return Err(MediaError::InvalidInput(
                "stereo exports use the standard audio constructors".into(),
            ));
        }
        Self::with_audio_movie_layout(render, audio, clips, 3, movie_profile, audio_layout)
    }
    fn with_audio_movie_profile(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
        profile: u32,
        movie_profile: Option<MovieProfile>,
    ) -> Result<Self, MediaError> {
        Self::with_audio_movie_layout(
            render,
            audio,
            clips,
            profile,
            movie_profile,
            kronello_model::ChannelMask::STEREO,
        )
    }
    fn with_audio_movie_layout(
        render: &RenderSnapshot,
        audio: AudioSourceMode,
        clips: Vec<AudioClip>,
        profile: u32,
        movie_profile: Option<MovieProfile>,
        audio_layout: kronello_model::ChannelMask,
    ) -> Result<Self, MediaError> {
        if !matches!(profile, 2 | 3) {
            return Err(MediaError::UnsupportedFeature(
                "audio profile version".into(),
            ));
        }
        if audio_layout != kronello_model::ChannelMask::STEREO && profile != 3 {
            return Err(MediaError::UnsupportedFeature(
                "multichannel audio requires audio envelope 3".into(),
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
            movie_profile,
            audio_layout,
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
        Self::with_audio_movie_profile(render, audio, clips, 3, Some(profile))
    }
    pub fn movie_profile(&self) -> MovieProfile {
        self.movie_profile.unwrap_or_default()
    }
    pub fn audio(&self) -> AudioSourceMode {
        self.audio
    }
    /// The declared output speaker layout (ADR-0124).
    pub fn audio_layout(&self) -> kronello_model::ChannelMask {
        self.audio_layout
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
        // Exports decode authored originals only (ADR-0119).
        if self.render.media_proxies() != kronello_render::MediaProxyMode::Off {
            return Err(MediaError::UnsupportedFeature(
                "export snapshots cannot substitute preview proxies".into(),
            ));
        }
        if self.movie_profile.is_some_and(|p| {
            (p == MovieProfile::ProResPcm24
                && self.audio_layout == kronello_model::ChannelMask::STEREO)
                || self.schema_version != 3
        }) {
            return Err(MediaError::UnsupportedFeature(
                "delivery profiles require audio envelope 3".into(),
            ));
        }
        // Multichannel output is defined only on audio envelope 3; stereo
        // envelopes keep their exact pre-AUDIO-010 semantics.
        if self.audio_layout != kronello_model::ChannelMask::STEREO && self.schema_version != 3 {
            return Err(MediaError::UnsupportedFeature(
                "multichannel audio requires audio envelope 3".into(),
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
        if (self.movie_profile() == MovieProfile::ProResSdrFromHdrPcm24V1
            && self.render.profile().hdr.is_none())
            || (self.movie_profile() != MovieProfile::ProResSdrFromHdrPcm24V1
                && self.movie_profile().hdr_transfer()
                    != self.render.profile().hdr.map(|h| h.transfer))
        {
            return Err(MediaError::UnsupportedFeature("movie HDR profile must match the fixed render HDR transfer; explicit SDR conversion required".into()));
        }
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
    /// Native speaker-mask bits when the container records one (ADR-0124).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_mask: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_primaries: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_transfer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_matrix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_range: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaProbe {
    pub streams: Vec<MediaStream>,
    /// Container-level duration; the only duration WebM publishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Rational>,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
}
impl MediaProbe {
    /// Require zero-origin ProRes + stereo 48 kHz PCM24, within one audio sample.
    pub fn verify_av(&self) -> Result<(), MediaError> {
        self.verify_movie(MovieProfile::ProResPcm24)
    }
    pub fn verify_movie(&self, profile: MovieProfile) -> Result<(), MediaError> {
        self.verify_movie_layout(profile, kronello_model::ChannelMask::STEREO)
    }
    /// Verify at the explicitly declared audio layout (ADR-0124): the probed
    /// stream must carry exactly `audio_layout`'s channel count, and a
    /// reported native mask must belong to the closed layout set — lossless
    /// profiles (PCM24/ALAC) must round-trip the exact mask.
    pub fn verify_movie_layout(
        &self,
        profile: MovieProfile,
        audio_layout: kronello_model::ChannelMask,
    ) -> Result<(), MediaError> {
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
            || audio.channels != Some(audio_layout.channels() as u32)
            || video.start != Some(Rational::ZERO)
            || audio.start != Some(Rational::ZERO)
        {
            return Err(MediaError::Encode(
                "invalid A/V codec, sample format, channel layout or start PTS".into(),
            ));
        }
        if let Some(mask) = audio.channel_mask {
            let parsed = kronello_model::ChannelMask::from_bits(mask).ok();
            // Lossless containers preserve the exact mask; lossy codecs may
            // normalize side/back surround naming but must still describe a
            // member of the closed set for the same channel count.
            let lossless = matches!(audio_codec, "pcm_s24le" | "alac");
            let ok = match parsed {
                Some(m) if lossless => m == audio_layout,
                Some(m) => m.channels() as u32 == audio.channels.unwrap_or(0),
                None => false,
            };
            if !ok {
                return Err(MediaError::Encode(
                    "muxed audio layout does not match the declared channel mask".into(),
                ));
            }
        }
        if let Some(transfer) = profile.hdr_transfer()
            && (video.pixel_format.as_deref() != Some("yuv422p10le")
                || video.color_primaries.as_deref() != Some("bt2020")
                || video.color_transfer.as_deref() != Some(transfer.tag())
                || video.color_matrix.as_deref() != Some("bt2020nc")
                || video.color_range.as_deref() != Some("tv"))
        {
            return Err(MediaError::Encode(format!(
                "HDR codec/10-bit/color metadata roundtrip mismatch: expected yuv422p10le/bt2020/{}/bt2020nc/tv; observed pixel_format={:?}, primaries={:?}, transfer={:?}, matrix={:?}, range={:?}",
                transfer.tag(),
                video.pixel_format,
                video.color_primaries,
                video.color_transfer,
                video.color_matrix,
                video.color_range,
            )));
        }
        let slack = profile.audio_frame_slack();
        let video_duration = video
            .duration
            // WebM publishes only the container duration; lossy profiles accept it.
            .or_else(|| (slack > 0).then_some(self.duration).flatten())
            .ok_or_else(|| MediaError::Encode("missing video duration".into()))?;
        if video_duration <= Rational::ZERO {
            return Err(MediaError::Encode("non-positive video duration".into()));
        }
        let audio_duration = match audio.duration {
            // WebM does not always publish a stream duration for lossy audio;
            // exact decode length is verified separately on the lossy path.
            None if slack > 0 => video_duration,
            other => other.ok_or_else(|| MediaError::Encode("missing audio duration".into()))?,
        };
        let difference = video_duration.checked_sub(audio_duration)?;
        let tolerance = Rational::new(1 + slack, 48_000)?;
        if audio_duration <= Rational::ZERO
            || difference >= tolerance
            || difference <= tolerance.checked_neg()?
        {
            return Err(MediaError::Encode(
                "A/V durations differ beyond the profile audio frame".into(),
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
    /// Explicit opaque background in the fixed profile working space (HDR 1=203 nits).
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
    /// Camera RAW containers are probed without FFmpeg (ADR-0136): detection
    /// owns the file before any generic demuxer sees it, BRAW/R3D report a
    /// typed vendor-SDK rejection, and ProRes RAW reports its QuickTime
    /// metadata with the rgba64h contract. Registration callers can lock the
    /// returned stream fields directly into `StreamMetadata`.
    pub fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput("expected local file".into()));
        }
        if let Some(detection) = crate::raw::sniff_camera_raw(&path)? {
            return crate::raw::probe_camera_raw(&path, detection);
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
            kronello_model::ChannelMask::STEREO,
        )
    }
    /// `audio_layout` is the declared contract of the audio intermediate:
    /// the mux fails when the file's channel count differs (ADR-0124).
    #[allow(clippy::too_many_arguments)] // Explicit codec contract accompanies both snapshot identities.
    pub fn mux_movie(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
        audio_layout: kronello_model::ChannelMask,
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
            duration: None,
            render_snapshot_hash: String::new(),
            export_snapshot_hash: String::new(),
        }
        .verify_movie_layout(profile, audio_layout)?;
        let temp = stage_file(output)?;
        self.native.mux_av(
            &video,
            &audio,
            temp.path(),
            render_hash,
            export_hash,
            profile,
            audio_layout.channels() as u32,
        )?;
        let probe = self.probe(temp.path())?;
        probe.verify_movie_layout(profile, audio_layout)?;
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
        let frame_slack = Rational::new(profile.audio_frame_slack(), 48_000)?;
        for input in [video_stream, audio_stream] {
            let output_stream = probe
                .streams
                .iter()
                .find(|s| s.kind == input.kind)
                .ok_or_else(|| MediaError::Encode("mux lost stream".into()))?;
            let duration_ok = match (output_stream.duration, input.duration) {
                (Some(out), Some(want)) if out == want => true,
                _ if frame_slack > Rational::ZERO => match output_stream.duration {
                    // Lossy containers may omit the stream duration or pad by
                    // up to one codec frame; decoded length is checked exactly.
                    None => true,
                    Some(out) => {
                        let lo = input
                            .duration
                            .map(|d| {
                                d.checked_sub(frame_slack)
                                    .map(|lo| out >= lo)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(true);
                        let hi = input
                            .duration
                            .map(|d| {
                                d.checked_add(frame_slack)
                                    .map(|hi| out <= hi)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(true);
                        lo && hi
                    }
                },
                _ => false,
            };
            if !duration_ok || output_stream.start != input.start {
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
        if snapshot
            .render
            .profile()
            .temporal
            .as_ref()
            .is_some_and(|t| t.frame_rate != request.frame_rate)
        {
            return Err(MediaError::InvalidInput(
                "temporal frame rate must match movie export frame rate".into(),
            ));
        }
        let profile = snapshot.movie_profile();
        if !profile.is_prores() {
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
        if snapshot.render.profile().working_space
            != if profile.hdr_transfer().is_some()
                || profile == MovieProfile::ProResSdrFromHdrPcm24V1
            {
                ColorSpace::LinearRec2020
            } else {
                ColorSpace::LinearRec709
            }
        {
            return Err(MediaError::UnsupportedFeature(
                "A/V working space must match explicit SDR/HDR movie profile".into(),
            ));
        }
        let background_peak = snapshot
            .render
            .profile()
            .hdr
            .map_or(1.0, |h| match h.transfer {
                kronello_render::HdrTransfer::Pq => 10000.0 / 203.0,
                kronello_render::HdrTransfer::Hlg => 1000.0 / 203.0,
            });
        if request
            .background
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=background_peak).contains(v))
        {
            return Err(MediaError::InvalidInput(
                "background must be finite nonnegative linear RGB within the explicit transfer range".into(),
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
        let audio_kind = profile.audio_kind();
        // ADR-0124: the snapshot declares the single output layout; the
        // bus converts sources deterministically and never folds down.
        let audio_layout = snapshot.audio_layout;
        let audio_file = stage.path().join(match audio_kind {
            crate::ffi::AudioEncoderKind::Opus => "audio.webm",
            _ => "audio.mov",
        });
        let mut audio_encoder = crate::ffi::NativeAudioEncoder::open(
            &self.native,
            &audio_file,
            audio_kind,
            audio_layout,
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
                Some(plan) => plan.mix_channels(&sources, range, audio_layout)?,
                None => {
                    kronello_audio::mix_channels(&snapshot.clips, &sources, range, audio_layout)?
                }
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
            codec: profile.codecs().1.into(),
            sample_rate: 48_000,
            channels: audio_layout.channels() as u32,
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
        let stride = if profile.hdr_transfer().is_some() {
            8
        } else {
            4
        };
        let mut produce = |number: usize| {
            checkpoint(number as u64)?;
            let (index, time) = times[number];
            let mut rgba =
                vec![
                    0_u8;
                    request.region.pixels[0] as usize * request.region.pixels[1] as usize * stride
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
                    // The sole full-frame buffer is RGBA8 or HDR RGBA64. No full-frame linear
                    // or display surface is allocated by this movie path.
                    let bytes = if let Some(transfer) = profile.hdr_transfer() {
                        hdr_rgba(&output.linear, request.background, transfer)
                    } else if profile == MovieProfile::ProResSdrFromHdrPcm24V1 {
                        let mapped: Vec<_> = output
                            .linear
                            .iter()
                            .map(|p| {
                                let opaque = [
                                    p[0] + request.background[0] * (1.0 - p[3]),
                                    p[1] + request.background[1] * (1.0 - p[3]),
                                    p[2] + request.background[2] * (1.0 - p[3]),
                                    1.0,
                                ];
                                kronello_render::hdr_to_sdr_linear(opaque)
                            })
                            .collect();
                        bt709_rgba(&mapped, [0.0; 3])
                    } else {
                        bt709_rgba(&output.linear, request.background)
                    }
                    .map_err(|e| kronello_render::RenderError::UnsupportedFeature(e.to_string()))?;
                    for row in 0..tile.pixels[1] as usize {
                        let destination = ((y as usize + row) * request.region.pixels[0] as usize
                            + x as usize)
                            * stride;
                        let source = row * tile.pixels[0] as usize * stride;
                        let width = tile.pixels[0] as usize * stride;
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
        };
        let video = if let Some(transfer) = profile.hdr_transfer() {
            self.encode_hdr_video_stream(&encode_request, times.len(), transfer, &mut produce)?
        } else {
            self.encode_video_stream(&encode_request, times.len(), &mut produce)?
        };
        let audio_stage_bytes = std::fs::metadata(&audio_file)?.len();
        let video_stage_bytes = std::fs::metadata(&video_file)?.len();
        let video_probe = self.probe(&video_file)?;
        let audio_probe = self.probe(&audio_file)?;
        let expected_video = request.range.end().checked_sub(request.range.start())?;
        let expected_audio = SAMPLE_RATE.sample_to_time(audio.frames as i64)?;
        let audio_slack = Rational::new(profile.audio_frame_slack(), 48_000)?;
        if !video_probe
            .streams
            .iter()
            .any(|s| s.kind == StreamKind::Video && s.duration == Some(expected_video))
            || !audio_probe.streams.iter().any(|s| {
                s.kind == StreamKind::Audio
                    && match s.duration {
                        // WebM lossy intermediates may publish no stream duration.
                        None => profile.audio_frame_slack() > 0,
                        Some(d) => {
                            d >= expected_audio
                                && expected_audio
                                    .checked_add(audio_slack)
                                    .map(|max| d <= max)
                                    .unwrap_or(false)
                        }
                    }
            })
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
            audio_layout,
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
fn hdr_rgba(
    pixels: &[[f32; 4]],
    background: [f32; 3],
    transfer: kronello_render::HdrTransfer,
) -> Result<Vec<u8>, MediaError> {
    let mut bytes = Vec::with_capacity(pixels.len() * 8);
    for p in pixels {
        let alpha = f64::from(p[3]);
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err(MediaError::InvalidInput("HDR alpha".into()));
        }
        let rgb = [0, 1, 2].map(|i| f64::from(p[i]) + f64::from(background[i]) * (1.0 - alpha));
        let code = transfer.encode(rgb).ok_or_else(|| {
            MediaError::UnsupportedFeature(
                "HDR output outside transfer range; explicit gamut/tone conversion required".into(),
            )
        })?;
        for v in code {
            bytes.extend(((v * 65535.0).round() as u16).to_le_bytes());
        }
        bytes.extend(u16::MAX.to_le_bytes());
    }
    Ok(bytes)
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
fn stereo_audio_layout() -> kronello_model::ChannelMask {
    kronello_model::ChannelMask::STEREO
}
fn is_stereo_layout(mask: &kronello_model::ChannelMask) -> bool {
    *mask == kronello_model::ChannelMask::STEREO
}
fn document_plan(render: &RenderSnapshot, profile: u32) -> Result<DocumentAudioPlan, MediaError> {
    let target = match render.target() {
        kronello_render::RenderTarget::Composition { composition } => {
            AudioTarget::Composition(composition)
        }
        kronello_render::RenderTarget::Sequence { sequence } => AudioTarget::Sequence(sequence),
        kronello_render::RenderTarget::Source { source } => match source {
            kronello_render::SourcePreviewRef::Composition { composition } => {
                AudioTarget::Composition(composition)
            }
            _ => {
                let resolved =
                    kronello_render::resolve_source(render.project(), &source.source_ref())?;
                let Some(stream_index) = resolved.audio_stream else {
                    // Sources without an audio stream export silent audio.
                    return Ok(DocumentAudioPlan::default());
                };
                AudioTarget::Source {
                    asset: resolved.asset,
                    stream_index,
                    offset: resolved.offset,
                }
            }
        },
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
