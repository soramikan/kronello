use crate::{
    CompositionId, CompositionInstanceId, ContentId, FiniteF64, ModelError, NodeId, Property,
    PropertyId, PropertySource, SchemaRegistry, Value,
};
use kronello_time::{Duration, FrameRate, TimeMap, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Design units, independent of output pixels or display DPI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "ExtentWire", into = "ExtentWire")]
pub struct DesignExtent {
    width: FiniteF64,
    height: FiniteF64,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtentWire {
    width: FiniteF64,
    height: FiniteF64,
}

impl DesignExtent {
    pub fn new(width: f64, height: f64) -> Result<Self, CompositionError> {
        let width = FiniteF64::new(width)?;
        let height = FiniteF64::new(height)?;
        if width.get() <= 0.0 || height.get() <= 0.0 {
            return Err(CompositionError::InvalidDesignExtent);
        }
        Ok(Self { width, height })
    }
    pub const fn width(self) -> f64 {
        self.width.get()
    }
    pub const fn height(self) -> f64 {
        self.height.get()
    }
}
impl TryFrom<ExtentWire> for DesignExtent {
    type Error = CompositionError;
    fn try_from(value: ExtentWire) -> Result<Self, Self::Error> {
        Self::new(value.width.get(), value.height.get())
    }
}
impl From<DesignExtent> for ExtentWire {
    fn from(value: DesignExtent) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

/// Editable document frame. After editing or decoding, validate the complete
/// definition set with validate_compositions before accepting it. Node storage
/// order has no semantic meaning; root_nodes and child_order carry draw order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub id: CompositionId,
    pub duration: Duration,
    pub design_extent: DesignExtent,
    pub edit_rate: FrameRate,
    pub root_nodes: Vec<NodeId>,
    pub nodes: Vec<SceneNode>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneNode {
    /// Display only; never used as identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Disabled containment subtrees do not enter the evaluated scene.
    #[serde(default = "node_enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<crate::Effect>,
    pub id: NodeId,
    pub kind: NodeKind,
    pub containment_parent: Option<NodeId>,
    pub transform_parent: Option<NodeId>,
    /// Ordered owned children, independent of transform parenting.
    pub child_order: Vec<NodeId>,
    pub active_range: TimeRange,
    pub properties: Vec<Property>,
}

/// Content hooks only; geometry, text layout, and rendering are later layers.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NodeKind {
    Group,
    Null,
    Shape { content_ref: ContentId },
    Text { content_ref: ContentId },
    CompositionInstance(CompositionInstance),
    Media(MediaNode),
}

impl<'de> Deserialize<'de> for NodeKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = crate::wire::Adjacent::deserialize(d)?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Content {
            content_ref: ContentId,
        }
        match wire.kind.as_str() {
            "group" => wire.unit(Self::Group),
            "null" => wire.unit(Self::Null),
            "shape" => {
                let p: Content = wire.value()?;
                Ok(Self::Shape {
                    content_ref: p.content_ref,
                })
            }
            "text" => {
                let p: Content = wire.value()?;
                Ok(Self::Text {
                    content_ref: p.content_ref,
                })
            }
            "composition_instance" => wire.value().map(Self::CompositionInstance),
            "media" => wire.value().map(Self::Media),
            _ => Err(wire.unknown(&[
                "group",
                "null",
                "shape",
                "text",
                "composition_instance",
                "media",
            ])),
        }
    }
}

/// Explicit media stream; AUDIO-003 executes audio only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaNode {
    pub asset: crate::AssetId,
    pub stream_index: u32,
    pub source_in: kronello_time::Time,
    /// Maps time relative to the owning node's active_range.start.
    pub time_map: TimeMap,
    /// A kronello.audio.volume Property owned by the same SceneNode.
    pub volume: PropertyId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompositionInstance {
    pub id: CompositionInstanceId,
    pub definition_ref: CompositionId,
    /// Targets are properties on the referenced Composition. Curve/expression
    /// metadata resolution and public template input policy are later layers.
    pub input_bindings: BTreeMap<PropertyId, PropertySource<Value>>,
    pub local_time_map: TimeMap,
    pub seed: u64,
}

/// Root-to-leaf stable placement identities. Empty denotes the root definition.
/// A path is contextual: resolve it from a root Composition to validate it.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct InstancePath(Vec<CompositionInstanceId>);

impl InstancePath {
    pub fn root() -> Self {
        Self::default()
    }
    pub fn new(ids: Vec<CompositionInstanceId>) -> Self {
        Self(ids)
    }
    pub fn ids(&self) -> &[CompositionInstanceId] {
        &self.0
    }
    pub fn child(&self, id: CompositionInstanceId) -> Self {
        let mut ids = self.0.clone();
        ids.push(id);
        Self(ids)
    }
    /// Requires a definition set accepted by validate_compositions. Does not
    /// generate identities, evaluate properties, or consult placement indices.
    pub fn resolve<'a>(
        &self,
        root: CompositionId,
        compositions: &'a [Composition],
    ) -> Result<&'a Composition, CompositionError> {
        let definitions: BTreeMap<_, _> = compositions.iter().map(|c| (c.id, c)).collect();
        if definitions.len() != compositions.len() {
            let mut seen = BTreeSet::new();
            let id = compositions.iter().find(|c| !seen.insert(c.id)).unwrap().id;
            return Err(CompositionError::DuplicateCompositionId { id });
        }
        let mut current = definitions
            .get(&root)
            .copied()
            .ok_or(CompositionError::CompositionNotFound { id: root })?;
        let mut visited = BTreeSet::from([root]);
        let mut edges = Vec::new();
        for (depth, id) in self.0.iter().enumerate() {
            let mut matches = current.nodes.iter().filter_map(|node| match &node.kind {
                NodeKind::CompositionInstance(instance) if instance.id == *id => {
                    Some((node.id, instance))
                }
                _ => None,
            });
            let (node, instance) = matches
                .next()
                .ok_or(CompositionError::InvalidInstancePath {
                    composition: current.id,
                    instance: *id,
                    depth,
                })?;
            if matches.next().is_some() {
                return Err(CompositionError::DuplicateInstanceId { id: *id });
            }
            edges.push(CompositionReference {
                composition: current.id,
                node,
                instance: *id,
                definition_ref: instance.definition_ref,
            });
            if !visited.insert(instance.definition_ref) {
                let start = edges
                    .iter()
                    .position(|e| e.composition == instance.definition_ref)
                    .unwrap();
                return Err(CompositionError::CompositionReferenceCycle {
                    path: edges[start..].to_vec(),
                });
            }
            current = definitions.get(&instance.definition_ref).copied().ok_or(
                CompositionError::MissingDefinition {
                    composition: current.id,
                    node,
                    instance: *id,
                    definition_ref: instance.definition_ref,
                },
            )?;
        }
        Ok(current)
    }
}

/// Property evaluation identity for a particular placement of a shared node.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PropertyKey {
    pub instance_path: InstancePath,
    pub node: NodeId,
    pub property: PropertyId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParentGraph {
    Containment,
    Transform,
}

/// An edge preserves the placement responsible for a definition-reference cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompositionReference {
    pub composition: CompositionId,
    pub node: NodeId,
    pub instance: CompositionInstanceId,
    pub definition_ref: CompositionId,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CompositionError {
    #[error("invalid Media node {node}: {message}")]
    InvalidMedia { node: NodeId, message: String },
    #[error("design extent must have positive finite dimensions")]
    InvalidDesignExtent,
    #[error("duplicate Composition ID: {id}")]
    DuplicateCompositionId { id: CompositionId },
    #[error("duplicate node ID: {id}")]
    DuplicateNodeId { id: NodeId },
    #[error("duplicate instance ID: {id}")]
    DuplicateInstanceId { id: CompositionInstanceId },
    #[error("duplicate property ID: {id}")]
    DuplicatePropertyId { id: PropertyId },
    #[error("Composition not found: {id}")]
    CompositionNotFound { id: CompositionId },
    #[error("{graph:?} parent {parent} missing in Composition {composition} for node {node}")]
    MissingParent {
        composition: CompositionId,
        graph: ParentGraph,
        node: NodeId,
        parent: NodeId,
    },
    #[error("containment cycle in Composition {composition}: {path:?}")]
    ContainmentCycle {
        composition: CompositionId,
        path: Vec<NodeId>,
    },
    #[error("transform cycle in Composition {composition}: {path:?}")]
    TransformCycle {
        composition: CompositionId,
        path: Vec<NodeId>,
    },
    #[error("Composition reference cycle: {path:?}")]
    CompositionReferenceCycle { path: Vec<CompositionReference> },
    #[error("invalid child order in Composition {composition}, parent {parent:?}")]
    InvalidChildOrder {
        composition: CompositionId,
        parent: Option<NodeId>,
    },
    #[error(
        "missing definition {definition_ref} for instance {instance} on node {node} in {composition}"
    )]
    MissingDefinition {
        composition: CompositionId,
        node: NodeId,
        instance: CompositionInstanceId,
        definition_ref: CompositionId,
    },
    #[error(
        "input property {property} missing in definition {definition_ref} for instance {instance}"
    )]
    MissingInputProperty {
        instance: CompositionInstanceId,
        definition_ref: CompositionId,
        property: PropertyId,
    },
    #[error("invalid binding of property {property} on instance {instance}: {source}")]
    InvalidInputBinding {
        instance: CompositionInstanceId,
        property: PropertyId,
        source: ModelError,
    },
    #[error("invalid property {property} in Composition {composition} on node {node:?}: {source}")]
    InvalidProperty {
        composition: CompositionId,
        node: Option<NodeId>,
        property: PropertyId,
        source: ModelError,
    },
    #[error("instance {instance} is not in Composition {composition} at path depth {depth}")]
    InvalidInstancePath {
        composition: CompositionId,
        instance: CompositionInstanceId,
        depth: usize,
    },
    #[error(transparent)]
    Value(#[from] ModelError),
}

/// Collects independent diagnostics. Cycles are traversed in stable ID order.
/// Validates reference closure,
/// ordering, property metadata and the three graphs separately. Does not resolve
/// geometry/text content, curve/expression catalogs, or execute TimeMaps.
pub fn validate_compositions(
    compositions: &[Composition],
    registry: &SchemaRegistry,
) -> Result<(), Vec<CompositionError>> {
    let mut errors = Vec::new();
    let mut definitions = BTreeMap::new();
    let mut node_ids = BTreeSet::new();
    let mut instance_ids = BTreeSet::new();
    let mut property_ids = BTreeSet::new();
    for composition in compositions {
        if definitions.insert(composition.id, composition).is_some() {
            errors.push(CompositionError::DuplicateCompositionId { id: composition.id });
        }
    }
    let mut references: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for composition in definitions.values() {
        let mut nodes = BTreeMap::new();
        validate_properties(
            composition,
            None,
            &composition.properties,
            registry,
            &mut property_ids,
            &mut errors,
        );
        for node in &composition.nodes {
            if !node_ids.insert(node.id) {
                errors.push(CompositionError::DuplicateNodeId { id: node.id });
            }
            nodes.insert(node.id, node);
            validate_properties(
                composition,
                Some(node.id),
                &node.properties,
                registry,
                &mut property_ids,
                &mut errors,
            );
            if let NodeKind::Media(media) = &node.kind {
                let valid = media.source_in >= kronello_time::Time::ZERO
                    && node
                        .properties
                        .iter()
                        .find(|p| p.id() == media.volume)
                        .is_some_and(|p| crate::validate_volume(p).is_ok());
                if !valid {
                    errors.push(CompositionError::InvalidMedia {
                        node: node.id,
                        message: "source_in or volume Property".into(),
                    });
                }
            }
            if let NodeKind::CompositionInstance(instance) = &node.kind {
                if !instance_ids.insert(instance.id) {
                    errors.push(CompositionError::DuplicateInstanceId { id: instance.id });
                }
                let edge = CompositionReference {
                    composition: composition.id,
                    node: node.id,
                    instance: instance.id,
                    definition_ref: instance.definition_ref,
                };
                references
                    .entry(composition.id)
                    .or_default()
                    .push((instance.definition_ref, edge));
                match definitions.get(&instance.definition_ref) {
                    None => errors.push(CompositionError::MissingDefinition {
                        composition: composition.id,
                        node: node.id,
                        instance: instance.id,
                        definition_ref: instance.definition_ref,
                    }),
                    Some(target) => validate_bindings(instance, target, registry, &mut errors),
                }
            }
        }
        let mut children: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for node in nodes.values() {
            children
                .entry(node.containment_parent)
                .or_default()
                .insert(node.id);
        }
        validate_order(
            composition,
            None,
            &composition.root_nodes,
            &children,
            &mut errors,
        );
        for node in nodes.values() {
            validate_order(
                composition,
                Some(node.id),
                &node.child_order,
                &children,
                &mut errors,
            );
        }
        for graph in [ParentGraph::Containment, ParentGraph::Transform] {
            let mut parents = BTreeMap::new();
            for node in nodes.values() {
                let parent = match graph {
                    ParentGraph::Containment => node.containment_parent,
                    ParentGraph::Transform => node.transform_parent,
                };
                let edges = parents.entry(node.id).or_insert_with(Vec::new);
                if let Some(parent) = parent {
                    if nodes.contains_key(&parent) {
                        edges.push((parent, node.id));
                    } else {
                        errors.push(CompositionError::MissingParent {
                            composition: composition.id,
                            graph,
                            node: node.id,
                            parent,
                        });
                    }
                }
            }
            for cycle in graph_cycles(&parents) {
                let mut path: Vec<_> = cycle.iter().map(|(from, _, _)| *from).collect();
                path.push(cycle.last().unwrap().1);
                errors.push(match graph {
                    ParentGraph::Containment => CompositionError::ContainmentCycle {
                        composition: composition.id,
                        path,
                    },
                    ParentGraph::Transform => CompositionError::TransformCycle {
                        composition: composition.id,
                        path,
                    },
                });
            }
        }
    }
    // Cycle traversal is independent of node storage order.
    for edges in references.values_mut() {
        edges.sort_by_key(|(target, edge)| (*target, edge.instance));
    }
    for cycle in graph_cycles(&references) {
        errors.push(CompositionError::CompositionReferenceCycle {
            path: cycle.into_iter().map(|(_, _, edge)| edge).collect(),
        });
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_properties(
    composition: &Composition,
    node: Option<NodeId>,
    properties: &[Property],
    registry: &SchemaRegistry,
    ids: &mut BTreeSet<PropertyId>,
    errors: &mut Vec<CompositionError>,
) {
    for property in properties {
        if !ids.insert(property.id()) {
            errors.push(CompositionError::DuplicatePropertyId { id: property.id() });
        }
        if let Err(source) = property.validate(registry) {
            errors.push(CompositionError::InvalidProperty {
                composition: composition.id,
                node,
                property: property.id(),
                source,
            });
        }
    }
}

fn validate_bindings(
    instance: &CompositionInstance,
    target: &Composition,
    registry: &SchemaRegistry,
    errors: &mut Vec<CompositionError>,
) {
    for (id, source) in &instance.input_bindings {
        if let Some(property) = target.properties.iter().find(|p| p.id() == *id) {
            // Validate an override using the existing Property contract. The shared
            // definition and its ordered modifiers remain unchanged.
            if let Err(source) = property.clone().set_source(source.clone(), registry) {
                errors.push(CompositionError::InvalidInputBinding {
                    instance: instance.id,
                    property: *id,
                    source,
                });
            }
        } else {
            errors.push(CompositionError::MissingInputProperty {
                instance: instance.id,
                definition_ref: target.id,
                property: *id,
            });
        }
    }
}

fn validate_order(
    composition: &Composition,
    parent: Option<NodeId>,
    order: &[NodeId],
    children: &BTreeMap<Option<NodeId>, BTreeSet<NodeId>>,
    errors: &mut Vec<CompositionError>,
) {
    let empty = BTreeSet::new();
    let expected = children.get(&parent).unwrap_or(&empty);
    let actual: BTreeSet<_> = order.iter().copied().collect();
    if &actual != expected || actual.len() != order.len() {
        errors.push(CompositionError::InvalidChildOrder {
            composition: composition.id,
            parent,
        });
    }
}

/// Iterative DFS avoids depending on the process stack for deeply nested scenes.
/// Each back edge gives a closed cycle; convergent DAG edges are not cycles.
fn graph_cycles<K: Copy + Ord, E: Clone>(graph: &BTreeMap<K, Vec<(K, E)>>) -> Vec<Vec<(K, K, E)>> {
    let mut finished = BTreeSet::new();
    let mut cycles = Vec::new();
    for start in graph.keys().copied() {
        if finished.contains(&start) {
            continue;
        }
        let mut stack = vec![(start, 0)];
        let mut active = BTreeMap::from([(start, 0)]);
        let mut path: Vec<(K, K, E)> = Vec::new();
        while let Some((node, next)) = stack.last_mut() {
            let edges = graph.get(node).map(Vec::as_slice).unwrap_or_default();
            if *next == edges.len() {
                let (node, _) = stack.pop().unwrap();
                active.remove(&node);
                finished.insert(node);
                if !stack.is_empty() {
                    path.pop();
                }
                continue;
            }
            let (target, edge) = &edges[*next];
            *next += 1;
            if let Some(index) = active.get(target) {
                let mut cycle = path[*index..].to_vec();
                cycle.push((*node, *target, edge.clone()));
                cycles.push(cycle);
            } else if !finished.contains(target) {
                path.push((*node, *target, edge.clone()));
                active.insert(*target, stack.len());
                stack.push((*target, 0));
            }
        }
    }
    cycles
}

fn node_enabled() -> bool {
    true
}
fn is_enabled(enabled: &bool) -> bool {
    *enabled
}
