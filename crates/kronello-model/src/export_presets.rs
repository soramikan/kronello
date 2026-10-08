//! Document-owned shared export presets (ADR-0130). These storage types mirror
//! the `render.submit` request shape field-for-field so a preset is a
//! destination-independent `JobRequest` equivalent. Leaf enums deliberately
//! reuse the wire names of `kronello_render::RenderTarget`, `OutputRegion`,
//! `RenderProfile`, `kronello_audio::AudioSourceMode`,
//! `kronello_media::DeliveryAudioCodec`, `kronello_render::HdrTransfer`,
//! `CutPolicy`/`TemporalSettings`, `kronello_service` `JobAudioClip` and
//! `JobOutput`; the service converts one way at submission, and a drift test
//! in `kronello-service` asserts every mirror variant decodes into the shared
//! request types.
use crate::{
    AssetId, CaptionFormat, ColorSpace, CompositionId, ExportPresetId, ProjectError, SequenceId,
};
use kronello_time::{FrameRate, Rational, Time, TimeRange};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Version of the preset payload schema. Payloads with a different version are
/// rejected by validation rather than silently interpreted or upgraded.
pub const EXPORT_PRESET_VERSION: u32 = 1;

/// Mirror of `kronello_render::RenderTarget` (`"kind"` tagged).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExportTarget {
    Composition { composition: CompositionId },
    Sequence { sequence: SequenceId },
}
/// Mirror of `kronello_render::OutputRegion`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportRegion {
    pub origin: [f64; 2],
    pub extent: [f64; 2],
    pub pixels: [u32; 2],
}
impl ExportRegion {
    /// The same bounds `OutputRegion::validate` enforces at render time.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self
            .origin
            .iter()
            .chain(&self.extent)
            .any(|v| !v.is_finite())
            || self.extent.iter().any(|v| *v <= 0.0)
            || self.pixels.contains(&0)
            || u64::from(self.pixels[0]) * u64::from(self.pixels[1]) > 33_177_600
        {
            return Err(ProjectError::InvalidDocument(
                "invalid output region or pixel budget".into(),
            ));
        }
        Ok(())
    }
}
/// Mirror of `kronello_audio::AudioSourceMode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportAudioMode {
    Document,
    #[default]
    Explicit,
    Silence,
}
/// Mirror of `kronello_media::DeliveryAudioCodec`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportAudioCodec {
    #[default]
    Alac,
    Aac,
    Opus,
}
/// Mirror of `kronello_render::HdrTransfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportTransfer {
    Pq,
    Hlg,
}
/// Mirror of `kronello_render::CutPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportCutPolicy {
    AvoidCrossing,
    AllowCrossing,
}
/// Mirror of `kronello_render::TemporalSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportTemporalSettings {
    pub frame_rate: FrameRate,
    pub shutter_angle: Rational,
    pub shutter_phase: Rational,
    pub samples: u32,
    pub cut_policy: ExportCutPolicy,
}
impl ExportTemporalSettings {
    /// The same bounds `TemporalSettings::validate` enforces at render time.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.samples == 0
            || self.samples > 4096
            || self.shutter_angle < Rational::ZERO
            || self.shutter_angle > Rational::from_integer(360)
        {
            return Err(ProjectError::InvalidDocument(
                "temporal samples must be 1..=4096 and shutter angle 0..=360".into(),
            ));
        }
        Ok(())
    }
}
/// Mirror of `kronello_render::HdrSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportHdrSettings {
    pub transfer: ExportTransfer,
}
/// Mirror of `kronello_render::RenderProfile`; defaults match its `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportProfile {
    pub working_space: ColorSpace,
    pub flatten_tolerance_px: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<ExportTemporalSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<ExportHdrSettings>,
}
impl Default for ExportProfile {
    fn default() -> Self {
        Self {
            working_space: ColorSpace::LinearRec709,
            flatten_tolerance_px: 0.02,
            temporal: None,
            hdr: None,
        }
    }
}
/// Mirror of `kronello_service::JobAudioClip`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportAudioClip {
    pub asset: AssetId,
    pub stream_index: u32,
    pub placement: TimeRange,
    pub source_in: Time,
    pub gain: f32,
}
/// Mirror of `kronello_service::JobOutput`; the same `"format"` tag, snake_case
/// variant names, field names and field defaults are required for the
/// one-to-one conversion at submission time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "format", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExportOutput {
    #[default]
    ImageSequence,
    ProResMov {
        #[serde(default)]
        audio: ExportAudioMode,
        #[serde(default = "movie_profile_v1")]
        profile_version: u32,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    ProResSdrFromHdrMov {
        profile_version: u32,
        #[serde(default)]
        audio: ExportAudioMode,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    ProResHdrMov {
        profile_version: u32,
        transfer: ExportTransfer,
        #[serde(default)]
        audio: ExportAudioMode,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    Av1Mp4 {
        profile_version: u32,
        #[serde(default)]
        audio: ExportAudioMode,
        #[serde(default)]
        audio_codec: ExportAudioCodec,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    H264Mov {
        profile_version: u32,
        #[serde(default)]
        audio: ExportAudioMode,
        #[serde(default)]
        audio_codec: ExportAudioCodec,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    HevcMov {
        profile_version: u32,
        #[serde(default)]
        audio: ExportAudioMode,
        #[serde(default)]
        audio_codec: ExportAudioCodec,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    Av1Webm {
        profile_version: u32,
        #[serde(default)]
        audio: ExportAudioMode,
        #[serde(default)]
        audio_codec: ExportAudioCodec,
        clips: Vec<ExportAudioClip>,
        background: [f32; 3],
    },
    /// Subtitle sidecar file; the target must be the same sequence.
    CaptionSidecar {
        sequence: SequenceId,
        caption_format: CaptionFormat,
    },
}
fn movie_profile_v1() -> u32 {
    1
}
impl ExportOutput {
    /// Explicit audio clips of the movie variants, for asset reference checks.
    pub fn audio_clips(&self) -> &[ExportAudioClip] {
        match self {
            Self::ImageSequence | Self::CaptionSidecar { .. } => &[],
            Self::ProResMov { clips, .. }
            | Self::ProResSdrFromHdrMov { clips, .. }
            | Self::ProResHdrMov { clips, .. }
            | Self::Av1Mp4 { clips, .. }
            | Self::H264Mov { clips, .. }
            | Self::HevcMov { clips, .. }
            | Self::Av1Webm { clips, .. } => clips,
        }
    }
}
/// A versioned, named, destination-independent export setting stored in the
/// project document. `destination` is supplied per submission, never stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPreset {
    /// Payload schema version; must equal [`EXPORT_PRESET_VERSION`].
    pub version: u32,
    pub id: ExportPresetId,
    pub name: String,
    /// Legacy single-composition shortcut; exactly one of `composition` and
    /// `target` must be set, matching the `RenderInput` rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<CompositionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ExportTarget>,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub region: ExportRegion,
    #[serde(default)]
    pub profile: ExportProfile,
    #[serde(default)]
    pub output: ExportOutput,
    /// Submission-time capability requirements, forwarded to the job request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_features: Vec<String>,
}
impl ExportPreset {
    /// Field-level checks independent of sibling document contents. Reference
    /// targets and audio clip assets are enforced by
    /// [`crate::Project::validate_storage`].
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != EXPORT_PRESET_VERSION {
            return Err(ProjectError::InvalidDocument(
                "unsupported export preset version".into(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(ProjectError::InvalidDocument(
                "export preset name must not be empty".into(),
            ));
        }
        if self.composition.is_some() == self.target.is_some() {
            return Err(ProjectError::InvalidDocument(
                "export preset requires exactly one of composition or target".into(),
            ));
        }
        self.region.validate()?;
        if let Some(temporal) = &self.profile.temporal {
            temporal.validate()?;
        }
        if !self.profile.flatten_tolerance_px.is_finite() {
            return Err(ProjectError::InvalidDocument(
                "flatten tolerance must be finite".into(),
            ));
        }
        for clip in self.output.audio_clips() {
            if !clip.gain.is_finite() {
                return Err(ProjectError::InvalidDocument(
                    "audio clip gain must be finite".into(),
                ));
            }
        }
        if let ExportOutput::CaptionSidecar { sequence, .. } = &self.output
            && self.target
                != Some(ExportTarget::Sequence {
                    sequence: *sequence,
                })
        {
            return Err(ProjectError::InvalidDocument(
                "caption_sidecar presets require a sequence target matching output.sequence".into(),
            ));
        }
        Ok(())
    }
}
