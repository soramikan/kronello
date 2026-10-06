//! Upper simulation compilation: authored source clocks select pure kernel states.
use crate::{RenderCache, RenderError, RenderSnapshot, render_registry};
use kronello_eval::{DependencyGraph, EvaluationSnapshot, ReferenceBindings, RuntimePropertyKey};
use kronello_eval::{SimulationLowering, SpecializationProvenance, SpecializedDefinition};
use kronello_model::*;
use kronello_simulation::{ParticleInputs, SimulationConfig, SimulationError};
use kronello_text::FontData;
use kronello_time::{Rational, Time, TimeMap};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;
pub(crate) struct Proof {
    pub authored: Vec<Composition>,
    pub definitions: BTreeMap<CompositionId, SpecializedDefinition>,
    pub lowerings: Vec<SimulationLowering>,
    pub canonical_clock: bool,
}
impl Proof {
    pub fn borrowed(&self) -> SpecializationProvenance<'_> {
        SpecializationProvenance {
            authored: &self.authored,
            definitions: self.definitions.clone(),
            lowerings: self.lowerings.clone(),
            canonical_clock: self.canonical_clock,
        }
    }
}

fn failure(code: &'static str, message: impl ToString) -> RenderError {
    RenderError::Backend {
        code,
        message: message.to_string(),
    }
}
fn derived(domain: &str, path: &InstancePath, id: Uuid) -> Uuid {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(id.as_bytes());
    for id in path.ids() {
        h.update(id.as_uuid().as_bytes());
    }
    let mut b: [u8; 16] = h.finalize()[..16].try_into().unwrap();
    b[6] = (b[6] & 15) | 128;
    b[8] = (b[8] & 63) | 128;
    Uuid::from_bytes(b)
}
/// Each transient definition belongs to one scope; authored definitions remain shared.
fn instantiate(
    source: CompositionId,
    path: &InstancePath,
    templates: &[Composition],
    out: &mut Vec<Composition>,
    root: bool,
    provenance: &mut BTreeMap<CompositionId, SpecializedDefinition>,
) -> Result<CompositionId, RenderError> {
    if path.ids().len() > 24 || out.len() > 4096 {
        return Err(failure("SIMULATION_BUDGET", "scope budget"));
    }
    let mut c = templates
        .iter()
        .find(|c| c.id == source)
        .cloned()
        .ok_or_else(|| failure("SIMULATION_SOURCE", "missing definition"))?;
    if !root {
        c.id = CompositionId::from_uuid(derived("simulation-scope-v1", path, c.id.as_uuid()));
    }
    let id = c.id;
    provenance.insert(
        id,
        SpecializedDefinition {
            authored: source,
            scope: path.clone(),
        },
    );
    // Reserve first: siblings cannot accidentally duplicate the root identity.
    let index = out.len();
    out.push(c.clone());
    for n in &mut c.nodes {
        if let NodeKind::CompositionInstance(i) = &mut n.kind {
            i.definition_ref = instantiate(
                i.definition_ref,
                &path.child(i.id),
                templates,
                out,
                false,
                provenance,
            )?;
        }
    }
    out[index] = c;
    Ok(id)
}
fn scopes(
    defs: &[Composition],
    root: CompositionId,
) -> Result<Vec<(InstancePath, CompositionId)>, RenderError> {
    let mut result = Vec::new();
    let mut pending = vec![(InstancePath::root(), root)];
    while let Some((path, id)) = pending.pop() {
        if result.len() > 4096 {
            return Err(failure("SIMULATION_BUDGET", "scope budget"));
        }
        let c = path
            .resolve(root, defs)
            .map_err(|e| failure("SIMULATION_SOURCE", e))?;
        for n in &c.nodes {
            if let NodeKind::CompositionInstance(i) = &n.kind {
                pending.push((path.child(i.id), i.definition_ref));
            }
        }
        result.push((path, id));
    }
    Ok(result)
}
fn canonical(defs: &[Composition]) -> Result<Vec<Composition>, RenderError> {
    let mut result = defs.to_vec();
    for c in &mut result {
        for n in &mut c.nodes {
            if let NodeKind::CompositionInstance(i) = &mut n.kind {
                i.local_time_map = i
                    .local_time_map
                    .canonical_source_clock()
                    .map_err(|e| failure("SIMULATION_CLOCK", e))?;
            }
        }
    }
    Ok(result)
}
fn inverse(
    defs: &[Composition],
    root: CompositionId,
    path: &InstancePath,
    q: Time,
) -> Result<Time, RenderError> {
    let mut maps = Vec::new();
    let mut prefix = InstancePath::root();
    for id in path.ids() {
        let c = prefix
            .resolve(root, defs)
            .map_err(|e| failure("SIMULATION_CLOCK", e))?;
        let map = c
            .nodes
            .iter()
            .find_map(|n| match &n.kind {
                NodeKind::CompositionInstance(i) if i.id == *id => Some(&i.local_time_map),
                _ => None,
            })
            .ok_or_else(|| failure("SIMULATION_CLOCK", "missing placement"))?;
        maps.push(map);
        prefix = prefix.child(*id);
    }
    maps.into_iter().rev().try_fold(q, |q, map| {
        map.inverse_canonical(q)
            .map_err(|e| failure("SIMULATION_CLOCK", e))
    })
}
struct Inputs {
    curves: Vec<AnimationCurve>,
    expressions: Vec<Expression>,
    audio: Vec<AudioAnalysisDataAsset>,
    data: Vec<ExpressionDataAsset>,
}
impl Inputs {
    fn new(p: &Project) -> Result<Self, RenderError> {
        Ok(Self {
            curves: p
                .curves
                .iter()
                .filter_map(|o| match o {
                    DocumentObject::Known(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
            expressions: p
                .expressions
                .iter()
                .filter_map(|o| match o {
                    DocumentObject::Known(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
            audio: p
                .audio_analyses
                .iter()
                .filter_map(|o| match o {
                    DocumentObject::Known(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
            data: p
                .expression_data_inputs()
                .map_err(|e| failure("SIMULATION_INPUT", e))?,
        })
    }
}
fn graph<'a>(
    snapshot: &RenderSnapshot,
    defs: &'a [Composition],
    inputs: &'a Inputs,
    registry: &'a SchemaRegistry,
    refs: &'a ReferenceBindings,
    dependencies: &'a kronello_eval::DependencyDeclarations,
    proof: &Proof,
) -> Result<DependencyGraph<'a>, RenderError> {
    let graph = DependencyGraph::compile_specialized_with_data(
        EvaluationSnapshot {
            compositions: defs,
            curves: &inputs.curves,
            expressions: &inputs.expressions,
            registry,
            reference_bindings: refs,
            dependencies,
            working_space: snapshot.profile().working_space,
        },
        snapshot.composition(),
        &inputs.audio,
        &inputs.data,
        &proof.borrowed(),
    )?;
    let (seeds, aliases) = snapshot.project().repeater_context();
    Ok(graph.with_repeater_context(seeds, aliases))
}
fn vector(value: &Value) -> Result<[f64; 2], RenderError> {
    match value {
        Value::Vec2(v) => Ok([v[0].get(), v[1].get()]),
        _ => Err(failure("SIMULATION_INPUT", "expected Vec2")),
    }
}

fn dynamics_hash(
    snapshot: &RenderSnapshot,
    defs: &[Composition],
    graph: &DependencyGraph<'_>,
    keys: &[RuntimePropertyKey],
    record: &ParticleSimulation,
) -> Result<[u8; 32], RenderError> {
    use std::collections::BTreeSet;
    let mut objects = Vec::new();
    let mut curves = BTreeSet::new();
    let mut expressions = BTreeSet::new();
    let mut assets = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut sources = Vec::new();
    for key in graph.dependency_order(keys)? {
        let path = key.instance_path();
        paths.insert(path.clone());
        let c = path
            .resolve(snapshot.composition(), defs)
            .map_err(|e| failure("SIMULATION_INPUT", e))?;
        match &key {
            RuntimePropertyKey::Node(k) => {
                let p = c
                    .nodes
                    .iter()
                    .find(|n| n.id == k.node)
                    .and_then(|n| n.properties.iter().find(|p| p.id() == k.property))
                    .ok_or_else(|| failure("SIMULATION_INPUT", "missing closure Property"))?;
                objects.push(serde_json::json!(["node", path, k.node, p]));
                sources.push(p.source().clone());
            }
            RuntimePropertyKey::Composition { property, .. } => {
                let p = c
                    .properties
                    .iter()
                    .find(|p| p.id() == *property)
                    .ok_or_else(|| failure("SIMULATION_INPUT", "missing input Property"))?;
                objects.push(serde_json::json!(["composition", path, p]));
                sources.push(p.source().clone());
            }
            RuntimePropertyKey::LayoutValue { text, .. } => {
                let node = c
                    .nodes
                    .iter()
                    .find(|n| n.id == *text)
                    .ok_or_else(|| failure("SIMULATION_INPUT", "missing layout text"))?;
                if let NodeKind::Text { content_ref } = node.kind {
                    let content = snapshot
                        .project()
                        .texts
                        .iter()
                        .find(|t| matches!(t,DocumentObject::Known(t)if t.id==content_ref));
                    objects.push(serde_json::json!(["layout", path, node, content]));
                }
            }
        }
    }
    // Include every ancestor clock and effective input binding, including parent
    // expression sources which replace the definition's authored default.
    for path in paths {
        let mut prefix = InstancePath::root();
        for id in path.ids() {
            let c = prefix
                .resolve(snapshot.composition(), defs)
                .map_err(|e| failure("SIMULATION_CLOCK", e))?;
            let i = c
                .nodes
                .iter()
                .find_map(|n| match &n.kind {
                    NodeKind::CompositionInstance(i) if i.id == *id => Some(i),
                    _ => None,
                })
                .ok_or_else(|| failure("SIMULATION_CLOCK", "missing closure placement"))?;
            objects.push(serde_json::json!([
                "clock",
                prefix,
                i.id,
                i.local_time_map,
                i.seed,
                i.input_bindings
            ]));
            sources.extend(i.input_bindings.values().cloned());
            prefix = prefix.child(*id);
        }
    }
    sources.extend(record.source_bindings.values().cloned());
    for source in sources {
        match source {
            PropertySource::Curve(id) => {
                curves.insert(id);
            }
            PropertySource::Expression(id) => {
                expressions.insert(id);
            }
            _ => {}
        }
    }
    for id in expressions {
        let e = snapshot
            .project()
            .expressions
            .iter()
            .find_map(|e| match e {
                DocumentObject::Known(e) if e.id == id => Some(e),
                _ => None,
            })
            .ok_or_else(|| failure("SIMULATION_INPUT", "missing expression"))?;
        for n in &e.nodes {
            match n {
                ExpressionNode::CurveSample { curve, .. } => {
                    curves.insert(*curve);
                }
                ExpressionNode::AudioFeature { asset, .. }
                | ExpressionNode::DataAssetCell { asset, .. } => {
                    assets.insert(*asset);
                }
                _ => {}
            }
        }
        objects.push(serde_json::to_value(e)?);
    }
    for id in curves {
        let c = snapshot
            .project()
            .curves
            .iter()
            .find_map(|c| match c {
                DocumentObject::Known(c) if c.id() == id => Some(c),
                _ => None,
            })
            .ok_or_else(|| failure("SIMULATION_INPUT", "missing curve"))?;
        objects.push(serde_json::to_value(c)?);
    }
    for id in assets {
        for a in &snapshot.project().audio_analyses {
            if matches!(a,DocumentObject::Known(a)if a.id==id) {
                objects.push(serde_json::to_value(a)?);
            }
        }
        for a in &snapshot.project().expression_data_assets {
            if matches!(a,DocumentObject::Known(a)if a.id==id) {
                objects.push(serde_json::to_value(a)?);
            }
        }
    }
    // Template inputs and layout contracts may project values into a closure.
    // Conservatively retain their immutable pins even when some are unrelated.
    let context = snapshot.project().repeater_context();
    Ok(Sha256::digest(serde_json::to_vec(&(
        "simulation-dynamics-closure-v1",
        record,
        snapshot.profile().working_space,
        objects,
        context,
        &snapshot.project().template_instances,
        &snapshot.project().templates,
        snapshot
            .project()
            .repeaters
            .iter()
            .filter_map(|r| match r {
                DocumentObject::Known(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| &r.instances)
            .filter_map(|i| i.expanded_source.as_ref())
            .map(|s| &s.layout_constraints)
            .collect::<Vec<_>>(),
    ))?)
    .into())
}

pub(crate) fn lower(
    snapshot: &RenderSnapshot,
    time: Time,
    fonts: &[FontData<'_>],
    cache: &mut RenderCache,
) -> Result<(Vec<Composition>, Option<Proof>), RenderError> {
    let actual = snapshot.definitions_at(Some(time))?;
    if snapshot.project().simulations.is_empty() {
        return Ok((actual, None));
    }
    let authored = snapshot.definitions()?;
    let mut proof = Proof {
        authored: actual.clone(),
        definitions: BTreeMap::new(),
        lowerings: Vec::new(),
        canonical_clock: false,
    };
    let mut clock_proof = Proof {
        authored: authored.clone(),
        definitions: BTreeMap::new(),
        lowerings: Vec::new(),
        canonical_clock: true,
    };
    let mut defs = Vec::new();
    instantiate(
        snapshot.composition(),
        &InstancePath::root(),
        &actual,
        &mut defs,
        true,
        &mut proof.definitions,
    )?;
    let mut clock_defs = Vec::new();
    instantiate(
        snapshot.composition(),
        &InstancePath::root(),
        &authored,
        &mut clock_defs,
        true,
        &mut clock_proof.definitions,
    )?;
    clock_defs = canonical(&clock_defs)?;
    let inputs = Inputs::new(snapshot.project())?;
    let registry = render_registry();
    let refs = ReferenceBindings::new();
    loop {
        let next = scopes(&defs, snapshot.composition())?
            .into_iter()
            .find_map(|(path, id)| {
                defs.iter()
                    .find(|c| c.id == id)?
                    .nodes
                    .iter()
                    .find_map(|n| match n.kind {
                        NodeKind::Simulation { content_ref } => {
                            Some((path.clone(), id, n.id, content_ref))
                        }
                        _ => None,
                    })
            });
        let Some((path, id, node, content)) = next else {
            break;
        };
        let record = snapshot
            .project()
            .simulation(content)
            .map_err(|e| failure(e.code(), e))?;
        let clock = clock_defs.clone();
        let mut runtime = crate::template::TemplateRuntime::compile_specialized(
            snapshot.project(),
            &clock,
            snapshot.composition(),
            &clock_proof.definitions,
        )?;
        let canonical_dependencies = runtime.dependencies.clone();
        let canonical_graph = graph(
            snapshot,
            &clock,
            &inputs,
            &registry,
            &refs,
            &canonical_dependencies,
            &clock_proof,
        )?;
        let actual_runtime = crate::template::TemplateRuntime::compile_specialized(
            snapshot.project(),
            &defs,
            snapshot.composition(),
            &proof.definitions,
        )?;
        let actual_graph = graph(
            snapshot,
            &defs,
            &inputs,
            &registry,
            &refs,
            &actual_runtime.dependencies,
            &proof,
        )?;
        let q = actual_graph.local_time(&path, time)?;
        let keys: Vec<RuntimePropertyKey> = record
            .inputs
            .ids()
            .into_iter()
            .map(|property| {
                PropertyKey {
                    instance_path: path.clone(),
                    node,
                    property,
                }
                .into()
            })
            .collect();
        let hash = dynamics_hash(snapshot, &clock, &canonical_graph, &keys, record)?;
        let (_, context_aliases) = snapshot.project().repeater_context();
        let simulation_aliases = snapshot
            .project()
            .simulation_context()
            .map_err(|e| failure(e.code(), e))?;
        let config = SimulationConfig {
            emitter: simulation_aliases
                .get(&record.id)
                .copied()
                .unwrap_or(record.id),
            instance: InstancePath::new(
                path.ids()
                    .iter()
                    .map(|id| context_aliases.get(id).copied().unwrap_or(*id))
                    .collect(),
            ),
            seed: record.seed,
            start: record.start,
            step: record.step,
            lifetime: record.lifetime,
            emission_interval: record.emission_interval,
            checkpoint_stride: 8,
        };
        // Layout evaluation uses a separate ordinary cache so borrowing the
        // kernel checkpoint cache never makes results depend on retention.
        let mut input_cache = RenderCache::new(crate::CacheConfig::disabled());
        let (state, stats) = cache
            .simulation
            .state_at(&config, hash, q, |tick| {
                let root_time = inverse(&clock, snapshot.composition(), &path, tick)?;
                runtime.layout_inputs(
                    snapshot.project(),
                    &clock,
                    &canonical_graph,
                    root_time,
                    fonts,
                    &mut input_cache,
                )?;
                let values = canonical_graph.evaluate_properties_with_inputs(
                    &keys,
                    root_time,
                    &runtime.inputs,
                )?;
                let v = |index: usize| {
                    values
                        .get(&keys[index])
                        .ok_or_else(|| failure("SIMULATION_INPUT", "missing input"))
                };
                let enabled = match v(4)? {
                    Value::Bool(v) => *v,
                    _ => return Err(failure("SIMULATION_INPUT", "expected Bool")),
                };
                let count = match v(5)? {
                    Value::Scalar(v)
                        if v.get() >= 0. && v.get() <= 32. && v.get().fract() == 0. =>
                    {
                        v.get() as u32
                    }
                    _ => {
                        return Err(failure(
                            "SIMULATION_INPUT",
                            "birth_count must be integer in [0,32]",
                        ));
                    }
                };
                Ok::<_, RenderError>(ParticleInputs {
                    origin: vector(v(0)?)?,
                    velocity: vector(v(1)?)?,
                    jitter: vector(v(2)?)?,
                    acceleration: vector(v(3)?)?,
                    enabled,
                    birth_count: count,
                })
            })
            .map_err(|e| match e {
                SimulationError::Input(e) => e,
                SimulationError::BudgetExceeded => failure("SIMULATION_BUDGET", "kernel budget"),
                SimulationError::Time(e) => failure("SIMULATION_CLOCK", e),
                SimulationError::InvalidConfig => {
                    failure("SIMULATION_CLOCK", "invalid kernel config")
                }
                SimulationError::InvalidInput => failure("SIMULATION_INPUT", "invalid dynamics"),
            })?;
        cache.simulation_stats.checkpoint_hits += u64::from(stats.checkpoint_hit);
        cache.simulation_stats.replayed_steps += stats.replayed_steps;
        cache.simulation_stats.sampled_inputs += stats.sampled_inputs;
        cache.simulation_stats.particle_updates += stats.particle_updates;
        drop(actual_graph);
        drop(canonical_graph);
        let parent = defs
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .unwrap()
            .clone();
        let mut children = Vec::new();
        let mut placements = Vec::new();
        for particle in state.particles {
            let particle_path = path.child(particle.id);
            let source = instantiate(
                record.source.composition,
                &particle_path,
                &authored,
                &mut defs,
                false,
                &mut proof.definitions,
            )?;
            instantiate(
                record.source.composition,
                &particle_path,
                &authored,
                &mut clock_defs,
                false,
                &mut clock_proof.definitions,
            )?;
            let placement = NodeId::from_uuid(derived(
                "simulation-placement-v1",
                &path,
                particle.id.as_uuid(),
            ));
            let descriptor = registry
                .lookup(&SchemaKey::new("kronello.transform.position").unwrap())
                .map_err(|e| failure("SIMULATION_INPUT", e))?;
            let property = Property::new(
                PropertyId::from_uuid(derived(
                    "simulation-position-v1",
                    &path,
                    particle.id.as_uuid(),
                )),
                DescriptorRef::new(descriptor),
                PropertySource::Constant(Value::Vec2([
                    FiniteF64::new(particle.position[0])
                        .map_err(|e| failure("SIMULATION_INPUT", e))?,
                    FiniteF64::new(particle.position[1])
                        .map_err(|e| failure("SIMULATION_INPUT", e))?,
                ])),
                vec![],
                &registry,
            )
            .map_err(|e| failure("SIMULATION_INPUT", e))?;
            placements.push(SceneNode {
                id: placement,
                tags: Default::default(),
                name: None,
                enabled: true,
                effects: Vec::new(),
                kind: NodeKind::CompositionInstance(CompositionInstance {
                    id: particle.id,
                    definition_ref: source,
                    input_bindings: record.source_bindings.clone(),
                    local_time_map: TimeMap::linear(
                        Time::ZERO
                            .checked_sub(particle.birth)
                            .map_err(|e| failure("SIMULATION_CLOCK", e))?,
                        Rational::ONE,
                    )
                    .map_err(|e| failure("SIMULATION_CLOCK", e))?,
                    seed: record.seed,
                }),
                containment_parent: Some(node),
                transform_parent: Some(node),
                child_order: Vec::new(),
                active_range: parent.active_range,
                properties: vec![property],
            });
            children.push(placement);
        }
        let lowering = SimulationLowering {
            composition: id,
            owner: node,
            source: record.source.composition,
            source_bindings: record.source_bindings.clone(),
            seed: record.seed,
            generated_nodes: placements.clone(),
        };
        proof.lowerings.push(lowering.clone());
        clock_proof.lowerings.push(lowering);
        for definitions in [&mut defs, &mut clock_defs] {
            let c = definitions.iter_mut().find(|c| c.id == id).unwrap();
            let n = c.nodes.iter_mut().find(|n| n.id == node).unwrap();
            n.kind = NodeKind::Group;
            n.child_order = children.clone();
            c.nodes.extend(placements.clone());
        }
        clock_defs = canonical(&clock_defs)?;
    }
    Ok((defs, Some(proof)))
}
