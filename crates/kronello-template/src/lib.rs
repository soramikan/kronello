//! Pure template input, immutable edition, and exact duration contracts.
use kronello_model::*;
use kronello_time::{Duration, ProtectedMiddleMode, Time, TimeMap, TimeMapPoint};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("UNSUPPORTED_FEATURE: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Expression(#[from] kronello_model::ExpressionError),
    #[error("invalid template: {0}")]
    Invalid(String),
    #[error("template definition content changed")]
    DefinitionChanged,
    #[error("invalid table shape, schema, or cell type")]
    InvalidDataTable,
    #[error("missing template asset {0}")]
    AssetMissing(AssetId),
    #[error("unknown template variant {0}")]
    VariantMissing(String),
    #[error("duration is below the protected intervals and minimum middle")]
    DurationTooShort,
    #[error("text {node} has {actual} lines; maximum {maximum}")]
    Overflow {
        node: NodeId,
        actual: usize,
        maximum: usize,
    },
    #[error(transparent)]
    Time(#[from] kronello_time::TimeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl TemplateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Expression(e) => e.code(),
            Self::Unsupported(_) => "UNSUPPORTED_FEATURE",
            Self::DefinitionChanged => "TEMPLATE_DEFINITION_CHANGED",
            Self::DurationTooShort => "DURATION_TOO_SHORT",
            Self::InvalidDataTable => "INVALID_DATA_TABLE",
            Self::AssetMissing(_) => "ASSET_MISSING",
            Self::VariantMissing(_) => "TEMPLATE_VARIANT_NOT_FOUND",
            Self::Overflow { .. } => "TEMPLATE_OVERFLOW",
            _ => "INVALID_TEMPLATE",
        }
    }
}
fn invalid(message: &str) -> TemplateError {
    TemplateError::Invalid(message.into())
}
pub fn duration_map(
    authoring: Duration,
    requested: Duration,
    policy: &TemplateDurationPolicy,
) -> Result<TimeMap, TemplateError> {
    let intro = policy.intro.as_time();
    let outro = policy.outro.as_time();
    let protected = intro.checked_add(outro)?;
    let minimum = protected.checked_add(policy.minimum_middle.as_time())?;
    if authoring.as_time() < minimum
        || requested.as_time() < minimum
        || authoring.as_time() <= protected
        || requested.as_time() <= protected
    {
        return Err(TemplateError::DurationTooShort);
    }
    if policy.middle_mode != TemplateMiddleMode::Stretch {
        return Ok(TimeMap::protected(
            authoring,
            requested,
            policy.intro,
            policy.outro,
            match policy.middle_mode {
                TemplateMiddleMode::Hold => ProtectedMiddleMode::Hold,
                TemplateMiddleMode::Loop => ProtectedMiddleMode::Loop,
                TemplateMiddleMode::Stretch => unreachable!(),
            },
        )?);
    }
    let mut points = vec![TimeMapPoint {
        parent: Time::ZERO,
        local: Time::ZERO,
    }];
    if intro > Time::ZERO {
        points.push(TimeMapPoint {
            parent: intro,
            local: intro,
        });
    }
    points.push(TimeMapPoint {
        parent: requested.as_time().checked_sub(outro)?,
        local: authoring.as_time().checked_sub(outro)?,
    });
    if outro > Time::ZERO {
        points.push(TimeMapPoint {
            parent: requested.as_time(),
            local: authoring.as_time(),
        });
    }
    Ok(TimeMap::piecewise_linear(points)?)
}
pub fn composition(project: &Project, id: CompositionId) -> Result<&Composition, TemplateError> {
    if project
        .compositions
        .iter()
        .any(|c| matches!(c, DocumentObject::Opaque(c) if c.id == id.as_uuid()))
    {
        return Err(TemplateError::Unsupported(format!(
            "opaque composition {id}"
        )));
    }
    project
        .compositions
        .iter()
        .find_map(|c| match c {
            DocumentObject::Known(c) if c.id == id => Some(c),
            _ => None,
        })
        .ok_or_else(|| invalid("composition missing or opaque"))
}
pub fn definition(project: &Project, id: uuid::Uuid) -> Result<&TemplateDefinition, TemplateError> {
    if project
        .templates
        .iter()
        .any(|d| matches!(d, DocumentObject::Opaque(d) if d.id == id))
    {
        return Err(TemplateError::Unsupported(format!("opaque template {id}")));
    }
    project
        .templates
        .iter()
        .find_map(|d| match d {
            DocumentObject::Known(d) if d.id == id => Some(d),
            _ => None,
        })
        .ok_or_else(|| invalid("definition missing or opaque"))
}
/// Includes every reachable node/content/curve. The edition never reads latest content.
pub fn authoring_hash(project: &Project, root: CompositionId) -> Result<String, TemplateError> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut objects = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let c = composition(project, id)?;
        objects.insert(id.as_uuid(), serde_json::to_value(c)?);
        for n in &c.nodes {
            match &n.kind {
                NodeKind::CompositionInstance(i) => {
                    pending.push(i.definition_ref);
                    // Nested template pins and overrides also affect this edition.
                    if project
                        .template_instances
                        .iter()
                        .any(|v| matches!(v, DocumentObject::Opaque(v) if v.id == i.id.as_uuid()))
                    {
                        return Err(TemplateError::Unsupported(format!(
                            "opaque template instance {}",
                            i.id
                        )));
                    }
                    if let Some(instance) =
                        project
                            .template_instances
                            .iter()
                            .find_map(|object| match object {
                                DocumentObject::Known(instance) if instance.id == i.id => {
                                    Some(instance)
                                }
                                _ => None,
                            })
                    {
                        objects.insert(instance.id.as_uuid(), serde_json::to_value(instance)?);
                        let nested = definition(project, instance.definition_ref)?;
                        objects.insert(nested.id, serde_json::to_value(nested)?);
                    }
                    for source in i.input_bindings.values() {
                        add_curve(project, source, &mut objects)?;
                    }
                }
                NodeKind::Text { content_ref } => {
                    if project.texts.iter().any(
                        |t| matches!(t, DocumentObject::Opaque(t) if t.id == content_ref.as_uuid()),
                    ) {
                        return Err(TemplateError::Unsupported(format!(
                            "opaque text {content_ref}"
                        )));
                    }
                    let t = project
                        .texts
                        .iter()
                        .find_map(|t| match t {
                            DocumentObject::Known(t) if t.id == *content_ref => Some(t),
                            _ => None,
                        })
                        .ok_or_else(|| invalid("text missing"))?;
                    objects.insert(content_ref.as_uuid(), serde_json::to_value(t)?);
                }
                NodeKind::Shape { content_ref } => {
                    if project.shapes.iter().any(
                        |s| matches!(s, DocumentObject::Opaque(s) if s.id == content_ref.as_uuid()),
                    ) {
                        return Err(TemplateError::Unsupported(format!(
                            "opaque shape {content_ref}"
                        )));
                    }
                    let s = project
                        .shapes
                        .iter()
                        .find_map(|s| match s {
                            DocumentObject::Known(s) if s.id == *content_ref => Some(s),
                            _ => None,
                        })
                        .ok_or_else(|| invalid("shape missing"))?;
                    objects.insert(content_ref.as_uuid(), serde_json::to_value(s)?);
                }
                _ => (),
            }
        }
        for p in c
            .properties
            .iter()
            .chain(c.nodes.iter().flat_map(|n| &n.properties))
        {
            add_curve(project, p.source(), &mut objects)?;
        }
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&objects)?)
    ))
}
fn add_curve(
    project: &Project,
    source: &PropertySource<Value>,
    objects: &mut BTreeMap<uuid::Uuid, serde_json::Value>,
) -> Result<(), TemplateError> {
    if let PropertySource::Curve(id) = source {
        if project
            .curves
            .iter()
            .any(|c| matches!(c, DocumentObject::Opaque(c) if c.id == id.as_uuid()))
        {
            return Err(TemplateError::Unsupported(format!("opaque curve {id}")));
        }
        let c = project
            .curves
            .iter()
            .find_map(|c| match c {
                DocumentObject::Known(c) if c.id() == *id => Some(c),
                _ => None,
            })
            .ok_or_else(|| invalid("curve missing"))?;
        objects.insert(id.as_uuid(), serde_json::to_value(c)?);
    }
    if let PropertySource::Expression(id) = source {
        if project
            .expressions
            .iter()
            .any(|e| matches!(e, DocumentObject::Opaque(e) if e.id == id.as_uuid()))
        {
            return Err(TemplateError::Unsupported(format!(
                "opaque expression {id}"
            )));
        }
        let e = project
            .expressions
            .iter()
            .find_map(|e| match e {
                DocumentObject::Known(e) if e.id == *id => Some(e),
                _ => None,
            })
            .ok_or_else(|| invalid("expression missing"))?;
        e.validate()?;
        objects.insert(id.as_uuid(), serde_json::to_value(e)?);
        for node in &e.nodes {
            if let kronello_model::ExpressionNode::CurveSample { curve, .. } = node {
                add_curve(project, &PropertySource::Curve(*curve), objects)?;
            }
        }
    }
    Ok(())
}
pub fn validate_input(input: &TemplateInput, value: &Value) -> Result<(), TemplateError> {
    if let Value::DataTable(table) = value {
        validate_table(table)?;
        let Value::DataTable(default) = &input.default else {
            return Err(TemplateError::InvalidDataTable);
        };
        if table.columns != default.columns {
            return Err(TemplateError::InvalidDataTable);
        }
    }
    if value.value_type() != input.value_type {
        return Err(invalid("input type mismatch"));
    }
    if let Value::Scalar(v) = value
        && (input.minimum.is_some_and(|m| v.get() < m.get())
            || input.maximum.is_some_and(|m| v.get() > m.get()))
    {
        return Err(invalid("input outside range"));
    }
    if let Value::Enum(v) = value
        && !input.choices.contains(v)
    {
        return Err(invalid("input outside enum choices"));
    }
    Ok(())
}
pub fn resolved_inputs(
    d: &TemplateDefinition,
    i: &TemplateInstance,
) -> Result<BTreeMap<String, Value>, TemplateError> {
    selected_definition(d, i.variant.as_deref())?;
    if i.definition_ref != d.id || i.version != d.version {
        return Err(invalid("pinned version mismatch"));
    }
    if i.inputs
        .keys()
        .any(|key| !d.public_inputs.contains_key(key))
    {
        return Err(invalid("input is not public"));
    }
    d.public_inputs
        .iter()
        .map(|(name, input)| {
            let v = i.inputs.get(name).unwrap_or(&input.default);
            validate_input(input, v)?;
            if let TemplateInputTarget::DataTable { bindings } = &input.target {
                project_table(bindings, v)?;
            }
            Ok((name.clone(), v.clone()))
        })
        .collect()
}
pub fn validate_definition(project: &Project, d: &TemplateDefinition) -> Result<(), TemplateError> {
    validate_selected_definition(project, d)?;
    for name in d.variants.keys() {
        if name.is_empty() {
            return Err(invalid("empty variant name"));
        }
        validate_selected_definition(project, &selected_definition(d, Some(name))?)?;
    }
    Ok(())
}
fn validate_selected_definition(
    project: &Project,
    d: &TemplateDefinition,
) -> Result<(), TemplateError> {
    if d.version.is_empty() {
        return Err(invalid("empty version"));
    }
    let c = composition(project, d.composition_ref)?;
    duration_map(c.duration, c.duration, &d.duration_policy)?;
    if d.content_hash != authoring_hash(project, d.composition_ref)? {
        return Err(TemplateError::DefinitionChanged);
    }
    let mut targets = BTreeSet::new();
    let mut projected = Vec::new();
    for input in d.public_inputs.values() {
        validate_input(input, &input.default)?;
        if let TemplateInputTarget::DataTable { bindings } = &input.target {
            if input.value_type != ValueType::DataTable
                || bindings.is_empty()
                || input.minimum.is_some()
                || input.maximum.is_some()
                || !input.choices.is_empty()
            {
                return Err(TemplateError::InvalidDataTable);
            }
            for (target, value) in project_table(bindings, &input.default)? {
                projected.push(TemplateInput {
                    value_type: value.value_type(),
                    default: value,
                    target,
                    minimum: None,
                    maximum: None,
                    choices: vec![],
                });
            }
        } else {
            projected.push(input.clone());
        }
    }
    for input in &projected {
        if !matches!(
            input.value_type,
            ValueType::Scalar
                | ValueType::String
                | ValueType::Enum
                | ValueType::Color
                | ValueType::AssetRef
                | ValueType::Bool
        ) {
            return Err(invalid("unsupported public input type"));
        }
        if input.value_type != ValueType::Scalar
            && (input.minimum.is_some() || input.maximum.is_some())
        {
            return Err(invalid("numeric constraints require Number input"));
        }
        validate_input(input, &input.default)?;
        if input.minimum.zip(input.maximum).is_some_and(|(a, b)| a > b) {
            return Err(invalid("reversed input range"));
        }
        match &input.target {
            TemplateInputTarget::Property { node, property } => {
                let (node, property) = (*node, *property);
                let n = c
                    .nodes
                    .iter()
                    .find(|n| n.id == node)
                    .ok_or_else(|| invalid("input node missing"))?;
                let p = n
                    .properties
                    .iter()
                    .find(|p| p.id() == property)
                    .ok_or_else(|| invalid("input property missing"))?;
                let mut registry = SchemaRegistry::with_builtin();
                for descriptor in shape_descriptors().into_iter().chain(text_descriptors()) {
                    registry
                        .register(descriptor)
                        .map_err(|_| invalid("descriptor registration"))?;
                }
                p.validate_final_value(&input.default, &registry)
                    .map_err(|_| invalid("input descriptor mismatch"))?;
                if !targets.insert((node, Some(property))) {
                    return Err(invalid("duplicate input target"));
                }
            }
            TemplateInputTarget::Text { node } => {
                let node = *node;
                if input.value_type != ValueType::String
                    || !c
                        .nodes
                        .iter()
                        .any(|n| n.id == node && matches!(n.kind, NodeKind::Text { .. }))
                {
                    return Err(invalid("invalid text target"));
                }
                let authored = c.nodes.iter().find(|n| n.id == node).unwrap();
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
                    .ok_or_else(|| invalid("text input content missing"))?;
                if text.styles.len() != 1
                    || !text.ruby.is_empty()
                    || text.direction != TextDirection::Horizontal
                {
                    return Err(invalid(
                        "text inputs require one uniform horizontal style without ruby",
                    ));
                }
                if !targets.insert((node, None)) {
                    return Err(invalid("duplicate input target"));
                }
            }
            TemplateInputTarget::MediaSlot { node } => {
                if input.value_type != ValueType::AssetRef
                    || !c
                        .nodes
                        .iter()
                        .any(|n| n.id == *node && n.kind == NodeKind::Null)
                {
                    return Err(invalid(
                        "MediaSlot requires an AssetRef and explicit Null slot",
                    ));
                }
                validate_asset(project, &input.default)?;
                if !targets.insert((*node, None)) {
                    return Err(invalid("duplicate input target"));
                }
            }
            TemplateInputTarget::DataTable { .. } => unreachable!("projected above"),
        }
    }
    let mut bound = BTreeSet::new();
    for b in &d.constraints.bands {
        if b.padding.iter().any(|v| v.get() < 0.0) {
            return Err(invalid("negative padding"));
        }
        let text = c
            .nodes
            .iter()
            .find(|n| n.id == b.text_node && matches!(n.kind, NodeKind::Text { .. }))
            .ok_or_else(|| invalid("text node missing"))?;
        if !text.child_order.is_empty() {
            return Err(TemplateError::Unsupported(
                "bounds followers require leaf text nodes".into(),
            ));
        }
        let band = c
            .nodes
            .iter()
            .find(|n| n.id == b.band_node && matches!(n.kind, NodeKind::Shape { .. }))
            .ok_or_else(|| invalid("band node missing"))?;
        if band.properties.iter().any(|p| {
            p.descriptor()
                .key
                .as_str()
                .starts_with("kronello.transform.")
                && p.descriptor().key.as_str() != "kronello.transform.position"
        }) {
            return Err(invalid("band transform supports position only"));
        }
        let NodeKind::Shape { content_ref } = band.kind else {
            unreachable!()
        };
        let shape = project
            .shapes
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) if s.id == content_ref => Some(s),
                _ => None,
            })
            .ok_or_else(|| invalid("band shape missing"))?;
        if !matches!(shape.geometry, ShapeGeometry::Rectangle {size, ..} if size == b.size_property)
        {
            return Err(invalid("band must bind its rectangle size"));
        }
        if text.active_range.intersection(band.active_range) != Some(band.active_range) {
            return Err(invalid("band active range must be covered by text"));
        }
        if text.containment_parent != band.containment_parent
            || text.transform_parent != band.transform_parent
        {
            return Err(invalid("band and text must share parent space"));
        }
        for (id, key) in [
            (b.size_property, "kronello.shape.size"),
            (b.position_property, "kronello.transform.position"),
        ] {
            if !band
                .properties
                .iter()
                .any(|p| p.id() == id && p.descriptor().key.as_str() == key)
                || !bound.insert(id)
                || targets.contains(&(b.band_node, Some(id)))
            {
                return Err(invalid("invalid or conflicting band target"));
            }
        }
    }
    for (node, max) in &d.constraints.max_lines {
        if *max == 0
            || !c
                .nodes
                .iter()
                .any(|n| n.id == *node && matches!(n.kind, NodeKind::Text { .. }))
        {
            return Err(invalid("invalid max_lines target"));
        }
    }
    Ok(())
}
pub fn validate_project(project: &Project) -> Result<(), TemplateError> {
    validate_project_with_opaque(project, false)
}

/// Storage preserves unsupported objects; executable known contracts still validate.
pub fn validate_stored_project(project: &Project) -> Result<(), TemplateError> {
    validate_project_with_opaque(project, true)
}

fn validate_project_with_opaque(
    project: &Project,
    preserve_opaque: bool,
) -> Result<(), TemplateError> {
    let mut ids = BTreeSet::new();
    let mut versions = BTreeSet::new();
    for d in &project.templates {
        let DocumentObject::Known(d) = d else {
            if preserve_opaque {
                continue;
            }
            return Err(TemplateError::Unsupported("opaque template".into()));
        };
        if !ids.insert(d.id) || !versions.insert((d.template_id, d.version.clone())) {
            return Err(invalid("duplicate definition edition"));
        }
        match validate_definition(project, d) {
            Err(TemplateError::Unsupported(_)) if preserve_opaque => (),
            result => result?,
        }
    }
    let mut instances = BTreeSet::new();
    for i in &project.template_instances {
        let DocumentObject::Known(i) = i else {
            if preserve_opaque {
                continue;
            }
            return Err(TemplateError::Unsupported(
                "opaque template instance".into(),
            ));
        };
        if !instances.insert(i.id) {
            return Err(invalid("duplicate template instance"));
        }
        match validate_instance(project, i) {
            Err(TemplateError::Unsupported(_)) if preserve_opaque => (),
            result => result?,
        }
    }
    Ok(())
}

pub fn validate_instance(project: &Project, i: &TemplateInstance) -> Result<(), TemplateError> {
    let edition = definition(project, i.definition_ref)?;
    validate_definition(project, edition)?;
    let selected = selected_definition(edition, i.variant.as_deref())?;
    let d = &selected;
    validate_definition(project, d)?;
    let values = resolved_inputs(edition, i)?;
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors().into_iter().chain(text_descriptors()) {
        registry
            .register(descriptor)
            .expect("distinct built-in descriptors");
    }
    for (target, value) in input_bindings(d, &values)? {
        if let TemplateInputTarget::MediaSlot { .. } = target {
            validate_asset(project, &value)?;
        }
        if let TemplateInputTarget::Property { node, property } = target {
            let authored = composition(project, d.composition_ref)?
                .nodes
                .iter()
                .find(|n| n.id == node)
                .unwrap();
            let target = authored
                .properties
                .iter()
                .find(|p| p.id() == property)
                .unwrap();
            target
                .validate_final_value(&value, &registry)
                .map_err(|_| invalid("input violates target Property constraints"))?;
        }
    }
    let expected = duration_map(
        composition(project, d.composition_ref)?.duration,
        i.duration,
        &d.duration_policy,
    )?;
    let placements: Vec<_> = project
        .compositions
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| &c.nodes)
        .filter_map(|n| match &n.kind {
            NodeKind::CompositionInstance(p) if p.id == i.id => Some((n, p)),
            _ => None,
        })
        .collect();
    if placements.len() != 1
        || placements[0].1.definition_ref != d.composition_ref
        || !placements[0].1.input_bindings.is_empty()
        || placements[0].1.local_time_map != expected
        || placements[0].0.active_range.start() != Time::ZERO
        || placements[0].0.active_range.duration()? != i.duration
    {
        return Err(invalid("template placement differs from pin"));
    }
    Ok(())
}

/// Only the selected Composition closure determines executable template features.
pub fn validate_reachable(project: &Project, root: CompositionId) -> Result<(), TemplateError> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut placements = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        for node in &composition(project, id)?.nodes {
            if let NodeKind::CompositionInstance(i) = &node.kind {
                placements.insert(i.id.as_uuid());
                pending.push(i.definition_ref);
            }
        }
    }
    let mut selected = project.clone();
    selected.template_instances.retain(|object| {
        let id = match object {
            DocumentObject::Known(i) => i.id.as_uuid(),
            DocumentObject::Opaque(i) => i.id,
        };
        placements.contains(&id)
    });
    let definitions: BTreeSet<_> = selected
        .template_instances
        .iter()
        .filter_map(|object| match object {
            DocumentObject::Known(i) => Some(i.definition_ref),
            DocumentObject::Opaque(_) => None,
        })
        .collect();
    selected.templates.retain(|object| {
        let id = match object {
            DocumentObject::Known(d) => d.id,
            DocumentObject::Opaque(d) => d.id,
        };
        definitions.contains(&id)
    });
    validate_project(&selected)
}
pub fn check_lines(node: NodeId, actual: usize, maximum: usize) -> Result<(), TemplateError> {
    if actual > maximum {
        Err(TemplateError::Overflow {
            node,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}
/// Semantic layout input supplied by the upper compiler after text layout.
pub fn band_values(
    min: [f64; 2],
    max: [f64; 2],
    position: [f64; 2],
    padding: [FiniteF64; 2],
) -> Result<([FiniteF64; 2], [FiniteF64; 2]), TemplateError> {
    let mut size = [FiniteF64::new(0.0).unwrap(); 2];
    let mut origin = size;
    for axis in 0..2 {
        if max[axis] < min[axis] {
            return Err(invalid("reversed bounds"));
        }
        size[axis] = FiniteF64::new(max[axis] - min[axis] + 2.0 * padding[axis].get())
            .map_err(|_| invalid("nonfinite bounds"))?;
        origin[axis] = FiniteF64::new(position[axis] + min[axis] - padding[axis].get())
            .map_err(|_| invalid("nonfinite bounds"))?;
    }
    Ok((size, origin))
}

/// Shared imports and edits cannot republish an existing edition or migrate a
/// placement implicitly. Edition removal is allowed once references are gone.
pub fn validate_transition(old: &Project, new: &Project) -> Result<(), TemplateError> {
    validate_pins(old, new, &BTreeSet::new())?;
    validate_project(new)
}

pub fn validate_stored_transition(old: &Project, new: &Project) -> Result<(), TemplateError> {
    validate_pins(old, new, &BTreeSet::new())?;
    validate_stored_project(new)
}

/// Only service commands identifying an explicit migration may change these pins.
pub fn validate_migration_transition(
    old: &Project,
    new: &Project,
    migrations: &BTreeSet<CompositionInstanceId>,
) -> Result<(), TemplateError> {
    validate_pins(old, new, migrations)?;
    validate_project(new)
}
fn validate_pins(
    old: &Project,
    new: &Project,
    migrations: &BTreeSet<CompositionInstanceId>,
) -> Result<(), TemplateError> {
    for object in &old.templates {
        let id = match object {
            DocumentObject::Known(d) => d.id,
            DocumentObject::Opaque(d) => d.id,
        };
        if let Some(next) = new.templates.iter().find(|v| match v {
            DocumentObject::Known(v) => v.id == id,
            DocumentObject::Opaque(v) => v.id == id,
        }) && next != object
        {
            return Err(TemplateError::DefinitionChanged);
        }
        if let DocumentObject::Known(d) = object
            && new
                .templates
                .iter()
                .any(|v| matches!(v, DocumentObject::Known(v) if v.id == d.id))
            && std::iter::once((d.composition_ref, &d.content_hash))
                .chain(
                    d.variants
                        .values()
                        .map(|v| (v.composition_ref, &v.content_hash)),
                )
                .any(|(composition, hash)| {
                    authoring_hash(old, composition).is_ok()
                        && authoring_hash(new, composition).ok().as_ref() != Some(hash)
                })
        {
            return Err(TemplateError::DefinitionChanged);
        }
    }
    for object in &old.template_instances {
        let id = match object {
            DocumentObject::Known(i) => i.id.as_uuid(),
            DocumentObject::Opaque(i) => i.id,
        };
        if let Some(next) = new.template_instances.iter().find(|v| match v {
            DocumentObject::Known(v) => v.id.as_uuid() == id,
            DocumentObject::Opaque(v) => v.id == id,
        }) && (matches!(object, DocumentObject::Opaque(_))
            || matches!(next, DocumentObject::Opaque(_)))
            && next != object
        {
            return Err(TemplateError::Unsupported("opaque instance edits".into()));
        }
        if let DocumentObject::Known(i) = object
            && let Some(next) = new.template_instances.iter().find_map(|v| match v {
                DocumentObject::Known(v) if v.id == i.id => Some(v),
                _ => None,
            })
            && (next.definition_ref != i.definition_ref
                || next.version != i.version
                || next.variant != i.variant)
            && !migrations.contains(&i.id)
        {
            return Err(invalid("implicit edition migration is unsupported"));
        }
    }
    Ok(())
}

/// Select explicit authoring content; resolution changes never choose a variant.
pub fn selected_definition(
    d: &TemplateDefinition,
    variant: Option<&str>,
) -> Result<TemplateDefinition, TemplateError> {
    let mut selected = d.clone();
    if let Some(name) = variant {
        let v = d
            .variants
            .get(name)
            .ok_or_else(|| TemplateError::VariantMissing(name.into()))?;
        if v.targets.keys().ne(d.public_inputs.keys()) {
            return Err(invalid("variant must bind every public input"));
        }
        selected.composition_ref = v.composition_ref;
        selected.constraints = v.constraints.clone();
        selected.content_hash = v.content_hash.clone();
        for (name, input) in &mut selected.public_inputs {
            input.target = v.targets[name].clone();
        }
    }
    selected.variants.clear();
    Ok(selected)
}
pub fn validate_table(table: &DataTable) -> Result<(), TemplateError> {
    if table.columns.is_empty()
        || table.columns.len() > 128
        || table.rows.len() > 10000
        || table.rows.len().saturating_mul(table.columns.len()) > 100000
        || table.columns.iter().any(|(name, ty)| {
            name.is_empty()
                || !matches!(
                    ty,
                    ValueType::String | ValueType::Scalar | ValueType::Color | ValueType::Bool
                )
        })
        || table.rows.iter().any(|row| {
            row.keys().ne(table.columns.keys())
                || row
                    .iter()
                    .any(|(name, value)| value.value_type() != table.columns[name])
        })
    {
        return Err(TemplateError::InvalidDataTable);
    }
    Ok(())
}
fn project_table(
    bindings: &[TemplateDataBinding],
    value: &Value,
) -> Result<Vec<(TemplateInputTarget, Value)>, TemplateError> {
    let Value::DataTable(table) = value else {
        return Err(TemplateError::InvalidDataTable);
    };
    validate_table(table)?;
    bindings
        .iter()
        .map(|binding| {
            let value = table
                .rows
                .get(binding.row)
                .and_then(|row| row.get(&binding.column))
                .ok_or(TemplateError::InvalidDataTable)?;
            let target = match binding.target {
                TemplateCellTarget::Property { node, property } => {
                    TemplateInputTarget::Property { node, property }
                }
                TemplateCellTarget::Text { node } => TemplateInputTarget::Text { node },
            };
            Ok((target, value.clone()))
        })
        .collect()
}
/// Flatten data projections into the same explicit Text/Property/Media bindings.
pub fn input_bindings(
    d: &TemplateDefinition,
    values: &BTreeMap<String, Value>,
) -> Result<Vec<(TemplateInputTarget, Value)>, TemplateError> {
    let mut result = Vec::new();
    for (name, input) in &d.public_inputs {
        let value = values.get(name).ok_or_else(|| invalid("input missing"))?;
        validate_input(input, value)?;
        match &input.target {
            TemplateInputTarget::DataTable { bindings } => {
                result.extend(project_table(bindings, value)?)
            }
            target => result.push((target.clone(), value.clone())),
        }
    }
    Ok(result)
}
pub fn validate_asset(project: &Project, value: &Value) -> Result<(), TemplateError> {
    let Value::AssetRef(id) = value else {
        return Err(invalid("MediaSlot requires AssetRef"));
    };
    if !project
        .assets
        .iter()
        .any(|asset| matches!(asset, DocumentObject::Known(asset) if asset.id == *id))
    {
        return Err(TemplateError::AssetMissing(*id));
    }
    Ok(())
}
