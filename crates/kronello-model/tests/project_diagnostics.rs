use kronello_model::{DocumentObject, Project, ProjectError};
use serde_json::{Value, json};
use uuid::Uuid;

fn raw_object(collection: &str, object: Value) -> Project {
    let mut raw = serde_json::to_value(Project::default()).unwrap();
    raw[collection] = json!([object]);
    serde_json::from_str(&raw.to_string()).unwrap()
}

#[test]
fn known_invalid_objects_have_typed_storage_diagnostics() {
    let id = Uuid::new_v4();
    for (collection, object, reason) in [
        (
            "curves",
            json!({"id":id, "interpolation_version":1, "value_type":"scalar", "keys":[
                {"time":{"num":"0","den":"1"}, "value":{"kind":"scalar","value":0},
                 "interpolation":{"kind":"cubic","value":{"control1":[1.1,0],"control2":[0.35,1]}}}
            ]}),
            "time handles",
        ),
        (
            "shapes",
            json!({"id":id,"geometry":{"kind":"ellipse","value":{}},"fill":null,"stroke":null}),
            "size",
        ),
        (
            "texts",
            json!({"id":id,"layout_version":1,"text":42}),
            "string",
        ),
        (
            "shapes",
            json!({"id":id,"geometry":{"kind":42,"value":{}},"fill":null,"stroke":null}),
            "expected value",
        ),
    ] {
        let project = raw_object(collection, object);
        let ProjectError::InvalidObject(diagnostic) = project.validate_storage().unwrap_err()
        else {
            panic!("expected object diagnostic for {collection}")
        };
        assert_eq!(diagnostic.collection, collection);
        assert_eq!(diagnostic.id, id);
        assert!(diagnostic.reason.contains(reason), "{}", diagnostic.reason);
    }
}

#[test]
fn future_versions_kinds_and_nested_fields_remain_lossless_and_are_addressed() {
    let id = Uuid::new_v4();
    for (collection, object) in [
        (
            "curves",
            json!({"id":id,"interpolation_version":99,"keys":"future format"}),
        ),
        (
            "shapes",
            json!({"id":id,"geometry":{"kind":"future_geometry","value":{"nested":42}},"fill":null,"stroke":null}),
        ),
        (
            "shapes",
            json!({"id":id,"geometry":{"kind":"ellipse","value":{"size":Uuid::new_v4(),"future":true}},"fill":null,"stroke":null}),
        ),
        ("texts", json!({"id":id,"layout_version":99,"text":42})),
    ] {
        let project = raw_object(collection, object.clone());
        project.validate_storage().unwrap();
        assert_eq!(
            serde_json::to_value(&project).unwrap()[collection][0],
            object
        );
        let ProjectError::UnsupportedObjects(diagnostics) = project.ensure_editable().unwrap_err()
        else {
            panic!("expected addressed unsupported objects")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].collection, collection);
        assert_eq!(diagnostics[0].id, id);
        assert!(!diagnostics[0].reason.is_empty());
    }
}

#[test]
fn every_opaque_object_is_listed() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut project = raw_object("shapes", json!({"id":a,"future":true}));
    let text = raw_object("texts", json!({"id":b,"layout_version":99,"future":true}));
    project.texts = text.texts;
    assert!(matches!(project.shapes[0], DocumentObject::Opaque(_)));
    let error = project.ensure_editable().unwrap_err();
    assert!(error.to_string().contains(&a.to_string()));
    assert!(error.to_string().contains(&b.to_string()));
    let ProjectError::UnsupportedObjects(objects) = error else {
        panic!()
    };
    assert_eq!(objects.len(), 2);
}

#[test]
fn unknown_map_payloads_are_preserved_but_known_invalid_map_payloads_are_rejected() {
    let definition: Value = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let mut unknown = definition.clone();
    let name = unknown["public_inputs"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    unknown["public_inputs"][&name]["default"] =
        json!({"kind":"future_value","value":{"arbitrary":42}});
    let project = raw_object("templates", unknown.clone());
    project.validate_storage().unwrap();
    assert!(matches!(project.templates[0], DocumentObject::Opaque(_)));
    assert_eq!(
        serde_json::to_value(&project).unwrap()["templates"][0],
        unknown
    );
    let mut invalid = definition;
    invalid["public_inputs"][&name]["default"] = json!({"kind":"scalar","value":"invalid number"});
    let project = raw_object("templates", invalid.clone());
    let ProjectError::InvalidObject(diagnostic) = project.validate_storage().unwrap_err() else {
        panic!()
    };
    assert_eq!(diagnostic.collection, "templates");
    assert_eq!(diagnostic.id.to_string(), invalid["id"].as_str().unwrap());
    assert!(!diagnostic.reason.is_empty());
}
