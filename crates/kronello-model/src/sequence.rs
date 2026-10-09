//! Exact timeline placement contracts, independent of execution backends.
use crate::*;
use kronello_time::{FrameRate, SampleRate, Time, TimeError, TimeMap, TimeMapPoint, TimeRange};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub id: SequenceId,
    pub extent: DesignExtent,
    pub frame_rate: FrameRate,
    pub audio_rate: SampleRate,
    pub working_space: ColorSpace,
    /// Authored bottom-to-top compositing order.
    pub tracks: Vec<Track>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<Transition>,
    /// Sequence-time annotations; bounded by the content extent end.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// In/Out share this range's start/end; absent means the whole sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_area: Option<TimeRange>,
    /// Implicit destination tracks for edits that take no explicit track, one
    /// per kind. Referenced tracks must exist and match the entry's kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<TargetTracks>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Track {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TrackState>,
    pub id: TrackId,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
}
/// Authored output switches plus the editing lock; `locked` defaults to off so
/// documents written before NLE-005 stay unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackState {
    pub visible: bool,
    pub muted: bool,
    /// Editing lock: mutations that would change this track or its clips are
    /// rejected during planning with `TRACK_LOCKED`.
    #[serde(default)]
    pub locked: bool,
}
impl Track {
    pub fn visible(&self) -> bool {
        self.state.is_none_or(|state| state.visible)
    }
    pub fn muted(&self) -> bool {
        self.state.is_some_and(|state| state.muted)
    }
    pub fn locked(&self) -> bool {
        self.state.is_some_and(|state| state.locked)
    }
}
/// Per-kind implicit edit destinations. A present entry must reference an
/// existing track of the matching kind; absent means untargeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetTracks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<TrackId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<TrackId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    /// Text cues rendered above every video track; never enters audio mixing.
    Caption,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: ClipId,
    pub source_ref: SourceRef,
    pub timeline_range: TimeRange,
    pub source_in: Time,
    pub time_map: TimeMap,
    /// Authored contribution switch. Disabled clips keep their timeline
    /// occupancy, links and metadata but are excluded from evaluation, video
    /// compositing, audio mixing, captions and transitions. Serialized only
    /// when off so documents written before NLE-005 roundtrip unchanged.
    #[serde(default = "clip_enabled", skip_serializing_if = "is_clip_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub audio_retime: AudioRetimePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_sampling: Option<ReverseSampling>,
    /// Absent in M2 documents means unity. Evaluated in source-local time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Box<Property>>,
    /// GUI-012 (ADR-0138): constant stereo balance in [-1, 1], absent means
    /// center. Applied to the clip's mixed output after volume, effects and
    /// fades; the basic (pre-AUDIO-004) mixer rejects it as unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<Box<Property>>,
    #[serde(default)]
    pub links: Vec<ClipId>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// FX-004 clip-local Bezier mask stack (ADR-0114). Masks multiply clip
    /// alpha after content drawing and before `effects`; empty on old documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
    /// Placement transform and effect parameters, evaluated in sequence time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
    /// Sequence-time annotations, confined to this placement's range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}
/// A single authored annotation in sequence time. Clip markers reference
/// sequence time (not clip-local source time) and stay inside the clip's
/// `timeline_range`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub id: MarkerId,
    pub time: Time,
    pub color: MarkerColor,
    /// MEDIA-004 (ADR-0133): semantic role. `chapter` entries transfer into
    /// chapter-capable delivery containers; absent in pre-M10 documents.
    #[serde(default, skip_serializing_if = "MarkerRole::is_standard")]
    pub role: MarkerRole,
    /// Chapter display title written to capable containers. `comment` stays
    /// the authoring annotation and is never transferred (ADR-0133).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}
/// Closed marker roles; transports never infer delivery semantics from text.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MarkerRole {
    /// Time annotation without delivery semantics.
    #[default]
    Standard,
    /// Output chapter boundary; the marker `title` is the chapter name.
    Chapter,
}
impl MarkerRole {
    fn is_standard(&self) -> bool {
        *self == Self::Standard
    }
}
/// Closed set; transports never guess a color from a label.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MarkerColor {
    #[default]
    Red,
    Green,
    Blue,
    Yellow,
    Purple,
    Cyan,
    Orange,
    White,
}
/// NLE-001 never performs implicit pitch/speed conversion.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AudioRetimePolicy {
    #[default]
    Reject,
    // AUDIO-004 v1: linear sample interpolation, with pitch following speed.
    ResampleV1,
    ReverseResampleV1,
    // AUDIO-010: deterministic WSOLA; the output length follows the time map
    // while the source pitch is preserved (ADR-0124). Hold segments (slope 0)
    // emit silence; reverse playback still requires `ReverseResampleV1`.
    PitchPreserveV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReverseSampling {
    ReverseGridV1,
}
/// The previous exact source-grid cell, including at integral source endpoints.
pub fn reverse_grid_time(source: Time, rate: Time) -> Result<Time, kronello_time::TimeError> {
    let position = source.checked_mul(rate)?;
    let floor = position.floor();
    let predecessor = if position == Time::from_integer(floor) {
        floor
            .checked_sub(1)
            .ok_or(kronello_time::TimeError::Overflow)?
    } else {
        floor
    };
    Time::from_integer(predecessor).checked_div(rate)
}
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRef {
    Composition {
        composition: CompositionId,
    },
    Asset {
        asset: AssetId,
        stream_index: u32,
    },
    Generator {
        generator: String,
        #[serde(default = "generator_version")]
        version: u32,
        #[serde(default = "generator_color")]
        color: Color,
    },
    Caption {
        caption: CaptionId,
    },
    /// FX-007 (ADR-0116): no payload; the clip applies `effects` to the
    /// composited lower video tracks across its timeline range.
    Adjustment,
    /// NLE-007 (ADR-0127): one active angle of a `Project.multicams` group.
    /// The clip's local source time lives in multicam-local time; the angle's
    /// `sync_offset` maps it to that angle's media time, so `clip.angle_switch`
    /// re-samples the same multicam interval through another stream.
    Multicam {
        multicam: MulticamId,
        angle: AngleId,
    },
}
// Decode variant payloads directly from JSON. Serde's internally-tagged Content
// buffer cannot preserve arbitrary-precision float values inside Color.
impl<'de> Deserialize<'de> for SourceRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        type Fields = std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>;
        struct Unique;
        impl<'de> serde::de::Visitor<'de> for Unique {
            type Value = Fields;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("source object with unique fields")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Fields, M::Error> {
                let mut fields = Fields::new();
                while let Some((key, value)) =
                    map.next_entry::<String, Box<serde_json::value::RawValue>>()?
                {
                    if fields.insert(key.clone(), value).is_some() {
                        return Err(M::Error::custom(format!("duplicate source field: {key}")));
                    }
                }
                Ok(fields)
            }
        }
        let mut fields = deserializer.deserialize_map(Unique)?;
        let kind = fields
            .remove("kind")
            .ok_or_else(|| D::Error::missing_field("kind"))?;
        let kind: String = serde_json::from_str(kind.get()).map_err(D::Error::custom)?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CompositionSource {
            composition: CompositionId,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct AssetSource {
            asset: AssetId,
            stream_index: u32,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct GeneratorSource {
            generator: String,
            #[serde(default = "generator_version")]
            version: u32,
            #[serde(default = "generator_color")]
            color: Color,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CaptionSource {
            caption: CaptionId,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct AdjustmentSource {}
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct MulticamSource {
            multicam: MulticamId,
            angle: AngleId,
        }
        let json = serde_json::to_string(&fields).map_err(D::Error::custom)?;
        match kind.as_str() {
            "composition" => {
                let p: CompositionSource = serde_json::from_str(&json).map_err(D::Error::custom)?;
                Ok(Self::Composition {
                    composition: p.composition,
                })
            }
            "asset" => {
                let p: AssetSource = serde_json::from_str(&json).map_err(D::Error::custom)?;
                Ok(Self::Asset {
                    asset: p.asset,
                    stream_index: p.stream_index,
                })
            }
            "generator" => {
                let p: GeneratorSource = serde_json::from_str(&json).map_err(D::Error::custom)?;
                Ok(Self::Generator {
                    generator: p.generator,
                    version: p.version,
                    color: p.color,
                })
            }
            "caption" => {
                let p: CaptionSource = serde_json::from_str(&json).map_err(D::Error::custom)?;
                Ok(Self::Caption { caption: p.caption })
            }
            "adjustment" => {
                serde_json::from_str::<AdjustmentSource>(&json).map_err(D::Error::custom)?;
                Ok(Self::Adjustment)
            }
            "multicam" => {
                let p: MulticamSource = serde_json::from_str(&json).map_err(D::Error::custom)?;
                Ok(Self::Multicam {
                    multicam: p.multicam,
                    angle: p.angle,
                })
            }
            _ => Err(D::Error::custom("unknown source kind")),
        }
    }
}
/// FX-007 adjustment clip semantic version pin (ADR-0116).
pub const ADJUSTMENT_VERSION: u32 = 1;
pub const SOLID_GENERATOR_ID: &str = "kronello.solid";
pub const GENERATOR_VERSION: u32 = 1;
fn generator_version() -> u32 {
    GENERATOR_VERSION
}
fn generator_color() -> Color {
    Color::from_srgb8([0; 3], None)
}
fn clip_enabled() -> bool {
    true
}
fn is_clip_enabled(enabled: &bool) -> bool {
    *enabled
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub outgoing: ClipId,
    pub incoming: ClipId,
    pub range: TimeRange,
    pub kind: TransitionKind,
    /// FX-003 per-kind payload. Absent in pre-FX-003 documents and required
    /// for all non-crossfade kinds; crossfade must not carry one (ADR-0109).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<TransitionParams>,
    pub version: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Crossfade,
    Wipe,
    Slide,
    Dip,
}
/// FX-003 transition payloads; the variant must match Transition::kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionParams {
    Wipe(WipeParams),
    Slide(SlideParams),
    Dip(DipParams),
}
/// Closed direction set shared by the wipe and slide v1 transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionDirection {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WipeParams {
    pub direction: TransitionDirection,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlideParams {
    pub direction: TransitionDirection,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DipParams {
    /// Straight authoring color composited at the transition midpoint.
    pub color: Color,
}
impl TransitionKind {
    /// FX-003: every kind except crossfade requires its matching payload.
    pub fn accepts(self, params: Option<TransitionParams>) -> bool {
        matches!(
            (self, params),
            (Self::Crossfade, None)
                | (Self::Wipe, Some(TransitionParams::Wipe(_)))
                | (Self::Slide, Some(TransitionParams::Slide(_)))
                | (Self::Dip, Some(TransitionParams::Dip(_)))
        )
    }
}
#[derive(Debug, Error)]
pub enum SequenceError {
    #[error("sequence source missing: {0}")]
    MissingSource(String),
    #[error("invalid clip/sequence: {0}")]
    Invalid(String),
    #[error("clips overlap on track {0}")]
    Overlap(TrackId),
    #[error("unsupported sequence feature: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Time(#[from] TimeError),
}
impl SequenceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingSource(_) => "SOURCE_MISSING",
            Self::Invalid(_) => "INVALID_CLIP",
            Self::Overlap(_) => "CLIP_OVERLAP",
            Self::Unsupported(_) => "UNSUPPORTED_FEATURE",
            Self::Time(TimeError::OutsideMapDomain) => "TIME_MAP_OUT_OF_DOMAIN",
            Self::Time(_) => "TIME_ERROR",
        }
    }
}
impl Clip {
    pub fn local_time(&self, parent: Time) -> Result<Time, SequenceError> {
        let mapped = self
            .time_map
            .map(parent.checked_sub(self.timeline_range.start())?)?;
        Ok(if self.reverse_sampling.is_some() {
            self.source_in.checked_sub(mapped)?
        } else {
            self.source_in.checked_add(mapped)?
        })
    }
    /// Cut only within the existing placement; preserve the speed at every point.
    pub fn trimmed(&self, range: TimeRange) -> Result<Self, SequenceError> {
        if range.is_empty()
            || range.start() < self.timeline_range.start()
            || range.end() > self.timeline_range.end()
        {
            return Err(SequenceError::Invalid(
                "trim must be a nonempty subset".into(),
            ));
        }
        let shift = range.start().checked_sub(self.timeline_range.start())?;
        let origin = self.time_map.map(shift)?;
        let duration = range.duration()?.as_time();
        let mut clip = self.clone();
        clip.timeline_range = range;
        clip.source_in = if self.reverse_sampling.is_some() {
            self.source_in.checked_sub(origin)?
        } else {
            self.source_in.checked_add(origin)?
        };
        clip.time_map = match &self.time_map {
            TimeMap::Linear(m) => TimeMap::linear(Time::ZERO, m.speed())?,
            TimeMap::PiecewiseLinear(m) => {
                let mut points = vec![TimeMapPoint {
                    parent: Time::ZERO,
                    local: Time::ZERO,
                }];
                for p in m.points() {
                    if p.parent > shift && p.parent < shift.checked_add(duration)? {
                        points.push(TimeMapPoint {
                            parent: p.parent.checked_sub(shift)?,
                            local: p.local.checked_sub(origin)?,
                        });
                    }
                }
                points.push(TimeMapPoint {
                    parent: duration,
                    local: self
                        .time_map
                        .map(shift.checked_add(duration)?)?
                        .checked_sub(origin)?,
                });
                TimeMap::piecewise_linear_with_interpolation(points, m.interpolation())?
            }
            _ => return Err(SequenceError::Unsupported("time map".into())),
        };
        // Markers cut away from the placement go away with the content.
        clip.markers.retain(|m| range.contains(m.time));
        Ok(clip)
    }
    /// Preserve source interval and local control values; rescale parent time.
    pub fn stretched(&self, range: TimeRange) -> Result<Self, SequenceError> {
        if range.is_empty() {
            return Err(SequenceError::Invalid("empty stretch".into()));
        }
        let old = self.timeline_range.duration()?.as_time();
        let new = range.duration()?.as_time();
        let mut clip = self.clone();
        clip.timeline_range = range;
        clip.time_map = match &self.time_map {
            TimeMap::Linear(m) => {
                TimeMap::linear(m.offset(), m.speed().checked_mul(old.checked_div(new)?)?)?
            }
            TimeMap::PiecewiseLinear(m) => TimeMap::piecewise_linear_with_interpolation(
                m.points()
                    .iter()
                    .map(|p| {
                        Ok(TimeMapPoint {
                            parent: p.parent.checked_mul(new.checked_div(old)?)?,
                            local: p.local,
                        })
                    })
                    .collect::<Result<Vec<_>, TimeError>>()?,
                m.interpolation(),
            )?,
            _ => return Err(SequenceError::Unsupported("time map".into())),
        };
        // Sequence-time markers keep their position inside the rescaled range.
        for marker in &mut clip.markers {
            marker.time = range.start().checked_add(
                marker
                    .time
                    .checked_sub(self.timeline_range.start())?
                    .checked_mul(new.checked_div(old)?)?,
            )?;
        }
        clip.markers.retain(|m| range.contains(m.time));
        Ok(clip)
    }
}
impl Sequence {
    pub fn validate(&self, project: &Project) -> Result<(), SequenceError> {
        if self.working_space == ColorSpace::Srgb || self.audio_rate != SampleRate::HZ_48000 {
            return Err(SequenceError::Unsupported(
                "linear working space and 48 kHz bus required".into(),
            ));
        }
        let ids: std::collections::BTreeSet<_> = self
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(|c| c.id)
            .collect();
        // Targeting references must resolve to an existing track of the
        // matching kind; a dangling or cross-kind target would silently steer
        // edits onto the wrong destination.
        if let Some(targets) = &self.targets {
            for (target, kind) in [
                (targets.video, TrackKind::Video),
                (targets.audio, TrackKind::Audio),
            ] {
                let Some(target) = target else { continue };
                let track = self
                    .tracks
                    .iter()
                    .find(|track| track.id == target)
                    .ok_or_else(|| SequenceError::MissingSource(target.to_string()))?;
                if track.kind != kind {
                    return Err(SequenceError::Invalid("target track kind mismatch".into()));
                }
            }
        }
        let mask_registry = SchemaRegistry::with_builtin();
        for (index, transition) in self.transitions.iter().enumerate() {
            let pair = self
                .tracks
                .iter()
                .find_map(|track| {
                    let a = track.clips.iter().find(|c| c.id == transition.outgoing)?;
                    let b = track.clips.iter().find(|c| c.id == transition.incoming)?;
                    Some((track, a, b))
                })
                .ok_or_else(|| {
                    SequenceError::Invalid("transition requires clips on one track".into())
                })?;
            let (track, a, b) = pair;
            // Caption cues never overlap; crossfades are a video-only transition.
            if track.kind == TrackKind::Caption {
                return Err(SequenceError::Invalid(
                    "transitions require a non-caption track".into(),
                ));
            }
            if a.timeline_range.start() >= b.timeline_range.start()
                || a.timeline_range.end() >= b.timeline_range.end()
                || a.timeline_range.intersection(b.timeline_range) != Some(transition.range)
                || self.transitions[..index].iter().any(|old| {
                    old.outgoing == transition.outgoing && old.incoming == transition.incoming
                })
                || track.clips.iter().any(|c| {
                    c.id != a.id
                        && c.id != b.id
                        && c.timeline_range.intersection(transition.range).is_some()
                })
            {
                return Err(SequenceError::Invalid("invalid transition overlap".into()));
            }
            // Kind/params pairing is structural: mismatches are invalid
            // regardless of which evaluator executes the transition.
            if !transition.kind.accepts(transition.params) {
                return Err(SequenceError::Invalid(
                    "transition kind and params mismatch".into(),
                ));
            }
        }
        for track in &self.tracks {
            for (index, clip) in track.clips.iter().enumerate() {
                if let Some(volume) = &clip.volume {
                    validate_volume(volume).map_err(|e| SequenceError::Invalid(e.to_string()))?;
                }
                if let Some(pan) = &clip.pan {
                    validate_pan(pan).map_err(|e| SequenceError::Invalid(e.to_string()))?;
                }
                if clip.timeline_range.is_empty()
                    || clip.source_in < Time::ZERO
                    || clip
                        .links
                        .iter()
                        .any(|id| !ids.contains(id) || *id == clip.id)
                {
                    return Err(SequenceError::Invalid(
                        "empty range, negative source_in or invalid link".into(),
                    ));
                }
                if clip
                    .links
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != clip.links.len()
                    || clip.links.iter().any(|id| {
                        !self
                            .tracks
                            .iter()
                            .flat_map(|t| &t.clips)
                            .any(|c| c.id == *id && c.links.contains(&clip.id))
                    })
                {
                    return Err(SequenceError::Invalid(
                        "links must be unique and reciprocal".into(),
                    ));
                }
                crate::BlendMode::from_properties(&clip.properties)
                    .map_err(|e| SequenceError::Invalid(e.to_string()))?;
                let mut properties = std::collections::BTreeSet::new();
                for p in &clip.properties {
                    if !properties.insert(p.id()) {
                        return Err(SequenceError::Invalid("clip property identity".into()));
                    }
                }
                if clip.effects.len() > 16 {
                    return Err(SequenceError::Invalid("clip effect budget".into()));
                }
                // FX-004: masks are a clip-local alpha stack on the video
                // pipeline only; audio and caption placements reject them.
                if track.kind != TrackKind::Video && !clip.masks.is_empty() {
                    return Err(SequenceError::Invalid("non-video clip masks".into()));
                }
                validate_clip_masks(&clip.masks, &clip.properties, &mask_registry)?;
                let mut clip_markers = std::collections::BTreeSet::new();
                for marker in &clip.markers {
                    if !clip_markers.insert(marker.id) {
                        return Err(SequenceError::Invalid(
                            "duplicate marker id in a clip".into(),
                        ));
                    }
                    if !clip.timeline_range.contains(marker.time) {
                        return Err(SequenceError::Invalid(
                            "clip marker outside its timeline range".into(),
                        ));
                    }
                }
                // Unknown effects remain storable and fail when the selected target executes.
                if track.clips[..index].iter().any(|c| {
                    c.timeline_range
                        .intersection(clip.timeline_range)
                        .is_some_and(|overlap| {
                            !self.transitions.iter().any(|tr| {
                                tr.range == overlap
                                    && ((tr.outgoing == c.id && tr.incoming == clip.id)
                                        || (tr.outgoing == clip.id && tr.incoming == c.id))
                            })
                        })
                }) {
                    return Err(SequenceError::Overlap(track.id));
                }
                let start = clip.local_time(clip.timeline_range.start())?;
                let end = clip.local_time(clip.timeline_range.end())?;
                if start < Time::ZERO {
                    return Err(SequenceError::Invalid("negative source time".into()));
                }
                if clip.reverse_sampling.is_some() {
                    if !matches!(&clip.time_map, TimeMap::Linear(m) if m.speed() > Time::ZERO)
                        || start <= end
                        || end < Time::ZERO
                    {
                        return Err(SequenceError::Invalid("reverse_grid_v1 requires a positive Linear magnitude map and nonnegative source envelope".into()));
                    }
                    if clip.audio_retime != AudioRetimePolicy::ReverseResampleV1 {
                        return Err(SequenceError::Unsupported(
                            "reverse_grid_v1 requires explicit reverse_resample_v1".into(),
                        ));
                    }
                } else if clip.audio_retime == AudioRetimePolicy::ReverseResampleV1 {
                    return Err(SequenceError::Unsupported(
                        "reverse_resample_v1 requires reverse_grid_v1".into(),
                    ));
                }
                // Caption clips live only on caption tracks, and caption tracks
                // accept nothing else (ADR-0107). Generic overlap rejection
                // above keeps cues nonoverlapping since transitions are banned.
                if (track.kind == TrackKind::Caption)
                    != matches!(&clip.source_ref, SourceRef::Caption { .. })
                {
                    return Err(SequenceError::Invalid(
                        "caption clips require a caption track".into(),
                    ));
                }
                // TRACK-003 (ADR-0123): intermediate-frame synthesis applies
                // only to forward-direction video-track asset clips. Every
                // other source keeps the legacy single-frame sample and must
                // not carry the mode.
                if clip.time_map.interpolation().is_some()
                    && (!matches!(&clip.source_ref, SourceRef::Asset { .. })
                        || clip.reverse_sampling.is_some()
                        || track.kind != TrackKind::Video)
                {
                    return Err(SequenceError::Unsupported(
                        "frame interpolation requires a forward video asset clip".into(),
                    ));
                }
                match &clip.source_ref {
                    SourceRef::Caption { caption } => {
                        if clip.source_in != Time::ZERO
                            || !matches!(&clip.time_map, TimeMap::Linear(m) if m.offset() == Time::ZERO && m.speed() == kronello_time::Rational::ONE)
                            || clip.reverse_sampling.is_some()
                            || clip.audio_retime != AudioRetimePolicy::Reject
                            || clip.volume.is_some()
                            || clip.pan.is_some()
                        {
                            return Err(SequenceError::Invalid(
                                "caption clip requires zero source_in and an identity time map without retime, reverse or gain".into(),
                            ));
                        }
                        if project.captions.iter().any(
                            |c| matches!(c, DocumentObject::Opaque(c) if c.id == caption.as_uuid()),
                        ) {
                            continue;
                        }
                        if !project
                            .captions
                            .iter()
                            .any(|c| matches!(c, DocumentObject::Known(c) if c.id == *caption))
                        {
                            return Err(SequenceError::MissingSource(caption.to_string()));
                        }
                    }
                    SourceRef::Composition { composition } => {
                        if project.compositions.iter().any(|c| matches!(c, DocumentObject::Opaque(c) if c.id == composition.as_uuid())) { continue; }
                        let source = project
                            .compositions
                            .iter()
                            .find_map(|c| match c {
                                DocumentObject::Known(c) if c.id == *composition => Some(c),
                                _ => None,
                            })
                            .ok_or_else(|| SequenceError::MissingSource(composition.to_string()))?;
                        if end > source.duration.as_time()
                            || (clip.reverse_sampling.is_some()
                                && start > source.duration.as_time())
                        {
                            return Err(SequenceError::Invalid(
                                "composition track or source bounds".into(),
                            ));
                        }
                    }
                    SourceRef::Asset {
                        asset,
                        stream_index,
                    } => {
                        if track.kind == TrackKind::Audio
                            && clip.audio_retime == AudioRetimePolicy::Reject
                            && !matches!(&clip.time_map, TimeMap::Linear(m) if m.speed() == kronello_time::Rational::ONE)
                        {
                            return Err(SequenceError::Unsupported(
                                "retimed audio (audio_retime=reject)".into(),
                            ));
                        }
                        if project.assets.iter().any(
                            |a| matches!(a, DocumentObject::Opaque(a) if a.id == asset.as_uuid()),
                        ) {
                            continue;
                        }
                        let source = project
                            .assets
                            .iter()
                            .find_map(|a| match a {
                                DocumentObject::Known(a) if a.id == *asset => Some(a),
                                _ => None,
                            })
                            .ok_or_else(|| SequenceError::MissingSource(asset.to_string()))?;
                        let stream = source
                            .streams
                            .iter()
                            .find(|s| s.index == *stream_index)
                            .ok_or_else(|| SequenceError::MissingSource("asset stream".into()))?;
                        let origin = if track.kind == TrackKind::Video {
                            stream.start_time.unwrap_or(Time::ZERO)
                        } else {
                            // Audio source time remains relative to decoded sample zero.
                            Time::ZERO
                        };
                        if start < origin
                            || (clip.reverse_sampling.is_some() && end < origin)
                            || stream
                                .duration
                                .map(|duration| origin.checked_add(duration))
                                .transpose()?
                                .is_some_and(|limit| {
                                    end > limit
                                        || (clip.reverse_sampling.is_some() && start > limit)
                                })
                        {
                            return Err(SequenceError::Invalid("asset source bounds".into()));
                        }
                        if clip.reverse_sampling.is_some() {
                            let limit = stream.duration.ok_or_else(|| {
                                SequenceError::Unsupported(
                                    "reverse requires locked source duration".into(),
                                )
                            })?;
                            if track.kind == TrackKind::Audio {
                                let first = clip
                                    .timeline_range
                                    .start()
                                    .checked_mul(Time::from_integer(48_000))?
                                    .floor();
                                let end = clip
                                    .timeline_range
                                    .end()
                                    .checked_mul(Time::from_integer(48_000))?
                                    .floor();
                                if first < end {
                                    for sample in [first, end - 1] {
                                        let position = clip
                                            .local_time(Time::new(sample, 48_000)?)?
                                            .checked_mul(Time::from_integer(48_000))?
                                            .checked_sub(Time::ONE)?;
                                        let floor = position.floor();
                                        let last = if position == Time::from_integer(floor) {
                                            floor
                                        } else {
                                            floor
                                                .checked_add(1)
                                                .ok_or(kronello_time::TimeError::Overflow)?
                                        };
                                        if floor < 0 || Time::new(last, 48_000)? >= limit {
                                            return Err(SequenceError::Invalid("reverse audio interpolation neighbor outside source bounds".into()));
                                        }
                                    }
                                }
                            } else if source.kind == AssetKind::Video
                                && (stream.width.is_none() || stream.height.is_none())
                            {
                                return Err(SequenceError::Unsupported(
                                    "reverse requires locked video dimensions".into(),
                                ));
                            }
                        }
                        if (track.kind == TrackKind::Audio
                            && !matches!(source.kind, AssetKind::Audio | AssetKind::Video))
                            || (track.kind == TrackKind::Video
                                && !matches!(source.kind, AssetKind::Video | AssetKind::Image))
                        {
                            return Err(SequenceError::Invalid(
                                "asset kind does not match track".into(),
                            ));
                        }
                        // TRACK-003: flow synthesis needs decoded neighbor
                        // frames; only video streams provide them.
                        if clip.time_map.interpolation().is_some()
                            && source.kind != AssetKind::Video
                        {
                            return Err(SequenceError::Unsupported(
                                "frame interpolation requires a video asset".into(),
                            ));
                        }
                    }
                    SourceRef::Generator { .. } => (),
                    SourceRef::Adjustment => {
                        // FX-007 (ADR-0116): video-track-only, no source window,
                        // no retime/reverse/audio gain; the clip's timeline
                        // range alone scopes the effect pass over lower video.
                        if track.kind != TrackKind::Video
                            || clip.source_in != Time::ZERO
                            || !matches!(&clip.time_map, TimeMap::Linear(m) if m.offset() == Time::ZERO && m.speed() == kronello_time::Rational::ONE)
                            || clip.reverse_sampling.is_some()
                            || clip.audio_retime != AudioRetimePolicy::Reject
                            || clip.volume.is_some()
                            || clip.pan.is_some()
                        {
                            return Err(SequenceError::Invalid(
                                "adjustment clip requires a video track, zero source_in and an identity time map without retime, reverse or gain".into(),
                            ));
                        }
                    }
                    SourceRef::Multicam { multicam, angle } => {
                        // NLE-007 (ADR-0127): multicam clips carry picture only
                        // and live on video tracks. Angle audio enters the
                        // program through ordinary asset clips.
                        if track.kind != TrackKind::Video {
                            return Err(SequenceError::Invalid(
                                "multicam clips require a video track".into(),
                            ));
                        }
                        let group = project
                            .multicams
                            .iter()
                            .find(|group| group.id == *multicam)
                            .ok_or_else(|| SequenceError::MissingSource(multicam.to_string()))?;
                        let angle = group
                            .angle(*angle)
                            .ok_or_else(|| SequenceError::MissingSource(angle.to_string()))?;
                        if project.assets.iter().any(
                            |a| matches!(a, DocumentObject::Opaque(a) if a.id == angle.asset.as_uuid()),
                        ) {
                            continue;
                        }
                        let source = project
                            .assets
                            .iter()
                            .find_map(|a| match a {
                                DocumentObject::Known(a) if a.id == angle.asset => Some(a),
                                _ => None,
                            })
                            .ok_or_else(|| SequenceError::MissingSource(angle.asset.to_string()))?;
                        let stream = source
                            .streams
                            .iter()
                            .find(|s| s.index == angle.stream_index)
                            .ok_or_else(|| SequenceError::MissingSource("asset stream".into()))?;
                        if !matches!(source.kind, AssetKind::Video | AssetKind::Image) {
                            return Err(SequenceError::Invalid(
                                "asset kind does not match track".into(),
                            ));
                        }
                        if source.kind == AssetKind::Video {
                            // Angle media time = multicam-local source time
                            // shifted by the angle's sync_offset.
                            let media_start = start.checked_add(angle.sync_offset)?;
                            let media_end = end.checked_add(angle.sync_offset)?;
                            let origin = stream.start_time.unwrap_or(Time::ZERO);
                            if media_start < origin
                                || (clip.reverse_sampling.is_some() && media_end < origin)
                                || stream
                                    .duration
                                    .map(|duration| origin.checked_add(duration))
                                    .transpose()?
                                    .is_some_and(|limit| {
                                        media_end > limit
                                            || (clip.reverse_sampling.is_some()
                                                && media_start > limit)
                                    })
                            {
                                return Err(SequenceError::Invalid("asset source bounds".into()));
                            }
                            if clip.reverse_sampling.is_some() {
                                stream.duration.ok_or_else(|| {
                                    SequenceError::Unsupported(
                                        "reverse requires locked source duration".into(),
                                    )
                                })?;
                                if stream.width.is_none() || stream.height.is_none() {
                                    return Err(SequenceError::Unsupported(
                                        "reverse requires locked video dimensions".into(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        // The authored extent is content-derived: sequence markers and the
        // In/Out work area may not point beyond the last clip end. An empty
        // work area is not a valid In/Out pair.
        let content_end = self
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(|c| c.timeline_range.end())
            .max()
            .unwrap_or(Time::ZERO);
        let mut sequence_markers = std::collections::BTreeSet::new();
        for marker in &self.markers {
            if !sequence_markers.insert(marker.id) {
                return Err(SequenceError::Invalid(
                    "duplicate marker id in a sequence".into(),
                ));
            }
            if marker.time < Time::ZERO || marker.time > content_end {
                return Err(SequenceError::Invalid(
                    "sequence marker outside the content extent".into(),
                ));
            }
        }
        if let Some(area) = &self.work_area
            && (area.is_empty() || area.end() > content_end)
        {
            return Err(SequenceError::Invalid("invalid work area".into()));
        }
        Ok(())
    }
}

/// Shared clip/media gain Property contract. Curves are resolved at execution.
pub fn validate_volume(property: &Property) -> Result<(), ModelError> {
    property.validate(&SchemaRegistry::with_builtin())?;
    if property.descriptor().key.as_str() != "kronello.audio.volume" {
        return Err(ModelError::SourceNotAllowed);
    }
    Ok(())
}
/// Shared clip pan contract (GUI-012): constant scalar in [-1, 1]. The
/// descriptor's capability flags already reject curves, expressions and
/// modifiers; only a Constant source remains.
pub fn validate_pan(property: &Property) -> Result<(), ModelError> {
    property.validate(&SchemaRegistry::with_builtin())?;
    if property.descriptor().key.as_str() != "kronello.audio.pan" {
        return Err(ModelError::SourceNotAllowed);
    }
    if !matches!(property.source(), PropertySource::Constant(_)) {
        return Err(ModelError::SourceNotAllowed);
    }
    Ok(())
}
