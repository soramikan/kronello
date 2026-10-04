//! Template commands share edit planning, revision, idempotency, and undo.
use crate::{EditApplyRequest, EditCommand, PlanRequest, ServiceError};
use kronello_model::*;
use kronello_store::ChangedKey;
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateCommand {
    Define {
        definition: TemplateDefinition,
    },
    Instantiate {
        composition: CompositionId,
        node: NodeId,
        index: usize,
        instance: TemplateInstance,
    },
    SetInput {
        instance: CompositionInstanceId,
        name: String,
        value: Value,
    },
    SetDuration {
        instance: CompositionInstanceId,
        duration: kronello_time::Duration,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateDefineRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub definition: TemplateDefinition,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateInstantiateRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub composition: CompositionId,
    pub node: NodeId,
    pub index: usize,
    pub instance: TemplateInstance,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSetInputRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub instance: CompositionInstanceId,
    pub name: String,
    pub value: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateSetDurationRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub instance: CompositionInstanceId,
    pub duration: kronello_time::Duration,
}
impl From<kronello_template::TemplateError> for ServiceError {
    fn from(e: kronello_template::TemplateError) -> Self {
        let mut error = Self::new(e.code(), e.to_string());
        if let kronello_template::TemplateError::Overflow {
            node,
            actual,
            maximum,
        } = e
        {
            error.details =
                Some(serde_json::json!({"node":node,"actual_lines":actual,"max_lines":maximum}));
        }
        error
    }
}
pub(crate) fn mutate(
    project: &mut Project,
    command: &TemplateCommand,
    keys: &mut BTreeSet<ChangedKey>,
) -> Result<(), ServiceError> {
    let changed = |id, parent| ChangedKey::Structure {
        object_id: id,
        parent_container_id: parent,
    };
    match command {
        TemplateCommand::SetDuration { instance, duration } => {
            let i = project
                .template_instances
                .iter()
                .find_map(|i| match i {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::invalid("template instance missing"))?;
            let d = kronello_template::definition(project, i.definition_ref)?;
            let map = kronello_template::duration_map(
                kronello_template::composition(project, d.composition_ref)?.duration,
                *duration,
                &d.duration_policy,
            )?;
            for c in &mut project.compositions {
                if let DocumentObject::Known(c) = c {
                    for n in &mut c.nodes {
                        if let NodeKind::CompositionInstance(p) = &mut n.kind
                            && p.id == *instance
                        {
                            p.local_time_map = map.clone();
                            n.active_range = TimeRange::from_start_duration(Time::ZERO, *duration)
                                .map_err(|e| ServiceError::invalid(e.to_string()))?;
                            keys.insert(changed(n.id.as_uuid(), c.id.as_uuid()));
                        }
                    }
                }
            }
            keys.insert(changed(instance.as_uuid(), instance.as_uuid()));
            let i = project
                .template_instances
                .iter_mut()
                .find_map(|i| match i {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .unwrap();
            i.duration = *duration;
        }
        TemplateCommand::Define { definition } => {
            if project.templates.iter().any(|d|matches!(d,DocumentObject::Known(d) if d.id==definition.id || (d.template_id==definition.template_id && d.version==definition.version))) {return Err(ServiceError::new("TEMPLATE_VERSION_EXISTS","publish a new immutable edition"));}
            let mut d = definition.clone();
            d.content_hash = kronello_template::authoring_hash(project, d.composition_ref)?;
            kronello_template::validate_definition(project, &d)?;
            keys.insert(changed(d.id, project.id));
            keys.insert(changed(d.composition_ref.as_uuid(), d.id));
            project.templates.push(DocumentObject::Known(d));
        }
        TemplateCommand::Instantiate {
            composition,
            node,
            index,
            instance,
        } => {
            let d = kronello_template::definition(project, instance.definition_ref)?;
            kronello_template::resolved_inputs(d, instance)?;
            let local_time_map = kronello_template::duration_map(
                kronello_template::composition(project, d.composition_ref)?.duration,
                instance.duration,
                &d.duration_policy,
            )?;
            let placement = CompositionInstance {
                id: instance.id,
                definition_ref: d.composition_ref,
                input_bindings: Default::default(),
                local_time_map,
                seed: 0,
            };
            keys.insert(changed(d.id, project.id));
            keys.insert(changed(instance.id.as_uuid(), composition.as_uuid()));
            keys.insert(changed(node.as_uuid(), composition.as_uuid()));
            let c = project
                .compositions
                .iter_mut()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == *composition => Some(c),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::invalid("placement composition missing"))?;
            if *index > c.root_nodes.len() || c.nodes.iter().any(|n| n.id == *node) {
                return Err(ServiceError::invalid("invalid placement index or node"));
            }
            c.root_nodes.insert(*index, *node);
            c.nodes.push(SceneNode {
                id: *node,
                kind: NodeKind::CompositionInstance(placement),
                containment_parent: None,
                transform_parent: None,
                child_order: vec![],
                active_range: TimeRange::from_start_duration(Time::ZERO, instance.duration)
                    .map_err(|e| ServiceError::invalid(e.to_string()))?,
                properties: vec![],
                effects: vec![],
            });
            project
                .template_instances
                .push(DocumentObject::Known(instance.clone()));
        }
        TemplateCommand::SetInput {
            instance,
            name,
            value,
        } => {
            let i = project
                .template_instances
                .iter()
                .find_map(|i| match i {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::invalid("template instance missing"))?;
            let d = kronello_template::definition(project, i.definition_ref)?;
            let input = d
                .public_inputs
                .get(name)
                .ok_or_else(|| ServiceError::new("INPUT_NOT_PUBLIC", "input is not public"))?;
            kronello_template::validate_input(input, value)?;
            keys.insert(changed(instance.as_uuid(), instance.as_uuid()));
            let i = project
                .template_instances
                .iter_mut()
                .find_map(|i| match i {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .unwrap();
            i.inputs.insert(name.clone(), value.clone());
        }
    }
    Ok(())
}
fn execute(
    project: PathBuf,
    base_revision: String,
    session_id: Uuid,
    idempotency_key: String,
    command: TemplateCommand,
) -> Result<kronello_store::Event, ServiceError> {
    let base_revision = crate::parse_revision(&base_revision)?.to_string();
    let commands = vec![EditCommand::Template(Box::new(command))];
    // Retry the exact service payload before planning against an older revision.
    let payload = serde_json::json!({"operation":"template.edit","base_revision":base_revision,"session_id":session_id,"commands":commands});
    let store = crate::open_existing(&project)?;
    let record = store.idempotency_record(&idempotency_key)?;
    store.close()?;
    if let Some(record) = record {
        if record.service_payload.as_ref() != Some(&payload) {
            return Err(kronello_store::StoreError::IdempotencyKeyReused.into());
        }
        return Ok(record.result);
    }
    let plan = crate::edit::plan(PlanRequest {
        project: project.clone(),
        base_revision: base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply_template(
        EditApplyRequest {
            project,
            base_revision,
            session_id,
            idempotency_key,
            commands,
            plan_hash: plan.plan_hash,
        },
        payload,
    )
}
pub(crate) fn define(r: TemplateDefineRequest) -> Result<kronello_store::Event, ServiceError> {
    execute(
        r.project,
        r.base_revision,
        r.session_id,
        r.idempotency_key,
        TemplateCommand::Define {
            definition: r.definition,
        },
    )
}
pub(crate) fn instantiate(
    r: TemplateInstantiateRequest,
) -> Result<kronello_store::Event, ServiceError> {
    execute(
        r.project,
        r.base_revision,
        r.session_id,
        r.idempotency_key,
        TemplateCommand::Instantiate {
            composition: r.composition,
            node: r.node,
            index: r.index,
            instance: r.instance,
        },
    )
}
pub(crate) fn set_input(r: TemplateSetInputRequest) -> Result<kronello_store::Event, ServiceError> {
    execute(
        r.project,
        r.base_revision,
        r.session_id,
        r.idempotency_key,
        TemplateCommand::SetInput {
            instance: r.instance,
            name: r.name,
            value: r.value,
        },
    )
}

pub(crate) fn set_duration(
    r: TemplateSetDurationRequest,
) -> Result<kronello_store::Event, ServiceError> {
    execute(
        r.project,
        r.base_revision,
        r.session_id,
        r.idempotency_key,
        TemplateCommand::SetDuration {
            instance: r.instance,
            duration: r.duration,
        },
    )
}
