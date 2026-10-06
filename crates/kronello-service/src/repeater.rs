//! Explicit, transactional materialization of one shared repeater source.
use crate::ServiceError;
mod materialize;
use kronello_model::{
    CompositionId, CompositionInstanceId, ContentId, DocumentObject, ExpandedRepeatSource,
    NodeKind, Project, RepeatSource,
};
use kronello_store::ChangedKey;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn derived(expansion: Uuid, original: Uuid) -> Uuid {
    let mut h = Sha256::new();
    h.update(b"kronello.repeater.expand.v1");
    h.update(expansion.as_bytes());
    h.update(original.as_bytes());
    let bytes = h.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[..16]);
    id[6] = (id[6] & 15) | 128;
    id[8] = (id[8] & 63) | 128;
    Uuid::from_bytes(id)
}
fn scan(value: &Value, key: &str, ids: &mut BTreeSet<Uuid>, owned_only: bool) {
    match value {
        Value::String(s) if !owned_only || matches!(key, "id" | "placement") => {
            if let Ok(id) = Uuid::parse_str(s) {
                ids.insert(id);
            }
        }
        Value::Array(a) => {
            for v in a {
                scan(v, key, ids, owned_only);
            }
        }
        Value::Object(o) => {
            // Literal strings remain authored data, even if they look like UUIDs.
            if o.get("kind")
                .or_else(|| o.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "string" | "enum"))
            {
                return;
            }
            for (k, v) in o {
                if !matches!(
                    k.as_str(),
                    "text" | "name" | "family" | "postscript_name" | "tags" | "path" | "locator"
                ) {
                    scan(v, k, ids, owned_only);
                }
            }
        }
        _ => (),
    }
}
fn remap(value: &mut Value, ids: &BTreeMap<Uuid, Uuid>) {
    match value {
        Value::String(s) => {
            if let Ok(id) = Uuid::parse_str(s)
                && let Some(new) = ids.get(&id)
            {
                *s = new.to_string();
            }
        }
        Value::Array(a) => {
            for v in a {
                remap(v, ids);
            }
        }
        Value::Object(o) => {
            if o.get("kind")
                .or_else(|| o.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "string" | "enum"))
            {
                return;
            }
            let old = std::mem::take(o);
            for (key, mut v) in old {
                if !matches!(
                    key.as_str(),
                    "text" | "name" | "family" | "postscript_name" | "tags" | "path" | "locator"
                ) {
                    remap(&mut v, ids);
                }
                let key = Uuid::parse_str(&key)
                    .ok()
                    .and_then(|id| ids.get(&id))
                    .map_or(key, ToString::to_string);
                o.insert(key, v);
            }
        }
        _ => (),
    }
}
fn copy<T: Serialize + DeserializeOwned>(
    v: &T,
    map: &BTreeMap<Uuid, Uuid>,
) -> Result<T, ServiceError> {
    let mut value = serde_json::to_value(v).map_err(|e| ServiceError::invalid(e.to_string()))?;
    remap(&mut value, map);
    serde_json::from_value(value).map_err(|e| ServiceError::invalid(e.to_string()))
}
pub(crate) fn expand(
    project: &mut Project,
    repeater: ContentId,
    instance: CompositionInstanceId,
    expansion: Uuid,
    keys: &mut BTreeSet<ChangedKey>,
) -> Result<(), ServiceError> {
    project
        .validate_repeaters()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let r = project
        .repeater(repeater)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let i = r
        .instances
        .iter()
        .find(|i| i.id == instance)
        .ok_or_else(|| ServiceError::new("REPEATER_MISSING", "instance not found"))?;
    if i.expanded_source.is_some() {
        return Err(ServiceError::new(
            "REPEATER_ALREADY_EXPANDED",
            "instance is already independent",
        ));
    }
    if expansion.is_nil() {
        return Err(ServiceError::invalid("expansion identity must not be nil"));
    }
    let source = r.source.clone();
    let (source_project, layout_constraints, materialized_aliases, materialized_simulation_aliases) =
        materialize::templates(project, source.composition, expansion)?;
    let mut old_simulation_aliases = project
        .simulation_context()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    old_simulation_aliases.extend(materialized_simulation_aliases);
    let mut pending = vec![source.composition];
    let mut comp_ids = BTreeSet::new();
    let mut repeat_ids = BTreeSet::new();
    let mut simulation_ids = BTreeSet::new();
    let mut content_ids = BTreeSet::new();
    let mut aliases = BTreeMap::new();
    let (_, mut old_aliases) = source_project.repeater_context();
    old_aliases.extend(materialized_aliases);
    while let Some(id) = pending.pop() {
        if !comp_ids.insert(id) {
            continue;
        }
        if comp_ids.len() > 1024 {
            return Err(ServiceError::new(
                "REPEATER_BUDGET",
                "expansion composition budget exceeded",
            ));
        }
        let c = source_project
            .compositions
            .iter()
            .find_map(|c| {
                if let DocumentObject::Known(c) = c {
                    (c.id == id).then_some(c)
                } else {
                    None
                }
            })
            .ok_or_else(|| ServiceError::new("REPEATER_MISSING", "source composition missing"))?;
        for n in &c.nodes {
            match &n.kind {
                NodeKind::CompositionInstance(i) => {
                    pending.push(i.definition_ref);
                    aliases.insert(
                        CompositionInstanceId::from_uuid(derived(expansion, i.id.as_uuid())),
                        *old_aliases.get(&i.id).unwrap_or(&i.id),
                    );
                }
                NodeKind::Shape { content_ref } | NodeKind::Text { content_ref } => {
                    content_ids.insert(*content_ref);
                }
                NodeKind::Simulation { content_ref } => {
                    simulation_ids.insert(*content_ref);
                    pending.push(
                        source_project
                            .simulation(*content_ref)
                            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?
                            .source
                            .composition,
                    );
                }
                NodeKind::Repeater { content_ref } => {
                    repeat_ids.insert(*content_ref);
                    let r = source_project
                        .repeater(*content_ref)
                        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
                    pending.push(r.source.composition);
                    for i in &r.instances {
                        pending.push(i.source(r).composition);
                        aliases.insert(
                            CompositionInstanceId::from_uuid(derived(expansion, i.id.as_uuid())),
                            *old_aliases.get(&i.id).unwrap_or(&i.id),
                        );
                    }
                }
                _ => (),
            }
        }
    }
    let comps: Vec<_> = source_project
        .compositions
        .iter()
        .filter_map(|o| {
            if let DocumentObject::Known(c) = o {
                comp_ids.contains(&c.id).then_some(c.clone())
            } else {
                None
            }
        })
        .collect();
    let simulations: Vec<_> = source_project
        .simulations
        .iter()
        .filter_map(|o| match o {
            DocumentObject::Known(s) if simulation_ids.contains(&s.id) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let repeats: Vec<_> = source_project
        .repeaters
        .iter()
        .filter_map(|o| {
            if let DocumentObject::Known(r) = o {
                repeat_ids.contains(&r.id).then_some(r.clone())
            } else {
                None
            }
        })
        .collect();
    let shapes: Vec<_> = source_project
        .shapes
        .iter()
        .filter_map(|o| {
            if let DocumentObject::Known(s) = o {
                content_ids.contains(&s.id).then_some(s.clone())
            } else {
                None
            }
        })
        .collect();
    let texts: Vec<_> = source_project
        .texts
        .iter()
        .filter_map(|o| {
            if let DocumentObject::Known(t) = o {
                content_ids.contains(&t.id).then_some(t.clone())
            } else {
                None
            }
        })
        .collect();
    let mattes: Vec<_> = source_project
        .mattes
        .iter()
        .filter_map(|o| {
            if let DocumentObject::Known(m) = o {
                comp_ids.contains(&m.composition).then_some(m.clone())
            } else {
                None
            }
        })
        .collect();
    let mut values = vec![
        serde_json::to_value(&comps),
        serde_json::to_value(&repeats),
        serde_json::to_value(&simulations),
        serde_json::to_value(&shapes),
        serde_json::to_value(&texts),
        serde_json::to_value(&mattes),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let mut refs = BTreeSet::new();
    for v in &values {
        scan(v, "", &mut refs, false);
    }
    let mut curves = Vec::new();
    let mut expressions = Vec::new();
    let mut included = BTreeSet::new();
    loop {
        let before = included.len();
        for o in &source_project.curves {
            if let DocumentObject::Known(c) = o
                && refs.contains(&c.id().as_uuid())
                && included.insert(c.id().as_uuid())
            {
                let v =
                    serde_json::to_value(c).map_err(|e| ServiceError::invalid(e.to_string()))?;
                scan(&v, "", &mut refs, false);
                values.push(v);
                curves.push(c.clone());
            }
        }
        for o in &source_project.expressions {
            if let DocumentObject::Known(e) = o
                && refs.contains(&e.id.as_uuid())
                && included.insert(e.id.as_uuid())
            {
                let v =
                    serde_json::to_value(e).map_err(|e| ServiceError::invalid(e.to_string()))?;
                scan(&v, "", &mut refs, false);
                values.push(v);
                expressions.push(e.clone());
            }
        }
        if included.len() == before {
            break;
        }
    }
    let mut owned = BTreeSet::new();
    for v in &values {
        scan(v, "", &mut owned, true);
    }
    let ids: BTreeMap<_, _> = owned
        .into_iter()
        .map(|id| (id, derived(expansion, id)))
        .collect();
    for (old, new) in &ids {
        keys.insert(ChangedKey::Structure {
            object_id: *new,
            parent_container_id: repeater.as_uuid(),
        });
        keys.insert(ChangedKey::Structure {
            object_id: *old,
            parent_container_id: repeater.as_uuid(),
        });
    }
    for c in comps {
        project
            .compositions
            .push(DocumentObject::Known(copy(&c, &ids)?));
    }
    for simulation in &simulations {
        project
            .simulations
            .push(DocumentObject::Known(copy(simulation, &ids)?));
    }
    for r in repeats {
        let mut copied: kronello_model::Repeater = copy(&r, &ids)?;
        for (old, new) in r.instances.iter().zip(&mut copied.instances) {
            if let (Some(old), Some(new)) = (&old.expanded_source, &mut new.expanded_source) {
                new.noise_aliases = old
                    .noise_aliases
                    .iter()
                    .map(|(key, target)| {
                        (
                            CompositionInstanceId::from_uuid(
                                *ids.get(&key.as_uuid()).unwrap_or(&key.as_uuid()),
                            ),
                            old_aliases.get(target).copied().unwrap_or(*target),
                        )
                    })
                    .collect();
                new.simulation_aliases = old
                    .simulation_aliases
                    .iter()
                    .map(|(key, target)| {
                        (
                            ContentId::from_uuid(
                                *ids.get(&key.as_uuid()).unwrap_or(&key.as_uuid()),
                            ),
                            old_simulation_aliases
                                .get(target)
                                .copied()
                                .unwrap_or(*target),
                        )
                    })
                    .collect();
            }
        }
        project.repeaters.push(DocumentObject::Known(copied));
    }
    for s in shapes {
        project.shapes.push(DocumentObject::Known(copy(&s, &ids)?));
    }
    for t in texts {
        project.texts.push(DocumentObject::Known(copy(&t, &ids)?));
    }
    for m in mattes {
        project.mattes.push(DocumentObject::Known(copy(&m, &ids)?));
    }
    for c in curves {
        project.curves.push(DocumentObject::Known(copy(&c, &ids)?));
    }
    for e in expressions {
        project
            .expressions
            .push(DocumentObject::Known(copy(&e, &ids)?));
    }
    let new_source = RepeatSource {
        composition: CompositionId::from_uuid(ids[&source.composition.as_uuid()]),
        root: kronello_model::NodeId::from_uuid(ids[&source.root.as_uuid()]),
    };
    let r = project
        .repeaters
        .iter_mut()
        .find_map(|r| {
            if let DocumentObject::Known(r) = r {
                (r.id == repeater).then_some(r)
            } else {
                None
            }
        })
        .expect("validated repeater");
    let i = r
        .instances
        .iter_mut()
        .find(|i| i.id == instance)
        .expect("validated instance");
    // Source inputs now address the copied owned public input properties.
    let old_bindings = std::mem::take(&mut i.input_bindings);
    i.input_bindings = old_bindings
        .into_iter()
        .map(|(id, v)| {
            (
                kronello_model::PropertyId::from_uuid(
                    *ids.get(&id.as_uuid()).unwrap_or(&id.as_uuid()),
                ),
                v,
            )
        })
        .collect();
    i.expanded_source = Some(ExpandedRepeatSource {
        source: new_source,
        noise_aliases: aliases,
        simulation_aliases: simulations
            .iter()
            .map(|s| {
                (
                    ContentId::from_uuid(ids[&s.id.as_uuid()]),
                    old_simulation_aliases.get(&s.id).copied().unwrap_or(s.id),
                )
            })
            .collect(),
        layout_constraints: copy(&layout_constraints, &ids)?,
    });
    keys.insert(ChangedKey::Structure {
        object_id: repeater.as_uuid(),
        parent_container_id: repeater.as_uuid(),
    });
    Ok(())
}
