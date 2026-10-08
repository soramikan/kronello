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
    AudioEncodeReport, EncodeCodec, ExecutionKind, MediaError, MediaPathReport, MediaRuntime,
    MediaTransferStats,
};

/// MEDIA-004 (ADR-0133): closed FFmpeg `dnxhd` encoder profile argument for
/// the DNxHD/DNxHR delivery profiles.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DnxProfile {
    /// DNxHD family; the encoder resolves the exact bit profile from the
    /// raster and frame rate and rejects non-compliant combinations.
    #[default]
    Dnxhd,
    DnxhrLb,
    DnxhrSq,
    DnxhrHq,
    DnxhrHqx,
    Dnxhr444,
}
impl DnxProfile {
    /// Index into the closed `DNX_KINDS` table in the native shim.
    pub(crate) fn kind(self) -> i32 {
        match self {
            Self::Dnxhd => 0,
            Self::DnxhrLb => 1,
            Self::DnxhrSq => 2,
            Self::DnxhrHq => 3,
            Self::DnxhrHqx => 4,
            Self::Dnxhr444 => 5,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryShape {
    /// Video + audio remuxed into one container through `km_mux_av`.
    Movie,
    /// Video-only elementary output (GIF).
    VideoOnly,
    /// Audio-only elementary output (MP3/FLAC); no frames are rendered.
    AudioOnly,
}
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
    // MEDIA-004 (ADR-0133): DNxHD/DNxHR + PCM24 delivery profiles.
    DnxhdMovPcm24V1,
    DnxhrLbMovPcm24V1,
    DnxhrSqMovPcm24V1,
    DnxhrHqMovPcm24V1,
    DnxhrHqxMovPcm24V1,
    Dnxhr444MovPcm24V1,
    DnxhdMxfPcm24V1,
    DnxhrLbMxfPcm24V1,
    DnxhrSqMxfPcm24V1,
    DnxhrHqMxfPcm24V1,
    DnxhrHqxMxfPcm24V1,
    Dnxhr444MxfPcm24V1,
    /// Deterministic two-pass indexed-color GIF (palette LUT + Bayer dither).
    GifV1,
    /// MP3 CBR 256k mono/stereo elementary output.
    Mp3V1,
    /// FLAC lossless elementary output.
    FlacV1,
}
impl MovieProfile {
    /// The DNx delivery profile for `(encoder profile, container)`; the closed
    /// pair set is exactly the twelve DNx variants.
    pub fn dnx(profile: DnxProfile, mxf: bool) -> Self {
        match (profile, mxf) {
            (DnxProfile::Dnxhd, false) => Self::DnxhdMovPcm24V1,
            (DnxProfile::DnxhrLb, false) => Self::DnxhrLbMovPcm24V1,
            (DnxProfile::DnxhrSq, false) => Self::DnxhrSqMovPcm24V1,
            (DnxProfile::DnxhrHq, false) => Self::DnxhrHqMovPcm24V1,
            (DnxProfile::DnxhrHqx, false) => Self::DnxhrHqxMovPcm24V1,
            (DnxProfile::Dnxhr444, false) => Self::Dnxhr444MovPcm24V1,
            (DnxProfile::Dnxhd, true) => Self::DnxhdMxfPcm24V1,
            (DnxProfile::DnxhrLb, true) => Self::DnxhrLbMxfPcm24V1,
            (DnxProfile::DnxhrSq, true) => Self::DnxhrSqMxfPcm24V1,
            (DnxProfile::DnxhrHq, true) => Self::DnxhrHqMxfPcm24V1,
            (DnxProfile::DnxhrHqx, true) => Self::DnxhrHqxMxfPcm24V1,
            (DnxProfile::Dnxhr444, true) => Self::Dnxhr444MxfPcm24V1,
        }
    }
    /// DNx encoder profile when this is a DNx leg, plus whether the delivery
    /// container is MXF (`true`) or MOV (`false`).
    pub fn dnx_profile(self) -> Option<(DnxProfile, bool)> {
        let mxf = matches!(
            self,
            Self::DnxhdMxfPcm24V1
                | Self::DnxhrLbMxfPcm24V1
                | Self::DnxhrSqMxfPcm24V1
                | Self::DnxhrHqMxfPcm24V1
                | Self::DnxhrHqxMxfPcm24V1
                | Self::Dnxhr444MxfPcm24V1
        );
        let profile = match self {
            Self::DnxhdMovPcm24V1 | Self::DnxhdMxfPcm24V1 => DnxProfile::Dnxhd,
            Self::DnxhrLbMovPcm24V1 | Self::DnxhrLbMxfPcm24V1 => DnxProfile::DnxhrLb,
            Self::DnxhrSqMovPcm24V1 | Self::DnxhrSqMxfPcm24V1 => DnxProfile::DnxhrSq,
            Self::DnxhrHqMovPcm24V1 | Self::DnxhrHqMxfPcm24V1 => DnxProfile::DnxhrHq,
            Self::DnxhrHqxMovPcm24V1 | Self::DnxhrHqxMxfPcm24V1 => DnxProfile::DnxhrHqx,
            Self::Dnxhr444MovPcm24V1 | Self::Dnxhr444MxfPcm24V1 => DnxProfile::Dnxhr444,
            _ => return None,
        };
        Some((profile, mxf))
    }
    /// Whether this SDR deliverable tone-maps an HDR render the same way
    /// `ProResSdrFromHdrPcm24V1` does instead of rejecting out-of-range pixels.
    fn sdr_delivery_conversion(self) -> bool {
        matches!(self, Self::ProResSdrFromHdrPcm24V1 | Self::GifV1) || self.dnx_profile().is_some()
    }
    pub fn shape(self) -> DeliveryShape {
        match self {
            Self::GifV1 => DeliveryShape::VideoOnly,
            Self::Mp3V1 | Self::FlacV1 => DeliveryShape::AudioOnly,
            _ => DeliveryShape::Movie,
        }
    }
    pub fn has_video(self) -> bool {
        !matches!(self.shape(), DeliveryShape::AudioOnly)
    }
    pub fn has_audio(self) -> bool {
        !matches!(self.shape(), DeliveryShape::VideoOnly)
    }
    /// Whether the published container round-trips the embedded `kronello_*`
    /// snapshot-identity tags (ADR-0133). MXF's fixed KLV metadata model and
    /// the elementary formats drop custom keys; those legs authenticate by
    /// probe shape and receipt hash instead.
    pub fn embeds_snapshot_identity(self) -> bool {
        self.shape() == DeliveryShape::Movie && !self.dnx_profile().is_some_and(|(_, mxf)| mxf)
    }
    /// Whether the delivery container can carry chapter markers (ADR-0133:
    /// mov/mp4 only — WebM/MXF/GIF/MP3/FLAC report a typed warning instead).
    pub fn supports_chapters(self) -> bool {
        self.shape() == DeliveryShape::Movie
            && !matches!(self, Self::Av1WebmOpusV1)
            && !self.dnx_profile().is_some_and(|(_, mxf)| mxf)
    }
    pub fn hdr_transfer(self) -> Option<kronello_render::HdrTransfer> {
        match self {
            Self::ProResPqPcm24V1 => Some(kronello_render::HdrTransfer::Pq),
            Self::ProResHlgPcm24V1 => Some(kronello_render::HdrTransfer::Hlg),
            _ => None,
        }
    }
    pub fn is_prores(self) -> bool {
        self.video_codec() == Some(EncodeCodec::ProRes)
    }
    /// Video encoder family; `None` for video-free and GIF legs (GIF has its
    /// own deterministic indexed-color path, not an `EncodeCodec` slot).
    pub fn video_codec(self) -> Option<EncodeCodec> {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => Some(EncodeCodec::ProRes),
            Self::Av1Mp4AlacV1 | Self::Av1Mp4AacV1 | Self::Av1WebmOpusV1 => Some(EncodeCodec::Av1),
            Self::H264AlacV1 | Self::H264AacV1 => Some(EncodeCodec::H264),
            Self::HevcAlacV1 | Self::HevcAacV1 => Some(EncodeCodec::Hevc),
            Self::DnxhdMovPcm24V1
            | Self::DnxhrLbMovPcm24V1
            | Self::DnxhrSqMovPcm24V1
            | Self::DnxhrHqMovPcm24V1
            | Self::DnxhrHqxMovPcm24V1
            | Self::Dnxhr444MovPcm24V1
            | Self::DnxhdMxfPcm24V1
            | Self::DnxhrLbMxfPcm24V1
            | Self::DnxhrSqMxfPcm24V1
            | Self::DnxhrHqMxfPcm24V1
            | Self::DnxhrHqxMxfPcm24V1
            | Self::Dnxhr444MxfPcm24V1 => Some(EncodeCodec::Dnx),
            Self::GifV1 | Self::Mp3V1 | Self::FlacV1 => None,
        }
    }
    /// Closed delivery audio codec for this profile's intermediate stage;
    /// `None` for video-only legs.
    pub(crate) fn audio_kind(self) -> Option<crate::ffi::AudioEncoderKind> {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1
            | Self::DnxhdMovPcm24V1
            | Self::DnxhrLbMovPcm24V1
            | Self::DnxhrSqMovPcm24V1
            | Self::DnxhrHqMovPcm24V1
            | Self::DnxhrHqxMovPcm24V1
            | Self::Dnxhr444MovPcm24V1
            | Self::DnxhdMxfPcm24V1
            | Self::DnxhrLbMxfPcm24V1
            | Self::DnxhrSqMxfPcm24V1
            | Self::DnxhrHqMxfPcm24V1
            | Self::DnxhrHqxMxfPcm24V1
            | Self::Dnxhr444MxfPcm24V1 => Some(crate::ffi::AudioEncoderKind::Pcm24),
            Self::Av1Mp4AlacV1 | Self::H264AlacV1 | Self::HevcAlacV1 => {
                Some(crate::ffi::AudioEncoderKind::Alac)
            }
            Self::H264AacV1 | Self::HevcAacV1 | Self::Av1Mp4AacV1 => {
                Some(crate::ffi::AudioEncoderKind::Aac)
            }
            Self::Av1WebmOpusV1 => Some(crate::ffi::AudioEncoderKind::Opus),
            Self::GifV1 => None,
            Self::Mp3V1 => Some(crate::ffi::AudioEncoderKind::Mp3),
            Self::FlacV1 => Some(crate::ffi::AudioEncoderKind::Flac),
        }
    }
    /// Container suffix the destination path must use.
    pub fn container(self) -> &'static str {
        match self {
            Self::Av1Mp4AlacV1 | Self::Av1Mp4AacV1 => "mp4",
            Self::Av1WebmOpusV1 => "webm",
            Self::DnxhdMxfPcm24V1
            | Self::DnxhrLbMxfPcm24V1
            | Self::DnxhrSqMxfPcm24V1
            | Self::DnxhrHqMxfPcm24V1
            | Self::DnxhrHqxMxfPcm24V1
            | Self::Dnxhr444MxfPcm24V1 => "mxf",
            Self::GifV1 => "gif",
            Self::Mp3V1 => "mp3",
            Self::FlacV1 => "flac",
            _ => "mov",
        }
    }
    /// Lossy codecs signal encoder delay/padding in container metadata; the
    /// stream duration may exceed the input by up to one codec frame.
    fn audio_frame_slack(self) -> i64 {
        match self {
            Self::H264AacV1 | Self::HevcAacV1 | Self::Av1Mp4AacV1 => 1024,
            Self::Av1WebmOpusV1 => 960,
            // MPEG Layer III frame at 48 kHz.
            Self::Mp3V1 => 1152,
            _ => 0,
        }
    }
    /// Native mux profile index; -1 for elementary outputs that never pass
    /// through `km_mux_av` (GIF/MP3/FLAC write their deliverable directly).
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
            Self::DnxhdMovPcm24V1
            | Self::DnxhrLbMovPcm24V1
            | Self::DnxhrSqMovPcm24V1
            | Self::DnxhrHqMovPcm24V1
            | Self::DnxhrHqxMovPcm24V1
            | Self::Dnxhr444MovPcm24V1 => 8,
            Self::DnxhdMxfPcm24V1
            | Self::DnxhrLbMxfPcm24V1
            | Self::DnxhrSqMxfPcm24V1
            | Self::DnxhrHqMxfPcm24V1
            | Self::DnxhrHqxMxfPcm24V1
            | Self::Dnxhr444MxfPcm24V1 => 9,
            Self::GifV1 | Self::Mp3V1 | Self::FlacV1 => -1,
        }
    }
    /// Probed codec names `(video, audio)`; a `None` slot means the profile
    /// has no such stream.
    fn codecs(self) -> (Option<&'static str>, Option<&'static str>) {
        match self {
            Self::ProResPcm24
            | Self::ProResPqPcm24V1
            | Self::ProResHlgPcm24V1
            | Self::ProResSdrFromHdrPcm24V1 => (Some("prores"), Some("pcm_s24le")),
            Self::Av1Mp4AlacV1 => (Some("av1"), Some("alac")),
            Self::H264AlacV1 => (Some("h264"), Some("alac")),
            Self::HevcAlacV1 => (Some("hevc"), Some("alac")),
            Self::H264AacV1 => (Some("h264"), Some("aac")),
            Self::HevcAacV1 => (Some("hevc"), Some("aac")),
            Self::Av1Mp4AacV1 => (Some("av1"), Some("aac")),
            Self::Av1WebmOpusV1 => (Some("av1"), Some("opus")),
            Self::GifV1 => (Some("gif"), None),
            Self::Mp3V1 => (None, Some("mp3")),
            Self::FlacV1 => (None, Some("flac")),
            _ => (Some("dnxhd"), Some("pcm_s24le")),
        }
    }
    /// Encoder names this profile requires at runtime: the video slot accepts
    /// any one name of the returned list (matching `select_encoder`), the
    /// audio slot requires the exact closed encoder name. Discovery and
    /// pre-flight capability checks share this table (ADR-0133).
    pub fn required_encoders(self) -> (&'static [&'static str], Option<&'static str>) {
        let video: &'static [&'static str] = match self.video_codec() {
            Some(EncodeCodec::Av1) => &["libsvtav1", "libaom-av1"],
            Some(EncodeCodec::ProRes) => &["prores_ks"],
            Some(EncodeCodec::H264) => &["h264_videotoolbox"],
            Some(EncodeCodec::Hevc) => &["hevc_videotoolbox"],
            Some(EncodeCodec::Dnx) => &["dnxhd"],
            None => {
                if self == Self::GifV1 {
                    &["gif"]
                } else {
                    &[]
                }
            }
        };
        (video, self.audio_kind().map(|kind| kind.encoder_name()))
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
        // MEDIA-004: audio-only legs carry no pixel contract; DNx/GIF and the
        // explicit SDR-from-HDR profile tone-map an HDR render like
        // ProResSdrFromHdrPcm24V1 instead of requiring an HDR deliverable.
        let profile = self.movie_profile();
        let render_hdr = self.render.profile().hdr.map(|h| h.transfer);
        if profile.has_video()
            && ((profile == MovieProfile::ProResSdrFromHdrPcm24V1 && render_hdr.is_none())
                || (profile.hdr_transfer() != render_hdr
                    && !(render_hdr.is_some() && profile.sdr_delivery_conversion())))
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
/// MEDIA-004 (ADR-0133): one container chapter in output-relative master
/// time. `id` is informational on probes; on write the muxer assigns its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaChapter {
    #[serde(default)]
    pub id: i64,
    pub start: Rational,
    /// Exclusive end; strictly greater than `start` at the 1/48000 master
    /// clock granularity used across the FFI boundary.
    pub end: Rational,
    /// Display title copied from the chapter marker; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}
/// Whether chapter markers transfer into a leg that can hold them
/// (ADR-0133). Only authored chapter markers in the export range move;
/// marker colors and comments never cross the container boundary.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ChapterPolicy {
    /// Copy chapters where the container supports them; otherwise record a
    /// typed `CHAPTERS_DROPPED` warning on the leg report.
    #[default]
    Transfer,
    /// Explicitly drop chapter markers for this leg without warning.
    Omit,
}
/// A typed non-fatal delivery notice recorded on a leg report (ADR-0133).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaWarning {
    pub code: String,
    pub message: String,
}
impl MediaWarning {
    pub(crate) fn chapters_dropped(container: &str, count: usize) -> Self {
        Self {
            code: "CHAPTERS_DROPPED".into(),
            message: format!(
                "{count} chapter marker(s) not transferred: {container} cannot carry chapters"
            ),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaProbe {
    pub streams: Vec<MediaStream>,
    /// Container-level duration; the only duration WebM publishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Rational>,
    /// MEDIA-004: container chapters in output-relative master time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<MediaChapter>,
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
        // MEDIA-004: chapter-capable containers (MOV/MP4) store markers as an
        // extra data track, so essence is exactly one video + one audio and
        // any further stream must be a probed chapter track.
        let video_count = self
            .streams
            .iter()
            .filter(|s| s.kind == StreamKind::Video)
            .count();
        let audio_count = self
            .streams
            .iter()
            .filter(|s| s.kind == StreamKind::Audio)
            .count();
        let chapter_tracks = self.streams.len() - video_count - audio_count;
        let stray_chapter_track =
            chapter_tracks > 0 && (!profile.supports_chapters() || self.chapters.is_empty());
        if video_count != 1 || audio_count != 1 || stray_chapter_track {
            let observed = self
                .streams
                .iter()
                .map(|s| format!("{:?}/{}", s.kind, s.codec))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(MediaError::Encode(format!(
                "expected exactly two output streams, observed {observed}"
            )));
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
        if video.codec != video_codec.unwrap_or_default()
            || audio.codec != audio_codec.unwrap_or_default()
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
            let lossless = matches!(audio_codec, Some("pcm_s24le" | "alac"));
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
    /// MEDIA-004: verify a probed deliverable against its declared profile
    /// shape — movie legs use [`Self::verify_movie_layout`]; GIF legs require
    /// exactly one zero-origin `gif` stream; MP3/FLAC legs require exactly one
    /// audio stream with the declared layout at 48 kHz.
    pub fn verify_delivery(
        &self,
        profile: MovieProfile,
        audio_layout: kronello_model::ChannelMask,
    ) -> Result<(), MediaError> {
        match profile.shape() {
            DeliveryShape::Movie => self.verify_movie_layout(profile, audio_layout),
            DeliveryShape::VideoOnly => {
                if self.streams.len() != 1 {
                    return Err(MediaError::Encode(
                        "expected exactly one GIF video stream".into(),
                    ));
                }
                let video = &self.streams[0];
                if video.kind != StreamKind::Video
                    || video.codec != "gif"
                    || (video.start.is_some() && video.start != Some(Rational::ZERO))
                {
                    return Err(MediaError::Encode("invalid GIF codec or start PTS".into()));
                }
                match video.duration.or(self.duration) {
                    Some(d) if d > Rational::ZERO => Ok(()),
                    _ => Err(MediaError::Encode("missing GIF duration".into())),
                }
            }
            DeliveryShape::AudioOnly => {
                if self.streams.len() != 1 {
                    return Err(MediaError::Encode(
                        "expected exactly one audio stream".into(),
                    ));
                }
                let audio = &self.streams[0];
                let (_, codec) = profile.codecs();
                // Elementary lossy containers surface the encoder delay as a
                // positive stream start (e.g. the MP3 Xing header); decoders
                // trim it, so up to one codec frame of signalled priming is a
                // valid zero-origin output. Lossless profiles keep the strict
                // zero-origin requirement.
                let priming_bound = Rational::new(profile.audio_frame_slack(), 48_000)?;
                let bad_start = match audio.start {
                    None => false,
                    Some(start) => start < Rational::ZERO || start > priming_bound,
                };
                if audio.kind != StreamKind::Audio
                    || audio.codec != codec.unwrap_or_default()
                    || audio.sample_rate != Some(48_000)
                    || audio.channels != Some(audio_layout.channels() as u32)
                    || bad_start
                {
                    return Err(MediaError::Encode(format!(
                        "invalid audio codec, sample format, channel layout or start PTS: codec={:?} rate={:?} channels={:?} start={:?}",
                        audio.codec, audio.sample_rate, audio.channels, audio.start
                    )));
                }
                // The caller compares the expected length separately; the
                // probe only guarantees a positive stream duration.
                match audio.duration.or(self.duration) {
                    Some(d) if d > Rational::ZERO => Ok(()),
                    _ => Err(MediaError::Encode("missing audio duration".into())),
                }
            }
        }
    }
    /// Compare probed container chapters against the expected marker list.
    /// Times are compared at the container timebase granularity: writes round
    /// to 1/48000 ticks and muxers may rescale once more, so each boundary
    /// must land within 2 ms of the authored rational.
    pub fn verify_chapters(&self, expected: &[MediaChapter]) -> Result<(), MediaError> {
        if self.chapters.len() != expected.len() {
            return Err(MediaError::Encode(format!(
                "chapter count differs: expected {}, probed {}",
                expected.len(),
                self.chapters.len()
            )));
        }
        let tolerance = Rational::new(1, 500)?;
        for (chapter, want) in self.chapters.iter().zip(expected) {
            let drift = |a: Rational, b: Rational| -> Result<bool, MediaError> {
                let d = a.checked_sub(b)?;
                Ok(d < tolerance && d > tolerance.checked_neg()?)
            };
            if chapter.title != want.title
                || !drift(chapter.start, want.start)?
                || !drift(chapter.end, want.end)?
            {
                return Err(MediaError::Encode(
                    "container chapter differs from the authored marker".into(),
                ));
            }
        }
        Ok(())
    }
}

/// MEDIA-004 (ADR-0133): one additional output leg sharing the single render
/// pass. The leg's movie profile, audio contract and snapshot identity come
/// from the matching `AvExportSnapshot` in the call, not from this spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryOutput {
    /// Publish destination; staged and renamed like the primary `output`.
    pub output: PathBuf,
    /// Versioned delivery profile for this leg; must equal the companion
    /// snapshot's `movie_profile`.
    pub profile: MovieProfile,
    /// Explicit opaque background in the profile's pixel class space.
    pub background: [f32; 3],
    #[serde(default)]
    pub chapters: ChapterPolicy,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AvExportRequest {
    /// Primary output destination; `movie_profile` comes from the snapshot.
    pub output: PathBuf,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub region: OutputRegion,
    /// Explicit opaque background in the fixed profile working space (HDR 1=203 nits).
    pub background: [f32; 3],
    pub clipping: ClippingPolicy,
    /// MEDIA-004: chapter transfer policy for the primary leg.
    #[serde(default, skip_serializing_if = "ChapterPolicy::is_transfer")]
    pub chapters: ChapterPolicy,
    /// MEDIA-004 (ADR-0133): additional output legs; empty keeps the
    /// pre-M10 single-output wire shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<DeliveryOutput>,
}
impl ChapterPolicy {
    fn is_transfer(&self) -> bool {
        *self == Self::Transfer
    }
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

/// Per-leg result of a multi-output delivery (ADR-0133): each output carries
/// its own probe, transfer report and warnings. `chapters` lists the markers
/// actually written into the container.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AvExportOutputReport {
    /// Destination as declared in the request leg.
    pub output: PathBuf,
    pub profile: MovieProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<MediaPathReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioEncodeReport>,
    pub probe: MediaProbe,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<MediaChapter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<MediaWarning>,
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
    /// Primary leg transfer report; absent for audio-only deliveries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<MediaPathReport>,
    /// Primary leg audio report; absent for video-only deliveries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioEncodeReport>,
    pub probe: MediaProbe,
    /// MEDIA-004: one entry per leg in request order, primary first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<AvExportOutputReport>,
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
        self.mux_movie_chapters(
            video,
            audio,
            output,
            render_hash,
            export_hash,
            profile,
            audio_layout,
            &[],
        )
    }
    /// MEDIA-004 (ADR-0133): mux one leg with authored chapters and verify the
    /// container wrote them back before publishing.
    #[allow(clippy::too_many_arguments)] // Mirrors the staged variant one to one.
    pub fn mux_movie_chapters(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
        audio_layout: kronello_model::ChannelMask,
        chapters: &[MediaChapter],
    ) -> Result<MediaProbe, MediaError> {
        let (temp, probe) = self.mux_movie_stage(
            video,
            audio,
            output,
            render_hash,
            export_hash,
            profile,
            audio_layout,
            chapters,
        )?;
        temp.as_file().sync_all()?;
        publish_file(temp, output)?;
        Ok(probe)
    }
    /// Stage one movie deliverable beside `output` without publishing it: a
    /// multi-output job holds every destination back until all legs verify,
    /// so a partial success can never be reported (ADR-0133).
    #[allow(clippy::too_many_arguments)] // Explicit codec contract accompanies both snapshot identities.
    fn mux_movie_stage(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
        audio_layout: kronello_model::ChannelMask,
        chapters: &[MediaChapter],
    ) -> Result<(tempfile::NamedTempFile, MediaProbe), MediaError> {
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
            chapters: vec![],
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
            chapters,
        )?;
        let probe = self.probe(temp.path())?;
        probe.verify_movie_layout(profile, audio_layout)?;
        // The staged container must carry exactly the chapters we wrote.
        probe.verify_chapters(chapters)?;
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
        if profile.embeds_snapshot_identity()
            && (probe.render_snapshot_hash != render_hash
                || probe.export_snapshot_hash != export_hash)
        {
            return Err(MediaError::Encode(
                "muxed snapshot identity mismatch".into(),
            ));
        }
        Ok((temp, probe))
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
        self.export_delivery_with_checkpoint(
            &[snapshot],
            project_path,
            fonts,
            backend,
            request,
            checkpoint,
        )
    }
    /// MEDIA-004 (ADR-0133): one render fans out to `1 + request.outputs`
    /// deliverables. `snapshots[i]` is the fixed export snapshot of leg `i`
    /// (leg 0 is `request.output` with `request.chapters`); every leg must
    /// share the same render snapshot. Any leg failure fails the job and no
    /// destination publishes unless all legs verify.
    pub fn export_delivery(
        &self,
        snapshots: &[&AvExportSnapshot],
        project_path: &Path,
        fonts: &[FontData<'_>],
        backend: &dyn RenderBackend,
        request: &AvExportRequest,
    ) -> Result<AvExportReport, MediaError> {
        self.export_delivery_with_checkpoint(
            snapshots,
            project_path,
            fonts,
            backend,
            request,
            &mut |_| Ok(()),
        )
    }
    /// Checkpoint-aware variant of [`Self::export_delivery`].
    pub fn export_delivery_with_checkpoint(
        &self,
        snapshots: &[&AvExportSnapshot],
        project_path: &Path,
        fonts: &[FontData<'_>],
        backend: &dyn RenderBackend,
        request: &AvExportRequest,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<AvExportReport, MediaError> {
        checkpoint(0)?;
        if snapshots.is_empty() || snapshots.len() != request.outputs.len() + 1 {
            return Err(MediaError::InvalidInput(
                "one export snapshot per delivery output".into(),
            ));
        }
        for snapshot in snapshots {
            snapshot.validate()?;
        }
        let snapshot = snapshots[0];
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
        request.region.validate()?;
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
        let samples = sample_range(request.range)?;
        if samples.is_empty() {
            return Err(MediaError::InvalidInput("empty audio output".into()));
        }
        // One fixed render snapshot feeds every leg.
        let render_hash = snapshot.render.content_hash()?;
        let mut export_hashes = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            if snapshot.render.content_hash()? != render_hash {
                return Err(MediaError::InvalidInput(
                    "all delivery outputs share one fixed render snapshot".into(),
                ));
            }
            export_hashes.push(snapshot.content_hash()?);
        }
        let render_space = snapshot.render.profile().working_space;
        let render_hdr = snapshot.render.profile().hdr.map(|h| h.transfer);
        let background_peak = render_hdr.map_or(1.0, |transfer| match transfer {
            kronello_render::HdrTransfer::Pq => 10000.0 / 203.0,
            kronello_render::HdrTransfer::Hlg => 1000.0 / 203.0,
        });
        // Resolved legs: the primary request output plus each declared extra
        // output, each pinned to its own frozen export snapshot.
        let mut legs = Vec::with_capacity(snapshots.len());
        legs.push(DeliveryLeg {
            snapshot,
            output: request.output.clone(),
            profile: snapshot.movie_profile(),
            background: request.background,
            chapters_policy: request.chapters,
        });
        for (spec, snapshot) in request.outputs.iter().zip(snapshots[1..].iter().copied()) {
            if spec.profile != snapshot.movie_profile() {
                return Err(MediaError::InvalidInput(
                    "delivery output profile differs from its export snapshot".into(),
                ));
            }
            legs.push(DeliveryLeg {
                snapshot,
                output: spec.output.clone(),
                profile: spec.profile,
                background: spec.background,
                chapters_policy: spec.chapters,
            });
        }
        // Every leg validates before the first frame renders (ADR-0133).
        let mut destinations = std::collections::BTreeSet::new();
        for leg in &legs {
            if leg.output.exists() {
                return Err(MediaError::OutputExists(leg.output.clone()));
            }
            let extension = leg
                .output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !extension.eq_ignore_ascii_case(leg.profile.container()) {
                return Err(MediaError::InvalidInput(format!(
                    "delivery output requires the .{} container",
                    leg.profile.container()
                )));
            }
            if !destinations.insert(leg.output.clone()) {
                return Err(MediaError::InvalidInput(
                    "delivery outputs must be distinct".into(),
                ));
            }
            // Movie encoders emit 4:2:0/4:2:2/DNx rasters; the shared region
            // must offer positive even dimensions for them. GIF and
            // audio-only legs accept arbitrary pixel geometry.
            if leg.profile.shape() == DeliveryShape::Movie
                && (request.region.pixels[0] & 1 != 0 || request.region.pixels[1] & 1 != 0)
            {
                return Err(MediaError::InvalidInput(
                    "movie delivery legs require even raster dimensions".into(),
                ));
            }
            // HDR legs need a Rec.2020 render; SDR legs take an SDR render or,
            // for explicit conversion profiles, an HDR render tone-mapped down.
            let space_ok = match leg.profile.hdr_transfer() {
                Some(_) => render_space == ColorSpace::LinearRec2020,
                None => {
                    render_space == ColorSpace::LinearRec709
                        || leg.profile.sdr_delivery_conversion()
                }
            };
            if !space_ok {
                return Err(MediaError::UnsupportedFeature(
                    "A/V working space must match explicit SDR/HDR movie profile".into(),
                ));
            }
            if leg
                .background
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=background_peak).contains(v))
            {
                return Err(MediaError::InvalidInput(
                    "background must be finite nonnegative linear RGB within the explicit transfer range".into(),
                ));
            }
            // Runtime encoder capability: the video slot accepts any eligible
            // registered alternative; GIF and the audio slot require exact
            // closed encoder names. A miss is typed, never a substitution.
            match leg.profile.video_codec() {
                Some(codec) => {
                    let selected = self.capabilities.select_encoder(codec)?;
                    if leg.profile == MovieProfile::Av1Mp4AlacV1 && selected.name != "libsvtav1" {
                        return Err(MediaError::EncoderUnavailable {
                            encoder: "libsvtav1".into(),
                            reason: "AV1 MP4 version 1 requires SVT-AV1".into(),
                            ffmpeg: None,
                        });
                    }
                }
                None if leg.profile == MovieProfile::GifV1 => {
                    self.capabilities.require_encoder("gif")?;
                }
                None => {}
            }
            if let Some(name) = leg.profile.required_encoders().1 {
                self.capabilities.require_encoder(name)?;
            }
        }
        // ADR-0133: chapter markers come from the fixed sequence target of the
        // shared render snapshot, clipped to the export range.
        let authored = export_chapters(&snapshot.render, request.range)?;
        let mut leg_chapters = Vec::with_capacity(legs.len());
        let mut leg_warnings: Vec<Vec<MediaWarning>> = Vec::with_capacity(legs.len());
        for leg in &legs {
            let mut warnings = Vec::new();
            let chapters = if leg.chapters_policy != ChapterPolicy::Transfer {
                Vec::new()
            } else if leg.profile.supports_chapters() {
                authored.clone()
            } else {
                if !authored.is_empty() {
                    warnings.push(MediaWarning::chapters_dropped(
                        leg.profile.container(),
                        authored.len(),
                    ));
                }
                Vec::new()
            };
            leg_chapters.push(chapters);
            leg_warnings.push(warnings);
        }
        let parent = request
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Every intermediate is on the destination volume and owned by RAII.
        let stage = tempfile::tempdir_in(parent)?;
        // Decode the union of audio sources referenced by any leg once.
        let mut sources = crate::streaming::SpoolSources::new();
        for snapshot in snapshots {
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
        }
        // Audio pass: one encoder per audio leg. Movie legs write an
        // intermediate inside the stage dir; elementary MP3/FLAC legs encode
        // straight into their staged publish artifact.
        let mut audio_reports: Vec<Option<AudioEncodeReport>> = Vec::with_capacity(legs.len());
        let mut audio_files: Vec<Option<LegStage>> = Vec::with_capacity(legs.len());
        for (index, leg) in legs.iter().enumerate() {
            let Some(kind) = leg.profile.audio_kind() else {
                audio_reports.push(None);
                audio_files.push(None);
                continue;
            };
            let file = if leg.profile.shape() == DeliveryShape::AudioOnly {
                LegStage::Publish(stage_file(&leg.output)?)
            } else {
                LegStage::Intermediate(
                    stage
                        .path()
                        .join(format!("audio_{index}.{}", kind.intermediate_suffix())),
                )
            };
            // ADR-0124: the snapshot declares the single output layout; the
            // bus converts sources deterministically and never folds down.
            let layout = leg.snapshot.audio_layout;
            let mut encoder =
                crate::ffi::NativeAudioEncoder::open(&self.native, file.path(), kind, layout)?;
            let plan = if leg.snapshot.audio == AudioSourceMode::Document {
                Some(document_plan(
                    &leg.snapshot.render,
                    leg.snapshot.schema_version,
                )?)
            } else {
                None
            };
            let mut clipped_samples = 0_usize;
            let mut start = samples.start;
            while start < samples.end {
                checkpoint(0)?;
                let end = samples.end.min(
                    start
                        .checked_add(encoder.block as i64)
                        .ok_or_else(|| MediaError::InvalidInput("audio batch overflow".into()))?,
                );
                let range = TimeRange::new(
                    SAMPLE_RATE.sample_to_time(start)?,
                    SAMPLE_RATE.sample_to_time(end)?,
                )?;
                let bus = match &plan {
                    Some(plan) => plan.mix_channels(&sources, range, layout)?,
                    None => {
                        kronello_audio::mix_channels(&leg.snapshot.clips, &sources, range, layout)?
                    }
                };
                let quantized = bus.quantize_pcm24(request.clipping)?;
                clipped_samples = clipped_samples
                    .checked_add(quantized.clipped_samples)
                    .ok_or_else(|| MediaError::InvalidInput("clipping count overflow".into()))?;
                encoder.frame(&quantized.samples)?;
                start = end;
            }
            encoder.finish()?;
            drop(encoder);
            audio_reports.push(Some(AudioEncodeReport {
                codec: kind.codec_name().into(),
                sample_rate: 48_000,
                channels: layout.channels() as u32,
                frames: usize::try_from(samples.end - samples.start)
                    .map_err(|_| MediaError::InvalidInput("audio count overflow".into()))?,
                clipped_samples,
            }));
            audio_files.push(Some(file));
        }
        let audio_spool_write_bytes = sources.decoded_bytes;
        let audio_window_read_bytes = sources.read_bytes();
        drop(sources);
        // Video pass: render each frame once, convert into every video leg's
        // pixel class and feed its encoder. GIF legs additionally fold the
        // palette histogram and spool the RGBA8 frame for phase 2.
        let width = request.region.pixels[0] as usize;
        let height = request.region.pixels[1] as usize;
        let time_base = request.frame_rate.frame_to_time(1)?;
        let mut sinks: Vec<Option<VideoSink>> = Vec::with_capacity(legs.len());
        let mut video_files: Vec<Option<PathBuf>> = Vec::with_capacity(legs.len());
        for (index, leg) in legs.iter().enumerate() {
            match leg.profile.shape() {
                DeliveryShape::AudioOnly => {
                    sinks.push(None);
                    video_files.push(None);
                }
                DeliveryShape::VideoOnly => {
                    let spool_path = stage.path().join(format!("gif_{index}.rgba"));
                    let spool = std::io::BufWriter::new(std::fs::File::create(&spool_path)?);
                    let encoder = crate::ffi::NativeGifEncoder::open(
                        &self.native,
                        request.region.pixels[0],
                        request.region.pixels[1],
                        time_base,
                    )?;
                    sinks.push(Some(VideoSink::Gif(GifSink {
                        encoder,
                        spool,
                        spool_path,
                        buffer: vec![0; width * height * 4],
                        profile: leg.profile,
                        background: leg.background,
                    })));
                    video_files.push(None);
                }
                DeliveryShape::Movie => {
                    let video_file = stage.path().join(format!("video_{index}.mov"));
                    let (encoder, name, hardware) =
                        if let Some((dnx, _)) = leg.profile.dnx_profile() {
                            let encoder = crate::ffi::NativeEncoder::open_dnx(
                                &self.native,
                                &video_file,
                                dnx.kind(),
                                request.region.pixels[0] as i32,
                                request.region.pixels[1] as i32,
                                time_base,
                            )?;
                            (encoder, "dnxhd".to_string(), false)
                        } else {
                            let codec = self.capabilities.select_encoder(
                                leg.profile.video_codec().unwrap_or(EncodeCodec::ProRes),
                            )?;
                            let name = codec.name.clone();
                            let hardware = codec.hardware;
                            let encoder = crate::ffi::NativeEncoder::open_color(
                                &self.native,
                                &video_file,
                                codec,
                                request.region.pixels[0] as i32,
                                request.region.pixels[1] as i32,
                                time_base,
                                leg.profile.hdr_transfer(),
                            )?;
                            (encoder, name, hardware)
                        };
                    let stride = if leg.profile.hdr_transfer().is_some() {
                        8
                    } else {
                        4
                    };
                    sinks.push(Some(VideoSink::Color(ColorSink {
                        encoder,
                        encoder_name: name,
                        hardware,
                        stride,
                        buffer: vec![0; width * height * stride],
                        pending: None,
                        last_span: 1,
                        profile: leg.profile,
                        background: leg.background,
                    })));
                    video_files.push(Some(video_file));
                }
            }
        }
        let mut metadata = Vec::new();
        for (number, (index, time)) in times.iter().enumerate() {
            checkpoint(number as u64)?;
            let mut frame_metadata = render_frame_tiles(
                &snapshot.render,
                fonts,
                backend,
                FrameRequest {
                    time: *time,
                    region: request.region,
                },
                &mut |[x, y], tile, output| {
                    // The sole full-frame buffers are per-leg RGBA8 or HDR
                    // RGBA64. No full-frame linear or display surface is
                    // allocated by this movie path.
                    for sink in sinks.iter_mut().flatten() {
                        let (buffer, stride, profile, background) = sink.conversion_target();
                        if buffer.len() != width * height * stride {
                            buffer.resize(width * height * stride, 0);
                        }
                        let bytes = if let Some(transfer) = profile.hdr_transfer() {
                            hdr_rgba(&output.linear, background, transfer)
                        } else if render_hdr.is_some() && profile.sdr_delivery_conversion() {
                            let mapped: Vec<_> = output
                                .linear
                                .iter()
                                .map(|p| {
                                    let opaque = [
                                        p[0] + background[0] * (1.0 - p[3]),
                                        p[1] + background[1] * (1.0 - p[3]),
                                        p[2] + background[2] * (1.0 - p[3]),
                                        1.0,
                                    ];
                                    kronello_render::hdr_to_sdr_linear(opaque)
                                })
                                .collect();
                            bt709_rgba(&mapped, [0.0; 3])
                        } else {
                            bt709_rgba(&output.linear, background)
                        }
                        .map_err(|e| {
                            kronello_render::RenderError::UnsupportedFeature(e.to_string())
                        })?;
                        for row in 0..tile.pixels[1] as usize {
                            let destination = ((y as usize + row) * width + x as usize) * stride;
                            let source = row * tile.pixels[0] as usize * stride;
                            let row_bytes = tile.pixels[0] as usize * stride;
                            buffer[destination..destination + row_bytes]
                                .copy_from_slice(&bytes[source..source + row_bytes]);
                        }
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
            let pts = time.checked_sub(request.range.start())?;
            let tick = pts.checked_div(time_base)?;
            if tick.denominator() != 1 || tick.numerator() < 0 {
                return Err(MediaError::InvalidInput(
                    "integral nonnegative frame PTS required".into(),
                ));
            }
            let tick = tick.numerator();
            for sink in sinks.iter_mut().flatten() {
                sink.deliver(tick)?;
            }
        }
        let mut video_reports: Vec<Option<MediaPathReport>> = Vec::with_capacity(legs.len());
        let expected_video = request.range.end().checked_sub(request.range.start())?;
        let expected_audio = SAMPLE_RATE.sample_to_time(samples.end - samples.start)?;
        // Staged publish artifacts per leg (movie mux results, GIF/elementary
        // encodes); publishing happens only after every leg verifies.
        let mut staged: Vec<Option<tempfile::NamedTempFile>> = Vec::with_capacity(legs.len());
        let mut probes: Vec<Option<MediaProbe>> = Vec::with_capacity(legs.len());
        for (index, leg) in legs.iter().enumerate() {
            match sinks[index].take() {
                Some(VideoSink::Color(mut sink)) => {
                    if let Some((frame, pts)) = sink.pending.take() {
                        sink.encoder.frame(&frame, pts, sink.last_span)?;
                    }
                    sink.encoder.finish()?;
                    let pixel_format = sink.encoder.pixel_format.clone();
                    let converted = sink
                        .encoder
                        .frame_size
                        .checked_mul(times.len() as u64)
                        .ok_or_else(|| {
                            MediaError::InvalidInput("transfer counter overflow".into())
                        })?;
                    let input_bytes = ((width * height * sink.stride) as u64)
                        .checked_mul(times.len() as u64)
                        .ok_or_else(|| {
                            MediaError::InvalidInput("transfer counter overflow".into())
                        })?;
                    drop(sink.encoder);
                    video_reports.push(Some(MediaPathReport {
                        decoder: None,
                        encoder: Some(sink.encoder_name),
                        execution: if sink.hardware {
                            ExecutionKind::Hardware
                        } else {
                            ExecutionKind::Software
                        },
                        input_pixel_format: if sink.stride == 8 { "rgba64le" } else { "rgba" }
                            .into(),
                        output_pixel_format: pixel_format,
                        transfer_path: if sink.stride == 8 {
                            "cpu_rec2100_rgba64_to_prores_10bit"
                        } else if sink.hardware {
                            "cpu_rgba_to_hardware_encoder"
                        } else {
                            "cpu_rgba_to_software_encoder"
                        }
                        .into(),
                        transfers: MediaTransferStats {
                            cpu_copy_bytes: 0,
                            cpu_conversion_input_bytes: input_bytes,
                            cpu_conversion_output_bytes: converted,
                            cpu_upload_bytes: if sink.hardware { converted } else { 0 },
                            ..Default::default()
                        },
                    }));
                    staged.push(None);
                    probes.push(None);
                }
                Some(VideoSink::Gif(mut sink)) => {
                    sink.encoder.palette()?;
                    use std::io::{Read, Write};
                    sink.spool.flush()?;
                    let mut spool = std::fs::File::open(&sink.spool_path)?;
                    let temp = stage_file(&leg.output)?;
                    sink.encoder.encode_start(temp.path())?;
                    let frame_bytes = width * height * 4;
                    let mut rgba = vec![0_u8; frame_bytes];
                    for _ in 0..times.len() {
                        checkpoint(0)?;
                        spool.read_exact(&mut rgba)?;
                        sink.encoder.encode_frame(&rgba)?;
                    }
                    sink.encoder.flush()?;
                    drop(sink);
                    let probe = self.probe(temp.path())?;
                    probe.verify_delivery(leg.profile, leg.snapshot.audio_layout)?;
                    // GIF stores frame delays in centiseconds; the container
                    // duration may drift by under one delay quantum per frame.
                    let tolerance = Rational::new(times.len() as i64 + 1, 100)?;
                    let duration = probe
                        .duration
                        .or_else(|| probe.streams[0].duration)
                        .ok_or_else(|| MediaError::Encode("missing GIF duration".into()))?;
                    let drift = duration.checked_sub(expected_video)?;
                    if drift >= tolerance || drift <= tolerance.checked_neg()? {
                        return Err(MediaError::Encode(format!(
                            "GIF duration differs from the export range: probed {duration:?} vs expected {expected_video:?}"
                        )));
                    }
                    video_reports.push(Some(MediaPathReport {
                        decoder: None,
                        encoder: Some("gif".into()),
                        execution: ExecutionKind::Software,
                        input_pixel_format: "rgba".into(),
                        output_pixel_format: "pal8".into(),
                        transfer_path: "cpu_rgba_to_indexed_gif".into(),
                        transfers: MediaTransferStats {
                            cpu_conversion_input_bytes: (frame_bytes * times.len()) as u64,
                            ..Default::default()
                        },
                    }));
                    staged.push(Some(temp));
                    probes.push(Some(probe));
                }
                None => {
                    video_reports.push(None);
                    staged.push(None);
                    probes.push(None);
                }
            }
        }
        // Elementary audio legs already wrote into their staged artifacts;
        // probe and verify each against its declared shape.
        let mut audio_stage_bytes = 0_u64;
        let mut video_stage_bytes = 0_u64;
        for (index, leg) in legs.iter().enumerate() {
            match leg.profile.shape() {
                DeliveryShape::AudioOnly => {
                    let Some(LegStage::Publish(temp)) = audio_files[index].take() else {
                        return Err(MediaError::Encode("missing staged audio output".into()));
                    };
                    audio_stage_bytes += std::fs::metadata(temp.path())?.len();
                    let probe = self.probe(temp.path())?;
                    probe.verify_delivery(leg.profile, leg.snapshot.audio_layout)?;
                    let slack = Rational::new(leg.profile.audio_frame_slack(), 48_000)?;
                    let expected = SAMPLE_RATE.sample_to_time(samples.end - samples.start)?;
                    let duration = probe
                        .streams
                        .first()
                        .and_then(|s| s.duration)
                        .or(probe.duration);
                    let duration_ok = match duration {
                        // Lossy elementary streams may omit stream duration.
                        None => leg.profile.audio_frame_slack() > 0,
                        Some(d) => {
                            d >= expected
                                && expected
                                    .checked_add(slack)
                                    .map(|max| d <= max)
                                    .unwrap_or(false)
                        }
                    };
                    if !duration_ok {
                        return Err(MediaError::Encode(
                            "encoded audio differs from requested duration".into(),
                        ));
                    }
                    staged[index] = Some(temp);
                    probes[index] = Some(probe);
                }
                DeliveryShape::Movie => {
                    let video_file = video_files[index]
                        .as_ref()
                        .ok_or_else(|| MediaError::Encode("missing video intermediate".into()))?;
                    let Some(LegStage::Intermediate(audio_path)) = &audio_files[index] else {
                        return Err(MediaError::Encode("missing audio intermediate".into()));
                    };
                    video_stage_bytes += std::fs::metadata(video_file)?.len();
                    audio_stage_bytes += std::fs::metadata(audio_path)?.len();
                    let video_probe = self.probe(video_file)?;
                    let audio_probe = self.probe(audio_path)?;
                    let audio_slack = Rational::new(leg.profile.audio_frame_slack(), 48_000)?;
                    if !video_probe
                        .streams
                        .iter()
                        .any(|s| s.kind == StreamKind::Video && s.duration == Some(expected_video))
                        || !audio_probe.streams.iter().any(|s| {
                            s.kind == StreamKind::Audio
                                && match s.duration {
                                    // WebM lossy intermediates may publish no
                                    // stream duration.
                                    None => leg.profile.audio_frame_slack() > 0,
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
                    let (temp, probe) = self.mux_movie_stage(
                        video_file,
                        audio_path,
                        &leg.output,
                        &render_hash,
                        &export_hashes[index],
                        leg.profile,
                        leg.snapshot.audio_layout,
                        &leg_chapters[index],
                    )?;
                    staged[index] = Some(temp);
                    probes[index] = Some(probe);
                }
                DeliveryShape::VideoOnly => {}
            }
        }
        checkpoint(metadata.len() as u64)?;
        // All legs verified; publish in request order. A failure here is still
        // atomic per destination and never reports partial success.
        let mut reports = Vec::with_capacity(legs.len());
        for (index, leg) in legs.iter().enumerate() {
            let temp = staged[index]
                .take()
                .ok_or_else(|| MediaError::Encode("missing staged deliverable".into()))?;
            temp.as_file().sync_all()?;
            publish_file(temp, &leg.output)?;
            reports.push(AvExportOutputReport {
                output: leg.output.clone(),
                profile: leg.profile,
                video: video_reports[index].take(),
                audio: audio_reports[index].take(),
                probe: probes[index]
                    .take()
                    .ok_or_else(|| MediaError::Encode("missing leg probe".into()))?,
                chapters: leg_chapters[index].clone(),
                warnings: std::mem::take(&mut leg_warnings[index]),
            });
        }
        let primary = reports[0].clone();
        Ok(AvExportReport {
            schema_version: 1,
            render_snapshot_hash: render_hash.clone(),
            export_snapshot_hash: export_hashes[0].clone(),
            audio_render_snapshot_hash: render_hash,
            audio_source: snapshot.audio,
            audio_profile_version: snapshot.schema_version,
            movie_profile: snapshot.movie_profile,
            request: request.clone(),
            sample_range: sample_range(request.range)?,
            frames: metadata,
            video: primary.video.clone(),
            audio: primary.audio.clone(),
            probe: primary.probe.clone(),
            outputs: reports,
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
/// One resolved delivery leg inside `export_delivery`: the primary output or
/// a `request.outputs` entry paired with its frozen export snapshot.
struct DeliveryLeg<'a> {
    snapshot: &'a AvExportSnapshot,
    output: PathBuf,
    profile: MovieProfile,
    background: [f32; 3],
    chapters_policy: ChapterPolicy,
}
/// Where an audio leg's encoder writes while the job is still staging.
enum LegStage {
    /// Movie intermediate inside the job's staging directory.
    Intermediate(PathBuf),
    /// Elementary deliverable staged beside its publish destination.
    Publish(tempfile::NamedTempFile),
}
impl LegStage {
    fn path(&self) -> &Path {
        match self {
            Self::Intermediate(path) => path.as_path(),
            Self::Publish(temp) => temp.path(),
        }
    }
}
/// A video leg's encoder plus its converted-frame state. Each sink owns one
/// full-frame buffer; the render pass fills it once per frame.
enum VideoSink<'a> {
    /// Movie intermediate encoder (`prores_ks`/`libsvtav1`/videotoolbox/dnxhd).
    Color(ColorSink<'a>),
    /// Two-phase indexed-color GIF path (ADR-0133).
    Gif(GifSink<'a>),
}
struct ColorSink<'a> {
    encoder: crate::ffi::NativeEncoder<'a>,
    encoder_name: String,
    hardware: bool,
    stride: usize,
    buffer: Vec<u8>,
    /// Previous frame held back so packet spans equal the distance to the
    /// next submitted tick; the final frame inherits the last observed span.
    pending: Option<(Vec<u8>, i64)>,
    last_span: i64,
    profile: MovieProfile,
    background: [f32; 3],
}
struct GifSink<'a> {
    encoder: crate::ffi::NativeGifEncoder<'a>,
    /// Phase-2 input: the same RGBA8 frames spooled to the destination volume.
    spool: std::io::BufWriter<std::fs::File>,
    spool_path: PathBuf,
    buffer: Vec<u8>,
    profile: MovieProfile,
    background: [f32; 3],
}
impl VideoSink<'_> {
    /// The full-frame buffer the tile callback fills plus this leg's pixel
    /// contract for the conversion.
    fn conversion_target(&mut self) -> (&mut Vec<u8>, usize, MovieProfile, [f32; 3]) {
        match self {
            Self::Color(sink) => (&mut sink.buffer, sink.stride, sink.profile, sink.background),
            Self::Gif(sink) => (&mut sink.buffer, 4, sink.profile, sink.background),
        }
    }
    /// Hand one converted frame to the encoder. Color encoders receive packet
    /// spans as the distance to the next submitted tick; GIF phase 1 folds the
    /// histogram and spools the frame for the deterministic second pass.
    fn deliver(&mut self, tick: i64) -> Result<(), MediaError> {
        use std::io::Write;
        match self {
            Self::Color(sink) => {
                let frame = std::mem::take(&mut sink.buffer);
                if let Some((mut previous, pts)) = sink.pending.replace((frame, tick)) {
                    sink.last_span = tick - pts;
                    sink.encoder.frame(&previous, pts, sink.last_span)?;
                    // Recycle the delivered allocation as the next buffer.
                    previous.clear();
                    sink.buffer = previous;
                }
                Ok(())
            }
            Self::Gif(sink) => {
                sink.encoder.frame(&sink.buffer)?;
                sink.spool.write_all(&sink.buffer)?;
                Ok(())
            }
        }
    }
}
/// MEDIA-004 (ADR-0133): authored chapter markers of the fixed sequence
/// target, clipped to `range` and rebased to output-relative time. Chapter
/// intervals run marker-to-marker (`[start, end)`); a chapter covering the
/// range start keeps it as its output `0`. Non-sequence targets and
/// non-chapter roles contribute nothing.
fn export_chapters(
    render: &RenderSnapshot,
    range: TimeRange,
) -> Result<Vec<MediaChapter>, MediaError> {
    let kronello_render::RenderTarget::Sequence { sequence } = render.target() else {
        return Ok(Vec::new());
    };
    let Some(sequence) = render.project().sequences.iter().find_map(|s| match s {
        DocumentObject::Known(s) if s.id == sequence => Some(s),
        _ => None,
    }) else {
        return Ok(Vec::new());
    };
    let mut points: Vec<(Rational, String)> = sequence
        .markers
        .iter()
        .filter(|m| m.role == kronello_model::MarkerRole::Chapter && m.time < range.end())
        .map(|m| (m.time, m.title.clone().unwrap_or_default()))
        .collect();
    points.sort_by_key(|(time, _)| *time);
    points.dedup_by_key(|(time, _)| *time);
    let mut chapters = Vec::with_capacity(points.len());
    for (index, (start, title)) in points.iter().enumerate() {
        let natural_end = points.get(index + 1).map(|(time, _)| *time);
        let start = (*start).max(range.start());
        let end = natural_end.unwrap_or(range.end()).min(range.end());
        if start >= end {
            continue;
        }
        chapters.push(MediaChapter {
            id: chapters.len() as i64,
            start: start.checked_sub(range.start())?,
            end: end.checked_sub(range.start())?,
            title: title.clone(),
        });
    }
    if chapters.len() > 1024 {
        return Err(MediaError::InvalidInput("chapter budget exceeded".into()));
    }
    Ok(chapters)
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
