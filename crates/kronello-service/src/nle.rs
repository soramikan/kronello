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
impl From<SequenceError> for ServiceError {
    fn from(error: SequenceError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}
fn sequence_mut(project: &mut Project, id: SequenceId) -> Result<&mut Sequence, ServiceError> {
    project
        .sequences
        .iter_mut()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == id => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))
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
            keys.insert(changed(sequence.as_uuid(), project.id));
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
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            s.validate(project)?;
        }
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
/// mux audio; INTEGRATION-001 will connect this to sequence A/V export.
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
