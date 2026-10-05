//! Template commands share edit planning, revision, idempotency, and undo.
use crate::{EditApplyRequest, EditCommand, PlanRequest, ServiceError};
use kronello_model::*;
use kronello_store::ChangedKey;
use kronello_time::{Time, TimeRange};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
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
    Migrate {
        instance: CompositionInstanceId,
        definition: Uuid,
        variant: Option<String>,
        inputs: BTreeMap<String, Value>,
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
        TemplateCommand::Migrate {
            instance,
            definition,
            variant,
            inputs,
        } => {
            let old = find_instance(project, *instance)?.clone();
            let previous = kronello_template::definition(project, old.definition_ref)?;
            let old_definition = old.definition_ref;
            let edition = kronello_template::definition(project, *definition)?;
            if previous.template_id != edition.template_id {
                return Err(ServiceError::new(
                    "TEMPLATE_MIGRATION_INCOMPATIBLE",
                    "different template families",
                ));
            }
            let selected = kronello_template::selected_definition(edition, variant.as_deref())?;
            let next = TemplateInstance {
                definition_ref: *definition,
                version: edition.version.clone(),
                variant: variant.clone(),
                inputs: inputs.clone(),
                ..old
            };
            kronello_template::resolved_inputs(edition, &next)?;
            let map = kronello_template::duration_map(
                kronello_template::composition(project, selected.composition_ref)?.duration,
                next.duration,
                &selected.duration_policy,
            )?;
            keys.insert(changed(*definition, project.id));
            keys.insert(changed(old_definition, project.id));
            keys.insert(changed(instance.as_uuid(), instance.as_uuid()));
            for object in &mut project.compositions {
                if let DocumentObject::Known(c) = object {
                    for node in &mut c.nodes {
                        if let NodeKind::CompositionInstance(p) = &mut node.kind
                            && p.id == *instance
                        {
                            p.definition_ref = selected.composition_ref;
                            p.local_time_map = map.clone();
                            keys.insert(changed(node.id.as_uuid(), c.id.as_uuid()));
                        }
                    }
                }
            }
            *project
                .template_instances
                .iter_mut()
                .find_map(|object| match object {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .unwrap() = next;
        }
        TemplateCommand::SetDuration { instance, duration } => {
            let i = project
                .template_instances
                .iter()
                .find_map(|i| match i {
                    DocumentObject::Known(i) if i.id == *instance => Some(i),
                    _ => None,
                })
                .ok_or_else(|| ServiceError::invalid("template instance missing"))?;
            let d = kronello_template::selected_definition(
                kronello_template::definition(project, i.definition_ref)?,
                i.variant.as_deref(),
            )?;
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
            for variant in d.variants.values_mut() {
                variant.content_hash =
                    kronello_template::authoring_hash(project, variant.composition_ref)?;
            }
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
            let edition = kronello_template::definition(project, instance.definition_ref)?;
            let d = kronello_template::selected_definition(edition, instance.variant.as_deref())?;
            kronello_template::resolved_inputs(edition, instance)?;
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
                tags: Default::default(),
                name: None,
                enabled: true,
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

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplatePreviewRequest {
    pub project: PathBuf,
    pub instance: TemplateInstance,
    pub time: Time,
    pub fonts: Vec<crate::FontInput>,
    /// Omitted returns semantic preview data without initializing a backend.
    #[serde(default)]
    pub region: Option<kronello_render::OutputRegion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateMigrationPlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub instance: CompositionInstanceId,
    pub definition: Uuid,
    #[serde(default)]
    pub variant: Option<String>,
    /// Omitted retains every existing override; incompatible inputs must be resolved explicitly.
    #[serde(default)]
    pub inputs: Option<BTreeMap<String, Value>>,
    pub time: Time,
    pub fonts: Vec<crate::FontInput>,
    /// Omitted returns semantic preview data without initializing a backend.
    #[serde(default)]
    pub region: Option<kronello_render::OutputRegion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplatePreviewNode {
    pub key: crate::SceneNodeKey,
    pub evaluated: crate::SceneNodeEvaluation,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplatePreviewResult {
    pub revision: String,
    pub instance: TemplateInstance,
    /// Effective variant bindings, public schemas, duration and bounds policies.
    pub definition: TemplateDefinition,
    pub design_extent: DesignExtent,
    pub local_time: Time,
    pub resolved_inputs: BTreeMap<String, Value>,
    pub media_slots: BTreeMap<NodeId, AssetId>,
    pub nodes: Vec<TemplatePreviewNode>,
    pub frame: Option<crate::FrameResult>,
    /// A failed preview is diagnostic data, never a render success.
    pub diagnostic: Option<ServiceError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateChange {
    pub field: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateMigrationPlan {
    pub plan: crate::EditPlan,
    pub changes: Vec<TemplateChange>,
    pub before: TemplatePreviewResult,
    pub after: TemplatePreviewResult,
}
fn find_instance(
    project: &Project,
    id: CompositionInstanceId,
) -> Result<&TemplateInstance, ServiceError> {
    project
        .template_instances
        .iter()
        .find_map(|object| match object {
            DocumentObject::Known(i) if i.id == id => Some(i),
            _ => None,
        })
        .ok_or_else(|| ServiceError::invalid("template instance missing"))
}
fn preview_candidate(
    stored: &kronello_store::Snapshot,
    path: &std::path::Path,
    instance: TemplateInstance,
    time: Time,
    fonts: &[crate::FontInput],
    region: Option<kronello_render::OutputRegion>,
    service: &crate::Service<'_>,
) -> Result<TemplatePreviewResult, ServiceError> {
    use sha2::{Digest, Sha256};
    let edition = kronello_template::definition(&stored.document, instance.definition_ref)?;
    let definition = kronello_template::selected_definition(edition, instance.variant.as_deref())?;
    let values = kronello_template::resolved_inputs(edition, &instance)?;
    let source = kronello_template::composition(&stored.document, definition.composition_ref)?;
    let map = kronello_template::duration_map(
        source.duration,
        instance.duration,
        &definition.duration_policy,
    )?;
    if time < Time::ZERO || time >= instance.duration.as_time() {
        return Err(ServiceError::invalid(
            "preview time must lie in [0,duration)",
        ));
    }
    let mut media_slots = BTreeMap::new();
    for (target, value) in kronello_template::input_bindings(&definition, &values)? {
        if let TemplateInputTarget::MediaSlot { node } = target {
            kronello_template::validate_asset(&stored.document, &value)?;
            let Value::AssetRef(asset) = value else {
                unreachable!("validated input")
            };
            media_slots.insert(node, asset);
        }
    }
    let mut result = TemplatePreviewResult {
        revision: stored.revision.to_string(),
        instance: instance.clone(),
        definition: definition.clone(),
        design_extent: source.design_extent,
        local_time: map
            .map(time)
            .map_err(|e| ServiceError::invalid(e.to_string()))?,
        resolved_inputs: values,
        media_slots,
        nodes: vec![],
        frame: None,
        diagnostic: None,
    };
    // Build an isolated, deterministic placement in an owned temporary snapshot.
    // Semantic construction uses no project writes or "latest" definition lookup.
    let hash = Sha256::digest(format!("template-preview:{}", instance.id).as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    let root_id = CompositionId::from_uuid(Uuid::from_bytes(bytes));
    if stored.document.compositions.iter().any(|c| match c {
        DocumentObject::Known(c) => c.id == root_id,
        DocumentObject::Opaque(c) => c.id == root_id.as_uuid(),
    }) {
        return Err(ServiceError::invalid("preview root identity collision"));
    }
    bytes.copy_from_slice(&hash[16..]);
    let node_id = NodeId::from_uuid(Uuid::from_bytes(bytes));
    let placement = SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        id: node_id,
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id: instance.id,
            definition_ref: definition.composition_ref,
            input_bindings: BTreeMap::new(),
            local_time_map: map,
            seed: 0,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::from_start_duration(Time::ZERO, instance.duration)
            .map_err(|e| ServiceError::invalid(e.to_string()))?,
        properties: vec![],
        effects: vec![],
    };
    let mut candidate = stored.clone();
    // Remove the old placement for this ID so validation sees exactly one pin.
    for object in &mut candidate.document.compositions {
        if let DocumentObject::Known(c) = object {
            let removed: BTreeSet<_> = c
                .nodes
                .iter()
                .filter(
                    |n| matches!(&n.kind, NodeKind::CompositionInstance(p) if p.id == instance.id),
                )
                .map(|n| n.id)
                .collect();
            c.nodes.retain(|n| !removed.contains(&n.id));
            c.root_nodes.retain(|id| !removed.contains(id));
            for node in &mut c.nodes {
                node.child_order.retain(|id| !removed.contains(id));
            }
        }
    }
    candidate
        .document
        .template_instances
        .retain(|object| match object {
            DocumentObject::Known(i) => i.id != instance.id,
            DocumentObject::Opaque(i) => i.id != instance.id.as_uuid(),
        });
    candidate
        .document
        .template_instances
        .push(DocumentObject::Known(instance));
    candidate
        .document
        .compositions
        .push(DocumentObject::Known(Composition {
            id: root_id,
            duration: result.instance.duration,
            design_extent: source.design_extent,
            edit_rate: source.edit_rate,
            root_nodes: vec![node_id],
            nodes: vec![placement],
            properties: vec![],
        }));
    match crate::query::evaluated_scene(&candidate, path, root_id, time, fonts) {
        Ok(scene) => {
            result.nodes = scene
                .nodes
                .into_iter()
                .filter(|n| !n.key.instance_path.ids().is_empty())
                .map(|n| {
                    let layout_bounds = match &n.content {
                        kronello_render::SceneContent::Text(layout) => Some(crate::QueryBounds {
                            min: layout.layout_bounds.min,
                            max: layout.layout_bounds.max,
                        }),
                        _ => None,
                    };
                    TemplatePreviewNode {
                        key: crate::SceneNodeKey {
                            instance_path: n.key.instance_path,
                            node: n.key.node,
                        },
                        evaluated: crate::SceneNodeEvaluation {
                            properties: n.properties,
                            text: n.text,
                            layout_bounds,
                            bounds: n.bounds,
                            world_transform: n.world_transform.0,
                            effects: n.effects,
                        },
                    }
                })
                .collect();
        }
        Err(error) => result.diagnostic = Some(error),
    }
    if result.diagnostic.is_none()
        && let Some(region) = region
    {
        let frame = (|| {
            region.validate()?;
            let input = crate::RenderInput {
                project: path.into(),
                composition: Some(root_id),
                target: None,
                region,
                profile: Default::default(),
                fonts: fonts.to_vec(),
            };
            let snapshot = crate::freeze_render_input(&candidate, &input)?;
            let bytes = crate::load_locked_fonts(&snapshot, &input)?;
            let locked: Vec<_> = snapshot
                .font_locks()
                .iter()
                .zip(&bytes)
                .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
                .collect();
            service.with_selected_backend(|backend| {
                let frame = kronello_render::render_frame(
                    &snapshot,
                    &locked,
                    backend,
                    kronello_render::FrameRequest { time, region },
                )?;
                Ok(crate::FrameResult {
                    metadata: frame.metadata,
                    linear: frame.pixels.linear,
                    display: frame.pixels.display,
                })
            })
        })();
        match frame {
            Ok(frame) => result.frame = Some(frame),
            Err(error) => result.diagnostic = Some(error),
        }
    }
    Ok(result)
}
pub(crate) fn preview(
    r: TemplatePreviewRequest,
    service: &crate::Service<'_>,
) -> Result<TemplatePreviewResult, ServiceError> {
    let store = crate::open_existing(&r.project)?;
    let stored = store.snapshot()?;
    store.close()?;
    preview_candidate(
        &stored, &r.project, r.instance, r.time, &r.fonts, r.region, service,
    )
}
pub(crate) fn migration_plan(
    r: TemplateMigrationPlanRequest,
    service: &crate::Service<'_>,
) -> Result<TemplateMigrationPlan, ServiceError> {
    let store = crate::open_existing(&r.project)?;
    let stored = store.snapshot()?;
    store.close()?;
    let base = crate::parse_revision(&r.base_revision)?;
    if stored.revision != base {
        return Err(kronello_store::StoreError::RevisionConflict {
            base,
            current: stored.revision,
        }
        .into());
    }
    let old = find_instance(&stored.document, r.instance)?.clone();
    let inputs = r.inputs.unwrap_or_else(|| old.inputs.clone());
    let plan = crate::edit::build(
        stored.document.clone(),
        base,
        vec![EditCommand::Template(Box::new(TemplateCommand::Migrate {
            instance: r.instance,
            definition: r.definition,
            variant: r.variant,
            inputs,
        }))],
    )?;
    let next = find_instance(&plan.candidate, r.instance)?.clone();
    let before = preview_candidate(
        &stored, &r.project, old, r.time, &r.fonts, r.region, service,
    )?;
    let mut candidate = stored.clone();
    candidate.document = plan.candidate.clone();
    let after = preview_candidate(
        &candidate, &r.project, next, r.time, &r.fonts, r.region, service,
    )?;
    let a = serde_json::json!({"definition":before.definition,"instance":before.instance,"resolved_inputs":before.resolved_inputs,"design_extent":before.design_extent,"media_slots":before.media_slots});
    let b = serde_json::json!({"definition":after.definition,"instance":after.instance,"resolved_inputs":after.resolved_inputs,"design_extent":after.design_extent,"media_slots":after.media_slots});
    let mut changes = vec![];
    fn compare(
        path: String,
        a: &serde_json::Value,
        b: &serde_json::Value,
        changes: &mut Vec<TemplateChange>,
    ) {
        if a == b {
            return;
        }
        if let (Some(a), Some(b)) = (a.as_object(), b.as_object()) {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                compare(
                    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    a.get(key).unwrap_or(&serde_json::Value::Null),
                    b.get(key).unwrap_or(&serde_json::Value::Null),
                    changes,
                );
            }
        } else {
            changes.push(TemplateChange {
                field: path,
                before: a.clone(),
                after: b.clone(),
            });
        }
    }
    compare(String::new(), &a, &b, &mut changes);
    Ok(TemplateMigrationPlan {
        plan,
        changes,
        before,
        after,
    })
}
