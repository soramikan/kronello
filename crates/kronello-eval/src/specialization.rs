//! Nonserializable evidence for upper-compiler source definition specialization.
use crate::EvaluationError;
use kronello_model::{
    Composition, CompositionError, CompositionId, InstancePath, NodeId, NodeKind, PropertyId,
    PropertySource, SceneNode, SchemaRegistry, Value, validate_compositions,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct SpecializedDefinition {
    pub authored: CompositionId,
    pub scope: InstancePath,
}
/// Exact dynamic children produced by a bounded, immutable simulation pass.
#[derive(Clone, Debug)]
pub struct SimulationLowering {
    pub composition: CompositionId,
    pub owner: NodeId,
    pub source: CompositionId,
    pub source_bindings: BTreeMap<PropertyId, PropertySource<Value>>,
    pub seed: u64,
    pub generated_nodes: Vec<SceneNode>,
}
/// Internal compiler evidence. This is not persisted or accepted from JSON API.
pub struct SpecializationProvenance<'a> {
    pub authored: &'a [Composition],
    pub definitions: BTreeMap<CompositionId, SpecializedDefinition>,
    pub lowerings: Vec<SimulationLowering>,
    pub canonical_clock: bool,
}
fn invalid(message: &'static str) -> EvaluationError {
    EvaluationError::InvalidSpecialization(message)
}
pub(crate) fn validate(
    defs: &[Composition],
    root: CompositionId,
    registry: &SchemaRegistry,
    proof: &SpecializationProvenance<'_>,
) -> Result<(), EvaluationError> {
    validate_compositions(proof.authored, registry)
        .map_err(EvaluationError::InvalidCompositions)?;
    if defs.len() > 4096 || proof.definitions.len() != defs.len() || proof.lowerings.len() > 4096 {
        return Err(invalid("definition/provenance bound or count"));
    }
    let authored: BTreeMap<_, _> = proof.authored.iter().map(|c| (c.id, c)).collect();
    let mut original_nodes = BTreeSet::new();
    let mut original_properties = BTreeSet::new();
    let mut original_instances = BTreeSet::new();
    let mut original_uuids = BTreeSet::new();
    for c in proof.authored {
        original_uuids.insert(c.id.as_uuid());
        for p in &c.properties {
            original_properties.insert(p.id());
            original_uuids.insert(p.id().as_uuid());
        }
        for n in &c.nodes {
            original_nodes.insert(n.id);
            original_uuids.insert(n.id.as_uuid());
            for p in &n.properties {
                original_properties.insert(p.id());
                original_uuids.insert(p.id().as_uuid());
            }
            if let NodeKind::CompositionInstance(i) = &n.kind {
                original_instances.insert(i.id);
                original_uuids.insert(i.id.as_uuid());
            }
        }
    }
    let mut scopes = BTreeSet::new();
    let mut derived_ids = BTreeSet::new();
    let mut total_nodes = 0usize;
    for c in defs {
        total_nodes = total_nodes.saturating_add(c.nodes.len());
        if total_nodes > 100_000 || !derived_ids.insert(c.id) {
            return Err(invalid("derived definition identity or node bound"));
        }
        let info = proof
            .definitions
            .get(&c.id)
            .ok_or_else(|| invalid("missing definition provenance"))?;
        if !scopes.insert(info.scope.clone()) || info.scope.ids().len() > 64 {
            return Err(invalid("duplicate or excessive scope"));
        }
        if info
            .scope
            .resolve(root, defs)
            .map_err(|_| invalid("invalid specialization scope"))?
            .id
            != c.id
        {
            return Err(invalid("scope does not resolve to derived definition"));
        }
        if info.scope.ids().is_empty() {
            if c.id != root || info.authored != root {
                return Err(invalid("invalid root provenance"));
            }
        } else if original_uuids.contains(&c.id.as_uuid()) {
            return Err(invalid(
                "derived definition collides with authored identity",
            ));
        }
    }
    let by_scope: BTreeMap<_, _> = proof
        .definitions
        .iter()
        .map(|(id, info)| (&info.scope, (*id, info.authored)))
        .collect();
    let mut generated_uuids = BTreeSet::new();
    let derived_uuids: BTreeSet<_> = derived_ids.iter().map(|id| id.as_uuid()).collect();
    let mut lowered = BTreeSet::new();
    for lowering in &proof.lowerings {
        if !lowered.insert((lowering.composition, lowering.owner))
            || lowering.generated_nodes.len() > 10_000
        {
            return Err(invalid("duplicate lowering or particle bound"));
        }
        let info = proof
            .definitions
            .get(&lowering.composition)
            .ok_or_else(|| invalid("lowering definition missing"))?;
        let original = authored
            .get(&info.authored)
            .ok_or_else(|| invalid("authored definition missing"))?;
        let owner = original
            .nodes
            .iter()
            .find(|n| n.id == lowering.owner)
            .ok_or_else(|| invalid("lowering owner missing"))?;
        if !matches!(owner.kind, NodeKind::Simulation { .. }) {
            return Err(invalid("lowering owner is not simulation"));
        }
        for n in &lowering.generated_nodes {
            let NodeKind::CompositionInstance(i) = &n.kind else {
                return Err(invalid("generated child must be placement"));
            };
            let target = proof
                .definitions
                .get(&i.definition_ref)
                .ok_or_else(|| invalid("generated source missing"))?;
            if target.authored != lowering.source
                || target.scope != info.scope.child(i.id)
                || i.seed != lowering.seed
                || i.input_bindings != lowering.source_bindings
                || n.containment_parent != Some(owner.id)
                || n.transform_parent != Some(owner.id)
                || n.active_range != owner.active_range
                || !n.enabled
                || n.name.is_some()
                || !n.tags.is_empty()
                || !n.effects.is_empty()
                || !n.child_order.is_empty()
                || n.properties.len() != 1
            {
                return Err(invalid("generated child ownership or source mismatch"));
            }
            if !matches!(&i.local_time_map, kronello_time::TimeMap::Linear(map) if map.speed() == kronello_time::Time::ONE)
            {
                return Err(invalid("generated particle age clock must have unit speed"));
            }
            let p = &n.properties[0];
            if p.descriptor().key.as_str() != "kronello.transform.position"
                || !p.modifiers().is_empty()
                || !matches!(p.source(), PropertySource::Constant(Value::Vec2(_)))
            {
                return Err(invalid(
                    "generated placement position must be constant Vec2",
                ));
            }
            // Generated identity is never authorized to alias any authored object.
            for id in [n.id.as_uuid(), i.id.as_uuid(), p.id().as_uuid()] {
                if original_uuids.contains(&id)
                    || derived_uuids.contains(&id)
                    || !generated_uuids.insert(id)
                {
                    return Err(invalid("generated identity collision"));
                }
            }
        }
    }
    for c in defs {
        let info = &proof.definitions[&c.id];
        let mut expected = (*authored
            .get(&info.authored)
            .ok_or_else(|| invalid("authored definition missing"))?)
        .clone();
        expected.id = c.id;
        for node in &mut expected.nodes {
            if let NodeKind::CompositionInstance(i) = &mut node.kind {
                let (id, original) = by_scope
                    .get(&info.scope.child(i.id))
                    .ok_or_else(|| invalid("nested scope provenance missing"))?;
                if *original != i.definition_ref {
                    return Err(invalid("nested authored definition mismatch"));
                }
                i.definition_ref = *id;
                if proof.canonical_clock {
                    i.local_time_map = i
                        .local_time_map
                        .canonical_source_clock()
                        .map_err(|_| invalid("unsupported canonical source clock"))?;
                }
            }
            if let Some(lowering) = proof
                .lowerings
                .iter()
                .find(|l| l.composition == c.id && l.owner == node.id)
            {
                node.kind = NodeKind::Group;
                node.child_order = lowering.generated_nodes.iter().map(|n| n.id).collect();
            }
        }
        for lowering in proof.lowerings.iter().filter(|l| l.composition == c.id) {
            expected.nodes.extend(lowering.generated_nodes.clone());
        }
        if expected != *c {
            return Err(invalid(
                "derived object differs from authorized authored specialization",
            ));
        }
    }
    if let Err(errors) = validate_compositions(defs, registry) {
        let errors: Vec<_> = errors
            .into_iter()
            .filter(|e| match e {
                CompositionError::DuplicateNodeId { id } => !original_nodes.contains(id),
                CompositionError::DuplicatePropertyId { id } => !original_properties.contains(id),
                CompositionError::DuplicateInstanceId { id } => !original_instances.contains(id),
                _ => true,
            })
            .collect();
        if !errors.is_empty() {
            return Err(EvaluationError::InvalidCompositions(errors));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DependencyDeclarations, DependencyGraph, EvaluationSnapshot, ReferenceBindings};
    use kronello_model::*;
    use kronello_time::{Time, TimeMap};
    fn fixture<'a>() -> (
        Vec<Composition>,
        Vec<Composition>,
        SpecializationProvenance<'a>,
    ) {
        let project: Project =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        let DocumentObject::Known(mut source) = project.compositions[0].clone() else {
            panic!()
        };
        source.nodes.truncate(1);
        source.root_nodes.truncate(1);
        source.nodes[0].kind = NodeKind::Null;
        source.nodes[0]
            .properties
            .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
        source.nodes[0].properties[0]
            .set_source(
                PropertySource::Constant(Value::Scalar(FiniteF64::new(0.7).unwrap())),
                &SchemaRegistry::with_builtin(),
            )
            .unwrap();
        let mut root = source.clone();
        root.id = CompositionId::new();
        root.properties.clear();
        root.nodes.clear();
        root.root_nodes.clear();
        let mut derived = Vec::new();
        let mut mappings = BTreeMap::new();
        mappings.insert(
            root.id,
            SpecializedDefinition {
                authored: root.id,
                scope: InstancePath::root(),
            },
        );
        for _ in 0..2 {
            let instance = CompositionInstanceId::new();
            let mut child = source.clone();
            child.id = CompositionId::new();
            let mut placement = source.nodes[0].clone();
            placement.id = NodeId::new();
            placement.properties.clear();
            placement.kind = NodeKind::CompositionInstance(CompositionInstance {
                id: instance,
                definition_ref: source.id,
                local_time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                input_bindings: BTreeMap::new(),
                seed: 0,
            });
            root.root_nodes.push(placement.id);
            root.nodes.push(placement);
            mappings.insert(
                child.id,
                SpecializedDefinition {
                    authored: source.id,
                    scope: InstancePath::new(vec![instance]),
                },
            );
            derived.push(child);
        }
        let authored = vec![root.clone(), source];
        for (n, child) in root.nodes.iter_mut().zip(&derived) {
            let NodeKind::CompositionInstance(i) = &mut n.kind else {
                panic!()
            };
            i.definition_ref = child.id;
        }
        derived.insert(0, root);
        // Authored borrow supplied by each test, never leaked or persisted.
        (
            authored,
            derived,
            SpecializationProvenance {
                authored: &[],
                definitions: mappings,
                lowerings: vec![],
                canonical_clock: false,
            },
        )
    }
    fn compile(
        defs: &[Composition],
        root: CompositionId,
        proof: Option<&SpecializationProvenance<'_>>,
    ) -> Result<(), EvaluationError> {
        let registry = SchemaRegistry::with_builtin();
        let refs = ReferenceBindings::new();
        let dependencies = DependencyDeclarations::new();
        let snapshot = EvaluationSnapshot {
            compositions: defs,
            expressions: &[],
            curves: &[],
            registry: &registry,
            reference_bindings: &refs,
            dependencies: &dependencies,
            working_space: ColorSpace::LinearRec709,
        };
        if let Some(p) = proof {
            DependencyGraph::compile_specialized_with_data(snapshot, root, &[], &[], p).map(|_| ())
        } else {
            DependencyGraph::compile(snapshot, root).map(|_| ())
        }
    }
    #[test]
    fn exact_scoped_authored_copies_compile_but_ordinary_duplicates_remain_invalid() {
        let (authored, defs, mut proof) = fixture();
        proof.authored = &authored;
        compile(&defs, defs[0].id, Some(&proof)).unwrap();
        assert!(matches!(
            compile(&defs, defs[0].id, None),
            Err(EvaluationError::InvalidCompositions(_))
        ));
        let mut altered = defs.clone();
        altered[1].nodes[0].name = Some("unauthorized change".into());
        assert!(matches!(
            compile(&altered, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        altered = defs.clone();
        altered[1].nodes[0].kind = NodeKind::Group;
        assert!(matches!(
            compile(&altered, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        altered = defs.clone();
        altered[1].nodes[0].properties[0]
            .set_source(
                PropertySource::Constant(Value::Scalar(FiniteF64::new(0.2).unwrap())),
                &SchemaRegistry::with_builtin(),
            )
            .unwrap();
        assert!(matches!(
            compile(&altered, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        let original = proof.definitions[&defs[1].id].clone();
        proof.definitions.get_mut(&defs[1].id).unwrap().scope = InstancePath::root();
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        proof.definitions.insert(defs[1].id, original);
        proof.definitions.get_mut(&defs[1].id).unwrap().authored = defs[0].id;
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
    }
    #[test]
    fn simulation_lowering_requires_owner_exact_source_and_unique_generated_identity() {
        let (mut authored, mut defs, mut proof) = fixture();
        authored[0].nodes.truncate(1);
        authored[0].root_nodes.truncate(1);
        let mut placement = defs[0].nodes[0].clone();
        let owner = authored[0].nodes[0].id;
        authored[0].nodes[0].kind = NodeKind::Simulation {
            content_ref: ContentId::new(),
        };
        defs.truncate(2);
        proof.definitions.remove(
            &proof
                .definitions
                .iter()
                .find(|(_, v)| {
                    v.scope.ids().first().is_some_and(|id| {
                        let NodeKind::CompositionInstance(i) = &placement.kind else {
                            panic!()
                        };
                        *id != i.id
                    })
                })
                .map(|(k, _)| *k)
                .unwrap(),
        );
        defs[0] = authored[0].clone();
        defs[0].nodes[0].kind = NodeKind::Group;
        placement.id = NodeId::new();
        placement.containment_parent = Some(owner);
        placement.transform_parent = Some(owner);
        let registry = SchemaRegistry::with_builtin();
        let descriptor = registry
            .lookup(&SchemaKey::new("kronello.transform.position").unwrap())
            .unwrap();
        placement.properties = vec![
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(descriptor),
                PropertySource::Constant(Value::Vec2([
                    FiniteF64::new(1.0).unwrap(),
                    FiniteF64::new(2.0).unwrap(),
                ])),
                vec![],
                &registry,
            )
            .unwrap(),
        ];
        defs[0].nodes[0].child_order = vec![placement.id];
        defs[0].nodes.push(placement.clone());
        proof.lowerings.push(SimulationLowering {
            composition: defs[0].id,
            owner,
            source: authored[1].id,
            source_bindings: BTreeMap::new(),
            seed: 0,
            generated_nodes: vec![placement],
        });
        proof.authored = &authored;
        compile(&defs, defs[0].id, Some(&proof)).unwrap();
        let original = proof.lowerings[0].generated_nodes[0].clone();
        proof.lowerings[0].generated_nodes[0].containment_parent = None;
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        proof.lowerings[0].generated_nodes[0] = original.clone();
        proof.lowerings[0].seed = 42;
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        proof.lowerings[0].seed = 0;
        let collision = authored[1].nodes[0].id;
        proof.lowerings[0].generated_nodes[0].id = collision;
        defs[0].nodes[1].id = collision;
        defs[0].nodes[0].child_order = vec![collision];
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
        proof.lowerings[0].generated_nodes[0].id = original.id;
        defs[0].nodes[1].id = original.id;
        defs[0].nodes[0].child_order = vec![original.id];
        proof.lowerings[0].owner = authored[1].nodes[0].id;
        assert!(matches!(
            compile(&defs, defs[0].id, Some(&proof)),
            Err(EvaluationError::InvalidSpecialization(_))
        ));
    }
}
