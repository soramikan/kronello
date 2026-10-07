//! Structured queries over one immutable stored revision.
use std::path::PathBuf;

use kronello_eval::{DependencyGraph, EvaluationSnapshot, RuntimePropertyKey};
use kronello_model::{
    ColorSpace, Composition, CompositionId, DesignExtent, DocumentObject, ExpressionId,
    InstancePath, NodeId, NodeKind, PropertyId, PropertyKey, Unit, Value, ValueType,
};
use kronello_time::{Duration, Time, TimeRange};
use serde::{Deserialize, Serialize};

use crate::{ServiceError, edit, open_existing};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneQueryRequest {
    #[serde(default)]
    pub search: SceneSearch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub project: PathBuf,
    pub composition: CompositionId,
    #[serde(default)]
    pub expand_instances: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<SceneEvaluationRequest>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneSearch {
    #[serde(default)]
    pub tags: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub kinds: std::collections::BTreeSet<SceneKind>,
    /// Overlap in each node's authored local composition time, not visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TimeRange>,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SceneKind {
    Group,
    Null,
    Shape,
    Text,
    CompositionInstance,
    Repeater,
    Simulation,
    Media,
}
impl SceneKind {
    fn of(kind: &NodeKind) -> Self {
        match kind {
            NodeKind::Group => Self::Group,
            NodeKind::Null => Self::Null,
            NodeKind::Shape { .. } => Self::Shape,
            NodeKind::Text { .. } => Self::Text,
            NodeKind::CompositionInstance(_) => Self::CompositionInstance,
            NodeKind::Media(_) => Self::Media,
            NodeKind::Repeater { .. } => Self::Repeater,
            NodeKind::Simulation { .. } => Self::Simulation,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneEvaluationRequest {
    pub time: Time,
    pub fonts: Vec<crate::FontInput>,
    /// COLOR-003 explicit `.cube` locators for render-consistent evaluation.
    #[serde(default)]
    pub luts: Vec<crate::LutInput>,
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
    /// Simultaneous stages in root Composition design_px (before matte clipping).
    pub bounds: kronello_render::LayoutValue,
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
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub tags: std::collections::BTreeSet<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
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
    /// COLOR-003 explicit `.cube` locators used only when `fonts` selects
    /// render-consistent evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub luts: Option<Vec<crate::LutInput>>,
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
/// Canonical surface text for one canonical expression AST (ADR-0105). The
/// stored AST is formatted read-only; metadata is never derived from text.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionFormatRequest {
    pub project: PathBuf,
    /// Format this stored expression; required when `expression` is omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression_id: Option<ExpressionId>,
    /// Format this supplied AST instead of reading a stored expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<Box<kronello_model::Expression>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionFormatResult {
    /// Snapshot revision the stored expression was read from, when retrieved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub expression_id: ExpressionId,
    /// Canonical surface text. Reparsing it with the same editing-envelope
    /// metadata reproduces the identical AST for every in-syntax expression.
    pub text: String,
}
pub(crate) fn expression_format(
    r: ExpressionFormatRequest,
) -> Result<ExpressionFormatResult, ServiceError> {
    let (expression, revision) = match (r.expression, r.expression_id) {
        (Some(expression), None) => (*expression, None),
        (None, Some(expression_id)) => {
            let store = open_existing(&r.project)?;
            let snapshot = store.snapshot()?;
            store.close()?;
            let expression = snapshot
                .document
                .expressions
                .iter()
                .find_map(|object| match object {
                    DocumentObject::Known(expression) if expression.id == expression_id => {
                        Some(expression.clone())
                    }
                    _ => None,
                })
                .ok_or_else(|| ServiceError::new("EXPRESSION_NOT_FOUND", "expression not found"))?;
            (expression, Some(snapshot.revision.to_string()))
        }
        _ => {
            return Err(ServiceError::invalid(
                "specify exactly one of expression or expression_id",
            ));
        }
    };
    let text = kronello_model::format_expression(&expression)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    Ok(ExpressionFormatResult {
        revision,
        expression_id: expression.id,
        text,
    })
}
fn definitions(project: &kronello_model::Project) -> Result<Vec<Composition>, ServiceError> {
    project
        .ensure_editable()
        .map_err(kronello_store::StoreError::from)?;
    project
        .compositions
        .iter()
        .map(|c| match c {
            DocumentObject::Known(c) => project
                .lower_repeater_composition(c)
                .map_err(|e| ServiceError::new(e.code(), e.to_string())),
            _ => Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "opaque composition",
            )),
        })
        .collect()
}
pub(crate) fn scene(mut r: SceneQueryRequest) -> Result<SceneQueryResult, ServiceError> {
    if r.limit.is_some_and(|limit| limit == 0 || limit > 1000)
        || r.search.range.is_some_and(|range| range.is_empty())
    {
        return Err(ServiceError::invalid(
            "scene limit must be 1..=1000 and search range nonempty",
        ));
    }
    r.search.tags = kronello_model::normalize_search_tags(&r.search.tags);
    if !kronello_model::valid_node_tags(&r.search.tags) {
        return Err(ServiceError::invalid("invalid search tags"));
    }
    let binding = serde_json::json!({"composition":r.composition,"expand_instances":r.expand_instances,
        "evaluation":r.evaluation,"search":r.search,"limit":r.limit});
    let cursor = r
        .cursor
        .as_deref()
        .map(crate::paging::Cursor::decode)
        .transpose()?;
    let store = open_existing(&r.project)?;
    let snapshot = if let Some(cursor) = &cursor {
        cursor.validate("scene.query", store.snapshot()?.document.id, &binding)?;
        store.snapshot_at(cursor.revision).map_err(|e| {
            if e.code() == "SNAPSHOT_NOT_FOUND" {
                crate::paging::expired()
            } else {
                e.into()
            }
        })?
    } else {
        store.snapshot()?
    };
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
                &request.luts,
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
            tags: n.tags.clone(),
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
                    bounds: n.bounds,
                    effects: n.effects.clone(),
                }),
            key: node_key,
            composition: c.id,
            kind: snapshot
                .document
                .compositions
                .iter()
                .find_map(|object| match object {
                    DocumentObject::Known(authored) if authored.id == c.id => authored
                        .nodes
                        .iter()
                        .find(|a| a.id == n.id)
                        .map(|a| a.kind.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| n.kind.clone()),
            containment_parent,
            transform_parent,
            children,
            active_range: n.active_range,
        });
    }
    let mut nodes: Vec<_> = nodes
        .into_iter()
        .filter(|n| {
            r.search.tags.is_subset(&n.tags)
                && (r.search.kinds.is_empty() || r.search.kinds.contains(&SceneKind::of(&n.kind)))
                && r.search.range.is_none_or(|range| {
                    !n.active_range.is_empty()
                        && n.active_range.start() < range.end()
                        && range.start() < n.active_range.end()
                })
        })
        .collect();
    if let Some(cursor) = &cursor {
        let after: SceneNodeKey = serde_json::from_value(cursor.after.clone())
            .map_err(|_| ServiceError::new("INVALID_CURSOR", "invalid scene continuation key"))?;
        let at = nodes
            .iter()
            .position(|n| n.key == after)
            .ok_or_else(crate::paging::expired)?;
        nodes.drain(..=at);
    }
    let next_cursor = if let Some(limit) = r.limit
        && nodes.len() > limit
    {
        nodes.truncate(limit);
        Some(
            crate::paging::Cursor::new(
                "scene.query",
                snapshot.document.id,
                snapshot.revision,
                binding,
                serde_json::to_value(&nodes.last().expect("nonempty page").key)?,
                None,
            )
            .encode()?,
        )
    } else {
        None
    };
    Ok(SceneQueryResult {
        next_cursor,
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
            let luts = r.luts.as_deref().unwrap_or_default();
            r.times
                .iter()
                .map(|time| {
                    evaluated_scene(&snapshot, &r.project, r.composition, *time, fonts, luts)
                })
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
    let audio_analyses = snapshot
        .document
        .audio_analysis_inputs()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let data_assets = snapshot
        .document
        .expression_data_inputs()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let graph = DependencyGraph::compile_with_data(
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
        &audio_analyses,
        &data_assets,
    )
    .map_err(evaluation)?;
    let (seeds, aliases) = snapshot.document.repeater_context();
    let graph = graph.with_repeater_context(seeds, aliases);
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

pub(crate) fn evaluated_scene(
    stored: &kronello_store::Snapshot,
    project: &std::path::Path,
    composition: CompositionId,
    time: Time,
    fonts: &[crate::FontInput],
    luts: &[crate::LutInput],
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
        media_proxies: kronello_render::MediaProxyMode::Off,
        luts: luts.to_vec(),
    };
    let snapshot =
        crate::freeze_render_input(stored, &input)?.with_luts(crate::load_locked_luts(&input)?);
    let bytes = crate::load_locked_fonts(&snapshot, &input)?;
    let fonts: Vec<_> = snapshot
        .font_locks()
        .iter()
        .zip(&bytes)
        .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
        .collect();
    Ok(kronello_render::build_scene_ir(&snapshot, time, &fonts)?)
}
