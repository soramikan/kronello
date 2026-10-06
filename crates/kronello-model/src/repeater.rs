//! Authored shared sources and stable instance placements; lowering is pure.
use crate::{
    Composition, CompositionId, CompositionInstance, CompositionInstanceId, ContentId,
    DocumentObject, NodeId, NodeKind, Project, Property, PropertyId, PropertySource, SceneNode,
    Value,
};
use kronello_time::{TimeMap, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const REPEATER_VERSION: u32 = 1;
pub const REPEATER_INSTANCE_LIMIT: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatSource {
    pub composition: CompositionId,
    pub root: NodeId,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpandedRepeatSource {
    pub source: RepeatSource,
    /// Fresh copied nested identities retain their original noise coordinate.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub noise_aliases: BTreeMap<CompositionInstanceId, CompositionInstanceId>,
    /// Copied simulation emitters retain their original event identity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub simulation_aliases: BTreeMap<ContentId, ContentId>,
    /// Dynamic text bounds rules survive template input materialization.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layout_constraints: BTreeMap<CompositionId, crate::TemplateConstraints>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatInstance {
    pub id: CompositionInstanceId,
    pub placement: NodeId,
    pub seed: u64,
    pub enabled: bool,
    pub active_range: TimeRange,
    pub local_time_map: TimeMap,
    pub properties: Vec<Property>,
    pub effects: Vec<crate::Effect>,
    pub input_bindings: BTreeMap<PropertyId, PropertySource<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_source: Option<ExpandedRepeatSource>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Repeater {
    pub id: ContentId,
    pub version: u32,
    pub source: RepeatSource,
    /// Array order is draw order only, never identity or random state.
    pub instances: Vec<RepeatInstance>,
}
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RepeaterError {
    #[error("unsupported repeater version {0}")]
    UnsupportedVersion(u32),
    #[error("missing repeater {0}")]
    Missing(ContentId),
    #[error("missing repeater source {0}")]
    MissingSource(CompositionId),
    #[error("repeater source must have the explicit single root {0}")]
    InvalidSource(NodeId),
    #[error("repeater has duplicate identities or an invalid placement")]
    InvalidIdentity,
    #[error("invalid materialized template layout constraints")]
    InvalidLayout,
    #[error("repeater instance budget exceeded")]
    Budget,
    #[error("repeater source dependency cycle")]
    Cycle,
    #[error("opaque repeater cannot be executed")]
    Opaque,
}
impl RepeatInstance {
    pub fn source<'a>(&'a self, repeater: &'a Repeater) -> &'a RepeatSource {
        self.expanded_source
            .as_ref()
            .map_or(&repeater.source, |s| &s.source)
    }
}
impl Project {
    pub fn repeater(&self, id: ContentId) -> Result<&Repeater, RepeaterError> {
        self.repeaters
            .iter()
            .find_map(|r| match r {
                DocumentObject::Known(r) if r.id == id => Some(Ok(r)),
                DocumentObject::Opaque(r) if r.id == id.as_uuid() => {
                    Some(Err(RepeaterError::Opaque))
                }
                _ => None,
            })
            .unwrap_or(Err(RepeaterError::Missing(id)))
    }
    pub fn lower_repeater_composition(
        &self,
        composition: &Composition,
    ) -> Result<Composition, RepeaterError> {
        let mut result = composition.clone();
        let mut nodes = Vec::new();
        let mut node_ids: BTreeSet<_> = composition.nodes.iter().map(|n| n.id).collect();
        let mut instance_ids = BTreeSet::new();
        for node in &mut result.nodes {
            let NodeKind::Repeater { content_ref } = node.kind else {
                continue;
            };
            let repeater = self.repeater(content_ref)?;
            if repeater.version != REPEATER_VERSION {
                return Err(RepeaterError::UnsupportedVersion(repeater.version));
            }
            if repeater.instances.len() > REPEATER_INSTANCE_LIMIT {
                return Err(RepeaterError::Budget);
            }
            if !node.child_order.is_empty() {
                return Err(RepeaterError::InvalidIdentity);
            }
            node.kind = NodeKind::Group;
            for instance in &repeater.instances {
                if !node_ids.insert(instance.placement) || !instance_ids.insert(instance.id) {
                    return Err(RepeaterError::InvalidIdentity);
                }
                let source = instance.source(repeater);
                let source_comp = self
                    .compositions
                    .iter()
                    .find_map(|c| match c {
                        DocumentObject::Known(c) if c.id == source.composition => Some(c),
                        _ => None,
                    })
                    .ok_or(RepeaterError::MissingSource(source.composition))?;
                if source_comp.root_nodes != [source.root]
                    || !source_comp.nodes.iter().any(|n| {
                        n.id == source.root
                            && n.containment_parent.is_none()
                            && n.transform_parent.is_none()
                    })
                {
                    return Err(RepeaterError::InvalidSource(source.root));
                }
                node.child_order.push(instance.placement);
                nodes.push(SceneNode {
                    id: instance.placement,
                    tags: Default::default(),
                    name: None,
                    enabled: instance.enabled,
                    effects: instance.effects.clone(),
                    kind: NodeKind::CompositionInstance(CompositionInstance {
                        id: instance.id,
                        definition_ref: source.composition,
                        input_bindings: instance.input_bindings.clone(),
                        local_time_map: instance.local_time_map.clone(),
                        seed: instance.seed,
                    }),
                    containment_parent: Some(node.id),
                    transform_parent: Some(node.id),
                    child_order: Vec::new(),
                    active_range: instance.active_range,
                    properties: instance.properties.clone(),
                });
            }
        }
        result.nodes.extend(nodes);
        Ok(result)
    }
    pub fn noise_alias_context(
        &self,
    ) -> Result<BTreeMap<CompositionInstanceId, CompositionInstanceId>, RepeaterError> {
        let mut raw = BTreeMap::new();
        for r in &self.repeaters {
            if let DocumentObject::Known(r) = r {
                for i in &r.instances {
                    if let Some(s) = &i.expanded_source {
                        for (key, value) in &s.noise_aliases {
                            if key == value
                                || raw.insert(*key, *value).is_some_and(|old| old != *value)
                            {
                                return Err(RepeaterError::InvalidIdentity);
                            }
                        }
                    }
                }
            }
        }
        let mut aliases = BTreeMap::new();
        for key in raw.keys() {
            let mut current = *key;
            let mut seen = BTreeSet::new();
            while let Some(target) = raw.get(&current) {
                if !seen.insert(current) {
                    return Err(RepeaterError::InvalidIdentity);
                }
                current = *target;
            }
            aliases.insert(*key, current);
        }
        Ok(aliases)
    }
    pub fn repeater_context(
        &self,
    ) -> (
        BTreeMap<CompositionInstanceId, u64>,
        BTreeMap<CompositionInstanceId, CompositionInstanceId>,
    ) {
        let mut seeds = BTreeMap::new();
        for r in &self.repeaters {
            if let DocumentObject::Known(r) = r {
                for i in &r.instances {
                    seeds.insert(i.id, i.seed);
                }
            }
        }
        // Validated callers reject malformed aliases; flattening is independent
        // of record iteration order and never introduces random coordinates.
        let aliases = self.noise_alias_context().unwrap_or_default();
        (seeds, aliases)
    }
}

impl RepeaterError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedVersion(_) | Self::Opaque => "UNSUPPORTED_FEATURE",
            Self::Missing(_) | Self::MissingSource(_) => "REPEATER_MISSING",
            Self::InvalidSource(_) => "REPEATER_SOURCE",
            Self::InvalidIdentity => "REPEATER_IDENTITY",
            Self::InvalidLayout => "REPEATER_LAYOUT",
            Self::Budget => "REPEATER_BUDGET",
            Self::Cycle => "REPEATER_CYCLE",
        }
    }
}
impl Project {
    pub fn validate_repeaters(&self) -> Result<(), RepeaterError> {
        self.noise_alias_context()?;
        let mut placements = BTreeSet::new();
        let mut instances = BTreeSet::new();
        let mut owners = BTreeSet::new();
        for c in &self.compositions {
            if let DocumentObject::Known(c) = c {
                for n in &c.nodes {
                    if let NodeKind::CompositionInstance(i) = &n.kind
                        && !instances.insert(i.id)
                    {
                        return Err(RepeaterError::InvalidIdentity);
                    }
                    placements.insert(n.id);
                    if let NodeKind::Repeater { content_ref } = n.kind
                        && !owners.insert(content_ref)
                    {
                        return Err(RepeaterError::InvalidIdentity);
                    }
                }
            }
        }
        for r in &self.repeaters {
            let DocumentObject::Known(r) = r else {
                return Err(RepeaterError::Opaque);
            };
            if r.version != REPEATER_VERSION {
                return Err(RepeaterError::UnsupportedVersion(r.version));
            }
            if r.instances.len() > REPEATER_INSTANCE_LIMIT {
                return Err(RepeaterError::Budget);
            }
            let source_comp = self
                .compositions
                .iter()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == r.source.composition => Some(c),
                    _ => None,
                })
                .ok_or(RepeaterError::MissingSource(r.source.composition))?;
            if source_comp.root_nodes != [r.source.root]
                || !source_comp.nodes.iter().any(|n| {
                    n.id == r.source.root
                        && n.containment_parent.is_none()
                        && n.transform_parent.is_none()
                })
            {
                return Err(RepeaterError::InvalidSource(r.source.root));
            }
            for i in &r.instances {
                if let Some(expanded) = &i.expanded_source {
                    for (id, rules) in &expanded.layout_constraints {
                        let c = self
                            .compositions
                            .iter()
                            .find_map(|c| match c {
                                DocumentObject::Known(c) if c.id == *id => Some(c),
                                _ => None,
                            })
                            .ok_or(RepeaterError::InvalidLayout)?;
                        for band in &rules.bands {
                            if !c.nodes.iter().any(|n| {
                                n.id == band.text_node && matches!(n.kind, NodeKind::Text { .. })
                            }) || !c.nodes.iter().any(|n| {
                                n.id == band.band_node
                                    && n.properties.iter().any(|p| p.id() == band.size_property)
                                    && n.properties
                                        .iter()
                                        .any(|p| p.id() == band.position_property)
                            }) || band.padding.iter().any(|p| p.get() < 0.)
                            {
                                return Err(RepeaterError::InvalidLayout);
                            }
                        }
                        for (node, limit) in &rules.max_lines {
                            if *limit == 0
                                || !c.nodes.iter().any(|n| {
                                    n.id == *node && matches!(n.kind, NodeKind::Text { .. })
                                })
                            {
                                return Err(RepeaterError::InvalidLayout);
                            }
                        }
                    }
                }
                if !instances.insert(i.id) || !placements.insert(i.placement) {
                    return Err(RepeaterError::InvalidIdentity);
                }
            }
        }
        let definitions: Vec<_> = self
            .compositions
            .iter()
            .filter_map(|c| {
                if let DocumentObject::Known(c) = c {
                    Some(c)
                } else {
                    None
                }
            })
            .map(|c| self.lower_repeater_composition(c))
            .collect::<Result<_, _>>()?;
        fn visit(
            id: CompositionId,
            definitions: &[Composition],
            visiting: &mut BTreeSet<CompositionId>,
            done: &mut BTreeSet<CompositionId>,
        ) -> Result<(), RepeaterError> {
            if done.contains(&id) {
                return Ok(());
            }
            if !visiting.insert(id) {
                return Err(RepeaterError::Cycle);
            }
            let c = definitions
                .iter()
                .find(|c| c.id == id)
                .ok_or(RepeaterError::MissingSource(id))?;
            for n in &c.nodes {
                if let NodeKind::CompositionInstance(i) = &n.kind {
                    visit(i.definition_ref, definitions, visiting, done)?;
                }
            }
            visiting.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut visiting = BTreeSet::new();
        let mut done = BTreeSet::new();
        for c in &definitions {
            visit(c.id, &definitions, &mut visiting, &mut done)?;
        }
        Ok(())
    }
}
