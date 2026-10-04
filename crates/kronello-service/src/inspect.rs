//! Read-only diagnostics over one stored revision. No device initialization.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use kronello_eval::{DependencyGraph, EvaluationSnapshot, RuntimePropertyKey};
use kronello_model::{
    Composition, CompositionId, DocumentObject, InstancePath, NodeKind, PropertyKey, SceneNode,
    Value,
};
use kronello_render::{ExplainBackend, MatteBinding, RenderCache, RenderPathPlan, SceneIr};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{FontInput, RenderInput, SceneNodeKey, ServiceError};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeExplainRequest {
    pub project: PathBuf,
    pub composition: CompositionId,
    pub key: SceneNodeKey,
    pub time: Time,
    #[serde(default)]
    pub fonts: Vec<FontInput>,
    /// Explicit transient matte bindings, using the existing RenderSnapshot contract.
    #[serde(default)]
    pub mattes: Vec<MatteBinding>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderExplainRequest {
    pub input: RenderInput,
    pub time: Time,
    #[serde(default)]
    pub mattes: Vec<MatteBinding>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityAssessment {
    Hidden,
    Blocked,
    /// No semantic hiding cause found; pixel coverage/occlusion is not measured.
    PotentiallyVisible,
    Indeterminate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationCategory {
    Opacity,
    ActiveRange,
    Parent,
    Mask,
    Asset,
    Font,
    Unsupported,
    Evaluation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationImpact {
    Hides,
    Blocks,
    Information,
}
/// Stable semantic causes, extensible without adding state to the inspector.
/// GUI's enabled/disabled document contract is integrated by its owning branch.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VisibilityCode {
    OpacityZero,
    OutsideActiveRange,
    AncestorOpacityZero,
    AncestorOutsideActiveRange,
    TransformCollapsed,
    MatteOnly,
    MaskZeroOpacity,
    MaskZeroCoverage,
    MaskZeroLuminance,
    MaskInactive,
    MaskCoverageUnresolved,
    NoDrawableContent,
    PaintAlphaZero,
    AssetMissing,
    FontMissing,
    AssetHashMismatch,
    UnsupportedFeature,
    UnsupportedSchema,
    GlyphMissing,
    InvalidRequest,
    AssetIoError,
    EvaluationFailed,
}
impl VisibilityCode {
    fn from_code(code: &str) -> Self {
        match code {
            "OPACITY_ZERO" => Self::OpacityZero,
            "OUTSIDE_ACTIVE_RANGE" => Self::OutsideActiveRange,
            "ANCESTOR_OPACITY_ZERO" => Self::AncestorOpacityZero,
            "ANCESTOR_OUTSIDE_ACTIVE_RANGE" => Self::AncestorOutsideActiveRange,
            "TRANSFORM_COLLAPSED" => Self::TransformCollapsed,
            "MATTE_ONLY" => Self::MatteOnly,
            "MASK_ZERO_OPACITY" => Self::MaskZeroOpacity,
            "MASK_ZERO_COVERAGE" => Self::MaskZeroCoverage,
            "MASK_ZERO_LUMINANCE" => Self::MaskZeroLuminance,
            "MASK_INACTIVE" => Self::MaskInactive,
            "MASK_COVERAGE_UNRESOLVED" => Self::MaskCoverageUnresolved,
            "NO_DRAWABLE_CONTENT" => Self::NoDrawableContent,
            "PAINT_ALPHA_ZERO" => Self::PaintAlphaZero,
            "ASSET_MISSING" => Self::AssetMissing,
            "FONT_MISSING" => Self::FontMissing,
            "ASSET_HASH_MISMATCH" => Self::AssetHashMismatch,
            "UNSUPPORTED_FEATURE" => Self::UnsupportedFeature,
            "UNSUPPORTED_SCHEMA" => Self::UnsupportedSchema,
            "GLYPH_MISSING" => Self::GlyphMissing,
            "INVALID_REQUEST" => Self::InvalidRequest,
            "ASSET_IO_ERROR" => Self::AssetIoError,
            _ => Self::EvaluationFailed,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityReason {
    pub code: VisibilityCode,
    pub category: ExplanationCategory,
    pub impact: ExplanationImpact,
    /// The responsible node; ancestors retain their own instance identity.
    pub subject: SceneNodeKey,
    pub details: serde_json::Value,
    /// Diagnostic text only. Clients branch on code/category/impact/details.
    pub message: String,
    /// Original typed failure, including expression/cycle/overflow codes.
    pub error: Option<ServiceError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectionDependency {
    /// containment_parent / transform_parent / matte / property / shape / text /
    /// font / curve / expression. Parent graphs remain separate.
    pub kind: String,
    pub node: Option<SceneNodeKey>,
    pub consumer: Option<InspectionPropertyKey>,
    pub upstream: Option<InspectionPropertyKey>,
    pub resource: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectionPropertyKey {
    Node {
        instance_path: InstancePath,
        node: kronello_model::NodeId,
        property: kronello_model::PropertyId,
    },
    Composition {
        instance_path: InstancePath,
        composition: CompositionId,
        property: kronello_model::PropertyId,
    },
    Layout {
        instance_path: InstancePath,
        text: kronello_model::NodeId,
        consumer: kronello_model::PropertyId,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeExplainResult {
    pub revision: String,
    pub composition: CompositionId,
    pub key: SceneNodeKey,
    pub time: Time,
    pub local_time: Option<Time>,
    pub opacity: Option<f64>,
    pub assessment: VisibilityAssessment,
    pub reasons: Vec<VisibilityReason>,
    pub dependencies: Vec<InspectionDependency>,
    /// Full-composition compiler failures are distinct from this node's causes.
    pub render_diagnostics: Vec<ServiceError>,
    pub pixel_visibility_observed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderExplainResult {
    pub revision: String,
    pub target: kronello_render::RenderTarget,
    pub time: Time,
    /// None on a compiler failure; never substitutes a successful partial plan.
    pub plan: Option<RenderPathPlan>,
    pub diagnostics: Vec<ServiceError>,
}

fn stored(project: &std::path::Path) -> Result<kronello_store::Snapshot, ServiceError> {
    let store = crate::open_existing(project)?;
    let value = store.snapshot()?;
    store.close()?;
    Ok(value)
}
fn compile(
    stored: &kronello_store::Snapshot,
    input: &RenderInput,
    time: Time,
    mattes: Vec<MatteBinding>,
    cache: &mut RenderCache,
) -> Result<(kronello_render::RenderSnapshot, SceneIr), ServiceError> {
    let snapshot = crate::freeze_render_input(stored, input)?.with_mattes(mattes);
    let bytes = crate::load_locked_fonts(&snapshot, input)?;
    let fonts: Vec<_> = snapshot
        .font_locks()
        .iter()
        .zip(&bytes)
        .map(|(identity, bytes)| kronello_text::FontData { identity, bytes })
        .collect();
    let scene = kronello_render::build_scene_ir_with_cache(&snapshot, time, &fonts, cache)?;
    Ok((snapshot, scene))
}
pub(crate) fn render(
    r: RenderExplainRequest,
    backend: ExplainBackend,
) -> Result<RenderExplainResult, ServiceError> {
    r.input.region.validate()?;
    let stored = stored(&r.input.project)?;
    let target = match (r.input.composition, r.input.target) {
        (Some(composition), None) => composition.into(),
        (None, Some(target)) => target,
        _ => {
            return Err(ServiceError::invalid(
                "specify exactly one of composition or target",
            ));
        }
    };
    let mut cache = RenderCache::default();
    let outcome =
        compile(&stored, &r.input, r.time, r.mattes, &mut cache).and_then(|(snapshot, scene)| {
            Ok(kronello_render::explain_render_path(
                &scene,
                snapshot.profile(),
                r.input.region,
                backend,
                &mut cache,
            )?)
        });
    let (plan, diagnostics) = match outcome {
        Ok(plan) => (Some(plan), vec![]),
        Err(error) => (None, vec![error]),
    };
    Ok(RenderExplainResult {
        revision: stored.revision.to_string(),
        target,
        time: r.time,
        plan,
        diagnostics,
    })
}

fn authored<'a>(
    definitions: &'a [Composition],
    root: CompositionId,
    key: &SceneNodeKey,
) -> Result<&'a SceneNode, ServiceError> {
    key.instance_path
        .resolve(root, definitions)
        .map_err(|e| ServiceError::invalid(e.to_string()))?
        .nodes
        .iter()
        .find(|n| n.id == key.node)
        .ok_or_else(|| ServiceError::invalid("node not found"))
}
fn enclosure(
    definitions: &[Composition],
    root: CompositionId,
    path: &InstancePath,
) -> Result<Option<SceneNodeKey>, ServiceError> {
    let Some((id, preceding)) = path.ids().split_last() else {
        return Ok(None);
    };
    let parent = InstancePath::new(preceding.to_vec());
    let composition = parent
        .resolve(root, definitions)
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let node = composition
        .nodes
        .iter()
        .find(|n| matches!(&n.kind, NodeKind::CompositionInstance(i) if i.id == *id))
        .ok_or_else(|| ServiceError::invalid("placement not found"))?;
    Ok(Some(SceneNodeKey {
        instance_path: parent,
        node: node.id,
    }))
}
fn local_time(
    definitions: &[Composition],
    root: CompositionId,
    path: &InstancePath,
    time: Time,
) -> Result<Time, ServiceError> {
    let mut current = root;
    let mut local = time;
    for id in path.ids() {
        let c = definitions
            .iter()
            .find(|c| c.id == current)
            .ok_or_else(|| ServiceError::invalid("composition not found"))?;
        let placement = c
            .nodes
            .iter()
            .find_map(|n| match &n.kind {
                NodeKind::CompositionInstance(i) if i.id == *id => Some(i),
                _ => None,
            })
            .ok_or_else(|| ServiceError::invalid("placement not found"))?;
        local = placement
            .local_time_map
            .map(local)
            .map_err(|e| ServiceError::new("EVALUATION_ERROR", e.to_string()))?;
        current = placement.definition_ref;
    }
    Ok(local)
}
fn parent(
    definitions: &[Composition],
    root: CompositionId,
    key: &SceneNodeKey,
    transform: bool,
) -> Result<Option<SceneNodeKey>, ServiceError> {
    let n = authored(definitions, root, key)?;
    let id = if transform {
        n.transform_parent
    } else {
        n.containment_parent
    };
    match id {
        Some(node) => Ok(Some(SceneNodeKey {
            instance_path: key.instance_path.clone(),
            node,
        })),
        None => enclosure(definitions, root, &key.instance_path),
    }
}
fn reason(
    result: &mut NodeExplainResult,
    key: &SceneNodeKey,
    code: &str,
    category: ExplanationCategory,
    impact: ExplanationImpact,
    details: serde_json::Value,
) {
    result.reasons.push(VisibilityReason {
        code: VisibilityCode::from_code(code),
        category,
        impact,
        subject: key.clone(),
        details,
        message: code.replace('_', " ").to_lowercase(),
        error: None,
    });
}
fn failure(
    result: &mut NodeExplainResult,
    key: &SceneNodeKey,
    error: ServiceError,
    details: serde_json::Value,
) {
    let category = match error.code.as_str() {
        "ASSET_MISSING" | "ASSET_HASH_MISMATCH" => ExplanationCategory::Asset,
        "FONT_MISSING" | "GLYPH_MISSING" => ExplanationCategory::Font,
        "UNSUPPORTED_FEATURE" | "UNSUPPORTED_SCHEMA" => ExplanationCategory::Unsupported,
        _ => ExplanationCategory::Evaluation,
    };
    result.reasons.push(VisibilityReason {
        code: VisibilityCode::from_code(&error.code),
        category,
        impact: ExplanationImpact::Blocks,
        subject: key.clone(),
        details,
        message: error.message.clone(),
        error: Some(error),
    });
}
fn dependency(
    result: &mut NodeExplainResult,
    kind: &str,
    node: Option<SceneNodeKey>,
    resource: Option<String>,
) {
    result.dependencies.push(InspectionDependency {
        kind: kind.into(),
        node,
        consumer: None,
        upstream: None,
        resource,
    });
}
fn wire_key(key: &RuntimePropertyKey) -> Option<InspectionPropertyKey> {
    match key {
        RuntimePropertyKey::Node(k) => Some(InspectionPropertyKey::Node {
            instance_path: k.instance_path.clone(),
            node: k.node,
            property: k.property,
        }),
        RuntimePropertyKey::Composition {
            instance_path,
            composition,
            property,
        } => Some(InspectionPropertyKey::Composition {
            instance_path: instance_path.clone(),
            composition: *composition,
            property: *property,
        }),
        RuntimePropertyKey::LayoutValue {
            instance_path,
            text,
            consumer,
        } => Some(InspectionPropertyKey::Layout {
            instance_path: instance_path.clone(),
            text: *text,
            consumer: *consumer,
        }),
    }
}

fn paints_match(
    node: &kronello_render::SceneNodeIr,
    predicate: impl Fn(kronello_model::Color) -> bool,
) -> bool {
    match &node.content {
        kronello_render::SceneContent::Shape { resolved, .. } => {
            let fill = resolved.fill.as_ref().is_none_or(|f| {
                f.gradient.as_ref().map_or_else(
                    || predicate(f.color),
                    |g| g.stops.iter().all(|s| predicate(s.color)),
                )
            });
            let stroke = resolved.stroke.as_ref().is_none_or(|s| {
                s.width.get() == 0.0
                    || s.gradient.as_ref().map_or_else(
                        || predicate(s.color),
                        |g| g.stops.iter().all(|s| predicate(s.color)),
                    )
            });
            fill && stroke
        }
        kronello_render::SceneContent::Text(layout) => {
            layout.glyphs.iter().all(|g| predicate(g.fill))
        }
        kronello_render::SceneContent::Empty => true,
    }
}
fn leaf(scene: &SceneIr, node: &kronello_render::SceneNodeIr) -> bool {
    !scene
        .nodes
        .iter()
        .any(|n| n.parent.as_ref() == Some(&node.key))
}

pub(crate) fn node(r: NodeExplainRequest) -> Result<NodeExplainResult, ServiceError> {
    let stored = stored(&r.project)?;
    // Known authored data remains inspectable even when final execution rejects
    // an unknown effect or a missing content object. Opaque nodes stay unsupported.
    let known: Vec<_> = stored
        .document
        .compositions
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let mut pending = vec![r.composition];
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if reachable.len() > 1024 {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "composition inspection budget exceeded",
            ));
        }
        if let Some(c) = known.iter().find(|c| c.id == id) {
            for n in &c.nodes {
                if let NodeKind::CompositionInstance(i) = &n.kind {
                    pending.push(i.definition_ref);
                }
            }
        }
    }
    let definitions: Vec<_> = known
        .into_iter()
        .filter(|c| reachable.contains(&c.id))
        .collect();
    authored(&definitions, r.composition, &r.key)?;
    let mut result = NodeExplainResult {
        revision: stored.revision.to_string(),
        composition: r.composition,
        key: r.key.clone(),
        time: r.time,
        local_time: None,
        opacity: None,
        assessment: VisibilityAssessment::Indeterminate,
        reasons: vec![],
        dependencies: vec![],
        render_diagnostics: vec![],
        pixel_visibility_observed: false,
    };
    let input = RenderInput {
        project: r.project.clone(),
        composition: Some(r.composition),
        target: None,
        region: kronello_render::OutputRegion {
            origin: [0.0; 2],
            extent: [1.0; 2],
            pixels: [1; 2],
        },
        profile: Default::default(),
        fonts: r.fonts.clone(),
    };
    let mut cache = RenderCache::default();
    let scene = match compile(&stored, &input, r.time, r.mattes.clone(), &mut cache) {
        Ok((_, scene)) => Some(scene),
        Err(error) => {
            result.render_diagnostics.push(error);
            None
        }
    };
    let registry = crate::edit::registry();
    let curves: Vec<_> = stored
        .document
        .curves
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let expressions: Vec<_> = stored
        .document
        .expressions
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let references = BTreeMap::new();
    let declarations = match kronello_render::inspection_layout_dependencies(
        &stored.document,
        &definitions,
        r.composition,
    ) {
        Ok(declarations) => declarations,
        Err(error) => {
            result.render_diagnostics.push(error.into());
            BTreeMap::new()
        }
    };
    let graph = DependencyGraph::compile(
        EvaluationSnapshot {
            compositions: &definitions,
            curves: &curves,
            expressions: &expressions,
            registry: &registry,
            reference_bindings: &references,
            dependencies: &declarations,
            working_space: kronello_model::ColorSpace::LinearRec709,
        },
        r.composition,
    );
    let graph = match graph {
        Ok(graph) => Some(graph),
        Err(error) => {
            result
                .render_diagnostics
                .push(kronello_render::RenderError::Evaluation(error).into());
            None
        }
    };
    let mut chain = vec![r.key.clone()];
    let mut ancestors = BTreeSet::from([(r.key.instance_path.clone(), r.key.node)]);
    while let Some(p) = parent(
        &definitions,
        r.composition,
        chain.last().expect("target"),
        false,
    )? {
        if chain.len() >= 100_000 || !ancestors.insert((p.instance_path.clone(), p.node)) {
            return Err(ServiceError::invalid("containment inspection budget/cycle"));
        }
        dependency(&mut result, "containment_parent", Some(p.clone()), None);
        chain.push(p);
    }
    chain.reverse();
    let mut inactive_scope = None;
    for key in &chain {
        let node_definition = authored(&definitions, r.composition, key)?;
        let own = *key == r.key;
        let local = if inactive_scope
            .as_ref()
            .is_some_and(|path| path != &key.instance_path)
        {
            None
        } else {
            let mapped = local_time(&definitions, r.composition, &key.instance_path, r.time);
            match mapped {
                Ok(time) => Some(time),
                Err(error) => {
                    failure(
                        &mut result,
                        key,
                        error,
                        json!({"instance_path":key.instance_path}),
                    );
                    None
                }
            }
        };
        if own {
            result.local_time = local;
        }
        if let Some(time) = local {
            if !node_definition.active_range.contains(time) {
                reason(
                    &mut result,
                    key,
                    if own {
                        "OUTSIDE_ACTIVE_RANGE"
                    } else {
                        "ANCESTOR_OUTSIDE_ACTIVE_RANGE"
                    },
                    if own {
                        ExplanationCategory::ActiveRange
                    } else {
                        ExplanationCategory::Parent
                    },
                    ExplanationImpact::Hides,
                    json!({"local_time":time,"active_range":node_definition.active_range}),
                );
                inactive_scope.get_or_insert_with(|| key.instance_path.clone());
            }
            let opacity_property = node_definition
                .properties
                .iter()
                .find(|p| p.descriptor().key.as_str() == "kronello.opacity");
            let ir = scene.as_ref().and_then(|scene| {
                scene
                    .nodes
                    .iter()
                    .find(|n| n.key.node == key.node && n.key.instance_path == key.instance_path)
            });
            let opacity = if let Some(ir) = ir {
                Ok(ir.opacity)
            } else if let (Some(g), Some(p)) = (&graph, opacity_property) {
                g.evaluate_property(
                    &RuntimePropertyKey::Node(PropertyKey {
                        instance_path: key.instance_path.clone(),
                        node: key.node,
                        property: p.id(),
                    }),
                    r.time,
                )
                .map(|value| match value {
                    Value::Scalar(v) => v.get(),
                    _ => unreachable!("validated opacity"),
                })
                .map_err(|e| ServiceError::from(kronello_render::RenderError::Evaluation(e)))
            } else if opacity_property.is_none() {
                Ok(1.0)
            } else {
                Err(ServiceError::new(
                    "EVALUATION_ERROR",
                    "opacity cannot be evaluated because graph compilation failed",
                ))
            };
            match opacity {
                Ok(value) => {
                    if own {
                        result.opacity = Some(value);
                    }
                    if value == 0.0 {
                        reason(
                            &mut result,
                            key,
                            if own {
                                "OPACITY_ZERO"
                            } else {
                                "ANCESTOR_OPACITY_ZERO"
                            },
                            if own {
                                ExplanationCategory::Opacity
                            } else {
                                ExplanationCategory::Parent
                            },
                            ExplanationImpact::Hides,
                            json!({"opacity":value}),
                        );
                    }
                }
                Err(error) => failure(
                    &mut result,
                    key,
                    error,
                    json!({"property":opacity_property.map(|p| p.id())}),
                ),
            }
        }
        inspect_content(
            &stored.document,
            &definitions,
            r.composition,
            key,
            &r.fonts,
            &mut result,
        )?;
        for binding in &r.mattes {
            if binding.matte.node == key.node
                && binding.matte.instance_path == key.instance_path
                && !binding.visible
            {
                reason(
                    &mut result,
                    key,
                    "MATTE_ONLY",
                    ExplanationCategory::Mask,
                    ExplanationImpact::Hides,
                    json!({"source":binding.source}),
                );
            }
            if binding.source.node == key.node && binding.source.instance_path == key.instance_path
            {
                let matte = SceneNodeKey {
                    instance_path: binding.matte.instance_path.clone(),
                    node: binding.matte.node,
                };
                authored(&definitions, r.composition, &matte)?;
                dependency(&mut result, "matte", Some(matte.clone()), None);
                let active = scene
                    .as_ref()
                    .and_then(|s| s.nodes.iter().find(|n| n.key == binding.matte));
                if active.is_some_and(|n| n.opacity == 0.0) {
                    reason(
                        &mut result,
                        &matte,
                        "MASK_ZERO_OPACITY",
                        ExplanationCategory::Mask,
                        ExplanationImpact::Hides,
                        json!({"kind":binding.kind,"source":key}),
                    );
                } else if active.is_some_and(|n| {
                    leaf(scene.as_ref().expect("active scene"), n)
                        && paints_match(n, |color| color.components().alpha.get() == 0.0)
                }) {
                    reason(
                        &mut result,
                        &matte,
                        "MASK_ZERO_COVERAGE",
                        ExplanationCategory::Mask,
                        ExplanationImpact::Hides,
                        json!({"kind":binding.kind,"source":key}),
                    );
                } else if binding.kind == kronello_render::MatteKind::Luminance
                    && active.is_some_and(|n| {
                        n.effects.is_empty()
                            && leaf(scene.as_ref().expect("active scene"), n)
                            && paints_match(n, |color| {
                                let c = color.components();
                                c.r.get() == 0.0 && c.g.get() == 0.0 && c.b.get() == 0.0
                            })
                    })
                {
                    reason(
                        &mut result,
                        &matte,
                        "MASK_ZERO_LUMINANCE",
                        ExplanationCategory::Mask,
                        ExplanationImpact::Hides,
                        json!({"source":key}),
                    );
                } else if scene.is_some() && active.is_none() {
                    reason(
                        &mut result,
                        &matte,
                        "MASK_INACTIVE",
                        ExplanationCategory::Mask,
                        ExplanationImpact::Blocks,
                        json!({"source":key}),
                    );
                } else {
                    reason(
                        &mut result,
                        &matte,
                        "MASK_COVERAGE_UNRESOLVED",
                        ExplanationCategory::Mask,
                        ExplanationImpact::Information,
                        json!({"kind":binding.kind,"source":key}),
                    );
                }
            }
        }
    }
    let mut transform = Some(r.key.clone());
    let mut visited = BTreeSet::new();
    while let Some(key) = transform {
        let identity = (key.instance_path.clone(), key.node);
        if !visited.insert(identity) || visited.len() > 100_000 {
            return Err(ServiceError::invalid("transform inspection budget/cycle"));
        }
        transform = parent(&definitions, r.composition, &key, true)?;
        if let Some(p) = &transform {
            dependency(&mut result, "transform_parent", Some(p.clone()), None);
        }
    }
    if let Some(ir) = scene.as_ref().and_then(|s| {
        s.nodes
            .iter()
            .find(|n| n.key.node == r.key.node && n.key.instance_path == r.key.instance_path)
    }) {
        let [a, b] = ir.world_transform.0;
        if !scene
            .as_ref()
            .expect("scene")
            .nodes
            .iter()
            .any(|n| n.parent.as_ref() == Some(&ir.key))
        {
            let transparent = !matches!(&ir.content, kronello_render::SceneContent::Empty)
                && !matches!(&ir.content, kronello_render::SceneContent::Text(layout) if layout.glyphs.is_empty())
                && paints_match(ir, |color| color.components().alpha.get() == 0.0);
            if transparent {
                reason(
                    &mut result,
                    &r.key,
                    "PAINT_ALPHA_ZERO",
                    ExplanationCategory::Opacity,
                    ExplanationImpact::Hides,
                    json!({"all_paints_transparent":true}),
                );
            }
            if matches!(&ir.content, kronello_render::SceneContent::Text(layout) if layout.glyphs.is_empty())
            {
                reason(
                    &mut result,
                    &r.key,
                    "NO_DRAWABLE_CONTENT",
                    ExplanationCategory::Asset,
                    ExplanationImpact::Hides,
                    json!({"glyph_count":0}),
                );
            }
        }
        if a[0] * b[1] - a[1] * b[0] == 0.0 {
            reason(
                &mut result,
                &r.key,
                "TRANSFORM_COLLAPSED",
                ExplanationCategory::Parent,
                ExplanationImpact::Hides,
                json!({"world_transform":ir.world_transform.0}),
            );
        }
    }
    if let Some(graph) = &graph {
        let node = authored(&definitions, r.composition, &r.key)?;
        let keys: Vec<_> = node
            .properties
            .iter()
            .map(|p| {
                RuntimePropertyKey::Node(PropertyKey {
                    instance_path: r.key.instance_path.clone(),
                    node: r.key.node,
                    property: p.id(),
                })
            })
            .collect();
        let order = match graph.dependency_order(&keys) {
            Ok(order) => order,
            Err(error) => {
                failure(
                    &mut result,
                    &r.key,
                    kronello_render::RenderError::Evaluation(error).into(),
                    json!({"stage":"dependency_order"}),
                );
                vec![]
            }
        };
        for key in order {
            for upstream in graph.dependencies(&key).expect("scheduled key") {
                result.dependencies.push(InspectionDependency {
                    kind: "property".into(),
                    node: None,
                    consumer: wire_key(&key),
                    upstream: wire_key(upstream),
                    resource: None,
                });
            }
        }
    }
    result.assessment = if result
        .reasons
        .iter()
        .any(|r| r.impact == ExplanationImpact::Blocks)
    {
        VisibilityAssessment::Blocked
    } else if result
        .reasons
        .iter()
        .any(|r| r.impact == ExplanationImpact::Hides)
    {
        VisibilityAssessment::Hidden
    } else if result.local_time.is_none()
        || scene.is_none()
        || result
            .reasons
            .iter()
            .any(|r| r.code == VisibilityCode::MaskCoverageUnresolved)
    {
        VisibilityAssessment::Indeterminate
    } else {
        VisibilityAssessment::PotentiallyVisible
    };
    Ok(result)
}
fn inspect_content(
    project: &kronello_model::Project,
    definitions: &[Composition],
    root: CompositionId,
    key: &SceneNodeKey,
    fonts: &[FontInput],
    result: &mut NodeExplainResult,
) -> Result<(), ServiceError> {
    let node = authored(definitions, root, key)?;
    for property in &node.properties {
        match property.source() {
            kronello_model::PropertySource::Curve(id) => {
                dependency(result, "curve", Some(key.clone()), Some(id.to_string()))
            }
            kronello_model::PropertySource::Expression(id) => dependency(
                result,
                "expression",
                Some(key.clone()),
                Some(id.to_string()),
            ),
            _ => (),
        }
    }
    for effect in &node.effects {
        if effect.definition().is_err() {
            failure(
                result,
                key,
                ServiceError::new("UNSUPPORTED_FEATURE", "unknown effect"),
                json!({"effect":effect}),
            );
        }
    }
    match node.kind {
        NodeKind::Shape { content_ref } => {
            dependency(
                result,
                "shape",
                Some(key.clone()),
                Some(content_ref.to_string()),
            );
            let content = project.shapes.iter().find(|s| match s {
                DocumentObject::Known(s) => s.id == content_ref,
                DocumentObject::Opaque(s) => s.id == content_ref.as_uuid(),
            });
            match content {
                None => failure(
                    result,
                    key,
                    ServiceError::new("ASSET_MISSING", "shape content missing"),
                    json!({"content":content_ref}),
                ),
                Some(DocumentObject::Opaque(_)) => failure(
                    result,
                    key,
                    ServiceError::new("UNSUPPORTED_FEATURE", "opaque shape"),
                    json!({"content":content_ref}),
                ),
                _ => (),
            }
        }
        NodeKind::Text { content_ref } => {
            dependency(
                result,
                "text",
                Some(key.clone()),
                Some(content_ref.to_string()),
            );
            let content = project.texts.iter().find(|s| match s {
                DocumentObject::Known(s) => s.id == content_ref,
                DocumentObject::Opaque(s) => s.id == content_ref.as_uuid(),
            });
            match content {
                None => failure(
                    result,
                    key,
                    ServiceError::new("ASSET_MISSING", "text content missing"),
                    json!({"content":content_ref}),
                ),
                Some(DocumentObject::Opaque(_)) => failure(
                    result,
                    key,
                    ServiceError::new("UNSUPPORTED_FEATURE", "opaque text"),
                    json!({"content":content_ref}),
                ),
                Some(DocumentObject::Known(text)) => {
                    if text.layout_version != kronello_model::TEXT_LAYOUT_VERSION
                        || text.direction != kronello_model::TextDirection::Horizontal
                        || !text.ruby.is_empty()
                    {
                        failure(
                            result,
                            key,
                            ServiceError::new(
                                "UNSUPPORTED_FEATURE",
                                "unsupported text layout contract",
                            ),
                            json!({"layout_version":text.layout_version,"direction":text.direction,"ruby_count":text.ruby.len()}),
                        );
                    }
                    let identities: BTreeSet<_> = text.styles.iter().map(|s| &s.font).collect();
                    for identity in identities {
                        dependency(
                            result,
                            "font",
                            Some(key.clone()),
                            Some(identity.sha256.clone()),
                        );
                        let locators: Vec<_> =
                            fonts.iter().filter(|f| &f.identity == identity).collect();
                        if locators.len() != 1 {
                            failure(
                                result,
                                key,
                                ServiceError::new(
                                    if locators.is_empty() {
                                        "FONT_MISSING"
                                    } else {
                                        "INVALID_REQUEST"
                                    },
                                    "expected one locked font locator",
                                ),
                                json!({"font":identity}),
                            );
                        } else {
                            match std::fs::read(&locators[0].path) {
                                Ok(bytes) => {
                                    use sha2::{Digest, Sha256};
                                    let actual = format!("{:x}", Sha256::digest(&bytes));
                                    if actual != identity.sha256 {
                                        failure(
                                            result,
                                            key,
                                            ServiceError::new(
                                                "ASSET_HASH_MISMATCH",
                                                "locked font hash mismatch",
                                            ),
                                            json!({"font":identity,"actual":actual}),
                                        );
                                    } else {
                                        match kronello_text::pin_font(&bytes, identity.face_index) {
                                            Ok(actual) if &actual != identity => failure(
                                                result,
                                                key,
                                                ServiceError::new(
                                                    "ASSET_HASH_MISMATCH",
                                                    "locked font identity mismatch",
                                                ),
                                                json!({"font":identity,"actual":actual}),
                                            ),
                                            Err(error) => failure(
                                                result,
                                                key,
                                                kronello_render::RenderError::Layout(error).into(),
                                                json!({"font":identity}),
                                            ),
                                            _ => (),
                                        }
                                    }
                                }
                                Err(error) => failure(
                                    result,
                                    key,
                                    ServiceError::new(
                                        if error.kind() == std::io::ErrorKind::NotFound {
                                            "FONT_MISSING"
                                        } else {
                                            "ASSET_IO_ERROR"
                                        },
                                        error.to_string(),
                                    ),
                                    json!({"font":identity}),
                                ),
                            }
                        }
                    }
                }
            }
        }
        NodeKind::Null | NodeKind::Group if node.child_order.is_empty() => {
            reason(
                result,
                key,
                "NO_DRAWABLE_CONTENT",
                ExplanationCategory::Asset,
                ExplanationImpact::Hides,
                json!({"kind":node.kind}),
            );
        }
        _ => (),
    }
    Ok(())
}
