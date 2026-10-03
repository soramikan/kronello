//! Deterministic edit planning and persisted selective undo policy.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{ServiceError, open_existing, parse_revision};
use kronello_model::{
    AnimationCurve, Composition, CompositionId, CurveId, DocumentObject, ExpressionId, Keyframe,
    NodeId, NodeKind, Project, Property, PropertyId, PropertySource, SceneNode, SchemaRegistry,
    Shape, SourceResolver, TextDocument, Value, ValueType,
};
use kronello_store::{ApplyRequest, ChangedKey, Event, Mutation, ProjectStore, StoreError};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Externally tagged commands decode directly, retaining strict fields and
/// concrete model number decoding instead of serde's tagged Content buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EditCommand {
    PropertySourceSet {
        object: Uuid,
        property: PropertyId,
        source: PropertySource<Value>,
        #[serde(default)]
        curve: Option<AnimationCurve>,
    },
    KeyframeInsert {
        curve: CurveId,
        key: Keyframe,
    },
    KeyframeUpsert {
        curve: CurveId,
        key: Keyframe,
    },
    KeyframeReplace {
        curve: CurveId,
        key: Keyframe,
    },
    KeyframeRemove {
        curve: CurveId,
        time: Time,
    },
    NodeAdd {
        composition: CompositionId,
        node: SceneNode,
        index: usize,
    },
    NodeRemove {
        composition: CompositionId,
        node: NodeId,
    },
    NodeReparent {
        composition: CompositionId,
        node: NodeId,
        parent: Option<NodeId>,
        index: usize,
    },
    TransformParentSet {
        composition: CompositionId,
        node: NodeId,
        parent: Option<NodeId>,
    },
    NodeReorder {
        composition: CompositionId,
        parent: Option<NodeId>,
        order: Vec<NodeId>,
    },
    ShapeSet {
        shape: Shape,
    },
    TextSet {
        text: TextDocument,
    },
    CompositionCreate {
        composition: Composition,
    },
    InstancePlace {
        composition: CompositionId,
        node: SceneNode,
        index: usize,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub commands: Vec<EditCommand>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditApplyRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub plan_hash: String,
    pub idempotency_key: String,
    pub session_id: Uuid,
    pub commands: Vec<EditCommand>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub event_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub project: PathBuf,
    #[serde(default = "zero")]
    pub since_revision: String,
}
fn zero() -> String {
    "0".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditPlan {
    pub project_id: Uuid,
    pub base_revision: String,
    pub commands: Vec<EditCommand>,
    pub mutations: Vec<Mutation>,
    pub inverse: Vec<Mutation>,
    pub changed_keys: BTreeSet<ChangedKey>,
    pub candidate: Project,
    pub plan_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub event: Event,
    pub undone: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryResult {
    pub revision: String,
    pub events: Vec<HistoryEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoConflict {
    pub event_id: Uuid,
    pub keys: BTreeSet<ChangedKey>,
}

fn invalid(e: impl std::fmt::Debug) -> ServiceError {
    ServiceError::new("INVALID_EDIT", format!("{e:?}"))
}
fn revision(base: u64, current: u64) -> Result<(), ServiceError> {
    if base != current {
        return Err(StoreError::RevisionConflict { base, current }.into());
    }
    Ok(())
}
fn registry() -> SchemaRegistry {
    let mut r = SchemaRegistry::with_builtin();
    for d in kronello_model::shape_descriptors()
        .into_iter()
        .chain(kronello_model::text_descriptors())
    {
        r.register(d).expect("built-in descriptors are distinct");
    }
    r
}
struct Catalog<'a>(&'a Project);
impl SourceResolver for Catalog<'_> {
    fn curve_value_type(&self, id: CurveId) -> Option<ValueType> {
        self.0.curves.iter().find_map(|c| match c {
            DocumentObject::Known(c) if c.id() == id => Some(c.value_type()),
            _ => None,
        })
    }
    fn expression_value_type(&self, _: ExpressionId) -> Option<ValueType> {
        None
    }
}
pub(crate) fn validate(project: &Project) -> Result<(), ServiceError> {
    project.ensure_editable().map_err(StoreError::from)?;
    let r = registry();
    let compositions: Vec<_> = project
        .compositions
        .iter()
        .map(|c| match c {
            DocumentObject::Known(c) => c.clone(),
            _ => unreachable!(),
        })
        .collect();
    // Commands and changed keys address objects by UUID without a type tag.
    // Reject cross-kind aliases that would make lookup or conflict checks
    // ambiguous even when each model collection is independently valid.
    let mut object_ids = BTreeSet::from([project.id]);
    object_ids.extend(project.curves.iter().filter_map(|c| match c {
        DocumentObject::Known(c) => Some(c.id().as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.shapes.iter().filter_map(|s| match s {
        DocumentObject::Known(s) => Some(s.id.as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.texts.iter().filter_map(|t| match t {
        DocumentObject::Known(t) => Some(t.id.as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    for c in &compositions {
        if !object_ids.insert(c.id.as_uuid()) {
            return Err(invalid("ambiguous object id"));
        }
        for n in &c.nodes {
            if !object_ids.insert(n.id.as_uuid()) {
                return Err(invalid("ambiguous object id"));
            }
        }
    }
    kronello_model::validate_compositions(&compositions, &r).map_err(invalid)?;
    kronello_model::validate_shape_contents(project, &r).map_err(invalid)?;
    kronello_model::validate_text_contents(project, &r).map_err(invalid)?;
    for (_, p) in properties(project) {
        p.validate_sources(&r, &Catalog(project)).map_err(invalid)?;
    }
    for c in &compositions {
        for n in &c.nodes {
            if let NodeKind::CompositionInstance(i) = &n.kind {
                let target = compositions
                    .iter()
                    .find(|c| c.id == i.definition_ref)
                    .ok_or_else(|| invalid("missing definition"))?;
                for (id, source) in &i.input_bindings {
                    let mut p = target
                        .properties
                        .iter()
                        .find(|p| p.id() == *id)
                        .ok_or_else(|| invalid("missing input"))?
                        .clone();
                    p.set_source(source.clone(), &r).map_err(invalid)?;
                    p.validate_sources(&r, &Catalog(project)).map_err(invalid)?;
                }
            }
        }
    }
    Ok(())
}
fn properties(project: &Project) -> Vec<(Uuid, &Property)> {
    let mut result = Vec::new();
    for c in &project.compositions {
        if let DocumentObject::Known(c) = c {
            result.extend(c.properties.iter().map(|p| (c.id.as_uuid(), p)));
            for n in &c.nodes {
                result.extend(n.properties.iter().map(|p| (n.id.as_uuid(), p)));
            }
        }
    }
    result
}
fn property_mut(
    project: &mut Project,
    object: Uuid,
    id: PropertyId,
) -> Result<&mut Property, ServiceError> {
    for c in &mut project.compositions {
        if let DocumentObject::Known(c) = c {
            if c.id.as_uuid() == object {
                return c
                    .properties
                    .iter_mut()
                    .find(|p| p.id() == id)
                    .ok_or_else(|| invalid("property not found"));
            }
            for n in &mut c.nodes {
                if n.id.as_uuid() == object {
                    return n
                        .properties
                        .iter_mut()
                        .find(|p| p.id() == id)
                        .ok_or_else(|| invalid("property not found"));
                }
            }
        }
    }
    Err(invalid("object not found"))
}
fn composition_mut(
    project: &mut Project,
    id: CompositionId,
) -> Result<&mut Composition, ServiceError> {
    project
        .compositions
        .iter_mut()
        .find_map(|c| match c {
            DocumentObject::Known(c) if c.id == id => Some(c),
            _ => None,
        })
        .ok_or_else(|| invalid("composition not found"))
}
fn node_mut(c: &mut Composition, id: NodeId) -> Result<&mut SceneNode, ServiceError> {
    c.nodes
        .iter_mut()
        .find(|n| n.id == id)
        .ok_or_else(|| invalid("node not found"))
}
fn order_mut(
    c: &mut Composition,
    parent: Option<NodeId>,
) -> Result<&mut Vec<NodeId>, ServiceError> {
    match parent {
        None => Ok(&mut c.root_nodes),
        Some(id) => Ok(&mut node_mut(c, id)?.child_order),
    }
}
fn structure(keys: &mut BTreeSet<ChangedKey>, object: Uuid, parent: Uuid) {
    keys.insert(ChangedKey::Structure {
        object_id: object,
        parent_container_id: parent,
    });
}
fn node_key(keys: &mut BTreeSet<ChangedKey>, c: CompositionId, n: NodeId, parent: Option<NodeId>) {
    structure(
        keys,
        n.as_uuid(),
        parent.map_or(c.as_uuid(), NodeId::as_uuid),
    );
}
fn curve_keys(project: &Project, id: CurveId, keys: &mut BTreeSet<ChangedKey>) {
    // Shared curve edits affect every direct property consumer, independently of
    // downstream evaluation dependencies.
    for (object, p) in properties(project) {
        if p.source() == &PropertySource::Curve(id) {
            keys.insert(ChangedKey::Value {
                object_id: object,
                property_id: p.id(),
            });
        }
    }
    for c in &project.compositions {
        if let DocumentObject::Known(c) = c {
            for n in &c.nodes {
                if let NodeKind::CompositionInstance(i) = &n.kind {
                    for (property, source) in &i.input_bindings {
                        if source == &PropertySource::Curve(id) {
                            keys.insert(ChangedKey::Value {
                                object_id: n.id.as_uuid(),
                                property_id: *property,
                            });
                        }
                    }
                }
            }
        }
    }
    structure(keys, id.as_uuid(), id.as_uuid());
}
fn apply_command(
    project: &mut Project,
    command: &EditCommand,
    keys: &mut BTreeSet<ChangedKey>,
) -> Result<(), ServiceError> {
    match command {
        EditCommand::PropertySourceSet {
            object,
            property,
            source,
            curve,
        } => {
            if matches!(source, PropertySource::Expression(_)) {
                return Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "expression edits are not implemented",
                ));
            }
            if let Some(curve) = curve {
                if source != &PropertySource::Curve(curve.id()) {
                    return Err(invalid("curve does not match source"));
                }
                curve_keys(project, curve.id(), keys);
                if let Some(existing) = project
                    .curves
                    .iter_mut()
                    .find(|c| matches!(c, DocumentObject::Known(c) if c.id() == curve.id()))
                {
                    *existing = DocumentObject::Known(curve.clone());
                } else {
                    project.curves.push(DocumentObject::Known(curve.clone()));
                }
            }
            property_mut(project, *object, *property)?
                .set_source(source.clone(), &registry())
                .map_err(invalid)?;
            if let PropertySource::Curve(id) = source {
                structure(keys, id.as_uuid(), id.as_uuid());
            }
            keys.insert(ChangedKey::Value {
                object_id: *object,
                property_id: *property,
            });
        }
        EditCommand::KeyframeInsert { curve, .. }
        | EditCommand::KeyframeUpsert { curve, .. }
        | EditCommand::KeyframeReplace { curve, .. }
        | EditCommand::KeyframeRemove { curve, .. } => {
            curve_keys(project, *curve, keys);
            let c = project
                .curves
                .iter_mut()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id() == *curve => Some(c),
                    _ => None,
                })
                .ok_or_else(|| invalid("curve not found"))?;
            match command {
                EditCommand::KeyframeInsert { key, .. } => {
                    c.insert_key(key.clone()).map_err(invalid)?
                }
                EditCommand::KeyframeUpsert { key, .. } => {
                    c.upsert_key(key.clone()).map_err(invalid)?;
                }
                EditCommand::KeyframeReplace { key, .. } => {
                    c.replace_key(key.clone()).map_err(invalid)?;
                }
                EditCommand::KeyframeRemove { time, .. } => {
                    c.ensure_supported_version().map_err(invalid)?;
                    let mut keys = c.keys().to_vec();
                    let i = keys
                        .iter()
                        .position(|k| k.time == *time)
                        .ok_or_else(|| invalid("keyframe not found"))?;
                    keys.remove(i);
                    *c = AnimationCurve::new(c.id(), c.value_type(), keys).map_err(invalid)?;
                }
                _ => unreachable!(),
            }
        }
        EditCommand::NodeAdd {
            composition,
            node,
            index,
        }
        | EditCommand::InstancePlace {
            composition,
            node,
            index,
        } => {
            node_references(node, keys);
            if matches!(command, EditCommand::InstancePlace { .. })
                && !matches!(node.kind, NodeKind::CompositionInstance(_))
            {
                return Err(invalid(
                    "instance placement requires a composition instance node",
                ));
            }
            let c = composition_mut(project, *composition)?;
            if c.nodes.iter().any(|n| n.id == node.id) {
                return Err(invalid("node already exists"));
            }
            node_key(keys, *composition, node.id, node.containment_parent);
            let order = order_mut(c, node.containment_parent)?;
            if *index > order.len() {
                return Err(invalid("index out of bounds"));
            }
            order.insert(*index, node.id);
            c.nodes.push(node.clone());
        }
        EditCommand::NodeRemove { composition, node } => {
            let c = composition_mut(project, *composition)?;
            let parent = node_mut(c, *node)?.containment_parent;
            let mut remove = BTreeSet::from([*node]);
            loop {
                let before = remove.len();
                for n in &c.nodes {
                    if n.containment_parent.is_some_and(|p| remove.contains(&p)) {
                        remove.insert(n.id);
                    }
                }
                if remove.len() == before {
                    break;
                }
            }
            for n in &c.nodes {
                if remove.contains(&n.id) {
                    node_key(keys, *composition, n.id, n.containment_parent);
                }
            }
            order_mut(c, parent)?.retain(|id| id != node);
            c.nodes.retain(|n| !remove.contains(&n.id));
        }
        EditCommand::NodeReparent {
            composition,
            node,
            parent,
            index,
        } => {
            let c = composition_mut(project, *composition)?;
            let old = node_mut(c, *node)?.containment_parent;
            node_key(keys, *composition, *node, old);
            node_key(keys, *composition, *node, *parent);
            order_mut(c, old)?.retain(|id| id != node);
            let order = order_mut(c, *parent)?;
            if *index > order.len() {
                return Err(invalid("index out of bounds"));
            }
            order.insert(*index, *node);
            node_mut(c, *node)?.containment_parent = *parent;
        }
        EditCommand::TransformParentSet {
            composition,
            node,
            parent,
        } => {
            let c = composition_mut(project, *composition)?;
            let n = node_mut(c, *node)?;
            node_key(keys, *composition, *node, n.containment_parent);
            for p in [n.transform_parent, *parent].into_iter().flatten() {
                structure(keys, p.as_uuid(), p.as_uuid());
            }
            n.transform_parent = *parent;
        }
        EditCommand::NodeReorder {
            composition,
            parent,
            order,
        } => {
            let c = composition_mut(project, *composition)?;
            for n in order_mut(c, *parent)?.iter().chain(order) {
                node_key(keys, *composition, *n, *parent);
            }
            *order_mut(c, *parent)? = order.clone();
        }
        EditCommand::CompositionCreate { composition } => {
            if project
                .compositions
                .iter()
                .any(|c| matches!(c, DocumentObject::Known(c) if c.id == composition.id))
            {
                return Err(invalid("composition already exists"));
            }
            structure(keys, composition.id.as_uuid(), project.id);
            for property in &composition.properties {
                if let PropertySource::Curve(id) = property.source() {
                    structure(keys, id.as_uuid(), id.as_uuid());
                }
            }
            for n in &composition.nodes {
                node_key(keys, composition.id, n.id, n.containment_parent);
                node_references(n, keys);
            }
            project
                .compositions
                .push(DocumentObject::Known(composition.clone()));
        }
        EditCommand::ShapeSet { shape } => {
            content_keys(project, shape.id.as_uuid(), keys);
            if let Some(s) = project
                .shapes
                .iter_mut()
                .find(|s| matches!(s, DocumentObject::Known(s) if s.id == shape.id))
            {
                *s = DocumentObject::Known(shape.clone());
            } else {
                project.shapes.push(DocumentObject::Known(shape.clone()));
            }
        }
        EditCommand::TextSet { text } => {
            content_keys(project, text.id.as_uuid(), keys);
            if let Some(t) = project
                .texts
                .iter_mut()
                .find(|t| matches!(t, DocumentObject::Known(t) if t.id == text.id))
            {
                *t = DocumentObject::Known(text.clone());
            } else {
                project.texts.push(DocumentObject::Known(text.clone()));
            }
        }
    }
    Ok(())
}
fn node_references(node: &SceneNode, keys: &mut BTreeSet<ChangedKey>) {
    for p in &node.properties {
        if let PropertySource::Curve(id) = p.source() {
            structure(keys, id.as_uuid(), id.as_uuid());
        }
    }
    match &node.kind {
        NodeKind::Shape { content_ref } | NodeKind::Text { content_ref } => {
            structure(keys, content_ref.as_uuid(), content_ref.as_uuid())
        }
        NodeKind::CompositionInstance(i) => {
            structure(keys, i.definition_ref.as_uuid(), i.definition_ref.as_uuid());
            for source in i.input_bindings.values() {
                if let PropertySource::Curve(id) = source {
                    structure(keys, id.as_uuid(), id.as_uuid());
                }
            }
        }
        _ => (),
    }
    if let Some(parent) = node.transform_parent {
        structure(keys, parent.as_uuid(), parent.as_uuid());
    }
}
fn content_keys(project: &Project, id: Uuid, keys: &mut BTreeSet<ChangedKey>) {
    structure(keys, id, id);
    for c in &project.compositions {
        if let DocumentObject::Known(c) = c {
            for n in &c.nodes {
                if matches!(n.kind, NodeKind::Shape { content_ref } | NodeKind::Text { content_ref } if content_ref.as_uuid() == id)
                {
                    structure(keys, id, n.id.as_uuid());
                }
            }
        }
    }
}
/// Diff ID-bearing document collections member by member. Scalar order arrays
/// remain atomic; structure conflicts protect their parent containers.
fn diff(old: &Json, new: &Json, path: &mut Vec<String>, out: &mut Vec<Mutation>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Json::Object(a), Json::Object(b)) => {
            for (key, value) in a {
                path.push(key.clone());
                if let Some(next) = b.get(key) {
                    diff(value, next, path, out);
                } else {
                    if path.len() == 1 && matches!(key.as_str(), "shapes" | "texts") {
                        diff(value, &Json::Array(vec![]), path, out);
                    } else {
                        out.push(Mutation::Remove { path: path.clone() });
                    }
                }
                path.pop();
            }
            for (key, value) in b {
                if !a.contains_key(key) {
                    path.push(key.clone());
                    if path.len() == 1 && matches!(key.as_str(), "shapes" | "texts") {
                        diff(&Json::Array(vec![]), value, path, out);
                    } else {
                        out.push(Mutation::Set {
                            path: path.clone(),
                            value: value.clone(),
                        });
                    }
                    path.pop();
                }
            }
        }
        (Json::Array(a), Json::Array(b))
            if a.iter()
                .chain(b)
                .all(|v| v.get("id").and_then(Json::as_str).is_some()) =>
        {
            let a: BTreeMap<_, _> = a.iter().map(|v| (v["id"].as_str().unwrap(), v)).collect();
            let b: BTreeMap<_, _> = b.iter().map(|v| (v["id"].as_str().unwrap(), v)).collect();
            for (id, value) in &a {
                path.push((*id).into());
                if let Some(next) = b.get(id) {
                    diff(value, next, path, out);
                } else {
                    out.push(Mutation::Remove { path: path.clone() });
                }
                path.pop();
            }
            for (id, value) in b {
                if !a.contains_key(id) {
                    path.push(id.into());
                    out.push(Mutation::Set {
                        path: path.clone(),
                        value: value.clone(),
                    });
                    path.pop();
                }
            }
        }
        _ => out.push(Mutation::Set {
            path: path.clone(),
            value: new.clone(),
        }),
    }
}
fn build(
    document: Project,
    base: u64,
    commands: Vec<EditCommand>,
) -> Result<EditPlan, ServiceError> {
    document.ensure_editable().map_err(StoreError::from)?;
    if commands.is_empty() {
        return Err(invalid("commands must not be empty"));
    }
    let mut candidate = document.clone();
    let mut changed_keys = BTreeSet::new();
    for command in &commands {
        apply_command(&mut candidate, command, &mut changed_keys)?;
    }
    validate(&candidate)?;
    let mut mutations = Vec::new();
    let mut value = serde_json::to_value(&document)?;
    diff(
        &value,
        &serde_json::to_value(&candidate)?,
        &mut Vec::new(),
        &mut mutations,
    );
    let mut inverse = Vec::new();
    for m in &mutations {
        inverse.push(m.apply(&mut value)?);
    }
    inverse.reverse();
    // Use the exact patch-normalized candidate (collection order is nonsemantic).
    let candidate: Project = serde_json::from_str(&value.to_string())?;
    validate(&candidate)?;
    let mut plan = EditPlan {
        project_id: document.id,
        base_revision: base.to_string(),
        commands,
        mutations,
        inverse,
        changed_keys,
        candidate,
        plan_hash: String::new(),
    };
    plan.plan_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(&plan)?)?)
    );
    Ok(plan)
}
pub(crate) fn plan(r: PlanRequest) -> Result<EditPlan, ServiceError> {
    let base = parse_revision(&r.base_revision)?;
    let store = open_existing(&r.project)?;
    let s = store.snapshot()?;
    store.close()?;
    revision(base, s.revision)?;
    build(s.document, base, r.commands)
}
fn key(key: &str) -> Result<(), ServiceError> {
    if key.is_empty() || key.len() > 256 {
        return Err(ServiceError::invalid(
            "idempotency_key must contain 1..256 UTF-8 bytes",
        ));
    }
    Ok(())
}
fn retry(store: &ProjectStore, key: &str, payload: &Json) -> Result<Option<Event>, ServiceError> {
    if let Some(r) = store.idempotency_record(key)? {
        if r.service_payload.as_ref() != Some(payload) {
            return Err(StoreError::IdempotencyKeyReused.into());
        }
        return Ok(Some(r.result));
    }
    Ok(None)
}
pub(crate) fn apply(r: EditApplyRequest) -> Result<Event, ServiceError> {
    key(&r.idempotency_key)?;
    let base = parse_revision(&r.base_revision)?;
    let payload = json!({"operation":"edit.apply", "base_revision":base.to_string(), "plan_hash":r.plan_hash, "session_id":r.session_id, "commands":r.commands});
    let mut store = open_existing(&r.project)?;
    let result = (|| {
        if let Some(event) = retry(&store, &r.idempotency_key, &payload)? {
            return Ok(event);
        }
        let s = store.snapshot()?;
        if base != s.revision {
            return retry(&store, &r.idempotency_key, &payload)?.ok_or_else(|| {
                StoreError::RevisionConflict {
                    base,
                    current: s.revision,
                }
                .into()
            });
        }
        let plan = build(s.document, base, r.commands)?;
        if plan.plan_hash != r.plan_hash {
            return Err(ServiceError::new(
                "PLAN_HASH_MISMATCH",
                "plan content differs from the planned hash",
            ));
        }
        let request = ApplyRequest {
            base_revision: base,
            session_id: r.session_id,
            mutations: plan.mutations,
            changed_keys: plan.changed_keys,
            idempotency_key: Some(r.idempotency_key.clone()),
            undo_of: None,
        };
        match store.apply_with_payload(request, payload.clone()) {
            Ok(e) => Ok(e),
            Err(StoreError::RevisionConflict { base, current }) => {
                retry(&store, &r.idempotency_key, &payload)?
                    .ok_or_else(|| StoreError::RevisionConflict { base, current }.into())
            }
            Err(e) => Err(e.into()),
        }
    })();
    store.close()?;
    result
}
fn active(events: &[Event]) -> BTreeSet<Uuid> {
    let mut cancelled = BTreeSet::new();
    let mut active = BTreeSet::new();
    for e in events.iter().rev() {
        if !cancelled.contains(&e.id) {
            active.insert(e.id);
            if let Some(id) = e.undo_of {
                cancelled.insert(id);
            }
        }
    }
    active
}
fn overlap(a: &ChangedKey, b: &ChangedKey) -> bool {
    match (a, b) {
        (
            ChangedKey::Value {
                object_id: a,
                property_id: p,
            },
            ChangedKey::Value {
                object_id: b,
                property_id: q,
            },
        ) => a == b && p == q,
        (
            ChangedKey::Structure {
                object_id: a,
                parent_container_id: p,
            },
            ChangedKey::Structure {
                object_id: b,
                parent_container_id: q,
            },
        ) => a == b || a == q || p == b || p == q,
        (
            ChangedKey::Structure { object_id, .. },
            ChangedKey::Value {
                object_id: other, ..
            },
        )
        | (
            ChangedKey::Value {
                object_id: other, ..
            },
            ChangedKey::Structure { object_id, .. },
        ) => object_id == other,
    }
}
pub(crate) fn undo(r: UndoRequest) -> Result<Event, ServiceError> {
    key(&r.idempotency_key)?;
    let base = parse_revision(&r.base_revision)?;
    let payload = json!({"operation":"edit.undo","base_revision":base.to_string(),"session_id":r.session_id,"event_id":r.event_id});
    let mut store = open_existing(&r.project)?;
    let result = (|| {
        if let Some(e) = retry(&store, &r.idempotency_key, &payload)? {
            return Ok(e);
        }
        let (s, events) = store.snapshot_and_events()?;
        if base != s.revision {
            return retry(&store, &r.idempotency_key, &payload)?.ok_or_else(|| {
                StoreError::RevisionConflict {
                    base,
                    current: s.revision,
                }
                .into()
            });
        }
        let target = events.iter().find(|e| e.id == r.event_id).ok_or_else(|| {
            ServiceError::new("EVENT_NOT_FOUND", "event is unavailable or compacted")
        })?;
        let active = active(&events);
        if !active.contains(&target.id) {
            return Err(ServiceError::new(
                "EVENT_ALREADY_UNDONE",
                "undo the undo event to redo",
            ));
        }
        let mut conflicts = Vec::new();
        for e in &events {
            if e.revision > target.revision && active.contains(&e.id) {
                let keys: BTreeSet<_> = e
                    .changed_keys
                    .iter()
                    .filter(|key| target.changed_keys.iter().any(|t| overlap(t, key)))
                    .cloned()
                    .collect();
                if !keys.is_empty() {
                    conflicts.push(UndoConflict {
                        event_id: e.id,
                        keys,
                    });
                }
            }
        }
        if !conflicts.is_empty() {
            let mut error =
                ServiceError::new("UNDO_CONFLICT", "later active events touch the target keys");
            error.details = Some(json!({"conflicts":conflicts}));
            return Err(error);
        }
        let mut candidate = serde_json::to_value(s.document)?;
        for m in &target.inverse {
            m.apply(&mut candidate)?;
        }
        validate(&serde_json::from_str(&candidate.to_string())?)?;
        let request = ApplyRequest {
            base_revision: base,
            session_id: r.session_id,
            mutations: target.inverse.clone(),
            changed_keys: target.changed_keys.clone(),
            idempotency_key: Some(r.idempotency_key.clone()),
            undo_of: Some(target.id),
        };
        match store.apply_with_payload(request, payload.clone()) {
            Ok(e) => Ok(e),
            Err(StoreError::RevisionConflict { base, current }) => {
                retry(&store, &r.idempotency_key, &payload)?
                    .ok_or_else(|| StoreError::RevisionConflict { base, current }.into())
            }
            Err(e) => Err(e.into()),
        }
    })();
    store.close()?;
    result
}
pub(crate) fn history(r: HistoryRequest) -> Result<HistoryResult, ServiceError> {
    let since = parse_revision(&r.since_revision)?;
    let store = open_existing(&r.project)?;
    let (s, events) = store.snapshot_and_events()?;
    store.close()?;
    let active = active(&events);
    Ok(HistoryResult {
        revision: s.revision.to_string(),
        events: events
            .into_iter()
            .filter(|e| e.revision > since)
            .map(|event| HistoryEntry {
                undone: !active.contains(&event.id),
                event,
            })
            .collect(),
    })
}
