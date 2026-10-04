//! Timeline edits reuse the common transaction planner and selective Undo.
use crate::*;
use kronello_model::*;
use kronello_store::ChangedKey;
use kronello_time::{TimeMap, TimeRange};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineCommand {
    SequenceCreate {
        sequence: Sequence,
    },
    ClipPlace {
        sequence: SequenceId,
        track: TrackId,
        clip: Clip,
    },
    ClipTrim {
        sequence: SequenceId,
        clip: ClipId,
        range: TimeRange,
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
}
pub(crate) fn sequence_query(
    request: SequenceQueryRequest,
) -> Result<SequenceQueryResult, ServiceError> {
    let store = open_existing(&request.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let mut project = snapshot.document;
    let sequence = sequence_mut(&mut project, request.sequence)?.clone();
    let mut clips = vec![];
    for track in &sequence.tracks {
        for clip in &track.clips {
            let mut video_color = None;
            let mut unsupported_reason = None;
            let kind = match &clip.source_ref {
                SourceRef::Composition { .. } => ClipKind::Composition,
                SourceRef::Generator {
                    generator, version, ..
                } => {
                    if generator != SOLID_GENERATOR_ID || *version != GENERATOR_VERSION {
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
        c.timeline_range = TimeRange::new(
            c.timeline_range
                .start()
                .checked_add(delta)
                .map_err(SequenceError::from)?,
            c.timeline_range
                .end()
                .checked_add(delta)
                .map_err(SequenceError::from)?,
        )
        .map_err(SequenceError::from)?;
    }
    Ok(())
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
            .any(|d| matches!(d, DocumentObject::Known(d) if d.composition_ref == id))
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
            t.clips.push(clip.clone());
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
            clip: r.clip,
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

/// Offline 48 kHz bus from an immutable project value. Image rendering does not
/// mux audio. Movie jobs accept explicit audio clips; sequence-track mux is
/// outside the INTEGRATION-001 image-sequence demo.
pub fn mix_sequence_audio(
    project: &Project,
    sequence: SequenceId,
    project_path: &std::path::Path,
    range: TimeRange,
) -> Result<kronello_audio::Bus, ServiceError> {
    let sequence = project
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))?;
    sequence.validate(project)?;
    let runtime = kronello_media::MediaRuntime::load()?;
    let mut clips = vec![];
    let mut sources = kronello_audio::AudioSources::new();
    for track in &sequence.tracks {
        if track.kind != TrackKind::Audio {
            continue;
        }
        for clip in &track.clips {
            if !clip.effects.is_empty() {
                return Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "audio clip effects",
                ));
            }
            let SourceRef::Asset {
                asset,
                stream_index,
            } = clip.source_ref
            else {
                return Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "audio source must be an asset",
                ));
            };
            let source = project
                .assets
                .iter()
                .find_map(|a| match a {
                    DocumentObject::Known(a) if a.id == asset => Some(a),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "audio asset missing"))?;
            if let std::collections::btree_map::Entry::Vacant(entry) =
                sources.entry((asset, stream_index))
            {
                entry.insert(
                    runtime
                        .decode_asset_audio(source, project_path, stream_index)?
                        .buffer,
                );
            }
            clips.push(kronello_audio::AudioClip {
                asset,
                stream_index,
                placement: clip.timeline_range,
                source_in: clip.local_time(clip.timeline_range.start())?,
                gain: kronello_audio::Gain::UNITY,
            });
        }
    }
    kronello_audio::mix(&clips, &sources, range)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))
}
