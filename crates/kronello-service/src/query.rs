//! Structured queries over one immutable stored revision.
use std::path::PathBuf;

use kronello_eval::{DependencyGraph, EvaluationSnapshot, RuntimePropertyKey};
use kronello_model::{
    ColorSpace, Composition, CompositionId, DesignExtent, DocumentObject, InstancePath, NodeId,
    NodeKind, PropertyId, PropertyKey, Unit, Value, ValueType,
};
use kronello_time::{Duration, Time, TimeRange};
use serde::{Deserialize, Serialize};

use crate::{ServiceError, edit, open_existing};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneQueryRequest {
    pub project: PathBuf,
    pub composition: CompositionId,
    #[serde(default)]
    pub expand_instances: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<SceneEvaluationRequest>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneEvaluationRequest {
    pub time: Time,
    pub fonts: Vec<crate::FontInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneNodeEvaluation {
    pub properties: std::collections::BTreeMap<PropertyId, Value>,
    pub text: Option<String>,
    pub layout_bounds: Option<QueryBounds>,
    pub world_transform: [[f64; 3]; 2],
    pub effects: Vec<kronello_model::ResolvedEffect>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneNodeKey {
    pub instance_path: InstancePath,
    pub node: NodeId,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneQueryNode {
    pub key: SceneNodeKey,
    pub composition: CompositionId,
    pub kind: NodeKind,
    pub containment_parent: Option<SceneNodeKey>,
    pub transform_parent: Option<SceneNodeKey>,
    /// Ordered children, with expanded definition roots before authored children.
    pub children: Vec<SceneNodeKey>,
    /// Authored half-open range in this instance's local composition time.
    pub active_range: TimeRange,
    /// Present only for active nodes when evaluation was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluated: Option<SceneNodeEvaluation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneQueryResult {
    pub revision: String,
    pub composition: CompositionId,
    pub duration: Duration,
    pub design_extent: DesignExtent,
    pub roots: Vec<SceneNodeKey>,
    /// Containment pre-order; includes inactive nodes without evaluating values.
    pub nodes: Vec<SceneQueryNode>,
}

/// A wire key for either a node property or a composition input. IDs and paths
/// are the evaluator's runtime identities, never display names or indices.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleKey {
    Node {
        instance_path: InstancePath,
        node: NodeId,
        property: PropertyId,
    },
    Composition {
        instance_path: InstancePath,
        composition: CompositionId,
        property: PropertyId,
    },
}
impl SampleKey {
    fn runtime(&self) -> RuntimePropertyKey {
        match self {
            Self::Node {
                instance_path,
                node,
                property,
            } => RuntimePropertyKey::Node(PropertyKey {
                instance_path: instance_path.clone(),
                node: *node,
                property: *property,
            }),
            Self::Composition {
                instance_path,
                composition,
                property,
            } => RuntimePropertyKey::Composition {
                instance_path: instance_path.clone(),
                composition: *composition,
                property: *property,
            },
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertySampleRequest {
    pub project: PathBuf,
    pub composition: CompositionId,
    #[schemars(length(min = 1, max = 100000))]
    pub keys: Vec<SampleKey>,
    #[schemars(length(min = 1, max = 100000))]
    pub times: Vec<Time>,
    /// Explicitly selects render-consistent active node values with font locks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fonts: Option<Vec<crate::FontInput>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertySample {
    pub key: SampleKey,
    pub value_type: ValueType,
    pub unit: Unit,
    pub values: Vec<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertySampleResult {
    pub revision: String,
    pub composition: CompositionId,
    pub times: Vec<Time>,
    /// Request key order; each values array follows request time order.
    pub samples: Vec<PropertySample>,
}
fn evaluation(e: kronello_eval::EvaluationError) -> ServiceError {
    ServiceError::new(e.code(), e.to_string())
}
fn definitions(project: &kronello_model::Project) -> Result<Vec<Composition>, ServiceError> {
    project
        .ensure_editable()
        .map_err(kronello_store::StoreError::from)?;
    project
        .compositions
        .iter()
        .map(|c| match c {
            DocumentObject::Known(c) => Ok(c.clone()),
            _ => Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "opaque composition",
            )),
        })
        .collect()
}
pub(crate) fn scene(r: SceneQueryRequest) -> Result<SceneQueryResult, ServiceError> {
    let store = open_existing(&r.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let compositions = definitions(&snapshot.document)?;
    let evaluated = r
        .evaluation
        .as_ref()
        .map(|request| {
            evaluated_scene(
                &snapshot,
                &r.project,
                r.composition,
                request.time,
                &request.fonts,
            )
        })
        .transpose()?;
    kronello_model::validate_compositions(&compositions, &edit::registry())
        .map_err(|e| ServiceError::new("EVALUATION_ERROR", format!("{e:?}")))?;
    let root = compositions
        .iter()
        .find(|c| c.id == r.composition)
        .ok_or_else(|| ServiceError::invalid("composition not found"))?;
    let root_path = InstancePath::root();
    let key = |path: &InstancePath, node| SceneNodeKey {
        instance_path: path.clone(),
        node,
    };
    let roots: Vec<_> = root
        .root_nodes
        .iter()
        .map(|id| key(&root_path, *id))
        .collect();
    // Iterative traversal also supports deeply nested authored containment.
    let mut pending: Vec<_> = roots.iter().rev().map(|key| (key.clone(), None)).collect();
    let mut nodes = Vec::new();
    while let Some((node_key, placement)) = pending.pop() {
        if nodes.len() >= 100_000 {
            return Err(ServiceError::invalid("expanded scene exceeds 100000 nodes"));
        }
        let c = node_key
            .instance_path
            .resolve(r.composition, &compositions)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let n = c
            .nodes
            .iter()
            .find(|n| n.id == node_key.node)
            .expect("validated node");
        let mut children = Vec::new();
        if r.expand_instances
            && let NodeKind::CompositionInstance(instance) = &n.kind
        {
            let path = node_key.instance_path.child(instance.id);
            let definition = path
                .resolve(r.composition, &compositions)
                .map_err(|e| ServiceError::invalid(e.to_string()))?;
            children.extend(definition.root_nodes.iter().map(|id| key(&path, *id)));
        }
        children.extend(
            n.child_order
                .iter()
                .map(|id| key(&node_key.instance_path, *id)),
        );
        let containment_parent = n
            .containment_parent
            .map(|id| key(&node_key.instance_path, id))
            .or_else(|| placement.clone());
        let transform_parent = n
            .transform_parent
            .map(|id| key(&node_key.instance_path, id))
            .or(placement);
        for child in children.iter().rev() {
            let enclosing = if child.instance_path != node_key.instance_path {
                Some(node_key.clone())
            } else {
                transform_enclosure(&child.instance_path, r.composition, &compositions)?
            };
            pending.push((child.clone(), enclosing));
        }
        nodes.push(SceneQueryNode {
            evaluated: evaluated
                .as_ref()
                .and_then(|scene| {
                    scene.nodes.iter().find(|n| {
                        n.key.instance_path == node_key.instance_path && n.key.node == node_key.node
                    })
                })
                .map(|n| SceneNodeEvaluation {
                    properties: n.properties.clone(),
                    text: n.text.clone(),
                    layout_bounds: match &n.content {
                        kronello_render::SceneContent::Text(layout) => Some(QueryBounds {
                            min: layout.layout_bounds.min,
                            max: layout.layout_bounds.max,
                        }),
                        _ => None,
                    },
                    world_transform: n.world_transform.0,
                    effects: n.effects.clone(),
                }),
            key: node_key,
            composition: c.id,
            kind: n.kind.clone(),
            containment_parent,
            transform_parent,
            children,
            active_range: n.active_range,
        });
    }
    Ok(SceneQueryResult {
        revision: snapshot.revision.to_string(),
        composition: r.composition,
        duration: root.duration,
        design_extent: root.design_extent,
        roots,
        nodes,
    })
}
fn transform_enclosure(
    path: &InstancePath,
    root: CompositionId,
    compositions: &[Composition],
) -> Result<Option<SceneNodeKey>, ServiceError> {
    let Some((last, parent_ids)) = path.ids().split_last() else {
        return Ok(None);
    };
    let parent = InstancePath::new(parent_ids.to_vec());
    let c = parent
        .resolve(root, compositions)
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let node = c
        .nodes
        .iter()
        .find(|n| matches!(&n.kind, NodeKind::CompositionInstance(i) if i.id == *last))
        .expect("validated instance path");
    Ok(Some(SceneNodeKey {
        instance_path: parent,
        node: node.id,
    }))
}
pub(crate) fn sample(r: PropertySampleRequest) -> Result<PropertySampleResult, ServiceError> {
    if r.times.is_empty()
        || r.keys.is_empty()
        || r.times.len().saturating_mul(r.keys.len()) > 100_000
    {
        return Err(ServiceError::invalid(
            "sample requires nonempty keys/times and at most 100000 values",
        ));
    }
    let store = open_existing(&r.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let compositions = definitions(&snapshot.document)?;
    let scenes = r
        .fonts
        .as_ref()
        .map(|fonts| {
            r.times
                .iter()
                .map(|time| evaluated_scene(&snapshot, &r.project, r.composition, *time, fonts))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let curves: Vec<_> = snapshot
        .document
        .curves
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let expressions: Vec<_> = snapshot
        .document
        .expressions
        .iter()
        .filter_map(|e| match e {
            DocumentObject::Known(e) => Some(e.clone()),
            _ => None,
        })
        .collect();
    let registry = edit::registry();
    let references = Default::default();
    let dependencies = Default::default();
    let graph = DependencyGraph::compile(
        EvaluationSnapshot {
            expressions: &expressions,
            compositions: &compositions,
            curves: &curves,
            registry: &registry,
            reference_bindings: &references,
            dependencies: &dependencies,
            working_space: ColorSpace::LinearRec709,
        },
        r.composition,
    )
    .map_err(evaluation)?;
    let mut samples = Vec::new();
    for key in &r.keys {
        let runtime = key.runtime();
        // Resolve descriptor from the definition, while evaluator verifies the
        // full runtime key, applies bindings/curves and diagnoses unsupported sources.
        let c = runtime
            .instance_path()
            .resolve(r.composition, &compositions)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let properties = match key {
            SampleKey::Node { node, .. } => {
                &c.nodes
                    .iter()
                    .find(|n| n.id == *node)
                    .ok_or_else(|| ServiceError::invalid("node not found"))?
                    .properties
            }
            SampleKey::Composition { composition, .. } if *composition == c.id => &c.properties,
            _ => {
                return Err(ServiceError::invalid(
                    "composition key does not match instance path",
                ));
            }
        };
        let id = match key {
            SampleKey::Node { property, .. } | SampleKey::Composition { property, .. } => property,
        };
        let property = properties
            .iter()
            .find(|p| p.id() == *id)
            .ok_or_else(|| ServiceError::invalid("property not found"))?;
        let descriptor = property
            .descriptor()
            .resolve(&registry)
            .map_err(|e| ServiceError::invalid(e.to_string()))?
            .definition();
        let values = if let Some(scenes) = &scenes {
            let SampleKey::Node {
                instance_path,
                node,
                property,
            } = key
            else {
                return Err(ServiceError::invalid(
                    "render-consistent samples require node keys",
                ));
            };
            scenes
                .iter()
                .map(|scene| {
                    scene
                        .nodes
                        .iter()
                        .find(|n| n.key.instance_path == *instance_path && n.key.node == *node)
                        .and_then(|n| n.properties.get(property))
                        .cloned()
                        .ok_or_else(|| {
                            ServiceError::invalid("sample node is inactive or property missing")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            r.times
                .iter()
                .map(|time| graph.evaluate_property(&runtime, *time).map_err(evaluation))
                .collect::<Result<Vec<_>, _>>()?
        };
        samples.push(PropertySample {
            key: key.clone(),
            unit: descriptor.unit,
            value_type: descriptor.value_type,
            values,
        });
    }
    Ok(PropertySampleResult {
        revision: snapshot.revision.to_string(),
        composition: r.composition,
        times: r.times,
        samples,
    })
}

fn evaluated_scene(
    stored: &kronello_store::Snapshot,
    project: &std::path::Path,
    composition: CompositionId,
    time: Time,
    fonts: &[crate::FontInput],
) -> Result<kronello_render::SceneIr, ServiceError> {
    let input = crate::RenderInput {
        project: project.into(),
        composition: Some(composition),
        target: None,
        region: kronello_render::OutputRegion {
            origin: [0.0; 2],
            extent: [1.0; 2],
            pixels: [1; 2],
        },
        profile: Default::default(),
        fonts: fonts.to_vec(),
    };
    let snapshot = crate::freeze_render_input(stored, &input)?;
    let bytes = crate::load_locked_fonts(&snapshot, &input)?;
    let fonts: Vec<_> = snapshot
        .font_locks()
        .iter()
        .zip(&bytes)
        .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
        .collect();
    Ok(kronello_render::build_scene_ir(&snapshot, time, &fonts)?)
}
