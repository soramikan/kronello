//! Upper compilation schedules text before bounds consumers, without reverse imports.
use crate::{RenderCache, RenderError, snapshot::content_asset};
use kronello_eval::{DependencyDeclarations, DependencyGraph, NodeKey, RuntimePropertyKey};
use kronello_model::*;
use kronello_text::{FontData, LayoutResult};
use kronello_time::Time;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct TemplateRuntime {
    pub inputs: BTreeMap<RuntimePropertyKey, Value>,
    pub texts: BTreeMap<NodeKey, String>,
    pub media_slots: BTreeMap<NodeKey, AssetId>,
    pub dependencies: DependencyDeclarations,
    pub layouts: BTreeMap<NodeKey, LayoutResult>,
    bands: Vec<(InstancePath, TemplateBandBinding)>,
    limits: BTreeMap<NodeKey, usize>,
    /// AI-003 (ADR-0126): tracking-driven crop rules per placement.
    reframes: Vec<(InstancePath, kronello_model::SmartReframeRule)>,
}
fn key(path: &InstancePath, node: NodeId, property: PropertyId) -> RuntimePropertyKey {
    PropertyKey {
        instance_path: path.clone(),
        node,
        property,
    }
    .into()
}
fn active_with_parents(node: &SceneNode, composition: &Composition, time: Time) -> bool {
    let mut current = node;
    loop {
        if !current.active_range.contains(time) {
            return false;
        }
        let Some(parent) = current.containment_parent else {
            return true;
        };
        current = composition
            .nodes
            .iter()
            .find(|node| node.id == parent)
            .unwrap();
    }
}
impl TemplateRuntime {
    pub fn compile(
        project: &Project,
        definitions: &[Composition],
        root: CompositionId,
    ) -> Result<Self, RenderError> {
        Self::compile_specialized(project, definitions, root, &BTreeMap::new())
    }
    pub fn compile_specialized(
        project: &Project,
        definitions: &[Composition],
        root: CompositionId,
        origins: &BTreeMap<CompositionId, kronello_eval::SpecializedDefinition>,
    ) -> Result<Self, RenderError> {
        if project
            .compositions
            .iter()
            .any(|c| matches!(c, DocumentObject::Known(c) if c.id == root))
        {
            kronello_template::validate_reachable(project, root)?;
        } else {
            let lowered = definitions
                .iter()
                .find(|c| c.id == root)
                .ok_or_else(|| RenderError::InvalidInput("missing root".into()))?;
            for node in &lowered.nodes {
                if let NodeKind::CompositionInstance(i) = &node.kind {
                    kronello_template::validate_reachable(
                        project,
                        origins
                            .get(&i.definition_ref)
                            .map_or(i.definition_ref, |info| info.authored),
                    )?;
                }
            }
        }
        let mut runtime = Self::default();
        let mut pending = vec![(InstancePath::root(), root)];
        while let Some((path, id)) = pending.pop() {
            let c = definitions
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| RenderError::InvalidInput("missing definition".into()))?;
            if path.ids().len() > 24 {
                return Err(RenderError::UnsupportedFeature(
                    "template nesting budget".into(),
                ));
            }
            for n in &c.nodes {
                if let NodeKind::CompositionInstance(placement) = &n.kind {
                    let child = path.child(placement.id);
                    pending.push((child.clone(), placement.definition_ref));
                    let constraints = if let Some(i) =
                        project.template_instances.iter().find_map(|i| match i {
                            DocumentObject::Known(i) if i.id == placement.id => Some(i),
                            _ => None,
                        }) {
                        let edition = kronello_template::definition(project, i.definition_ref)?;
                        let selected =
                            kronello_template::selected_definition(edition, i.variant.as_deref())?;
                        let d = &selected;
                        let values = kronello_template::resolved_inputs(edition, i)?;
                        for (target, value) in kronello_template::input_bindings(d, &values)? {
                            match target {
                                TemplateInputTarget::Property { node, property } => {
                                    runtime.inputs.insert(key(&child, node, property), value);
                                }
                                TemplateInputTarget::Text { node } => {
                                    let Value::String(value) = value else {
                                        unreachable!("validated type")
                                    };
                                    runtime.texts.insert(
                                        NodeKey {
                                            instance_path: child.clone(),
                                            node,
                                        },
                                        value,
                                    );
                                }
                                TemplateInputTarget::MediaSlot { node } => {
                                    kronello_template::validate_asset(project, &value)?;
                                    let Value::AssetRef(asset) = value else {
                                        unreachable!("validated type")
                                    };
                                    runtime.media_slots.insert(
                                        NodeKey {
                                            instance_path: child.clone(),
                                            node,
                                        },
                                        asset,
                                    );
                                }
                                TemplateInputTarget::DataTable { .. } => {
                                    unreachable!("projected bindings")
                                }
                            }
                        }
                        selected.constraints
                    } else {
                        let Some(constraints) = project
                            .repeaters
                            .iter()
                            .filter_map(|r| match r {
                                DocumentObject::Known(r) => Some(r),
                                _ => None,
                            })
                            .flat_map(|r| &r.instances)
                            .filter_map(|i| i.expanded_source.as_ref())
                            .find_map(|s| {
                                s.layout_constraints.get(
                                    &origins
                                        .get(&placement.definition_ref)
                                        .map_or(placement.definition_ref, |info| info.authored),
                                )
                            })
                        else {
                            continue;
                        };
                        constraints.clone()
                    };
                    for b in &constraints.bands {
                        let text = definitions
                            .iter()
                            .find(|c| c.id == placement.definition_ref)
                            .ok_or_else(|| {
                                RenderError::InvalidInput(
                                    "missing specialized template definition".into(),
                                )
                            })?
                            .nodes
                            .iter()
                            .find(|n| n.id == b.text_node)
                            .unwrap();
                        let mut upstream: Vec<_> = text
                            .properties
                            .iter()
                            .map(|p| key(&child, text.id, p.id()))
                            .collect();
                        if b.bounds == BoundsStage::Visual {
                            // Effect support is in Composition coordinates, so visual
                            // following also depends on the shared parent transform chain.
                            let mut current =
                                text.transform_parent.map(|node| (child.clone(), node));
                            let mut scope = child.clone();
                            loop {
                                if let Some((path, node)) = current.take() {
                                    let composition = path
                                        .resolve(root, definitions)
                                        .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                                    let parent =
                                        composition.nodes.iter().find(|n| n.id == node).unwrap();
                                    upstream.extend(
                                        parent.properties.iter().map(|p| key(&path, node, p.id())),
                                    );
                                    if let Some(node) = parent.transform_parent {
                                        current = Some((path, node));
                                        continue;
                                    }
                                    scope = path;
                                }
                                let Some((last, ids)) = scope.ids().split_last() else {
                                    break;
                                };
                                let path = InstancePath::new(ids.to_vec());
                                let composition = path
                                    .resolve(root, definitions)
                                    .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                                let parent = composition.nodes.iter().find(|n| {
                                    matches!(&n.kind, NodeKind::CompositionInstance(p) if p.id == *last)
                                }).unwrap();
                                current = Some((path, parent.id));
                            }
                        }
                        for target in [b.size_property, b.position_property] {
                            let layout = RuntimePropertyKey::LayoutValue {
                                instance_path: child.clone(),
                                text: b.text_node,
                                consumer: target,
                            };
                            runtime
                                .dependencies
                                .insert(layout.clone(), upstream.clone());
                            runtime
                                .dependencies
                                .insert(key(&child, b.band_node, target), vec![layout]);
                        }
                        runtime.bands.push((child.clone(), b.clone()));
                    }
                    for rule in &constraints.smart_reframes {
                        rule.settings
                            .validate()
                            .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                        let media = definitions
                            .iter()
                            .find(|c| c.id == placement.definition_ref)
                            .ok_or_else(|| {
                                RenderError::InvalidInput(
                                    "missing specialized template definition".into(),
                                )
                            })?
                            .nodes
                            .iter()
                            .find(|n| n.id == rule.node)
                            .ok_or_else(|| {
                                RenderError::InvalidInput("smart reframe node missing".into())
                            })?;
                        if !matches!(media.kind, NodeKind::Media(_) | NodeKind::Null) {
                            return Err(RenderError::InvalidInput(
                                "smart reframe requires a media node".into(),
                            ));
                        }
                        for (property, descriptor) in [
                            (rule.crop_origin_property, "kronello.media.crop_origin"),
                            (rule.crop_size_property, "kronello.media.crop_size"),
                        ] {
                            if !media.properties.iter().any(|p| {
                                p.id() == property && p.descriptor().key.as_str() == descriptor
                            }) {
                                return Err(RenderError::InvalidInput(
                                    "smart reframe crop property missing".into(),
                                ));
                            }
                            let layout = RuntimePropertyKey::LayoutValue {
                                instance_path: child.clone(),
                                text: rule.node,
                                consumer: property,
                            };
                            runtime.dependencies.insert(layout.clone(), Vec::new());
                            runtime
                                .dependencies
                                .insert(key(&child, rule.node, property), vec![layout]);
                        }
                        if !project
                            .tracking_data_assets
                            .iter()
                            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == rule.tracking))
                        {
                            return Err(RenderError::InvalidInput(
                                "smart reframe tracking asset missing".into(),
                            ));
                        }
                        runtime.reframes.push((child.clone(), rule.clone()));
                    }
                    runtime
                        .limits
                        .extend(constraints.max_lines.iter().map(|(node, limit)| {
                            (
                                NodeKey {
                                    instance_path: child.clone(),
                                    node: *node,
                                },
                                *limit,
                            )
                        }));
                }
            }
        }
        Ok(runtime)
    }
    pub fn text_override(&self, key: &NodeKey, text: &mut ResolvedText) -> Result<(), RenderError> {
        if let Some(value) = self.texts.get(key) {
            if text.styles.len() > 1 || !text.ruby.is_empty() {
                return Err(RenderError::UnsupportedFeature(
                    "template text inputs require one uniform style without ruby".into(),
                ));
            }
            if let Some(style) = text.styles.first_mut() {
                style.range = TextRange {
                    start: 0,
                    end: value.len(),
                };
            }
            if value.is_empty() {
                text.styles.clear();
            }
            text.text = value.clone();
        }
        Ok(())
    }
    pub fn layout_inputs(
        &mut self,
        project: &Project,
        definitions: &[Composition],
        graph: &DependencyGraph<'_>,
        time: Time,
        fonts: &[FontData<'_>],
        cache: &mut RenderCache,
    ) -> Result<(), RenderError> {
        // Each call is a pure function of its arguments: layouts and
        // layout-derived values from earlier calls cannot leak in, so a text
        // that is inactive now cannot leave stale bounds behind. Node and
        // Composition inputs are compile-time bindings and are retained.
        self.layouts.clear();
        self.inputs
            .retain(|key, _| !matches!(key, RuntimePropertyKey::LayoutValue { .. }));
        let requested: std::collections::BTreeSet<_> = self
            .bands
            .iter()
            .map(|(path, b)| NodeKey {
                instance_path: path.clone(),
                node: b.text_node,
            })
            .chain(self.limits.keys().cloned())
            .collect();
        for node in requested {
            let path = &node.instance_path;
            // Bounds are requested only for an active text in an active placement.
            let mut parent = InstancePath::root();
            let mut c = &definitions[0];
            let mut active = true;
            for instance in path.ids() {
                let placement = c
                    .nodes
                    .iter()
                    .find(|n| matches!(&n.kind,NodeKind::CompositionInstance(p) if p.id==*instance))
                    .unwrap();
                if !active_with_parents(placement, c, graph.local_time(&parent, time)?) {
                    active = false;
                    break;
                }
                let NodeKind::CompositionInstance(p) = &placement.kind else {
                    unreachable!()
                };
                parent = parent.child(*instance);
                c = definitions
                    .iter()
                    .find(|c| c.id == p.definition_ref)
                    .unwrap();
            }
            if !active {
                continue;
            }
            let authored = c.nodes.iter().find(|n| n.id == node.node).unwrap();
            if !active_with_parents(authored, c, graph.local_time(path, time)?) {
                continue;
            }
            let NodeKind::Text { content_ref } = authored.kind else {
                unreachable!()
            };
            let text = project
                .texts
                .iter()
                .find_map(|t| match t {
                    DocumentObject::Known(t) if t.id == content_ref => Some(t),
                    _ => None,
                })
                .ok_or(TextError::MissingContent { id: content_ref })?;
            let keys: Vec<_> = authored
                .properties
                .iter()
                .map(|p| key(path, node.node, p.id()))
                .collect();
            let values = graph
                .evaluate_properties_with_inputs(&keys, time, &self.inputs)?
                .into_iter()
                .filter_map(|(key, v)| match key {
                    RuntimePropertyKey::Node(k) => Some((k.property, v)),
                    _ => None,
                })
                .collect();
            let mut resolved = text.resolve(&values)?;
            self.text_override(&node, &mut resolved)?;
            let layout = cache.layout(&resolved, fonts)?;
            if let Some(limit) = self.limits.get(&node) {
                kronello_template::check_lines(node.node, layout.lines.len(), *limit)?;
            }
            crate::bounds::check_overflow(&node, &layout)?;
            self.layouts.insert(node, layout);
        }
        let targets: Vec<_> = self
            .bands
            .iter()
            .flat_map(|(path, b)| {
                [
                    key(path, b.band_node, b.size_property),
                    key(path, b.band_node, b.position_property),
                ]
            })
            .collect();
        let order: BTreeMap<_, _> = graph
            .dependency_order(&targets)?
            .into_iter()
            .enumerate()
            .map(|(i, k)| (k, i))
            .collect();
        let mut bands: Vec<_> = self.bands.iter().collect();
        bands.sort_by_key(|(path, b)| {
            [b.size_property, b.position_property]
                .map(|consumer| {
                    order[&RuntimePropertyKey::LayoutValue {
                        instance_path: path.clone(),
                        text: b.text_node,
                        consumer,
                    }]
                })
                .into_iter()
                .max()
                .unwrap()
        });
        for (path, b) in bands {
            let node = NodeKey {
                instance_path: path.clone(),
                node: b.text_node,
            };
            let Some(layout) = self.layouts.get(&node) else {
                continue;
            };
            let transform = graph
                .node_transform_with_inputs(&node, time, &self.inputs)?
                .affine();
            let mut bounds = crate::bounds::text_bounds(layout, transform, &[], None)?;
            if b.bounds == BoundsStage::Visual {
                let c = path
                    .resolve(definitions[0].id, definitions)
                    .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                let authored = c.nodes.iter().find(|n| n.id == b.text_node).unwrap();
                let keys: Vec<_> = authored
                    .properties
                    .iter()
                    .map(|p| key(path, b.text_node, p.id()))
                    .collect();
                let values = graph
                    .evaluate_properties_with_inputs(&keys, time, &self.inputs)?
                    .into_iter()
                    .filter_map(|(k, v)| match k {
                        RuntimePropertyKey::Node(k) => Some((k.property, v)),
                        _ => None,
                    })
                    .collect();
                let effects = authored
                    .effects
                    .iter()
                    .map(|e| Ok(e.definition()?.resolve(&values)?))
                    .collect::<Result<Vec<_>, RenderError>>()?;
                let parent =
                    graph.node_parent_world_transform_with_inputs(&node, time, &self.inputs)?;
                let visual =
                    crate::bounds::text_bounds(layout, parent.compose(transform), &effects, None)?
                        .visual_bounds;
                bounds.visual_bounds = visual.map(|b| b.transform(inverse(parent)?)).transpose()?;
            }
            let selected = bounds.select(b.bounds).unwrap_or_else(|| {
                let origin = transform.transform_point([0.0; 2]);
                crate::DesignBounds {
                    min: origin,
                    max: origin,
                }
            });
            let min = selected.min;
            let max = selected.max;
            let (size, position) = kronello_template::band_values(min, max, [0.0; 2], b.padding)?;
            self.inputs.insert(
                RuntimePropertyKey::LayoutValue {
                    instance_path: path.clone(),
                    text: b.text_node,
                    consumer: b.size_property,
                },
                Value::Vec2(size),
            );
            self.inputs.insert(
                RuntimePropertyKey::LayoutValue {
                    instance_path: path.clone(),
                    text: b.text_node,
                    consumer: b.position_property,
                },
                Value::Vec2(position),
            );
        }
        // AI-003 (ADR-0126): smart-reframe windows are pure functions of the
        // versioned tracking asset, settings and source time. Inactive rules
        // skip insertion exactly like inactive band texts.
        let reframes = self.reframes.clone();
        for (path, rule) in &reframes {
            let local = graph.local_time(path, time)?;
            let mut parent = InstancePath::root();
            let mut c = &definitions[0];
            let mut active = true;
            for instance in path.ids() {
                let placement = c
                    .nodes
                    .iter()
                    .find(|n| matches!(&n.kind, NodeKind::CompositionInstance(p) if p.id == *instance))
                    .unwrap();
                if !active_with_parents(placement, c, graph.local_time(&parent, time)?) {
                    active = false;
                    break;
                }
                let NodeKind::CompositionInstance(p) = &placement.kind else {
                    unreachable!()
                };
                parent = parent.child(*instance);
                c = definitions
                    .iter()
                    .find(|c| c.id == p.definition_ref)
                    .unwrap();
            }
            if !active {
                continue;
            }
            let authored = c.nodes.iter().find(|n| n.id == rule.node).unwrap();
            if !active_with_parents(authored, c, local) {
                continue;
            }
            let zero = Value::Vec2([FiniteF64::new(0.0).expect("finite"); 2]);
            let mut insert = |consumer, value: Value| {
                self.inputs.insert(
                    RuntimePropertyKey::LayoutValue {
                        instance_path: path.clone(),
                        text: rule.node,
                        consumer,
                    },
                    value,
                );
            };
            let relative = local.checked_sub(authored.active_range.start())?;
            let (asset_id, stream_index, source_time) = match &authored.kind {
                NodeKind::Media(media) => (
                    media.asset,
                    media.stream_index,
                    media.source_in.checked_add(media.time_map.map(relative)?)?,
                ),
                NodeKind::Null => {
                    let node = NodeKey {
                        instance_path: path.clone(),
                        node: rule.node,
                    };
                    let Some(&asset) = self.media_slots.get(&node) else {
                        // An unbound slot draws nothing; the disabled crop
                        // keeps consumer evaluation total.
                        insert(rule.crop_origin_property, zero.clone());
                        insert(rule.crop_size_property, zero.clone());
                        continue;
                    };
                    let asset_data = content_asset(project, asset)?;
                    let stream = asset_data
                        .streams
                        .iter()
                        .find(|s| s.width.is_some() && s.height.is_some())
                        .ok_or_else(|| {
                            RenderError::UnsupportedFeature(
                                "smart reframe media slot has no visual stream".into(),
                            )
                        })?;
                    (
                        asset,
                        stream.index,
                        stream
                            .start_time
                            .unwrap_or(Time::ZERO)
                            .checked_add(relative)?,
                    )
                }
                _ => unreachable!("validated smart reframe node kind"),
            };
            let asset_data = content_asset(project, asset_id)?;
            let stream = asset_data
                .streams
                .iter()
                .find(|s| s.index == stream_index)
                .ok_or_else(|| RenderError::InvalidInput("smart reframe stream missing".into()))?;
            let (Some(width), Some(height)) = (stream.width, stream.height) else {
                return Err(RenderError::UnsupportedFeature(
                    "smart reframe source dimensions missing".into(),
                ));
            };
            let tracking = project
                .tracking_data_assets
                .iter()
                .find_map(|a| match a {
                    DocumentObject::Known(a) if a.id == rule.tracking => Some(a),
                    _ => None,
                })
                .ok_or_else(|| {
                    RenderError::InvalidInput("smart reframe tracking asset missing".into())
                })?;
            tracking
                .validate()
                .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
            if !project.assets.iter().any(|a| {
                matches!(a, DocumentObject::Known(a)
                    if a.id == tracking.source.asset
                        && a.content_hash == tracking.source.content_hash)
            }) {
                return Err(RenderError::InvalidInput(
                    "stale smart reframe tracking source".into(),
                ));
            }
            let window = kronello_scene::reframe_window(
                tracking,
                &rule.settings,
                source_time,
                [f64::from(width), f64::from(height)],
            )
            .map_err(|e| RenderError::Backend {
                code: e.code(),
                message: e.to_string(),
            })?;
            let finite = |v: f64| {
                FiniteF64::new(v).map_err(|_| {
                    RenderError::InvalidInput("smart reframe window not finite".into())
                })
            };
            insert(
                rule.crop_origin_property,
                Value::Vec2([finite(window[0])?, finite(window[1])?]),
            );
            insert(
                rule.crop_size_property,
                Value::Vec2([finite(window[2])?, finite(window[3])?]),
            );
        }
        Ok(())
    }
}

fn inverse(affine: kronello_eval::Affine2) -> Result<kronello_eval::Affine2, RenderError> {
    let [a, b] = affine.0;
    let det = a[0] * b[1] - a[1] * b[0];
    if det == 0.0 || !det.is_finite() {
        return Err(RenderError::SingularLayoutTransform);
    }
    let matrix = kronello_eval::Affine2([
        [b[1] / det, -a[1] / det, (a[1] * b[2] - b[1] * a[2]) / det],
        [-b[0] / det, a[0] / det, (b[0] * a[2] - a[0] * b[2]) / det],
    ]);
    if !matrix.0.iter().flatten().all(|v| v.is_finite()) {
        return Err(RenderError::SingularLayoutTransform);
    }
    Ok(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kronello_eval::{EvaluationError, EvaluationSnapshot};

    #[test]
    fn compiler_declares_each_stage_and_diagnoses_closed_wrap_band_cycles() {
        for stage in [BoundsStage::Layout, BoundsStage::Ink, BoundsStage::Visual] {
            let mut project: Project =
                serde_json::from_str(include_str!("../../../examples/template-001.project.json"))
                    .unwrap();
            let mut d: TemplateDefinition = serde_json::from_str(include_str!(
                "../../../examples/template-001.definition.json"
            ))
            .unwrap();
            d.constraints.bands[0].bounds = stage;
            d.content_hash =
                kronello_template::authoring_hash(&project, d.composition_ref).unwrap();
            let duration = kronello_time::Duration::new(Time::from_integer(5)).unwrap();
            let id = CompositionInstanceId::new();
            let DocumentObject::Known(root) = &mut project.compositions[0] else {
                panic!()
            };
            let node = SceneNode {
                tags: Default::default(),
                name: None,
                enabled: true,
                id: NodeId::new(),
                kind: NodeKind::CompositionInstance(CompositionInstance {
                    id,
                    definition_ref: d.composition_ref,
                    input_bindings: BTreeMap::new(),
                    local_time_map: kronello_template::duration_map(
                        duration,
                        duration,
                        &d.duration_policy,
                    )
                    .unwrap(),
                    seed: 0,
                }),
                containment_parent: None,
                transform_parent: None,
                child_order: vec![],
                active_range: kronello_time::TimeRange::from_start_duration(Time::ZERO, duration)
                    .unwrap(),
                properties: vec![],
                effects: vec![],
            };
            root.root_nodes.push(node.id);
            root.nodes.push(node);
            let root_id = root.id;
            project.templates.push(DocumentObject::Known(d.clone()));
            project
                .template_instances
                .push(DocumentObject::Known(TemplateInstance {
                    id,
                    definition_ref: d.id,
                    version: d.version,
                    duration,
                    variant: None,
                    inputs: BTreeMap::new(),
                }));
            let definitions: Vec<_> = project
                .compositions
                .iter()
                .map(|c| {
                    let DocumentObject::Known(c) = c else {
                        panic!()
                    };
                    c.clone()
                })
                .collect();
            let runtime = TemplateRuntime::compile(&project, &definitions, root_id).unwrap();
            let binding = &d.constraints.bands[0];
            let path = InstancePath::root().child(id);
            let text = definitions[1]
                .nodes
                .iter()
                .find(|n| n.id == binding.text_node)
                .unwrap();
            let wrap = text
                .properties
                .iter()
                .find(|p| p.descriptor().key.as_str() == "kronello.text.wrap_width")
                .unwrap();
            let size = key(&path, binding.band_node, binding.size_property);
            let wrap = key(&path, binding.text_node, wrap.id());
            let layout = RuntimePropertyKey::LayoutValue {
                instance_path: path,
                text: binding.text_node,
                consumer: binding.size_property,
            };
            assert_eq!(runtime.dependencies[&size], std::slice::from_ref(&layout));
            assert!(runtime.dependencies[&layout].contains(&wrap));
            let mut dependencies = runtime.dependencies;
            // The reverse declaration is the EXPR/constraint compiler boundary;
            // no expression execution or dynamic dependency discovery is involved.
            dependencies.insert(wrap.clone(), vec![size.clone()]);
            let registry = crate::render_registry();
            let refs = Default::default();
            let curves: Vec<_> = project
                .curves
                .iter()
                .map(|c| {
                    let DocumentObject::Known(c) = c else {
                        panic!()
                    };
                    c.clone()
                })
                .collect();
            let expressions = Vec::new();
            let error = DependencyGraph::compile(
                EvaluationSnapshot {
                    expressions: &expressions,
                    compositions: &definitions,
                    curves: &curves,
                    registry: &registry,
                    reference_bindings: &refs,
                    dependencies: &dependencies,
                    working_space: ColorSpace::LinearRec709,
                },
                root_id,
            )
            .err()
            .unwrap();
            assert_eq!(error.code(), "PROPERTY_DEPENDENCY_CYCLE");
            let EvaluationError::DependencyCycle { path } = error else {
                panic!()
            };
            assert_eq!(path.first(), path.last());
            assert!(path.contains(&layout) && path.contains(&wrap) && path.contains(&size));
        }
    }
}
