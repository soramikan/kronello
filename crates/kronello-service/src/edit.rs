//! Deterministic edit planning and persisted selective undo policy.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{ServiceError, open_existing, parse_revision};
use kronello_model::{
    AnimationCurve, Composition, CompositionId, CurveId, DocumentObject, ExpressionId, Keyframe,
    Modifier, ModifierId, NodeId, NodeKind, Project, Property, PropertyId, PropertySource,
    SceneNode, SchemaRegistry, Shape, SourceResolver, TextDocument, Value, ValueType,
};
use kronello_store::{ApplyRequest, ChangedKey, Event, Mutation, ProjectStore, StoreError};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Externally tagged commands decode directly, retaining strict fields and
/// concrete model number decoding instead of serde's tagged Content buffer.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EditCommand {
    MatteSet {
        matte: kronello_model::MatteRelation,
    },
    MatteRemove {
        id: Uuid,
    },
    SimulationSet {
        simulation: kronello_model::ParticleSimulation,
    },
    SimulationRemove {
        id: kronello_model::ContentId,
    },
    RepeaterSet {
        repeater: kronello_model::Repeater,
    },
    RepeaterRemove {
        id: kronello_model::ContentId,
    },
    RepeaterExpand {
        repeater: kronello_model::ContentId,
        instance: kronello_model::CompositionInstanceId,
        expansion_id: Uuid,
    },
    Timeline(Box<crate::TimelineCommand>),
    ExpressionSet {
        expression: kronello_model::Expression,
    },
    /// Commit the complete expression text as the property's expression source.
    /// `metadata` is the editing envelope (id / semantic version / value_type /
    /// budget); it is never re-derived from text. Only complete committed text
    /// may be submitted — uncommitted IME-style input is never parsed or
    /// applied, and malformed input fails planning with typed diagnostics and
    /// preserves the previously stored expression.
    PropertyExpressionTextSet {
        object: Uuid,
        property: PropertyId,
        text: String,
        metadata: kronello_model::ExpressionMetadata,
    },
    Template(Box<crate::TemplateCommand>),
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
    NodeTagsSet {
        composition: CompositionId,
        node: NodeId,
        tags: BTreeSet<String>,
    },
    NodeRename {
        composition: CompositionId,
        node: NodeId,
        name: Option<String>,
    },
    NodePropertyInsert {
        composition: CompositionId,
        node: NodeId,
        property: Property,
    },
    NodeEnabledSet {
        composition: CompositionId,
        node: NodeId,
        enabled: bool,
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
    /// Upsert one validated caption cue document.
    CaptionSet {
        caption: kronello_model::CaptionDocument,
    },
    /// Remove a cue document. Planning rejects a removal that would leave a
    /// caption clip referencing a missing document.
    CaptionRemove {
        id: kronello_model::CaptionId,
    },
    /// Upsert one validated external asset record. COLOR-003 registers `.cube`
    /// documents as `AssetKind::Data`; the locator stays external and the
    /// content hash is caller-independent only through the import operation
    /// that verifies it.
    AssetSet {
        asset: kronello_model::Asset,
    },
    /// NLE-007 (ADR-0127): upsert one multicam group with resolved
    /// `sync_offset`s. `multicam.create` computes the offsets before
    /// planning; dropping an angle still referenced by a clip fails
    /// whole-document validation like any other broken reference.
    MulticamSet {
        multicam: kronello_model::MulticamAsset,
    },
    CompositionCreate {
        composition: Composition,
    },
    InstancePlace {
        composition: CompositionId,
        node: SceneNode,
        index: usize,
    },
    ModifierInsert {
        object: Uuid,
        property: PropertyId,
        modifier: Modifier,
        index: usize,
    },
    ModifierReplace {
        object: Uuid,
        property: PropertyId,
        modifier: Modifier,
    },
    ModifierRemove {
        object: Uuid,
        property: PropertyId,
        modifier: ModifierId,
    },
    ModifierReorder {
        object: Uuid,
        property: PropertyId,
        order: Vec<ModifierId>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub commands: Vec<EditCommand>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditApplyRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub plan_hash: String,
    pub idempotency_key: String,
    pub session_id: Uuid,
    pub commands: Vec<EditCommand>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: Uuid,
    pub idempotency_key: String,
    pub event_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub project: PathBuf,
    #[serde(default = "zero")]
    pub since_revision: String,
    #[serde(default = "history_limit")]
    #[schemars(range(min = 1, max = 1000))]
    pub limit: usize,
    #[serde(default)]
    pub session_id: Option<Uuid>,
}
fn history_limit() -> usize {
    100
}
fn zero() -> String {
    "0".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub event: Event,
    pub undone: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryResult {
    pub revision: String,
    pub events: Vec<HistoryEntry>,
    pub next_since_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoConflict {
    pub event_id: Uuid,
    pub keys: BTreeSet<ChangedKey>,
}

fn invalid(e: impl std::fmt::Debug) -> ServiceError {
    ServiceError::new("INVALID_EDIT", format!("{e:?}"))
}
/// Syntax and AST validation failures share their typed diagnostics payload
/// with every transport; the GUI reads the same byte ranges and expectations.
fn expression_text_error(error: kronello_model::ExpressionTextError) -> ServiceError {
    let mut service = ServiceError::new(error.code(), error.to_string());
    if let Some(diagnostics) = error.diagnostics() {
        service.details = Some(json!({ "diagnostics": diagnostics }));
    }
    service
}
fn revision(base: u64, current: u64) -> Result<(), ServiceError> {
    if base != current {
        return Err(StoreError::RevisionConflict { base, current }.into());
    }
    Ok(())
}
pub(crate) fn registry() -> SchemaRegistry {
    let mut r = SchemaRegistry::with_builtin();
    for d in kronello_model::shape_descriptors()
        .into_iter()
        .chain(kronello_model::text_descriptors())
        .chain(kronello_model::simulation_descriptors())
        .chain(kronello_model::effect_descriptors())
        .chain(kronello_model::caption_descriptors())
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
    fn expression_value_type(&self, id: ExpressionId) -> Option<ValueType> {
        self.0.expressions.iter().find_map(|e| match e {
            DocumentObject::Known(e) if e.id == id => Some(e.value_type),
            _ => None,
        })
    }
}
pub(crate) fn validate(project: &Project) -> Result<(), ServiceError> {
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            s.validate(project)?;
        }
    }
    project.ensure_editable().map_err(StoreError::from)?;
    project
        .validate_repeaters()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    project
        .validate_simulations()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    kronello_template::validate_project(project)?;
    let r = registry();
    let mut compositions: Vec<_> = project
        .compositions
        .iter()
        .map(|c| match c {
            DocumentObject::Known(c) => project.lower_repeater_composition(c),
            _ => unreachable!(),
        })
        .collect::<Result<_, _>>()
        .map_err(|e: kronello_model::RepeaterError| ServiceError::new(e.code(), e.to_string()))?;
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            compositions.push(kronello_render::lower_sequence(project, s.id)?);
        }
    }
    // Commands and changed keys address objects by UUID without a type tag.
    // Reject cross-kind aliases that would make lookup or conflict checks
    // ambiguous even when each model collection is independently valid.
    let mut object_ids = BTreeSet::from([project.id]);
    object_ids.extend(project.curves.iter().filter_map(|c| match c {
        DocumentObject::Known(c) => Some(c.id().as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.expressions.iter().filter_map(|e| match e {
        DocumentObject::Known(e) => Some(e.id.as_uuid()),
        _ => None,
    }));
    object_ids.extend(project.shapes.iter().filter_map(|s| match s {
        DocumentObject::Known(s) => Some(s.id.as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.texts.iter().filter_map(|t| match t {
        DocumentObject::Known(t) => Some(t.id.as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.captions.iter().filter_map(|c| match c {
        DocumentObject::Known(c) => Some(c.id.as_uuid()),
        DocumentObject::Opaque(_) => None,
    }));
    object_ids.extend(project.simulations.iter().filter_map(|s| match s {
        DocumentObject::Known(s) => Some(s.id.as_uuid()),
        _ => None,
    }));
    object_ids.extend(project.repeaters.iter().filter_map(|r| match r {
        DocumentObject::Known(r) => Some(r.id.as_uuid()),
        _ => None,
    }));
    object_ids.extend(
        project
            .expression_data_assets
            .iter()
            .filter_map(|a| match a {
                DocumentObject::Known(a) => Some(a.id.as_uuid()),
                _ => None,
            }),
    );
    object_ids.extend(project.templates.iter().filter_map(|d| match d {
        DocumentObject::Known(d) => Some(d.id),
        _ => None,
    }));
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            object_ids.insert(s.id.as_uuid());
            for t in &s.tracks {
                object_ids.insert(t.id.as_uuid());
                for c in &t.clips {
                    object_ids.insert(c.id.as_uuid());
                }
            }
        }
    }
    for c in &compositions {
        let lowered = project
            .sequences
            .iter()
            .any(|s| matches!(s, DocumentObject::Known(s) if s.id.as_uuid() == c.id.as_uuid()));
        if !lowered && !object_ids.insert(c.id.as_uuid()) {
            return Err(invalid("ambiguous object id"));
        }
        for n in &c.nodes {
            if !lowered && !object_ids.insert(n.id.as_uuid()) {
                return Err(invalid("ambiguous object id"));
            }
        }
    }
    kronello_model::validate_compositions(&compositions, &r).map_err(invalid)?;
    project
        .validate_mattes()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    kronello_model::validate_shape_contents(project, &r).map_err(|e| match e {
        kronello_model::ShapeError::MorphCorrespondence { .. }
        | kronello_model::ShapeError::InvalidTrimRange
        | kronello_model::ShapeError::UnsupportedStrokeVersion
        | kronello_model::ShapeError::InvalidDashArray
        | kronello_model::ShapeError::StrokeBudgetExceeded
        | kronello_model::ShapeError::OpenStrokeAlignment => {
            ServiceError::from(kronello_render::RenderError::Shape(e))
        }
        _ => invalid(e),
    })?;
    kronello_model::validate_text_contents(project, &r).map_err(invalid)?;
    kronello_model::validate_caption_contents(project)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
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
    for sequence in &project.sequences {
        if let DocumentObject::Known(sequence) = sequence {
            for clip in sequence.tracks.iter().flat_map(|t| &t.clips) {
                if let Some(volume) = &clip.volume {
                    volume
                        .validate_sources(&r, &Catalog(project))
                        .map_err(invalid)?;
                }
            }
        }
    }
    let expressions: Vec<_> = project
        .expressions
        .iter()
        .filter_map(|e| match e {
            DocumentObject::Known(e) => Some(e.clone()),
            _ => None,
        })
        .collect();
    for e in &expressions {
        e.validate()
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    }
    let curves: Vec<_> = project
        .curves
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let refs = Default::default();
    let deps = Default::default();
    let audio_analyses = project.audio_analysis_inputs().map_err(invalid)?;
    for c in &compositions {
        let data_assets = project.expression_data_inputs().map_err(invalid)?;
        kronello_eval::DependencyGraph::compile_with_data(
            kronello_eval::EvaluationSnapshot {
                compositions: &compositions,
                curves: &curves,
                expressions: &expressions,
                registry: &r,
                reference_bindings: &refs,
                dependencies: &deps,
                working_space: kronello_model::ColorSpace::LinearRec709,
            },
            c.id,
            &audio_analyses,
            &data_assets,
        )
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    }
    Ok(())
}
fn properties(project: &Project) -> Vec<(Uuid, &Property)> {
    let mut result = Vec::new();
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            result.extend(
                s.tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .flat_map(|c| c.properties.iter().map(|p| (c.id.as_uuid(), p))),
            );
        }
    }
    for c in &project.compositions {
        if let DocumentObject::Known(c) = c {
            result.extend(c.properties.iter().map(|p| (c.id.as_uuid(), p)));
            for n in &c.nodes {
                result.extend(n.properties.iter().map(|p| (n.id.as_uuid(), p)));
            }
        }
    }
    for r in &project.repeaters {
        if let DocumentObject::Known(r) = r {
            for i in &r.instances {
                result.extend(i.properties.iter().map(|p| (i.placement.as_uuid(), p)));
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
    // NLE-005: a clip property edit is a clip mutation. Generic property
    // commands funnel through here, so the locked-track guard lives at the
    // shared resolution point and applies to every caller at once.
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            for track in &s.tracks {
                if track.locked() && track.clips.iter().any(|c| c.id.as_uuid() == object) {
                    return Err(ServiceError::new(
                        "TRACK_LOCKED",
                        "clip is on a locked track",
                    ));
                }
            }
        }
    }
    for s in &mut project.sequences {
        if let DocumentObject::Known(s) = s {
            for c in s.tracks.iter_mut().flat_map(|t| &mut t.clips) {
                if c.id.as_uuid() == object {
                    return c
                        .properties
                        .iter_mut()
                        .find(|p| p.id() == id)
                        .ok_or_else(|| invalid("clip property not found"));
                }
            }
        }
    }
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
    for r in &mut project.repeaters {
        if let DocumentObject::Known(r) = r {
            for i in &mut r.instances {
                if i.placement.as_uuid() == object {
                    return i
                        .properties
                        .iter_mut()
                        .find(|p| p.id() == id)
                        .ok_or_else(|| invalid("repeater instance property not found"));
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
        EditCommand::SimulationSet { simulation } => {
            structure(
                keys,
                simulation.id.as_uuid(),
                simulation.source.composition.as_uuid(),
            );
            if let Some(old) = project
                .simulations
                .iter_mut()
                .find(|s| matches!(s, DocumentObject::Known(s) if s.id == simulation.id))
            {
                *old = DocumentObject::Known(simulation.clone());
            } else {
                project
                    .simulations
                    .push(DocumentObject::Known(simulation.clone()));
            }
        }
        EditCommand::SimulationRemove { id } => {
            structure(keys, id.as_uuid(), id.as_uuid());
            let index = project
                .simulations
                .iter()
                .position(|s| matches!(s, DocumentObject::Known(s) if s.id == *id))
                .ok_or_else(|| invalid("simulation not found"))?;
            project.simulations.remove(index);
        }
        EditCommand::RepeaterSet { repeater } => {
            structure(
                keys,
                repeater.id.as_uuid(),
                repeater.source.composition.as_uuid(),
            );
            if let Ok(old) = project.repeater(repeater.id) {
                for i in &old.instances {
                    structure(keys, i.placement.as_uuid(), repeater.id.as_uuid());
                }
            }
            for i in &repeater.instances {
                structure(keys, i.placement.as_uuid(), repeater.id.as_uuid());
            }
            if let Some(existing) = project
                .repeaters
                .iter_mut()
                .find(|r| matches!(r, DocumentObject::Known(r) if r.id == repeater.id))
            {
                *existing = DocumentObject::Known(repeater.clone());
            } else {
                project
                    .repeaters
                    .push(DocumentObject::Known(repeater.clone()));
            }
        }
        EditCommand::RepeaterRemove { id } => {
            structure(keys, id.as_uuid(), id.as_uuid());
            let index = project
                .repeaters
                .iter()
                .position(|r| matches!(r, DocumentObject::Known(r) if r.id == *id))
                .ok_or_else(|| invalid("repeater not found"))?;
            project.repeaters.remove(index);
        }
        EditCommand::RepeaterExpand {
            repeater,
            instance,
            expansion_id,
        } => {
            crate::repeater::expand(project, *repeater, *instance, *expansion_id, keys)?;
        }
        EditCommand::MatteSet { matte } => {
            structure(keys, matte.composition.as_uuid(), matte.id);
            structure(keys, matte.source.as_uuid(), matte.matte.as_uuid());
            if let Some(existing) = project
                .mattes
                .iter_mut()
                .find(|m| matches!(m,DocumentObject::Known(m) if m.id==matte.id))
            {
                *existing = DocumentObject::Known(matte.clone());
            } else {
                project.mattes.push(DocumentObject::Known(matte.clone()));
            }
        }
        EditCommand::MatteRemove { id } => {
            let index = project
                .mattes
                .iter()
                .position(|m| matches!(m,DocumentObject::Known(m) if m.id==*id))
                .ok_or_else(|| invalid("matte relation not found"))?;
            let DocumentObject::Known(matte) = &project.mattes[index] else {
                unreachable!()
            };
            structure(keys, matte.composition.as_uuid(), *id);
            structure(keys, matte.source.as_uuid(), matte.matte.as_uuid());
            project.mattes.remove(index);
        }
        EditCommand::ModifierInsert {
            object,
            property,
            modifier,
            index,
        } => {
            let p = property_mut(project, *object, *property)?;
            let mut modifiers = p.modifiers().to_vec();
            if *index > modifiers.len() {
                return Err(invalid("modifier index out of bounds"));
            }
            modifiers.insert(*index, modifier.clone());
            p.set_modifiers(modifiers, &registry()).map_err(invalid)?;
            keys.insert(ChangedKey::Value {
                object_id: *object,
                property_id: *property,
            });
        }
        EditCommand::ModifierReplace {
            object,
            property,
            modifier,
        } => {
            let p = property_mut(project, *object, *property)?;
            let mut modifiers = p.modifiers().to_vec();
            let current = modifiers
                .iter_mut()
                .find(|m| m.id == modifier.id)
                .ok_or_else(|| invalid("modifier not found"))?;
            *current = modifier.clone();
            p.set_modifiers(modifiers, &registry()).map_err(invalid)?;
            keys.insert(ChangedKey::Value {
                object_id: *object,
                property_id: *property,
            });
        }
        EditCommand::ModifierRemove {
            object,
            property,
            modifier,
        } => {
            let p = property_mut(project, *object, *property)?;
            let mut modifiers = p.modifiers().to_vec();
            let index = modifiers
                .iter()
                .position(|m| m.id == *modifier)
                .ok_or_else(|| invalid("modifier not found"))?;
            modifiers.remove(index);
            p.set_modifiers(modifiers, &registry()).map_err(invalid)?;
            keys.insert(ChangedKey::Value {
                object_id: *object,
                property_id: *property,
            });
        }
        EditCommand::ModifierReorder {
            object,
            property,
            order,
        } => {
            let p = property_mut(project, *object, *property)?;
            let ids: BTreeSet<_> = p.modifiers().iter().map(|m| m.id).collect();
            if order.len() != ids.len() || order.iter().copied().collect::<BTreeSet<_>>() != ids {
                return Err(invalid("modifier order must be an exact permutation"));
            }
            let modifiers = order
                .iter()
                .map(|id| p.modifiers().iter().find(|m| m.id == *id).unwrap().clone())
                .collect();
            p.set_modifiers(modifiers, &registry()).map_err(invalid)?;
            keys.insert(ChangedKey::Value {
                object_id: *object,
                property_id: *property,
            });
        }
        EditCommand::ExpressionSet { expression } => {
            expression
                .validate()
                .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
            structure(keys, expression.id.as_uuid(), expression.id.as_uuid());
            for (object, p) in properties(project) {
                if p.source() == &PropertySource::Expression(expression.id) {
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
                                if source == &PropertySource::Expression(expression.id) {
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
            if let Some(existing) = project
                .expressions
                .iter_mut()
                .find(|e| matches!(e, DocumentObject::Known(e) if e.id == expression.id))
            {
                *existing = DocumentObject::Known(expression.clone());
            } else {
                project
                    .expressions
                    .push(DocumentObject::Known(expression.clone()));
            }
        }
        EditCommand::PropertyExpressionTextSet {
            object,
            property,
            text,
            metadata,
        } => {
            // The parse runs inside planning/applying so a rejected text never
            // mutates the candidate project. Metadata stays envelope-owned.
            let expression =
                kronello_model::parse_expression(text, metadata).map_err(expression_text_error)?;
            let id = expression.id;
            apply_command(project, &EditCommand::ExpressionSet { expression }, keys)?;
            apply_command(
                project,
                &EditCommand::PropertySourceSet {
                    object: *object,
                    property: *property,
                    source: PropertySource::Expression(id),
                    curve: None,
                },
                keys,
            )?;
        }
        EditCommand::Timeline(command) => crate::nle::mutate(project, command, keys)?,
        EditCommand::Template(command) => crate::template::mutate(project, command, keys)?,
        EditCommand::PropertySourceSet {
            object,
            property,
            source,
            curve,
        } => {
            if let PropertySource::Expression(id) = source {
                structure(keys, id.as_uuid(), id.as_uuid());
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
        EditCommand::NodePropertyInsert {
            composition,
            node,
            property,
        } => {
            if properties(project)
                .iter()
                .any(|(_, p)| p.id() == property.id())
            {
                return Err(invalid("property ID already exists"));
            }
            let n = node_mut(composition_mut(project, *composition)?, *node)?;
            if n.properties
                .iter()
                .any(|p| p.descriptor().key == property.descriptor().key)
                && !registry()
                    .lookup(&property.descriptor().key)
                    .map_err(invalid)?
                    .definition()
                    .repeatable
            {
                return Err(invalid("property key already exists on node"));
            }
            property.validate(&registry()).map_err(invalid)?;
            structure(keys, n.id.as_uuid(), n.id.as_uuid());
            keys.insert(ChangedKey::Value {
                object_id: n.id.as_uuid(),
                property_id: property.id(),
            });
            n.properties.push(property.clone());
        }
        EditCommand::NodeTagsSet {
            composition,
            node,
            tags,
        } => {
            if !kronello_model::valid_node_tags(tags) {
                return Err(ServiceError::invalid("invalid node tags"));
            }
            let n = node_mut(composition_mut(project, *composition)?, *node)?;
            structure(keys, n.id.as_uuid(), n.id.as_uuid());
            n.tags = tags.clone();
        }
        EditCommand::NodeRename {
            composition,
            node,
            name,
        } => {
            let c = composition_mut(project, *composition)?;
            let n = node_mut(c, *node)?;
            structure(keys, n.id.as_uuid(), n.id.as_uuid());
            n.name = name.clone();
        }
        EditCommand::NodeEnabledSet {
            composition,
            node,
            enabled,
        } => {
            let c = composition_mut(project, *composition)?;
            let n = node_mut(c, *node)?;
            structure(keys, n.id.as_uuid(), n.id.as_uuid());
            n.enabled = *enabled;
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
        EditCommand::CaptionSet { caption } => {
            caption
                .validate()
                .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
            caption_keys(project, caption.id, keys);
            if let Some(c) = project
                .captions
                .iter_mut()
                .find(|c| matches!(c, DocumentObject::Known(c) if c.id == caption.id))
            {
                *c = DocumentObject::Known(caption.clone());
            } else {
                project
                    .captions
                    .push(DocumentObject::Known(caption.clone()));
            }
        }
        EditCommand::AssetSet { asset } => {
            asset
                .validate()
                .map_err(|e| ServiceError::new("INVALID_DOCUMENT", e.to_string()))?;
            structure(keys, asset.id.as_uuid(), project.id);
            if let Some(a) = project
                .assets
                .iter_mut()
                .find(|a| matches!(a, DocumentObject::Known(a) if a.id == asset.id))
            {
                *a = DocumentObject::Known(asset.clone());
            } else {
                project.assets.push(DocumentObject::Known(asset.clone()));
            }
        }
        EditCommand::MulticamSet { multicam } => {
            multicam
                .validate()
                .map_err(|e| ServiceError::new("INVALID_DOCUMENT", e.to_string()))?;
            for angle in &multicam.angles {
                if !project
                    .assets
                    .iter()
                    .any(|a| matches!(a, DocumentObject::Known(a) if a.id == angle.asset))
                {
                    return Err(ServiceError::new(
                        "ASSET_MISSING",
                        "multicam angle asset missing",
                    ));
                }
            }
            structure(keys, multicam.id.as_uuid(), project.id);
            for angle in &multicam.angles {
                structure(keys, angle.id.as_uuid(), multicam.id.as_uuid());
            }
            if let Some(existing) = project
                .multicams
                .iter_mut()
                .find(|group| group.id == multicam.id)
            {
                *existing = multicam.clone();
            } else {
                project.multicams.push(multicam.clone());
            }
        }
        EditCommand::CaptionRemove { id } => {
            let before = project.captions.len();
            project
                .captions
                .retain(|c| !matches!(c, DocumentObject::Known(c) if c.id == *id));
            if project.captions.len() == before {
                return Err(ServiceError::new("SOURCE_MISSING", "caption missing"));
            }
            caption_keys(project, *id, keys);
        }
    }
    Ok(())
}
fn node_references(node: &SceneNode, keys: &mut BTreeSet<ChangedKey>) {
    for p in &node.properties {
        if let PropertySource::Expression(id) = p.source() {
            structure(keys, id.as_uuid(), id.as_uuid());
        }
        if let PropertySource::Curve(id) = p.source() {
            structure(keys, id.as_uuid(), id.as_uuid());
        }
    }
    match &node.kind {
        NodeKind::Shape { content_ref }
        | NodeKind::Text { content_ref }
        | NodeKind::Simulation { content_ref }
        | NodeKind::Repeater { content_ref } => {
            structure(keys, content_ref.as_uuid(), content_ref.as_uuid())
        }
        NodeKind::CompositionInstance(i) => {
            structure(keys, i.definition_ref.as_uuid(), i.definition_ref.as_uuid());
            for source in i.input_bindings.values() {
                if let PropertySource::Expression(id) = source {
                    structure(keys, id.as_uuid(), id.as_uuid());
                }
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
/// Caption cue edits invalidate the cue and every clip placement that
/// displays it, so selective Undo scopes to the dependent placements too.
fn caption_keys(project: &Project, id: kronello_model::CaptionId, keys: &mut BTreeSet<ChangedKey>) {
    structure(keys, id.as_uuid(), id.as_uuid());
    for s in &project.sequences {
        if let DocumentObject::Known(s) = s {
            for t in &s.tracks {
                for c in &t.clips {
                    if matches!(c.source_ref, kronello_model::SourceRef::Caption { caption } if caption == id)
                    {
                        structure(keys, c.id.as_uuid(), t.id.as_uuid());
                    }
                }
            }
        }
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
/// Only these model collections have nonsemantic storage order. All other
/// arrays (including modifiers, draw order, curve keys, paths, gradient stops,
/// text styles/ruby and instance paths) are replaced as a unit.
fn unordered_collection(path: &[String]) -> bool {
    match path {
        [collection] => matches!(
            collection.as_str(),
            "compositions"
                | "curves"
                | "expressions"
                | "shapes"
                | "texts"
                | "captions"
                | "sequences"
                | "mattes"
        ),
        [compositions, _, collection] if compositions == "compositions" => {
            matches!(collection.as_str(), "nodes" | "properties")
        }
        [compositions, _, nodes, _, properties] => {
            compositions == "compositions" && nodes == "nodes" && properties == "properties"
        }
        _ => false,
    }
}
/// Diff unordered model collections by stable ID; preserve all ordered arrays.
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
                    if path.len() == 1
                        && matches!(
                            key.as_str(),
                            "expressions"
                                | "shapes"
                                | "texts"
                                | "captions"
                                | "templates"
                                | "template_instances"
                                | "sequences"
                        )
                    {
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
                    if path.len() == 1
                        && matches!(
                            key.as_str(),
                            "expressions"
                                | "shapes"
                                | "texts"
                                | "captions"
                                | "templates"
                                | "template_instances"
                                | "sequences"
                        )
                    {
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
            if unordered_collection(path)
                && a.iter()
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
pub(crate) fn build(
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
    let migrations = commands
        .iter()
        .filter_map(|command| match command {
            EditCommand::Template(command) => match command.as_ref() {
                crate::TemplateCommand::Migrate { instance, .. } => Some(*instance),
                _ => None,
            },
            _ => None,
        })
        .collect();
    kronello_template::validate_migration_transition(&document, &candidate, &migrations)?;
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
    // Normalize only unordered collection storage; ordered arrays retain the request order.
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
    apply_template(r, payload)
}
pub(crate) fn apply_template(r: EditApplyRequest, payload: Json) -> Result<Event, ServiceError> {
    key(&r.idempotency_key)?;
    let base = parse_revision(&r.base_revision)?;
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
fn validate_undo<'a>(
    snapshot: &kronello_store::Snapshot,
    events: &'a [Event],
    event_id: Uuid,
) -> Result<&'a Event, ServiceError> {
    let target = events
        .iter()
        .find(|e| e.id == event_id)
        .ok_or_else(|| ServiceError::new("EVENT_NOT_FOUND", "event is unavailable or compacted"))?;
    let active = active(events);
    if !active.contains(&target.id) {
        return Err(ServiceError::new(
            "EVENT_ALREADY_UNDONE",
            "undo the undo event to redo",
        ));
    }
    let mut conflicts = Vec::new();
    for e in events {
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
    let mut candidate = serde_json::to_value(&snapshot.document)?;
    for m in &target.inverse {
        m.apply(&mut candidate)?;
    }
    validate(&serde_json::from_str(&candidate.to_string())?)?;
    Ok(target)
}
pub(crate) fn undo(r: UndoRequest) -> Result<Event, ServiceError> {
    undo_before_apply(r, || {})
}
// The callback permits deterministic interleavings in unit tests. It runs after
// the preliminary read validation and before acquiring the writer transaction.
fn undo_before_apply(r: UndoRequest, before_apply: impl FnOnce()) -> Result<Event, ServiceError> {
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
        let target = validate_undo(&s, &events, r.event_id)?;
        let request = ApplyRequest {
            base_revision: base,
            session_id: r.session_id,
            mutations: target.inverse.clone(),
            changed_keys: target.changed_keys.clone(),
            idempotency_key: Some(r.idempotency_key.clone()),
            undo_of: Some(target.id),
        };
        before_apply();
        match store.apply_with_payload_checked(request, payload.clone(), |snapshot, events| {
            validate_undo(snapshot, events, r.event_id).map(|_| ())
        }) {
            Ok(e) => Ok(e),
            Err(e) if e.code == "REVISION_CONFLICT" => {
                retry(&store, &r.idempotency_key, &payload)?.ok_or(e)
            }
            Err(e) => Err(e),
        }
    })();
    store.close()?;
    result
}
pub(crate) fn history(r: HistoryRequest) -> Result<HistoryResult, ServiceError> {
    let since = parse_revision(&r.since_revision)?;
    if r.limit == 0 || r.limit > 1000 {
        return Err(ServiceError::invalid("history limit must be 1..=1000"));
    }
    let store = open_existing(&r.project)?;
    let (s, events) = store.snapshot_and_events()?;
    store.close()?;
    let binding =
        serde_json::json!({"since_revision":since,"limit":r.limit,"session_id":r.session_id});
    let cursor = r
        .cursor
        .as_deref()
        .map(crate::paging::Cursor::decode)
        .transpose()?;
    let floor = events.first().map(|e| e.id.to_string());
    let revision = if let Some(c) = &cursor {
        c.validate("history.list", s.document.id, &binding)?;
        if c.floor != floor || c.revision > s.revision {
            return Err(crate::paging::expired());
        }
        c.revision
    } else {
        s.revision
    };
    let events: Vec<_> = events
        .into_iter()
        .filter(|e| e.revision <= revision)
        .collect();
    let active = active(&events);
    let after = if let Some(c) = &cursor {
        events
            .iter()
            .find(|e| serde_json::json!(e.id) == c.after)
            .ok_or_else(crate::paging::expired)?
            .revision
    } else {
        since
    };
    let mut selected = events.into_iter().filter(|e| {
        e.revision > after && r.session_id.is_none_or(|session| e.session_id == session)
    });
    let entries: Vec<_> = selected
        .by_ref()
        .take(r.limit)
        .map(|event| HistoryEntry {
            undone: !active.contains(&event.id),
            event,
        })
        .collect();
    let more = selected.next().is_some();
    let last = entries.last().filter(|_| more);
    let next_cursor = last
        .map(|entry| {
            crate::paging::Cursor::new(
                "history.list",
                s.document.id,
                revision,
                binding,
                serde_json::json!(entry.event.id),
                floor,
            )
            .encode()
        })
        .transpose()?;
    Ok(HistoryResult {
        revision: revision.to_string(),
        next_since_revision: last.map(|e| e.event.revision.to_string()),
        next_cursor,
        events: entries,
    })
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use kronello_store::OpenOptions;

    #[test]
    fn unordered_collections_keep_member_patches_for_selective_undo() {
        let old = json!([{"id":"b", "value":1}, {"id":"a", "value":2}]);
        let reordered = json!([{"id":"a", "value":2}, {"id":"b", "value":1}]);
        let changed = json!([{"id":"a", "value":3}, {"id":"b", "value":1}]);
        for (segments, pointer) in [
            (vec!["compositions"], "/compositions"),
            (vec!["curves"], "/curves"),
            (vec!["shapes"], "/shapes"),
            (vec!["texts"], "/texts"),
            (
                vec!["compositions", "comp", "nodes"],
                "/compositions/0/nodes",
            ),
            (
                vec!["compositions", "comp", "properties"],
                "/compositions/0/properties",
            ),
            (
                vec!["compositions", "comp", "nodes", "node", "properties"],
                "/compositions/0/nodes/0/properties",
            ),
        ] {
            let mut path: Vec<String> = segments.into_iter().map(String::from).collect();
            let mut mutations = Vec::new();
            diff(&old, &reordered, &mut path, &mut mutations);
            assert!(mutations.is_empty(), "storage order is nonsemantic");
            diff(&old, &changed, &mut path, &mut mutations);
            let mut member_path = path.clone();
            member_path.extend(["a".into(), "value".into()]);
            assert_eq!(
                mutations,
                vec![Mutation::Set {
                    path: member_path,
                    value: json!(3),
                }]
            );
            let mut document = json!({
                "compositions": [{
                    "id":"comp",
                    "nodes":[{"id":"node", "properties":[]}],
                    "properties":[]
                }],
                "curves":[], "shapes":[], "texts":[]
            });
            *document.pointer_mut(pointer).unwrap() = old.clone();
            let inverse = mutations[0].apply(&mut document).unwrap();
            let mut other_path = path.clone();
            other_path.extend(["b".into(), "value".into()]);
            Mutation::Set {
                path: other_path,
                value: json!(99),
            }
            .apply(&mut document)
            .unwrap();
            inverse.apply(&mut document).unwrap();
            assert_eq!(
                document.pointer(pointer).unwrap(),
                &json!([{"id":"b", "value":99}, {"id":"a", "value":2}]),
                "Undo must retain an independent member edit"
            );
        }
    }

    #[test]
    fn ordered_arrays_with_id_members_are_atomic_and_invert_exactly() {
        let old = json!([{"id":"b", "value":1}, {"id":"a", "value":2}]);
        let new = json!([{"id":"a", "value":2}, {"id":"b", "value":1}, {"id":"c", "value":3}]);
        for path in [
            vec![
                "compositions",
                "comp",
                "nodes",
                "node",
                "properties",
                "prop",
                "modifiers",
            ],
            vec!["compositions", "comp", "nodes", "node", "child_order"],
            vec!["compositions", "comp", "root_nodes"],
            vec!["curves", "curve", "keys"],
            vec!["shapes", "shape", "geometry", "segments"],
            vec!["shapes", "shape", "fill", "stops"],
            vec!["texts", "text", "styles"],
            vec!["texts", "text", "ruby"],
            vec![
                "compositions",
                "comp",
                "nodes",
                "node",
                "kind",
                "value",
                "instance_path",
            ],
        ] {
            let mut path: Vec<String> = path.into_iter().map(String::from).collect();
            let mut mutations = Vec::new();
            diff(&old, &new, &mut path, &mut mutations);
            assert_eq!(
                mutations,
                vec![Mutation::Set {
                    path: path.clone(),
                    value: new.clone()
                }]
            );
            let mut document = old.clone();
            for segment in path.iter().rev() {
                document = json!({segment: document});
            }
            let before = document.clone();
            let inverse = mutations[0].apply(&mut document).unwrap();
            let redo = inverse.apply(&mut document).unwrap();
            assert_eq!(document, before);
            redo.apply(&mut document).unwrap();
            let mut actual = &document;
            for segment in &path {
                actual = &actual[segment];
            }
            assert_eq!(actual, &new);
        }
    }

    #[test]
    fn concurrent_compact_between_undo_validation_and_commit_rejects_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compact-undo.kronello");
        let document: Project =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
        store
            .import_json(
                0,
                Uuid::new_v4(),
                &serde_json::to_string(&document).unwrap(),
            )
            .unwrap();
        let DocumentObject::Known(composition) = &document.compositions[0] else {
            panic!("fixture composition must be known");
        };
        let node = &composition.nodes[0];
        // Use genuine service plans on distinct properties, so the Undo passes
        // preliminary conflict validation with service-derived, nonempty keys.
        let change = |property: usize, value: f64| EditCommand::PropertySourceSet {
            object: node.id.as_uuid(),
            property: node.properties[property].id(),
            source: serde_json::from_value(json!({
                "kind":"constant", "value":{"kind":"scalar", "value":value}
            }))
            .unwrap(),
            curve: None,
        };
        let prepare = |plan: EditPlan| ApplyRequest {
            base_revision: plan.base_revision.parse().unwrap(),
            session_id: Uuid::new_v4(),
            mutations: plan.mutations,
            changed_keys: plan.changed_keys,
            idempotency_key: None,
            undo_of: None,
        };
        let target = store
            .apply(prepare(
                build(document.clone(), 1, vec![change(1, 1.0)]).unwrap(),
            ))
            .unwrap();
        let later = store
            .apply(prepare(
                build(store.snapshot().unwrap().document, 2, vec![change(4, 0.5)]).unwrap(),
            ))
            .unwrap();
        assert!(!target.changed_keys.is_empty());
        assert!(!later.changed_keys.is_empty());
        assert!(target.changed_keys.is_disjoint(&later.changed_keys));
        let before = store.snapshot().unwrap();
        let r = UndoRequest {
            project: path.clone(),
            base_revision: "3".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "compacted-undo".into(),
            event_id: target.id,
        };
        let error = undo_before_apply(r, || {
            // This independent connection commits compact only after A's Undo
            // has passed read validation. Revision remains 3.
            store.compact(3).unwrap();
            assert_eq!(store.snapshot().unwrap(), before);
            assert!(
                !store
                    .events_since(0)
                    .unwrap()
                    .iter()
                    .any(|e| e.id == target.id)
            );
        })
        .unwrap_err();
        assert_eq!(error.code, "EVENT_NOT_FOUND");
        assert_eq!(store.snapshot().unwrap(), before);
        assert_eq!(store.events_since(0).unwrap().len(), 1);
        assert!(
            store
                .idempotency_record("compacted-undo")
                .unwrap()
                .is_none()
        );
        store.close().unwrap();
    }
    #[test]
    fn concurrent_undo_retry_receipt_precedes_compacted_target_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compact-retry.kronello");
        let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
        let target = store
            .apply(ApplyRequest {
                base_revision: 0,
                session_id: Uuid::new_v4(),
                mutations: vec![Mutation::Set {
                    path: vec!["name".into()],
                    value: json!("target"),
                }],
                changed_keys: BTreeSet::new(),
                idempotency_key: None,
                undo_of: None,
            })
            .unwrap();
        let r = UndoRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "retry-undo".into(),
            event_id: target.id,
        };
        let mut committed = None;
        let event = undo_before_apply(r.clone(), || {
            // B wins the same exact request, then deletes the original target.
            committed = Some(undo(r).unwrap());
            store.compact(2).unwrap();
            assert!(
                !store
                    .events_since(0)
                    .unwrap()
                    .iter()
                    .any(|e| e.id == target.id)
            );
        })
        .unwrap();
        assert_eq!(event, committed.unwrap());
        assert_eq!(store.snapshot().unwrap().revision, 2);
        assert_eq!(store.events_since(0).unwrap(), vec![event]);
        store.close().unwrap();
    }
    #[test]
    fn locked_undo_validation_rejects_conflicts_and_already_undone_targets() {
        for already_undone in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("locked-validation.kronello");
            let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
            let document = store.snapshot().unwrap().document;
            let changed_keys = BTreeSet::from([ChangedKey::Structure {
                object_id: document.id,
                parent_container_id: document.id,
            }]);
            let target = store
                .apply(ApplyRequest {
                    base_revision: 0,
                    session_id: Uuid::new_v4(),
                    mutations: vec![Mutation::Set {
                        path: vec!["name".into()],
                        value: json!("target"),
                    }],
                    changed_keys: changed_keys.clone(),
                    idempotency_key: None,
                    undo_of: None,
                })
                .unwrap();
            let later = if already_undone {
                undo(UndoRequest {
                    project: path.clone(),
                    base_revision: "1".into(),
                    session_id: Uuid::new_v4(),
                    idempotency_key: "first-undo".into(),
                    event_id: target.id,
                })
                .unwrap()
            } else {
                store
                    .apply(ApplyRequest {
                        base_revision: 1,
                        session_id: Uuid::new_v4(),
                        mutations: vec![Mutation::Set {
                            path: vec!["name".into()],
                            value: json!("later"),
                        }],
                        changed_keys: changed_keys.clone(),
                        idempotency_key: None,
                        undo_of: None,
                    })
                    .unwrap()
            };
            let before = store.snapshot().unwrap();
            let events_before = store.events_since(0).unwrap();
            // Feed a previously prepared inverse directly to the locked path:
            // it must recheck active status and conflicts before writing.
            let error = store
                .apply_with_payload_checked(
                    ApplyRequest {
                        base_revision: 2,
                        session_id: Uuid::new_v4(),
                        mutations: target.inverse.clone(),
                        changed_keys,
                        idempotency_key: Some("locked-check".into()),
                        undo_of: Some(target.id),
                    },
                    json!({"operation":"test.undo"}),
                    |snapshot, events| validate_undo(snapshot, events, target.id).map(|_| ()),
                )
                .unwrap_err();
            if already_undone {
                assert_eq!(error.code, "EVENT_ALREADY_UNDONE");
            } else {
                assert_eq!(error.code, "UNDO_CONFLICT");
                assert_eq!(
                    error.details.unwrap()["conflicts"][0]["event_id"],
                    later.id.to_string()
                );
            }
            assert_eq!(store.snapshot().unwrap(), before);
            assert_eq!(store.events_since(0).unwrap(), events_before);
            assert!(store.idempotency_record("locked-check").unwrap().is_none());
            store.close().unwrap();
        }
    }
}
