use kronello_model::*;
use kronello_time::{Duration, Time};
use serde_json::json;

fn fixture() -> Project {
    let mut project: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    project
        .template_instances
        .push(DocumentObject::Known(TemplateInstance {
            id: CompositionInstanceId::new(),
            definition_ref: definition.id,
            version: definition.version.clone(),
            duration: Duration::new(Time::from_integer(8)).unwrap(),
            inputs: std::collections::BTreeMap::from([(
                "headline".into(),
                Value::String("別の日本語".into()),
            )]),
        }));
    project.templates.push(DocumentObject::Known(definition));
    project
}

#[test]
fn template_editions_and_separate_pinned_inputs_round_trip() {
    let project = fixture();
    project.validate_storage().unwrap();
    let encoded = serde_json::to_value(&project).unwrap();
    assert_eq!(serde_json::from_value::<Project>(encoded).unwrap(), project);
    let DocumentObject::Known(definition) = &project.templates[0] else {
        panic!()
    };
    let DocumentObject::Known(instance) = &project.template_instances[0] else {
        panic!()
    };
    assert_eq!(instance.definition_ref, definition.id);
    assert_eq!(instance.version, definition.version);
    assert_ne!(
        instance.inputs["headline"],
        definition.public_inputs["headline"].default
    );
}

#[test]
fn bounds_selection_is_explicit_and_legacy_layout_default_round_trips() {
    let project = fixture();
    let original = serde_json::to_value(&project).unwrap();
    assert!(
        original["templates"][0]["constraints"]["bands"][0]
            .get("bounds")
            .is_none()
    );
    let DocumentObject::Known(d) = &project.templates[0] else {
        panic!()
    };
    assert_eq!(d.constraints.bands[0].bounds, BoundsStage::Layout);
    for stage in ["ink", "visual"] {
        let mut encoded = original.clone();
        encoded["templates"][0]["constraints"]["bands"][0]["bounds"] = json!(stage);
        let decoded: Project = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }
    let mut future = original;
    future["templates"][0]["constraints"]["bands"][0]["bounds"] = json!("unknown_stage");
    let decoded: Project = serde_json::from_value(future.clone()).unwrap();
    assert!(matches!(decoded.templates[0], DocumentObject::Opaque(_)));
    assert_eq!(serde_json::to_value(decoded).unwrap(), future);
}

#[test]
fn legacy_documents_omit_template_collections_and_future_contracts_remain_opaque() {
    let legacy = Project::default();
    let encoded = serde_json::to_value(&legacy).unwrap();
    assert!(encoded.get("templates").is_none());
    assert!(encoded.get("template_instances").is_none());
    assert_eq!(serde_json::from_value::<Project>(encoded).unwrap(), legacy);
    for (collection, field, value) in [
        ("templates", "future_constraint", json!({"minimum": 3})),
        ("template_instances", "future_input_mode", json!("dynamic")),
    ] {
        let mut encoded = serde_json::to_value(fixture()).unwrap();
        encoded[collection][0][field] = value;
        let future: Project = serde_json::from_value(encoded.clone()).unwrap();
        future.validate_storage().unwrap();
        assert!(future.ensure_editable().is_err());
        assert_eq!(serde_json::to_value(future).unwrap(), encoded);
    }
    let mut encoded = serde_json::to_value(fixture()).unwrap();
    encoded["templates"][0]["duration_policy"]["middle_mode"] = json!("loop");
    let future: Project = serde_json::from_value(encoded.clone()).unwrap();
    assert!(matches!(future.templates[0], DocumentObject::Opaque(_)));
    assert_eq!(serde_json::to_value(future).unwrap(), encoded);
}

#[test]
fn template_identity_collisions_and_shadowed_collections_are_rejected() {
    let mut project = fixture();
    project.templates.push(project.templates[0].clone());
    assert!(project.validate_storage().is_err());
    let mut project = fixture();
    let DocumentObject::Known(instance) = &mut project.template_instances[0] else {
        panic!()
    };
    instance.id = CompositionInstanceId::from_uuid(project.id);
    assert!(project.validate_storage().is_err());
    let mut project = Project::default();
    project.unknown_fields.insert("templates".into(), json!([]));
    assert!(project.validate_storage().is_err());
}

#[test]
fn templates_instances_and_assets_share_identity_validation_and_round_trip() {
    let mut project = fixture();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: "a".repeat(64),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("movie.mp4".into()),
            absolute: None,
        },
    };
    project.assets.push(DocumentObject::Known(asset));
    project.validate_storage().unwrap();
    let encoded = serde_json::to_value(&project).unwrap();
    assert_eq!(serde_json::from_value::<Project>(encoded).unwrap(), project);
    let DocumentObject::Known(instance) = &project.template_instances[0] else {
        panic!()
    };
    let DocumentObject::Known(definition) = &project.templates[0] else {
        panic!()
    };
    for id in [project.id, instance.id.as_uuid(), definition.id] {
        let mut duplicate = project.clone();
        let DocumentObject::Known(asset) = &mut duplicate.assets[0] else {
            panic!()
        };
        asset.id = AssetId::from_uuid(id);
        assert!(duplicate.validate_storage().is_err());
    }
    for collection in ["templates", "template_instances", "assets"] {
        let mut shadow = project.clone();
        shadow.unknown_fields.insert(collection.into(), json!([]));
        assert!(shadow.validate_storage().is_err());
    }
    let mut shadow = project.clone();
    shadow.assets[0] = DocumentObject::Opaque(OpaqueObject {
        id: uuid::Uuid::new_v4(),
        fields: [("id".into(), json!(uuid::Uuid::new_v4()))].into(),
    });
    assert!(shadow.validate_storage().is_err());
    let legacy = serde_json::to_value(Project::default()).unwrap();
    assert!(legacy.get("assets").is_none());
}
