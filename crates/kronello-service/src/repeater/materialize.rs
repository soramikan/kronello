//! Bake nested template inputs into editable copies while retaining layout rules.
use super::{copy, derived, scan};
use crate::ServiceError;
use kronello_model::*;
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(super) type Materialized = (
    Project,
    BTreeMap<CompositionId, TemplateConstraints>,
    BTreeMap<CompositionInstanceId, CompositionInstanceId>,
    BTreeMap<ContentId, ContentId>,
);
fn known_comp(project: &Project, id: CompositionId) -> Result<Composition, ServiceError> {
    kronello_template::composition(project, id)
        .cloned()
        .map_err(Into::into)
}
fn encode<T: serde::Serialize>(v: &T) -> Result<Json, ServiceError> {
    serde_json::to_value(v).map_err(|e| ServiceError::invalid(e.to_string()))
}

pub(super) fn templates(
    original: &Project,
    root: CompositionId,
    expansion: Uuid,
) -> Result<Materialized, ServiceError> {
    let mut work = original.clone();
    let mut constraints = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    let mut simulation_aliases = BTreeMap::new();
    let mut template_ids: BTreeMap<_, _> = original
        .template_instances
        .iter()
        .filter_map(|t| {
            if let DocumentObject::Known(t) = t {
                Some((t.id, t.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > 1024 {
            return Err(ServiceError::new(
                "REPEATER_BUDGET",
                "template materialization budget exceeded",
            ));
        }
        let mut owner = known_comp(&work, id)?;
        for node in &mut owner.nodes {
            match &mut node.kind {
                NodeKind::CompositionInstance(placement) => {
                    if let Some(instance) = template_ids.get(&placement.id).cloned() {
                        let edition =
                            kronello_template::definition(original, instance.definition_ref)?;
                        let selected = kronello_template::selected_definition(
                            edition,
                            instance.variant.as_deref(),
                        )?;
                        let inputs = kronello_template::resolved_inputs(edition, &instance)?;
                        let mut source = known_comp(&work, placement.definition_ref)?;
                        let content_ids: BTreeSet<_> = source
                            .nodes
                            .iter()
                            .filter_map(|n| match n.kind {
                                NodeKind::Shape { content_ref }
                                | NodeKind::Text { content_ref } => Some(content_ref),
                                _ => None,
                            })
                            .collect();
                        let mut shapes: Vec<_> = work
                            .shapes
                            .iter()
                            .filter_map(|s| {
                                if let DocumentObject::Known(s) = s {
                                    content_ids.contains(&s.id).then_some(s.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let mut texts: Vec<_> = work
                            .texts
                            .iter()
                            .filter_map(|s| {
                                if let DocumentObject::Known(s) = s {
                                    content_ids.contains(&s.id).then_some(s.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let repeat_ids: BTreeSet<_> = source
                            .nodes
                            .iter()
                            .filter_map(|n| match n.kind {
                                NodeKind::Repeater { content_ref } => Some(content_ref),
                                _ => None,
                            })
                            .collect();
                        let repeats: Vec<_> = work
                            .repeaters
                            .iter()
                            .filter_map(|r| {
                                if let DocumentObject::Known(r) = r {
                                    repeat_ids.contains(&r.id).then_some(r.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let simulation_ids: BTreeSet<_> = source
                            .nodes
                            .iter()
                            .filter_map(|n| match n.kind {
                                NodeKind::Simulation { content_ref } => Some(content_ref),
                                _ => None,
                            })
                            .collect();
                        let simulations: Vec<_> = work
                            .simulations
                            .iter()
                            .filter_map(|s| match s {
                                DocumentObject::Known(s) if simulation_ids.contains(&s.id) => {
                                    Some(s.clone())
                                }
                                _ => None,
                            })
                            .collect();
                        for (target, value) in
                            kronello_template::input_bindings(&selected, &inputs)?
                        {
                            match target {
                                TemplateInputTarget::Property { node, property } => {
                                    let n = source
                                        .nodes
                                        .iter_mut()
                                        .find(|n| n.id == node)
                                        .ok_or_else(|| {
                                            ServiceError::invalid("template property node missing")
                                        })?;
                                    let p = n
                                        .properties
                                        .iter_mut()
                                        .find(|p| p.id() == property)
                                        .ok_or_else(|| {
                                            ServiceError::invalid("template property missing")
                                        })?;
                                    p.set_source(
                                        PropertySource::Constant(value),
                                        &kronello_render::render_registry(),
                                    )
                                    .map_err(|e| ServiceError::invalid(e.to_string()))?;
                                }
                                TemplateInputTarget::Text { node } => {
                                    let n = source.nodes.iter().find(|n| n.id == node).ok_or_else(
                                        || ServiceError::invalid("template text node missing"),
                                    )?;
                                    let NodeKind::Text { content_ref } = n.kind else {
                                        return Err(ServiceError::invalid(
                                            "template text target is not text",
                                        ));
                                    };
                                    let mut independent = texts
                                        .iter()
                                        .find(|t| t.id == content_ref)
                                        .cloned()
                                        .ok_or_else(|| {
                                            ServiceError::invalid("template text content missing")
                                        })?;
                                    independent.id = ContentId::from_uuid(derived(
                                        derived(expansion, placement.id.as_uuid()),
                                        node.as_uuid(),
                                    ));
                                    let independent_id = independent.id;
                                    texts.push(independent);
                                    source.nodes.iter_mut().find(|n| n.id == node).unwrap().kind =
                                        NodeKind::Text {
                                            content_ref: independent_id,
                                        };
                                    let t = texts
                                        .iter_mut()
                                        .find(|t| t.id == independent_id)
                                        .ok_or_else(|| {
                                            ServiceError::invalid("template text content missing")
                                        })?;
                                    let Value::String(text) = value else {
                                        return Err(ServiceError::invalid("template text type"));
                                    };
                                    if t.styles.len() > 1 || !t.ruby.is_empty() {
                                        return Err(ServiceError::new(
                                            "UNSUPPORTED_FEATURE",
                                            "template text inputs require one uniform style without ruby",
                                        ));
                                    }
                                    if let Some(style) = t.styles.first_mut() {
                                        style.range = TextRange {
                                            start: 0,
                                            end: text.len(),
                                        };
                                    }
                                    if text.is_empty() {
                                        t.styles.clear();
                                    }
                                    t.text = text;
                                }
                                TemplateInputTarget::MediaSlot { node } => {
                                    kronello_template::validate_asset(original, &value)?;
                                    let Value::AssetRef(asset) = value else {
                                        return Err(ServiceError::invalid("template media type"));
                                    };
                                    let n =
                                        source.nodes.iter_mut().find(|n| n.id == node).ok_or_else(
                                            || ServiceError::invalid("template media node missing"),
                                        )?;
                                    let NodeKind::Media(media) = &mut n.kind else {
                                        return Err(ServiceError::invalid(
                                            "template media target is not media",
                                        ));
                                    };
                                    media.asset = asset;
                                }
                                TemplateInputTarget::DataTable { .. } => {
                                    unreachable!("projected table bindings")
                                }
                            }
                        }
                        let mut values = vec![
                            encode(&source)?,
                            encode(&shapes)?,
                            encode(&texts)?,
                            encode(&repeats)?,
                            encode(&simulations)?,
                        ];
                        let mut refs = BTreeSet::new();
                        for v in &values {
                            scan(v, "", &mut refs, false);
                        }
                        let mut included = BTreeSet::new();
                        let mut curves = Vec::new();
                        let mut expressions = Vec::new();
                        loop {
                            let before = included.len();
                            for c in &work.curves {
                                if let DocumentObject::Known(c) = c
                                    && refs.contains(&c.id().as_uuid())
                                    && included.insert(c.id().as_uuid())
                                {
                                    let v = encode(c)?;
                                    scan(&v, "", &mut refs, false);
                                    values.push(v);
                                    curves.push(c.clone());
                                }
                            }
                            for e in &work.expressions {
                                if let DocumentObject::Known(e) = e
                                    && refs.contains(&e.id.as_uuid())
                                    && included.insert(e.id.as_uuid())
                                {
                                    let v = encode(e)?;
                                    scan(&v, "", &mut refs, false);
                                    values.push(v);
                                    expressions.push(e.clone());
                                }
                            }
                            if before == included.len() {
                                break;
                            }
                        }
                        let mut owned = BTreeSet::new();
                        for v in &values {
                            scan(v, "", &mut owned, true);
                        }
                        let namespace = derived(expansion, placement.id.as_uuid());
                        let map: BTreeMap<_, _> = owned
                            .into_iter()
                            .map(|id| (id, derived(namespace, id)))
                            .collect();
                        for n in &source.nodes {
                            if let NodeKind::CompositionInstance(i) = &n.kind {
                                let new = CompositionInstanceId::from_uuid(map[&i.id.as_uuid()]);
                                aliases.insert(new, *aliases.get(&i.id).unwrap_or(&i.id));
                                if let Some(t) = template_ids.get(&i.id).cloned() {
                                    template_ids.insert(new, t);
                                }
                            }
                        }
                        for r in &repeats {
                            for i in &r.instances {
                                let new = CompositionInstanceId::from_uuid(map[&i.id.as_uuid()]);
                                aliases.insert(new, *aliases.get(&i.id).unwrap_or(&i.id));
                            }
                        }
                        let copied: Composition = copy(&source, &map)?;
                        placement.definition_ref = copied.id;
                        placement.input_bindings = placement
                            .input_bindings
                            .iter()
                            .map(|(p, v)| {
                                Ok((
                                    PropertyId::from_uuid(
                                        *map.get(&p.as_uuid()).unwrap_or(&p.as_uuid()),
                                    ),
                                    copy(v, &map)?,
                                ))
                            })
                            .collect::<Result<_, ServiceError>>()?;
                        constraints.insert(copied.id, copy(&selected.constraints, &map)?);
                        for s in shapes.drain(..) {
                            work.shapes.push(DocumentObject::Known(copy(&s, &map)?));
                        }
                        for t in texts {
                            work.texts.push(DocumentObject::Known(copy(&t, &map)?));
                        }
                        for simulation in simulations {
                            let copied: ParticleSimulation = copy(&simulation, &map)?;
                            simulation_aliases.insert(
                                copied.id,
                                *simulation_aliases
                                    .get(&simulation.id)
                                    .unwrap_or(&simulation.id),
                            );
                            work.simulations.push(DocumentObject::Known(copied));
                        }
                        for r in repeats {
                            work.repeaters.push(DocumentObject::Known(copy(&r, &map)?));
                        }
                        for c in curves {
                            work.curves.push(DocumentObject::Known(copy(&c, &map)?));
                        }
                        for e in expressions {
                            work.expressions
                                .push(DocumentObject::Known(copy(&e, &map)?));
                        }
                        let mattes: Vec<_> = work
                            .mattes
                            .iter()
                            .filter_map(|m| {
                                if let DocumentObject::Known(m) = m {
                                    (m.composition == source.id).then_some(m.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        for m in mattes {
                            let mut local = map.clone();
                            local.insert(m.id, derived(namespace, m.id));
                            work.mattes.push(DocumentObject::Known(copy(&m, &local)?));
                        }
                        work.compositions.push(DocumentObject::Known(copied));
                    }
                    pending.push(placement.definition_ref);
                }
                NodeKind::Simulation { content_ref } => {
                    pending.push(
                        work.simulation(*content_ref)
                            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?
                            .source
                            .composition,
                    );
                }
                NodeKind::Repeater { content_ref } => {
                    let r = work
                        .repeater(*content_ref)
                        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
                    pending.push(r.source.composition);
                    for i in &r.instances {
                        pending.push(i.source(r).composition);
                    }
                }
                _ => (),
            }
        }
        let entry = work
            .compositions
            .iter_mut()
            .find(|c| matches!(c,DocumentObject::Known(c)if c.id==id))
            .unwrap();
        *entry = DocumentObject::Known(owner);
    }
    Ok((work, constraints, aliases, simulation_aliases))
}
