//! Upper compilation schedules text before bounds consumers, without reverse imports.
use crate::{RenderCache, RenderError};
use kronello_eval::{DependencyDeclarations, DependencyGraph, NodeKey, RuntimePropertyKey};
use kronello_model::*;
use kronello_text::{FontData, LayoutResult};
use kronello_time::Time;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct TemplateRuntime {
    pub inputs: BTreeMap<RuntimePropertyKey, Value>,
    pub texts: BTreeMap<NodeKey, String>,
    pub dependencies: DependencyDeclarations,
    pub layouts: BTreeMap<NodeKey, LayoutResult>,
    bands: Vec<(InstancePath, TemplateBandBinding)>,
    limits: BTreeMap<NodeKey, usize>,
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
                    kronello_template::validate_reachable(project, i.definition_ref)?;
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
                    let Some(i) = project.template_instances.iter().find_map(|i| match i {
                        DocumentObject::Known(i) if i.id == placement.id => Some(i),
                        _ => None,
                    }) else {
                        continue;
                    };
                    let d = kronello_template::definition(project, i.definition_ref)?;
                    let values = kronello_template::resolved_inputs(d, i)?;
                    for (name, input) in &d.public_inputs {
                        let value = values[name].clone();
                        match input.target {
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
                        }
                    }
                    for b in &d.constraints.bands {
                        let text = kronello_template::composition(project, d.composition_ref)?
                            .nodes
                            .iter()
                            .find(|n| n.id == b.text_node)
                            .unwrap();
                        let upstream: Vec<_> = text
                            .properties
                            .iter()
                            .map(|p| key(&child, text.id, p.id()))
                            .collect();
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
                    runtime
                        .limits
                        .extend(d.constraints.max_lines.iter().map(|(node, limit)| {
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
            self.layouts.insert(node, layout);
        }
        for (path, b) in &self.bands {
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
            let bounds = layout.layout_bounds;
            let corners = [
                [bounds.min[0], bounds.min[1]],
                [bounds.min[0], bounds.max[1]],
                [bounds.max[0], bounds.min[1]],
                [bounds.max[0], bounds.max[1]],
            ]
            .map(|point| transform.transform_point(point));
            let min = [0, 1].map(|axis| {
                corners
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::INFINITY, f64::min)
            });
            let max = [0, 1].map(|axis| {
                corners
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
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
        Ok(())
    }
}
