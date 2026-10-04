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
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: ClipId,
    pub source_ref: SourceRef,
    pub timeline_range: TimeRange,
    pub source_in: Time,
    pub time_map: TimeMap,
    #[serde(default)]
    pub audio_retime: AudioRetimePolicy,
    #[serde(default)]
    pub links: Vec<ClipId>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Placement transform and effect parameters, evaluated in sequence time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
}
/// NLE-001 never performs implicit pitch/speed conversion.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AudioRetimePolicy {
    #[default]
    Reject,
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
            _ => Err(D::Error::custom("unknown source kind")),
        }
    }
}
pub const SOLID_GENERATOR_ID: &str = "kronello.solid";
pub const GENERATOR_VERSION: u32 = 1;
fn generator_version() -> u32 {
    GENERATOR_VERSION
}
fn generator_color() -> Color {
    Color::from_srgb8([0; 3], None)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub outgoing: ClipId,
    pub incoming: ClipId,
    pub range: TimeRange,
    pub kind: TransitionKind,
    pub version: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Crossfade,
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
        Ok(self.source_in.checked_add(
            self.time_map
                .map(parent.checked_sub(self.timeline_range.start())?)?,
        )?)
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
        clip.source_in = self.source_in.checked_add(origin)?;
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
                TimeMap::piecewise_linear(points)?
            }
            _ => return Err(SequenceError::Unsupported("time map".into())),
        };
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
            TimeMap::PiecewiseLinear(m) => TimeMap::piecewise_linear(
                m.points()
                    .iter()
                    .map(|p| {
                        Ok(TimeMapPoint {
                            parent: p.parent.checked_mul(new.checked_div(old)?)?,
                            local: p.local,
                        })
                    })
                    .collect::<Result<Vec<_>, TimeError>>()?,
            )?,
            _ => return Err(SequenceError::Unsupported("time map".into())),
        };
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
            if track.kind != TrackKind::Video
                || a.timeline_range.start() >= b.timeline_range.start()
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
                return Err(SequenceError::Invalid("invalid crossfade overlap".into()));
            }
        }
        for track in &self.tracks {
            for (index, clip) in track.clips.iter().enumerate() {
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
                let mut properties = std::collections::BTreeSet::new();
                for p in &clip.properties {
                    if !properties.insert(p.id()) {
                        return Err(SequenceError::Invalid("clip property identity".into()));
                    }
                }
                if track.kind == TrackKind::Audio
                    && (!clip.effects.is_empty() || !clip.properties.is_empty())
                {
                    return Err(SequenceError::Unsupported(
                        "audio clip properties/effects".into(),
                    ));
                }
                if clip.effects.len() > 16 {
                    return Err(SequenceError::Invalid("clip effect budget".into()));
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
                match &clip.source_ref {
                    SourceRef::Composition { composition } => {
                        if track.kind != TrackKind::Video {
                            return Err(SequenceError::Invalid(
                                "composition requires video track".into(),
                            ));
                        }
                        if project.compositions.iter().any(|c| matches!(c, DocumentObject::Opaque(c) if c.id == composition.as_uuid())) { continue; }
                        let source = project
                            .compositions
                            .iter()
                            .find_map(|c| match c {
                                DocumentObject::Known(c) if c.id == *composition => Some(c),
                                _ => None,
                            })
                            .ok_or_else(|| SequenceError::MissingSource(composition.to_string()))?;
                        if end > source.duration.as_time() {
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
                            || stream
                                .duration
                                .map(|duration| origin.checked_add(duration))
                                .transpose()?
                                .is_some_and(|limit| end > limit)
                        {
                            return Err(SequenceError::Invalid("asset source bounds".into()));
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
                    }
                    SourceRef::Generator { .. } => {
                        if track.kind != TrackKind::Video {
                            return Err(SequenceError::Invalid(
                                "generator requires video track".into(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
