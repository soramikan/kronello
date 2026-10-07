//! Timeline edits reuse the common transaction planner and selective Undo.
use crate::*;
use kronello_model::*;
use kronello_store::ChangedKey;
use kronello_time::{Time, TimeMap, TimeMapPoint, TimeRange};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineCommand {
    TrackStateSet {
        sequence: SequenceId,
        track: TrackId,
        state: TrackState,
    },
    ClipTimeSet {
        sequence: SequenceId,
        clip: ClipId,
        source_in: kronello_time::Time,
        time_map: TimeMap,
        audio_retime: AudioRetimePolicy,
        #[serde(default)]
        reverse_sampling: Option<ReverseSampling>,
    },
    SequenceCreate {
        sequence: Sequence,
    },
    ClipPlace {
        sequence: SequenceId,
        track: TrackId,
        clip: Box<Clip>,
    },
    ClipTrim {
        sequence: SequenceId,
        clip: ClipId,
        range: TimeRange,
    },
    /// Split strictly inside the placement, without implicit linked/transition edits.
    ClipSplit {
        sequence: SequenceId,
        clip: ClipId,
        time: kronello_time::Time,
        right_clip: ClipId,
    },
    ClipStretch {
        sequence: SequenceId,
        clip: ClipId,
        range: TimeRange,
    },
    /// Move one placement or its transitive reciprocal link component.
    ClipMove {
        sequence: SequenceId,
        clip: ClipId,
        delta: kronello_time::Time,
        linked: bool,
    },
    /// Slide the source window inside an unchanged placement. `linked` slips
    /// the complete reciprocal component so linked audio follows video.
    ClipSlip {
        sequence: SequenceId,
        clip: ClipId,
        delta: kronello_time::Time,
        linked: bool,
    },
    /// Move the selected placement(s) by delta while the adjacent clips on the
    /// same tracks absorb the shift through their source handles. Every
    /// selected run requires an adjacent clip on both sides.
    ClipSlide {
        sequence: SequenceId,
        clip: ClipId,
        delta: kronello_time::Time,
        linked: bool,
    },
    /// Move the shared edit point after `clip`; each selected placement's
    /// directly adjacent successor trims or extends symmetrically.
    ClipRoll {
        sequence: SequenceId,
        clip: ClipId,
        delta: kronello_time::Time,
        linked: bool,
    },
    /// Remove one placement or its complete reciprocal link component and keep
    /// the resulting gap. Incident transitions must be removed explicitly.
    ClipDelete {
        sequence: SequenceId,
        clip: ClipId,
        linked: bool,
    },
    /// Delete `range` on the listed tracks and close the gap by shifting all
    /// later content by the range's duration. A clip straddling both range
    /// edges is rejected; a partially covered clip trims the covered side.
    /// `linked` expands affected clips to complete reciprocal components,
    /// which are evaluated by their own overlap with `range` on any track.
    RippleDelete {
        sequence: SequenceId,
        tracks: Vec<TrackId>,
        range: TimeRange,
        linked: bool,
    },
    /// Place `clip` at its `timeline_range.start()`, shifting every clip at or
    /// after that point on the target track right by the clip's duration. The
    /// typed gap-opening counterpart of `ClipOverwrite`; a clip straddling the
    /// insertion point makes the whole operation fail. `linked` expands the
    /// shifted set to complete reciprocal components across tracks.
    ClipInsert {
        sequence: SequenceId,
        track: TrackId,
        clip: Box<Clip>,
        linked: bool,
    },
    /// Place `clip` over its `timeline_range`, replacing covered placements or
    /// covered split segments without moving neighbours. The typed overwrite
    /// counterpart of `ClipInsert`; transitions sharing the range conflict.
    ClipOverwrite {
        sequence: SequenceId,
        track: TrackId,
        clip: Box<Clip>,
    },
    /// Upsert a sequence marker (`clip` absent) or a marker scoped to that
    /// clip's timeline range. Marker times are always sequence times.
    MarkerSet {
        sequence: SequenceId,
        #[serde(default)]
        clip: Option<ClipId>,
        marker: Marker,
    },
    MarkerRemove {
        sequence: SequenceId,
        #[serde(default)]
        clip: Option<ClipId>,
        marker: MarkerId,
    },
    /// Move a marker within its owning list; recolor or recomment through
    /// `MarkerSet` with the same marker id.
    MarkerMove {
        sequence: SequenceId,
        #[serde(default)]
        clip: Option<ClipId>,
        marker: MarkerId,
        time: Time,
    },
    /// Set or clear the In/Out work area. Rendering only observes the explicit
    /// `SequenceRenderRequest.range`; the service never reads this field.
    WorkAreaSet {
        sequence: SequenceId,
        #[serde(default)]
        work_area: Option<TimeRange>,
    },
    /// Replace the selected clips' links with one reciprocal group. Old edges
    /// incident to the group are removed at both endpoints.
    ClipLink {
        sequence: SequenceId,
        clips: Vec<ClipId>,
    },
    /// Shift placements starting at/after pivot on explicit tracks. Reject a
    /// clip straddling pivot. linked=true includes complete link components.
    Ripple {
        sequence: SequenceId,
        tracks: Vec<TrackId>,
        pivot: kronello_time::Time,
        delta: kronello_time::Time,
        linked: bool,
    },
    TransitionSet {
        sequence: SequenceId,
        transition: Transition,
    },
    TransitionRemove {
        sequence: SequenceId,
        outgoing: ClipId,
        incoming: ClipId,
    },
    ClipSetEffects {
        sequence: SequenceId,
        clip: ClipId,
        properties: Vec<Property>,
        effects: Vec<Effect>,
    },
    ClipSetVolume {
        sequence: SequenceId,
        clip: ClipId,
        volume: Option<Property>,
    },
    InstanceRetime {
        composition: CompositionId,
        node: NodeId,
        time_map: TimeMap,
    },
    TemplateInstanceRetime {
        instance: CompositionInstanceId,
        duration: kronello_time::Duration,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceQueryRequest {
    pub project: PathBuf,
    pub sequence: SequenceId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClipKind {
    Video,
    Image,
    Audio,
    Composition,
    Generator,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipQuery {
    pub track: TrackId,
    pub kind: ClipKind,
    pub clip: Clip,
    pub video_color: Option<kronello_media::VideoColorPolicy>,
    pub unsupported_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceQueryResult {
    pub revision: String,
    pub sequence: Sequence,
    pub clips: Vec<ClipQuery>,
    pub asset_status: Vec<AssetStatus>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetAvailability {
    PresentUnverified,
    Missing,
    Error,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetStatus {
    pub asset: AssetId,
    pub availability: AssetAvailability,
    pub size_bytes: Option<u64>,
    pub error: Option<ServiceError>,
}
pub(crate) fn sequence_query(
    request: SequenceQueryRequest,
) -> Result<SequenceQueryResult, ServiceError> {
    let store = open_existing(&request.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let mut project = snapshot.document;
    let sequence = sequence_mut(&mut project, request.sequence)?.clone();
    let asset_status = project
        .assets
        .iter()
        .filter_map(|object| {
            let DocumentObject::Known(asset) = object else {
                return None;
            };
            let status = match kronello_media::locate_asset(asset, &request.project) {
                Ok(located) => AssetStatus {
                    asset: asset.id,
                    availability: AssetAvailability::PresentUnverified,
                    size_bytes: Some(located.size_bytes),
                    error: None,
                },
                Err(error) => AssetStatus {
                    asset: asset.id,
                    availability: if error.code() == "ASSET_MISSING" {
                        AssetAvailability::Missing
                    } else {
                        AssetAvailability::Error
                    },
                    size_bytes: None,
                    error: Some(error.into()),
                },
            };
            Some(status)
        })
        .collect();
    let mut clips = vec![];
    for track in &sequence.tracks {
        for clip in &track.clips {
            let mut video_color = None;
            let mut unsupported_reason = None;
            let kind = match &clip.source_ref {
                SourceRef::Composition { .. } => ClipKind::Composition,
                SourceRef::Generator {
                    generator,
                    version,
                    color,
                } => {
                    let supported = if track.kind == TrackKind::Audio {
                        matches!(
                            generator.as_str(),
                            kronello_audio::AUDIO_GENERATOR_SILENCE
                                | kronello_audio::AUDIO_GENERATOR_TONE
                        ) && *version == 1
                            && *color == Color::from_srgb8([0; 3], None)
                    } else {
                        generator == SOLID_GENERATOR_ID && *version == GENERATOR_VERSION
                    };
                    if !supported {
                        unsupported_reason =
                            Some("UNSUPPORTED_FEATURE: generator id/version".into());
                    }
                    ClipKind::Generator
                }
                SourceRef::Asset {
                    asset,
                    stream_index,
                } => {
                    let asset = project
                        .assets
                        .iter()
                        .find_map(|a| match a {
                            DocumentObject::Known(a) if a.id == *asset => Some(a),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            ServiceError::new("ASSET_MISSING", "asset metadata missing")
                        })?;
                    let kind = if track.kind == TrackKind::Audio {
                        ClipKind::Audio
                    } else if asset.kind == AssetKind::Image {
                        ClipKind::Image
                    } else {
                        ClipKind::Video
                    };
                    if kind != ClipKind::Audio {
                        let stream = asset
                            .streams
                            .iter()
                            .find(|s| s.index == *stream_index)
                            .ok_or_else(|| {
                                ServiceError::new("SOURCE_MISSING", "asset stream missing")
                            })?;
                        match kronello_media::video_color_policy(stream) {
                            Ok(policy) => video_color = Some(policy),
                            Err(error) => {
                                unsupported_reason = Some(format!("{}: {error}", error.code()))
                            }
                        }
                    }
                    if kind == ClipKind::Image {
                        unsupported_reason =
                            Some("UNSUPPORTED_FEATURE: sequence image source rendering".into());
                    }
                    kind
                }
            };
            clips.push(ClipQuery {
                track: track.id,
                kind,
                clip: clip.clone(),
                video_color,
                unsupported_reason,
            });
        }
    }
    Ok(SequenceQueryResult {
        revision: snapshot.revision.to_string(),
        sequence,
        clips,
        asset_status,
    })
}
impl From<SequenceError> for ServiceError {
    fn from(error: SequenceError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}
fn sequence_mut(project: &mut Project, id: SequenceId) -> Result<&mut Sequence, ServiceError> {
    if project
        .sequences
        .iter()
        .any(|s| matches!(s, DocumentObject::Opaque(s) if s.id == id.as_uuid()))
    {
        return Err(ServiceError::new("UNSUPPORTED_FEATURE", "opaque sequence"));
    }
    project
        .sequences
        .iter_mut()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == id => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))
}
/// Track clip lists stay sorted by timeline start so placement order is
/// canonical across plan / apply / persistence round-trips.
fn insert_sorted(clips: &mut Vec<Clip>, clip: Clip) {
    let at = clips
        .iter()
        .position(|c| c.timeline_range.start() >= clip.timeline_range.start())
        .unwrap_or(clips.len());
    clips.insert(at, clip);
}
fn expand_links(
    sequence: &Sequence,
    selected: &mut BTreeSet<ClipId>,
    linked: bool,
) -> Result<(), ServiceError> {
    loop {
        let before = selected.len();
        for c in sequence.tracks.iter().flat_map(|t| &t.clips) {
            if selected.contains(&c.id) {
                if !linked && c.links.iter().any(|id| !selected.contains(id)) {
                    return Err(ServiceError::new(
                        "LINKED_EDIT_REQUIRED",
                        "edit would separate linked placements",
                    ));
                }
                if linked {
                    selected.extend(&c.links);
                }
            }
        }
        if before == selected.len() {
            break;
        }
    }
    Ok(())
}
fn shift(
    sequence: &mut Sequence,
    selected: &BTreeSet<ClipId>,
    delta: kronello_time::Time,
) -> Result<(), ServiceError> {
    for transition in &mut sequence.transitions {
        match (
            selected.contains(&transition.outgoing),
            selected.contains(&transition.incoming),
        ) {
            (true, true) => {
                transition.range = TimeRange::new(
                    transition
                        .range
                        .start()
                        .checked_add(delta)
                        .map_err(SequenceError::from)?,
                    transition
                        .range
                        .end()
                        .checked_add(delta)
                        .map_err(SequenceError::from)?,
                )
                .map_err(SequenceError::from)?
            }
            (false, false) => (),
            _ => {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "move both transition endpoints or remove transition in the same plan",
                ));
            }
        }
    }
    for c in sequence
        .tracks
        .iter_mut()
        .flat_map(|t| &mut t.clips)
        .filter(|c| selected.contains(&c.id))
    {
        *c = moved(c, delta)?;
    }
    Ok(())
}
/// Translate a placement in sequence time; its markers ride with it.
fn moved(clip: &Clip, delta: Time) -> Result<Clip, ServiceError> {
    let mut next = clip.clone();
    next.timeline_range = TimeRange::new(
        next.timeline_range
            .start()
            .checked_add(delta)
            .map_err(SequenceError::from)?,
        next.timeline_range
            .end()
            .checked_add(delta)
            .map_err(SequenceError::from)?,
    )
    .map_err(SequenceError::from)?;
    for marker in &mut next.markers {
        marker.time = marker
            .time
            .checked_add(delta)
            .map_err(SequenceError::from)?;
    }
    Ok(next)
}
/// Move the clip's start edge. Shrinking delegates to `trimmed`; growing
/// shifts the time map domain so already placed content stays put and earlier
/// source content fills the head. Candidate validation enforces source bounds.
fn left_edge(clip: &Clip, start: Time) -> Result<Clip, ServiceError> {
    if start >= clip.timeline_range.start() {
        return clip
            .trimmed(TimeRange::new(start, clip.timeline_range.end()).map_err(SequenceError::from)?)
            .map_err(Into::into);
    }
    let head = clip
        .timeline_range
        .start()
        .checked_sub(start)
        .map_err(SequenceError::from)?;
    let mut next = clip.clone();
    next.timeline_range =
        TimeRange::new(start, clip.timeline_range.end()).map_err(SequenceError::from)?;
    next.time_map = match &clip.time_map {
        // local(u - head) keeps existing content fixed while extending the
        // parent domain left: linear maps shift the offset, piecewise maps
        // shift every control point. Protected maps cannot express a domain
        // shift and fail explicitly.
        TimeMap::Linear(m) => TimeMap::linear(
            m.offset()
                .checked_sub(head.checked_mul(m.speed()).map_err(SequenceError::from)?)
                .map_err(SequenceError::from)?,
            m.speed(),
        )
        .map_err(SequenceError::from)?,
        TimeMap::PiecewiseLinear(m) => TimeMap::piecewise_linear(
            m.points()
                .iter()
                .map(|p| {
                    Ok(TimeMapPoint {
                        parent: p.parent.checked_add(head)?,
                        local: p.local,
                    })
                })
                .collect::<Result<Vec<_>, kronello_time::TimeError>>()
                .map_err(SequenceError::from)?,
        )
        .map_err(SequenceError::from)?,
        _ => {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "head extension requires a linear or piecewise map",
            ));
        }
    };
    Ok(next)
}
/// Move the clip's end edge. Shrinking delegates to `trimmed`; growing keeps
/// the authored map and must land inside its domain and the source bounds,
/// which candidate validation enforces uniformly.
fn right_edge(clip: &Clip, end: Time) -> Result<Clip, ServiceError> {
    if end <= clip.timeline_range.end() {
        return clip
            .trimmed(TimeRange::new(clip.timeline_range.start(), end).map_err(SequenceError::from)?)
            .map_err(Into::into);
    }
    let mut next = clip.clone();
    next.timeline_range =
        TimeRange::new(clip.timeline_range.start(), end).map_err(SequenceError::from)?;
    Ok(next)
}
/// Apply an edge edit to `clip`; when an earlier step already replaced it, the
/// new edge applies to that replacement so one clip can absorb two run edges.
fn edit_edge(
    edits: &mut std::collections::BTreeMap<ClipId, Clip>,
    clip: &Clip,
    edit: impl FnOnce(&Clip) -> Result<Clip, ServiceError>,
) -> Result<(), ServiceError> {
    let base = edits.get(&clip.id).cloned().unwrap_or_else(|| clip.clone());
    edits.insert(clip.id, edit(&base)?);
    Ok(())
}
fn sequence_ref(project: &Project, id: SequenceId) -> Result<&Sequence, ServiceError> {
    if project
        .sequences
        .iter()
        .any(|s| matches!(s, DocumentObject::Opaque(s) if s.id == id.as_uuid()))
    {
        return Err(ServiceError::new("UNSUPPORTED_FEATURE", "opaque sequence"));
    }
    project
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == id => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))
}
fn has_clip(sequence: &Sequence, id: ClipId) -> bool {
    sequence
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .any(|c| c.id == id)
}
fn timeline_keys(
    sequence: &Sequence,
    _project: Uuid,
    clips: &BTreeSet<ClipId>,
    keys: &mut BTreeSet<ChangedKey>,
) {
    keys.insert(ChangedKey::Structure {
        object_id: sequence.id.as_uuid(),
        parent_container_id: sequence.id.as_uuid(),
    });
    for track in &sequence.tracks {
        for c in &track.clips {
            if clips.contains(&c.id) {
                keys.insert(ChangedKey::Structure {
                    object_id: c.id.as_uuid(),
                    parent_container_id: track.id.as_uuid(),
                });
            }
        }
    }
}
fn protected_content(project: &Project, root: CompositionId) -> bool {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if project
            .templates
            .iter()
            .any(|d| matches!(d, DocumentObject::Known(d) if d.composition_ref == id || d.variants.values().any(|v| v.composition_ref == id)))
        {
            return true;
        }
        if let Some(c) = project.compositions.iter().find_map(|c| match c {
            DocumentObject::Known(c) if c.id == id => Some(c),
            _ => None,
        }) {
            pending.extend(c.nodes.iter().filter_map(|n| match &n.kind {
                NodeKind::CompositionInstance(i) => Some(i.definition_ref),
                _ => None,
            }));
        }
    }
    false
}

// Commands are planned and then applied independently. Owned IDs must therefore
// be deterministic from stable UUIDs, rather than allocated during mutation.
fn split_owned_uuid(right: ClipId, original: Uuid) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"kronello.clip-split-owned-v1");
    digest.update(right.as_uuid().as_bytes());
    digest.update(original.as_bytes());
    let hash = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // UUID v8, RFC variant: application-defined deterministic identity.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn split_owned_objects(clip: &mut Clip) -> Result<(), ServiceError> {
    let right = clip.id;
    let mut ids = std::collections::BTreeMap::new();
    // Property's wire representation retains the source verbatim (including
    // shared Curve/Expression IDs); only objects owned by this placement change.
    fn copy_property(p: &Property, right: ClipId) -> Result<Property, ServiceError> {
        let mut value = serde_json::to_value(p)?;
        value["id"] = serde_json::json!(split_owned_uuid(right, p.id().as_uuid()));
        for modifier in value["modifiers"].as_array_mut().into_iter().flatten() {
            let original = serde_json::from_value::<Uuid>(modifier["id"].clone())?;
            modifier["id"] = serde_json::json!(split_owned_uuid(right, original));
        }
        Ok(serde_json::from_value(value)?)
    }
    for property in &mut clip.properties {
        let old = property.id();
        *property = copy_property(property, right)?;
        ids.insert(old, property.id());
    }
    if let Some(volume) = &mut clip.volume {
        **volume = copy_property(volume, right)?;
    }
    for effect in &mut clip.effects {
        let Effect::Known(effect) = effect else {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "opaque split effect",
            ));
        };
        let remap = |id: &mut PropertyId| -> Result<(), ServiceError> {
            *id = *ids.get(id).ok_or_else(|| {
                ServiceError::new("INVALID_EDIT", "split effect property missing")
            })?;
            Ok(())
        };
        match &mut effect.parameters {
            EffectParameters::GaussianBlur { sigma } => remap(sigma)?,
            EffectParameters::DropShadow {
                sigma,
                offset,
                color,
                opacity,
            } => {
                remap(sigma)?;
                remap(offset)?;
                remap(color)?;
                remap(opacity)?;
            }
            EffectParameters::AudioGain { gain } => remap(gain)?,
            EffectParameters::ColorExposure { exposure, offset } => {
                remap(exposure)?;
                remap(offset)?;
            }
            EffectParameters::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                for id in [in_black, in_white, gamma, out_black, out_white] {
                    remap(id)?;
                }
            }
            EffectParameters::ColorCurves { curve } => remap(curve)?,
            EffectParameters::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => {
                remap(hue_shift)?;
                remap(saturation)?;
                remap(lightness)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn mutate(
    project: &mut Project,
    command: &TimelineCommand,
    keys: &mut BTreeSet<ChangedKey>,
) -> Result<(), ServiceError> {
    let changed = |id, parent| ChangedKey::Structure {
        object_id: id,
        parent_container_id: parent,
    };
    match command {
        TimelineCommand::ClipSplit {
            sequence,
            clip,
            time,
            right_clip,
        } => {
            if project.sequences.iter().any(|object| matches!(object,
                DocumentObject::Known(s) if s.tracks.iter().flat_map(|t| &t.clips).any(|c| c.id == *right_clip))) {
                return Err(ServiceError::new("INVALID_EDIT", "split right clip ID already exists"));
            }
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            if s.transitions
                .iter()
                .any(|t| t.outgoing == *clip || t.incoming == *clip)
            {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "split requires explicit transition removal",
                ));
            }
            let track = s
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip))
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "split clip missing"))?;
            let index = track
                .clips
                .iter()
                .position(|c| c.id == *clip)
                .expect("located clip");
            let original = &track.clips[index];
            if !original.links.is_empty() {
                return Err(ServiceError::new(
                    "LINKED_EDIT_REQUIRED",
                    "linked clip split is not implicit",
                ));
            }
            if *time <= original.timeline_range.start() || *time >= original.timeline_range.end() {
                return Err(ServiceError::new(
                    "INVALID_EDIT",
                    "split must be strictly inside the clip",
                ));
            }
            let left_range = TimeRange::new(original.timeline_range.start(), *time)
                .map_err(SequenceError::from)?;
            let right_range = TimeRange::new(*time, original.timeline_range.end())
                .map_err(SequenceError::from)?;
            let left = original.trimmed(left_range)?;
            let mut right = original.trimmed(right_range)?;
            right.id = *right_clip;
            split_owned_objects(&mut right)?;
            track.clips[index] = left;
            track.clips.insert(index + 1, right);
            timeline_keys(s, project_id, &BTreeSet::from([*clip, *right_clip]), keys);
        }
        TimelineCommand::ClipMove {
            sequence,
            clip,
            delta,
            linked,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let mut selected = BTreeSet::from([*clip]);
            if !s
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .any(|c| c.id == *clip)
            {
                return Err(ServiceError::new("SOURCE_MISSING", "clip missing"));
            }
            expand_links(s, &mut selected, *linked)?;
            shift(s, &selected, *delta)?;
            timeline_keys(s, project_id, &selected, keys);
        }
        TimelineCommand::Ripple {
            sequence,
            tracks,
            pivot,
            delta,
            linked,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let tracks: BTreeSet<_> = tracks.iter().copied().collect();
            if tracks.is_empty()
                || tracks
                    .iter()
                    .any(|id| !s.tracks.iter().any(|t| t.id == *id))
            {
                return Err(ServiceError::new(
                    "INVALID_CLIP",
                    "explicit ripple tracks required",
                ));
            }
            let mut selected = BTreeSet::new();
            for track in s.tracks.iter().filter(|t| tracks.contains(&t.id)) {
                for clip in &track.clips {
                    if clip.timeline_range.start() < *pivot && clip.timeline_range.end() > *pivot {
                        return Err(ServiceError::new(
                            "INVALID_CLIP",
                            "ripple pivot straddles a clip",
                        ));
                    }
                    if clip.timeline_range.start() >= *pivot {
                        selected.insert(clip.id);
                    }
                }
            }
            if selected.is_empty() {
                return Err(ServiceError::new("INVALID_CLIP", "ripple selects no clips"));
            }
            expand_links(s, &mut selected, *linked)?;
            shift(s, &selected, *delta)?;
            timeline_keys(s, project_id, &selected, keys);
        }
        TimelineCommand::ClipSlip {
            sequence,
            clip,
            delta,
            linked,
        } => {
            // Read-phase on the stored document so link expansion and the
            // protected-content check plan against one snapshot.
            let source = sequence_ref(project, *sequence)?;
            if !has_clip(source, *clip) {
                return Err(ServiceError::new("SOURCE_MISSING", "clip missing"));
            }
            let mut selected = BTreeSet::from([*clip]);
            expand_links(source, &mut selected, *linked)?;
            for c in source
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .filter(|c| selected.contains(&c.id))
            {
                if let SourceRef::Composition { composition } = &c.source_ref
                    && protected_content(project, *composition)
                {
                    return Err(ServiceError::new(
                        "PROTECTED_INTERVAL",
                        "use protected clip retime for protected content",
                    ));
                }
            }
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            // The placement stays fixed; only the source window origin moves.
            // Validation rejects a window that leaves the source bounds.
            for c in s
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.clips)
                .filter(|c| selected.contains(&c.id))
            {
                c.source_in = c
                    .source_in
                    .checked_add(*delta)
                    .map_err(SequenceError::from)?;
            }
            timeline_keys(s, project_id, &selected, keys);
        }
        TimelineCommand::ClipSlide {
            sequence,
            clip,
            delta,
            linked,
        } => {
            let source = sequence_ref(project, *sequence)?;
            if !has_clip(source, *clip) {
                return Err(ServiceError::new("SOURCE_MISSING", "clip missing"));
            }
            let mut selected = BTreeSet::from([*clip]);
            expand_links(source, &mut selected, *linked)?;
            if source
                .transitions
                .iter()
                .any(|t| selected.contains(&t.outgoing) || selected.contains(&t.incoming))
            {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "slide requires explicit transition removal",
                ));
            }
            // Plan every replacement against the original document; failure at
            // any edge rejects the whole command before mutation.
            let mut edits = std::collections::BTreeMap::new();
            for track in &source.tracks {
                let mut members: Vec<&Clip> = track
                    .clips
                    .iter()
                    .filter(|c| selected.contains(&c.id))
                    .collect();
                members.sort_by_key(|c| c.timeline_range.start());
                let mut index = 0;
                while index < members.len() {
                    let run_start = members[index].timeline_range.start();
                    let mut run_end = members[index].timeline_range.end();
                    let mut run = vec![members[index]];
                    index += 1;
                    while index < members.len() && members[index].timeline_range.start() == run_end
                    {
                        run_end = members[index].timeline_range.end();
                        run.push(members[index]);
                        index += 1;
                    }
                    let previous = track
                        .clips
                        .iter()
                        .find(|c| !selected.contains(&c.id) && c.timeline_range.end() == run_start)
                        .ok_or_else(|| {
                            ServiceError::new(
                                "INVALID_CLIP",
                                "slide requires an adjacent leading clip",
                            )
                        })?;
                    let next = track
                        .clips
                        .iter()
                        .find(|c| !selected.contains(&c.id) && c.timeline_range.start() == run_end)
                        .ok_or_else(|| {
                            ServiceError::new(
                                "INVALID_CLIP",
                                "slide requires an adjacent trailing clip",
                            )
                        })?;
                    let start_edge = run_start.checked_add(*delta).map_err(SequenceError::from)?;
                    let end_edge = run_end.checked_add(*delta).map_err(SequenceError::from)?;
                    edit_edge(&mut edits, previous, |c| right_edge(c, start_edge))?;
                    edit_edge(&mut edits, next, |c| left_edge(c, end_edge))?;
                    for member in run {
                        edit_edge(&mut edits, member, |c| moved(c, *delta))?;
                    }
                }
            }
            let edited: BTreeSet<_> = edits.keys().copied().collect();
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            for c in s.tracks.iter_mut().flat_map(|t| &mut t.clips) {
                if let Some(next) = edits.remove(&c.id) {
                    *c = next;
                }
            }
            timeline_keys(s, project_id, &edited, keys);
        }
        TimelineCommand::ClipRoll {
            sequence,
            clip,
            delta,
            linked,
        } => {
            let source = sequence_ref(project, *sequence)?;
            if !has_clip(source, *clip) {
                return Err(ServiceError::new("SOURCE_MISSING", "clip missing"));
            }
            let mut selected = BTreeSet::from([*clip]);
            expand_links(source, &mut selected, *linked)?;
            let mut edits = std::collections::BTreeMap::new();
            for track in &source.tracks {
                for member in track.clips.iter().filter(|c| selected.contains(&c.id)) {
                    let next = track
                        .clips
                        .iter()
                        .find(|c| {
                            !selected.contains(&c.id)
                                && c.timeline_range.start() == member.timeline_range.end()
                        })
                        .ok_or_else(|| {
                            ServiceError::new(
                                "INVALID_CLIP",
                                "roll requires an adjacent following clip",
                            )
                        })?;
                    let boundary = member
                        .timeline_range
                        .end()
                        .checked_add(*delta)
                        .map_err(SequenceError::from)?;
                    edit_edge(&mut edits, member, |c| right_edge(c, boundary))?;
                    edit_edge(&mut edits, next, |c| left_edge(c, boundary))?;
                }
            }
            // A transition sharing a rolled edit point has no defined result;
            // the caller removes it explicitly first.
            if source
                .transitions
                .iter()
                .any(|t| edits.contains_key(&t.outgoing) || edits.contains_key(&t.incoming))
            {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "roll requires explicit transition removal",
                ));
            }
            let edited: BTreeSet<_> = edits.keys().copied().collect();
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            for c in s.tracks.iter_mut().flat_map(|t| &mut t.clips) {
                if let Some(next) = edits.remove(&c.id) {
                    *c = next;
                }
            }
            timeline_keys(s, project_id, &edited, keys);
        }
        TimelineCommand::ClipDelete {
            sequence,
            clip,
            linked,
        } => {
            let source = sequence_ref(project, *sequence)?;
            if !has_clip(source, *clip) {
                return Err(ServiceError::new("SOURCE_MISSING", "clip missing"));
            }
            let mut selected = BTreeSet::from([*clip]);
            expand_links(source, &mut selected, *linked)?;
            if source
                .transitions
                .iter()
                .any(|t| selected.contains(&t.outgoing) || selected.contains(&t.incoming))
            {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "remove incident transitions before deleting",
                ));
            }
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            for track in &mut s.tracks {
                track.clips.retain(|c| !selected.contains(&c.id));
            }
            // The gap stays; surviving links already exclude the component.
            timeline_keys(s, project_id, &BTreeSet::new(), keys);
            for id in &selected {
                keys.insert(changed(id.as_uuid(), sequence.as_uuid()));
            }
        }
        TimelineCommand::RippleDelete {
            sequence,
            tracks,
            range,
            linked,
        } => {
            let source = sequence_ref(project, *sequence)?;
            if range.is_empty() || range.start() < Time::ZERO {
                return Err(ServiceError::new("INVALID_CLIP", "empty ripple range"));
            }
            let listed: BTreeSet<_> = tracks.iter().copied().collect();
            if listed.len() != tracks.len()
                || listed.is_empty()
                || listed
                    .iter()
                    .any(|id| !source.tracks.iter().any(|t| t.id == *id))
            {
                return Err(ServiceError::new(
                    "INVALID_CLIP",
                    "explicit ripple tracks required",
                ));
            }
            let length = range
                .end()
                .checked_sub(range.start())
                .map_err(SequenceError::from)?;
            // Classify every clip on a listed track by its overlap with range.
            let mut doomed = BTreeSet::new();
            let mut shifted = BTreeSet::new();
            let mut hit = BTreeSet::new();
            let classify = |clip: &Clip,
                            doomed: &mut BTreeSet<ClipId>,
                            shifted: &mut BTreeSet<ClipId>|
             -> Result<Option<TimeRange>, ServiceError> {
                let Some(overlap) = clip.timeline_range.intersection(*range) else {
                    if clip.timeline_range.start() >= range.end() {
                        shifted.insert(clip.id);
                    }
                    return Ok(None);
                };
                if clip.timeline_range.start() < range.start()
                    && clip.timeline_range.end() > range.end()
                {
                    return Err(ServiceError::new(
                        "INVALID_CLIP",
                        "ripple delete cannot split a clip straddling the range",
                    ));
                }
                if overlap == clip.timeline_range {
                    doomed.insert(clip.id);
                } else if clip.timeline_range.start() < range.start() {
                    // Keep the head before the range.
                    return Ok(Some(
                        TimeRange::new(clip.timeline_range.start(), range.start())
                            .map_err(SequenceError::from)?,
                    ));
                } else {
                    // Keep the tail after the range, moved into the gap.
                    return Ok(Some(
                        TimeRange::new(range.end(), clip.timeline_range.end())
                            .map_err(SequenceError::from)?,
                    ));
                }
                Ok(None)
            };
            let mut trims = std::collections::BTreeMap::new();
            for track in source.tracks.iter().filter(|t| listed.contains(&t.id)) {
                for clip in &track.clips {
                    if let Some(keep) = classify(clip, &mut doomed, &mut shifted)? {
                        trims.insert(clip.id, keep);
                    }
                }
            }
            hit.extend(doomed.iter().chain(trims.keys()).copied());
            hit.extend(shifted.iter().copied());
            expand_links(source, &mut hit, *linked)?;
            // Linked partners pulled in from unlisted tracks are evaluated by
            // their own overlap; never by the triggering clip's fate.
            for track in &source.tracks {
                for clip in &track.clips {
                    if hit.contains(&clip.id)
                        && !doomed.contains(&clip.id)
                        && !trims.contains_key(&clip.id)
                        && !shifted.contains(&clip.id)
                        && let Some(keep) = classify(clip, &mut doomed, &mut shifted)?
                    {
                        trims.insert(clip.id, keep);
                    }
                }
            }
            // Transitions die only with both endpoints; a partially covered or
            // shifted endpoint conflicts explicitly.
            for transition in &source.transitions {
                let touched = doomed.contains(&transition.outgoing)
                    || doomed.contains(&transition.incoming)
                    || trims.contains_key(&transition.outgoing)
                    || trims.contains_key(&transition.incoming);
                if touched
                    && !(doomed.contains(&transition.outgoing)
                        && doomed.contains(&transition.incoming))
                {
                    return Err(ServiceError::new(
                        "TRANSITION_EDIT_CONFLICT",
                        "ripple delete requires explicit transition removal",
                    ));
                }
            }
            let backward = Time::ZERO
                .checked_sub(length)
                .map_err(SequenceError::from)?;
            let mut edits = std::collections::BTreeMap::new();
            for (id, keep) in &trims {
                let clip = source
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == *id)
                    .expect("classified clip");
                let trimmed = clip.trimmed(*keep)?;
                edits.insert(
                    *id,
                    if keep.start() == range.end() {
                        moved(&trimmed, backward)?
                    } else {
                        trimmed
                    },
                );
            }
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            for track in &mut s.tracks {
                track.clips.retain(|c| !doomed.contains(&c.id));
                for c in &mut track.clips {
                    c.links.retain(|id| !doomed.contains(id));
                }
            }
            for c in s.tracks.iter_mut().flat_map(|t| &mut t.clips) {
                if let Some(next) = edits.remove(&c.id) {
                    *c = next;
                }
            }
            s.transitions
                .retain(|t| !(doomed.contains(&t.outgoing) && doomed.contains(&t.incoming)));
            shift(s, &shifted, backward)?;
            let mut affected: BTreeSet<_> = doomed.iter().chain(shifted.iter()).copied().collect();
            affected.extend(trims.keys().copied());
            timeline_keys(s, project_id, &affected, keys);
            for id in &doomed {
                keys.insert(changed(id.as_uuid(), sequence.as_uuid()));
            }
        }
        TimelineCommand::ClipInsert {
            sequence,
            track,
            clip,
            linked,
        } => {
            let source = sequence_ref(project, *sequence)?;
            let at = clip.timeline_range.start();
            let duration = clip
                .timeline_range
                .duration()
                .map_err(SequenceError::from)?
                .as_time();
            if duration <= Time::ZERO {
                return Err(ServiceError::new("INVALID_CLIP", "empty insert range"));
            }
            let target = source
                .tracks
                .iter()
                .find(|t| t.id == *track)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "track missing"))?;
            let mut shifted = BTreeSet::new();
            for c in &target.clips {
                if c.timeline_range.start() < at && c.timeline_range.end() > at {
                    return Err(ServiceError::new(
                        "INVALID_CLIP",
                        "insert point straddles a clip; split first or overwrite",
                    ));
                }
                if c.timeline_range.start() >= at {
                    shifted.insert(c.id);
                }
            }
            if has_clip(source, clip.id) {
                return Err(ServiceError::new("INVALID_CLIP", "clip id already placed"));
            }
            expand_links(source, &mut shifted, *linked)?;
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let target = s
                .tracks
                .iter_mut()
                .find(|t| t.id == *track)
                .expect("track checked");
            insert_sorted(&mut target.clips, (**clip).clone());
            shift(s, &shifted, duration)?;
            timeline_keys(s, project_id, &shifted, keys);
            keys.insert(changed(clip.id.as_uuid(), track.as_uuid()));
        }
        TimelineCommand::ClipOverwrite {
            sequence,
            track,
            clip,
        } => {
            let source = sequence_ref(project, *sequence)?;
            let range = clip.timeline_range;
            if range.is_empty() {
                return Err(ServiceError::new("INVALID_CLIP", "empty overwrite range"));
            }
            let target = source
                .tracks
                .iter()
                .find(|t| t.id == *track)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "track missing"))?;
            if has_clip(source, clip.id) {
                return Err(ServiceError::new("INVALID_CLIP", "clip id already placed"));
            }
            // Overwrite never separates linked placements; covered or trimmed
            // linked clips must be removed through a linked operation first.
            let mut doomed = BTreeSet::new();
            let mut edits = std::collections::BTreeMap::new();
            for c in &target.clips {
                let Some(overlap) = c.timeline_range.intersection(range) else {
                    continue;
                };
                if !c.links.is_empty() {
                    return Err(ServiceError::new(
                        "LINKED_EDIT_REQUIRED",
                        "overwrite would separate linked placements",
                    ));
                }
                if overlap == c.timeline_range {
                    doomed.insert(c.id);
                    continue;
                }
                if c.timeline_range.start() < range.start() {
                    edit_edge(&mut edits, c, |c| right_edge(c, range.start()))?;
                }
                if c.timeline_range.end() > range.end() {
                    edit_edge(&mut edits, c, |c| left_edge(c, range.end()))?;
                }
            }
            if source.transitions.iter().any(|t| {
                doomed.contains(&t.outgoing)
                    || doomed.contains(&t.incoming)
                    || edits.contains_key(&t.outgoing)
                    || edits.contains_key(&t.incoming)
            }) {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "overwrite requires explicit transition removal",
                ));
            }
            let affected: BTreeSet<_> = doomed.iter().chain(edits.keys()).copied().collect();
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let target = s
                .tracks
                .iter_mut()
                .find(|t| t.id == *track)
                .expect("track checked");
            target.clips.retain(|c| !doomed.contains(&c.id));
            for c in target.clips.iter_mut() {
                if let Some(next) = edits.remove(&c.id) {
                    *c = next;
                }
            }
            insert_sorted(&mut target.clips, (**clip).clone());
            timeline_keys(s, project_id, &affected, keys);
            keys.insert(changed(clip.id.as_uuid(), track.as_uuid()));
        }
        TimelineCommand::MarkerSet {
            sequence,
            clip,
            marker,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let mut clips = BTreeSet::new();
            let list = match clip {
                Some(id) => {
                    clips.insert(*id);
                    &mut s
                        .tracks
                        .iter_mut()
                        .flat_map(|t| &mut t.clips)
                        .find(|c| c.id == *id)
                        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?
                        .markers
                }
                None => &mut s.markers,
            };
            match list.iter_mut().find(|m| m.id == marker.id) {
                Some(existing) => *existing = marker.clone(),
                None => list.push(marker.clone()),
            }
            timeline_keys(s, project_id, &clips, keys);
        }
        TimelineCommand::MarkerRemove {
            sequence,
            clip,
            marker,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let mut clips = BTreeSet::new();
            let list = match clip {
                Some(id) => {
                    clips.insert(*id);
                    &mut s
                        .tracks
                        .iter_mut()
                        .flat_map(|t| &mut t.clips)
                        .find(|c| c.id == *id)
                        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?
                        .markers
                }
                None => &mut s.markers,
            };
            let before = list.len();
            list.retain(|m| m.id != *marker);
            if list.len() == before {
                return Err(ServiceError::new("SOURCE_MISSING", "marker missing"));
            }
            timeline_keys(s, project_id, &clips, keys);
        }
        TimelineCommand::MarkerMove {
            sequence,
            clip,
            marker,
            time,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let mut clips = BTreeSet::new();
            let list = match clip {
                Some(id) => {
                    clips.insert(*id);
                    &mut s
                        .tracks
                        .iter_mut()
                        .flat_map(|t| &mut t.clips)
                        .find(|c| c.id == *id)
                        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?
                        .markers
                }
                None => &mut s.markers,
            };
            let target = list
                .iter_mut()
                .find(|m| m.id == *marker)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "marker missing"))?;
            target.time = *time;
            timeline_keys(s, project_id, &clips, keys);
        }
        TimelineCommand::WorkAreaSet {
            sequence,
            work_area,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            s.work_area = *work_area;
            timeline_keys(s, project_id, &BTreeSet::new(), keys);
        }
        TimelineCommand::ClipLink { sequence, clips } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let selected: BTreeSet<_> = clips.iter().copied().collect();
            if selected.is_empty()
                || selected.len() != clips.len()
                || selected
                    .iter()
                    .any(|id| !s.tracks.iter().flat_map(|t| &t.clips).any(|c| c.id == *id))
            {
                return Err(ServiceError::new(
                    "INVALID_CLIP",
                    "link clips missing/duplicated",
                ));
            }
            let mut affected = selected.clone();
            for c in s.tracks.iter_mut().flat_map(|t| &mut t.clips) {
                if selected.contains(&c.id) {
                    affected.extend(&c.links);
                    c.links = selected.iter().copied().filter(|id| *id != c.id).collect();
                } else if c.links.iter().any(|id| selected.contains(id)) {
                    affected.insert(c.id);
                    c.links.retain(|id| !selected.contains(id));
                }
            }
            timeline_keys(s, project_id, &affected, keys);
        }
        TimelineCommand::TransitionSet {
            sequence,
            transition,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            s.transitions
                .retain(|t| t.outgoing != transition.outgoing || t.incoming != transition.incoming);
            s.transitions.push(transition.clone());
            timeline_keys(
                s,
                project_id,
                &BTreeSet::from([transition.outgoing, transition.incoming]),
                keys,
            );
        }
        TimelineCommand::TransitionRemove {
            sequence,
            outgoing,
            incoming,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let count = s.transitions.len();
            s.transitions
                .retain(|t| t.outgoing != *outgoing || t.incoming != *incoming);
            if count == s.transitions.len() {
                return Err(ServiceError::new("SOURCE_MISSING", "transition missing"));
            }
            timeline_keys(s, project_id, &BTreeSet::from([*outgoing, *incoming]), keys);
        }
        TimelineCommand::TrackStateSet {
            sequence,
            track,
            state,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let target = s
                .tracks
                .iter_mut()
                .find(|t| t.id == *track)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "track missing"))?;
            target.state = Some(*state);
            let clips = target.clips.iter().map(|c| c.id).collect();
            keys.insert(changed(track.as_uuid(), sequence.as_uuid()));
            timeline_keys(s, project_id, &clips, keys);
        }
        TimelineCommand::ClipTimeSet {
            sequence,
            clip,
            source_in,
            time_map,
            audio_retime,
            reverse_sampling,
        } => {
            let project_id = project.id;
            let source = project.sequences.iter().find_map(|object| match object {
                DocumentObject::Known(s) if s.id == *sequence => s
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == *clip)
                    .map(|c| c.source_ref.clone()),
                _ => None,
            });
            if let Some(SourceRef::Composition { composition }) = source
                && protected_content(project, composition)
            {
                return Err(ServiceError::new(
                    "PROTECTED_INTERVAL",
                    "use protected clip retime for protected content",
                ));
            }
            let s = sequence_mut(project, *sequence)?;
            if s.transitions
                .iter()
                .any(|t| t.incoming == *clip || t.outgoing == *clip)
            {
                return Err(ServiceError::new(
                    "TRANSITION_EDIT_CONFLICT",
                    "retime requires explicit transition removal",
                ));
            }
            let c = s
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.clips)
                .find(|c| c.id == *clip)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?;
            if !c.links.is_empty() {
                return Err(ServiceError::new(
                    "LINKED_EDIT_REQUIRED",
                    "retime linked clips explicitly in one transaction",
                ));
            }
            c.source_in = *source_in;
            c.time_map = time_map.clone();
            c.audio_retime = *audio_retime;
            c.reverse_sampling = *reverse_sampling;
            timeline_keys(s, project_id, &BTreeSet::from([*clip]), keys);
        }
        TimelineCommand::ClipSetEffects {
            sequence,
            clip,
            properties,
            effects,
        } => {
            let project_id = project.id;
            let s = sequence_mut(project, *sequence)?;
            let c = s
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.clips)
                .find(|c| c.id == *clip)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?;
            c.properties = properties.clone();
            c.effects = effects.clone();
            timeline_keys(s, project_id, &BTreeSet::from([*clip]), keys);
        }
        TimelineCommand::SequenceCreate { sequence } => {
            keys.insert(changed(sequence.id.as_uuid(), project.id));
            project
                .sequences
                .push(DocumentObject::Known(sequence.clone()));
        }
        TimelineCommand::ClipPlace {
            sequence,
            track,
            clip,
        } => {
            let s = sequence_mut(project, *sequence)?;
            let t = s
                .tracks
                .iter_mut()
                .find(|t| t.id == *track)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "track missing"))?;
            t.clips.push((**clip).clone());
            keys.insert(changed(sequence.as_uuid(), sequence.as_uuid()));
            keys.insert(changed(clip.id.as_uuid(), track.as_uuid()));
        }
        TimelineCommand::ClipTrim {
            sequence,
            clip,
            range,
        }
        | TimelineCommand::ClipStretch {
            sequence,
            clip,
            range,
        } => {
            if matches!(command, TimelineCommand::ClipStretch { .. }) {
                let source = project.sequences.iter().find_map(|s| match s {
                    DocumentObject::Known(s) if s.id == *sequence => s
                        .tracks
                        .iter()
                        .flat_map(|t| &t.clips)
                        .find(|c| c.id == *clip)
                        .map(|c| c.source_ref.clone()),
                    _ => None,
                });
                if let Some(SourceRef::Composition { composition }) = source
                    && protected_content(project, composition)
                {
                    return Err(ServiceError::new(
                        "PROTECTED_INTERVAL",
                        "retime the protected instance instead of stretching its containing clip",
                    ));
                }
            }
            let s = sequence_mut(project, *sequence)?;
            let c = s
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.clips)
                .find(|c| c.id == *clip)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?;
            *c = if matches!(command, TimelineCommand::ClipTrim { .. }) {
                c.trimmed(*range)?
            } else {
                c.stretched(*range)?
            };
            // Track membership/order is authored; edits on the same sequence conflict conservatively.
            keys.insert(changed(sequence.as_uuid(), sequence.as_uuid()));
            keys.insert(changed(clip.as_uuid(), sequence.as_uuid()));
        }
        TimelineCommand::ClipSetVolume {
            sequence,
            clip,
            volume,
        } => {
            if let Some(volume) = volume {
                validate_volume(volume)
                    .map_err(|e| ServiceError::new("INVALID_AUDIO_INPUT", e.to_string()))?;
            }
            let s = sequence_mut(project, *sequence)?;
            let c = s
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.clips)
                .find(|c| c.id == *clip)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "clip missing"))?;
            c.volume = volume.clone().map(Box::new);
            keys.insert(changed(sequence.as_uuid(), project.id));
            keys.insert(changed(clip.as_uuid(), sequence.as_uuid()));
        }
        TimelineCommand::InstanceRetime {
            composition,
            node,
            time_map,
        } => {
            let target = project.compositions.iter().find_map(|c| match c {
                DocumentObject::Known(c) if c.id == *composition => {
                    c.nodes.iter().find_map(|n| match &n.kind {
                        NodeKind::CompositionInstance(i) if n.id == *node => Some(i.definition_ref),
                        _ => None,
                    })
                }
                _ => None,
            });
            if target.is_some_and(|id| protected_content(project, id)) {
                return Err(ServiceError::new(
                    "PROTECTED_INTERVAL",
                    "use template_instance.retime for protected content",
                ));
            }
            let template_ids: BTreeSet<_> = project
                .template_instances
                .iter()
                .filter_map(|i| match i {
                    DocumentObject::Known(i) => Some(i.id),
                    _ => None,
                })
                .collect();
            let c = project
                .compositions
                .iter_mut()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == *composition => Some(c),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "composition missing"))?;
            let n = c
                .nodes
                .iter_mut()
                .find(|n| n.id == *node)
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "instance node missing"))?;
            let NodeKind::CompositionInstance(i) = &mut n.kind else {
                return Err(ServiceError::new(
                    "INVALID_INSTANCE",
                    "node is not an instance",
                ));
            };
            if template_ids.contains(&i.id) {
                return Err(ServiceError::new(
                    "PROTECTED_INTERVAL",
                    "use template_instance.retime for protected template duration",
                ));
            }
            let first = time_map
                .map(n.active_range.start())
                .map_err(|e| ServiceError::from(SequenceError::Time(e)))?;
            let last = time_map
                .map(n.active_range.end())
                .map_err(|e| ServiceError::from(SequenceError::Time(e)))?;
            let target = i.definition_ref;
            let id = i.id;
            i.local_time_map = time_map.clone();
            let duration = project
                .compositions
                .iter()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == target => Some(c.duration.as_time()),
                    _ => None,
                })
                .ok_or_else(|| {
                    ServiceError::new("SOURCE_MISSING", "instance definition missing")
                })?;
            if first < kronello_time::Time::ZERO || last > duration {
                return Err(ServiceError::new(
                    "INVALID_INSTANCE",
                    "retime exceeds source duration",
                ));
            }
            keys.insert(changed(node.as_uuid(), composition.as_uuid()));
            keys.insert(changed(id.as_uuid(), composition.as_uuid()));
        }
        TimelineCommand::TemplateInstanceRetime { instance, duration } => crate::template::mutate(
            project,
            &TemplateCommand::SetDuration {
                instance: *instance,
                duration: *duration,
            },
            keys,
        )?,
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceCreateRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub sequence: Sequence,
}
pub(crate) fn sequence_create(
    r: SequenceCreateRequest,
) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::SequenceCreate {
            sequence: r.sequence,
        },
    ))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipPlaceRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub sequence: SequenceId,
    pub track: TrackId,
    pub clip: Clip,
}
pub(crate) fn clip_place(r: ClipPlaceRequest) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::ClipPlace {
            sequence: r.sequence,
            track: r.track,
            clip: Box::new(r.clip),
        },
    ))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipTrimRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub sequence: SequenceId,
    pub clip: ClipId,
    pub range: TimeRange,
}
pub(crate) fn clip_trim(r: ClipTrimRequest) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(TimelineCommand::ClipTrim {
        sequence: r.sequence,
        clip: r.clip,
        range: r.range,
    }))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipStretchRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub sequence: SequenceId,
    pub clip: ClipId,
    pub range: TimeRange,
}
pub(crate) fn clip_stretch(r: ClipStretchRequest) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::ClipStretch {
            sequence: r.sequence,
            clip: r.clip,
            range: r.range,
        },
    ))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstanceRetimeRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub composition: CompositionId,
    pub node: NodeId,
    pub time_map: TimeMap,
}
pub(crate) fn instance_retime(
    r: InstanceRetimeRequest,
) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::InstanceRetime {
            composition: r.composition,
            node: r.node,
            time_map: r.time_map,
        },
    ))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateInstanceRetimeRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub instance: CompositionInstanceId,
    pub duration: kronello_time::Duration,
}
pub(crate) fn template_instance_retime(
    r: TemplateInstanceRetimeRequest,
) -> Result<kronello_store::Event, ServiceError> {
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::TemplateInstanceRetime {
            instance: r.instance,
            duration: r.duration,
        },
    ))];
    let plan = crate::edit::plan(PlanRequest {
        project: r.project.clone(),
        base_revision: r.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(EditApplyRequest {
        project: r.project,
        base_revision: r.base_revision,
        session_id: r.session_id,
        idempotency_key: r.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

/// Offline document audio uses the same pure plan as synchronous/job MOV export.
pub fn mix_sequence_audio(
    project: &Project,
    sequence: SequenceId,
    project_path: &std::path::Path,
    range: TimeRange,
) -> Result<kronello_audio::Bus, ServiceError> {
    let plan = kronello_audio::DocumentAudioPlan::compile(
        project,
        kronello_audio::AudioTarget::Sequence(sequence),
    )
    .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let runtime = kronello_media::MediaRuntime::load()?;
    let mut sources = kronello_audio::AudioSources::new();
    for clip in plan.clips() {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            sources.entry((clip.asset, clip.stream_index))
        {
            let asset = project
                .assets
                .iter()
                .find_map(|a| match a {
                    DocumentObject::Known(a) if a.id == clip.asset => Some(a),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::new("ASSET_MISSING", "audio asset missing"))?;
            entry.insert(
                runtime
                    .decode_asset_audio(asset, project_path, clip.stream_index)?
                    .buffer,
            );
        }
    }
    plan.mix(&sources, range)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))
}
