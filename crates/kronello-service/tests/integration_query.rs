use kronello_service::{BackendSelection, Service};
use serde_json::{Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    path: std::path::PathBuf,
    composition: Value,
    instance: uuid::Uuid,
    font: Value,
    band: Value,
    text: Value,
}
impl Fixture {
    fn new() -> Self {
        Self::with_options(None, None)
    }
    fn with_options(stage: Option<&str>, wrap_width: Option<f64>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("query.kronello");
        let mut document: Value = serde_json::from_str(include_str!(
            "../../../examples/integration-001.project.json"
        ))
        .unwrap();
        let mut definition: Value = serde_json::from_str(include_str!(
            "../../../examples/integration-001.definition.json"
        ))
        .unwrap();
        if let Some(stage) = stage {
            definition["constraints"]["bands"][0]["bounds"] = json!(stage);
        }
        if let Some(width) = wrap_width {
            for composition in document["compositions"].as_array_mut().unwrap() {
                for node in composition["nodes"].as_array_mut().unwrap() {
                    for p in node["properties"].as_array_mut().unwrap() {
                        if p["descriptor"]["key"] == "kronello.text.wrap_width" {
                            p["source"]["value"]["value"] = json!(width);
                        }
                    }
                }
            }
        }
        let instance = uuid::Uuid::new_v4();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let font = json!({"identity":document["texts"][0]["styles"][0]["font"],
            "path":root.join("target/fixtures/external/NotoSansCJKjp-Regular.otf")});
        let fixture = Self {
            _temp: temp,
            path,
            composition: document["compositions"][0]["id"].clone(),
            instance,
            font,
            band: document["compositions"][2]["nodes"][0]["id"].clone(),
            text: document["compositions"][2]["nodes"][1]["id"].clone(),
        };
        fixture
            .ok(json!({"operation":"project.create","project":fixture.path,"document":document}));
        let session = uuid::Uuid::new_v4();
        let headline = if wrap_width.is_some() {
            "日"
        } else {
            "長い日本語字幕"
        };
        fixture.ok(
            json!({"operation":"template.define","project":fixture.path,"base_revision":"1",
            "session_id":session,"idempotency_key":"define","definition":definition}),
        );
        fixture.ok(json!({"operation":"template.instantiate","project":fixture.path,"base_revision":"2",
            "session_id":session,"idempotency_key":"instantiate","composition":fixture.composition,
            "node":uuid::Uuid::new_v4(),"index":0,"instance":{"id":instance,
            "definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"8","den":"1"},
            "inputs":{"headline":{"kind":"string","value":headline},
            "accent":{"kind":"color","value":{"space":"srgb","components":{"r":0.1,"g":0.2,"b":0.9,"alpha":1.0}}}}}}));
        fixture
    }
    fn execute(&self, request: Value) -> Value {
        serde_json::to_value(Service::new(BackendSelection::Gpu).execute_json(&request.to_string()))
            .unwrap()
    }
    fn ok(&self, request: Value) -> Value {
        let response = self.execute(request);
        assert_eq!(response["status"], "success", "{response}");
        response["result"]["value"].clone()
    }
    fn scene(&self) -> Value {
        json!({"operation":"scene.query","project":self.path,"composition":self.composition,
            "expand_instances":true,"evaluation":{"time":{"num":"0","den":"1"},"fonts":[self.font]}})
    }
    fn sample(&self) -> Value {
        json!({"operation":"property.sample","project":self.path,"composition":self.composition,
            "fonts":[self.font],"times":[{"num":"0","den":"1"}],
            "keys":[{"kind":"node","instance_path":[self.instance],"node":self.band,
            "property":"4e4837d7-b8f7-47e6-a455-cc35e5b00cdd"}]})
    }
}

#[test]
fn evaluated_template_queries_share_layout_inputs_and_do_not_initialize_gpu_or_edit() {
    let f = Fixture::new();
    let before = f.ok(json!({"operation":"project.export","project":f.path}));
    let scene = f.ok(f.scene());
    let band = scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"]["node"] == f.band)
        .unwrap();
    let text = scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"]["node"] == f.text)
        .unwrap();
    assert_eq!(text["evaluated"]["text"], "長い日本語字幕");
    let bounds = &text["evaluated"]["layout_bounds"];
    let size = &band["evaluated"]["properties"]["1d77434e-64ef-4de4-9e38-98bb71bb03fe"]["value"];
    assert_eq!(
        size[0].as_f64().unwrap(),
        bounds["max"][0].as_f64().unwrap() - bounds["min"][0].as_f64().unwrap() + 8.0
    );
    assert!(band["evaluated"]["effects"][0].get("DropShadow").is_some());
    let sampled = f.ok(f.sample());
    assert_eq!(
        sampled["samples"][0]["values"][0],
        band["evaluated"]["properties"]["4e4837d7-b8f7-47e6-a455-cc35e5b00cdd"]
    );
    assert_eq!(
        sampled["samples"][0]["values"][0]["value"]["components"]["b"],
        0.9
    );
    let mut authored = f.scene();
    authored.as_object_mut().unwrap().remove("evaluation");
    assert!(
        f.ok(authored)["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n.get("evaluated").is_none())
    );
    assert_eq!(
        before,
        f.ok(json!({"operation":"project.export","project":f.path}))
    );
}

#[test]
fn node_explain_reports_renderer_layout_dependencies_for_template_band() {
    let f = Fixture::new();
    let before = f.ok(json!({"operation":"project.export","project":f.path}));
    let result = f.ok(json!({"operation":"node.explain","project":f.path,"composition":f.composition,"key":{"instance_path":[f.instance],"node":f.band},"time":{"num":"0","den":"1"},"fonts":[f.font]}));
    assert!(
        result["render_diagnostics"].as_array().unwrap().is_empty(),
        "{result}"
    );
    assert!(
        result["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["upstream"]["kind"] == "layout" && d["upstream"]["text"] == f.text)
    );
    assert_eq!(
        f.ok(json!({"operation":"project.export","project":f.path})),
        before
    );
}

#[test]
fn evaluated_queries_keep_font_errors_local_paths_and_inactive_node_boundaries() {
    let f = Fixture::new();
    for operation in ["scene", "sample"] {
        for (fonts, expected) in [
            (json!([]), "FONT_MISSING"),
            (json!([f.font, f.font]), "INVALID_REQUEST"),
            (
                json!([{"identity":f.font["identity"],"path":"https://example.invalid/font.otf"}]),
                "INVALID_REQUEST",
            ),
            (
                json!([{"identity":f.font["identity"],"path":f.path}]),
                "ASSET_HASH_MISMATCH",
            ),
        ] {
            let mut request = if operation == "scene" {
                f.scene()
            } else {
                f.sample()
            };
            if operation == "scene" {
                request["evaluation"]["fonts"] = fonts;
            } else {
                request["fonts"] = fonts;
            }
            assert_eq!(f.execute(request)["error"]["code"], expected);
        }
    }
    let mut request = f.scene();
    request["evaluation"]["time"] = json!({"num":"8","den":"1"});
    assert!(
        f.ok(request)["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n.get("evaluated").is_none())
    );
    let mut request = f.sample();
    request["times"] = json!([{"num":"8","den":"1"}]);
    assert_eq!(f.execute(request)["error"]["code"], "INVALID_REQUEST");
}

#[test]
fn evaluated_query_returns_all_bounds_stages_and_explicit_follower_values() {
    for stage in ["layout", "ink", "visual"] {
        let f = Fixture::with_options(Some(stage), None);
        let scene = f.ok(f.scene());
        let nodes = scene["nodes"].as_array().unwrap();
        let text = &nodes.iter().find(|n| n["key"]["node"] == f.text).unwrap()["evaluated"];
        let band = &nodes.iter().find(|n| n["key"]["node"] == f.band).unwrap()["evaluated"];
        let bounds = &text["bounds"];
        for name in ["layout_bounds", "ink_bounds", "visual_bounds"] {
            assert!(bounds[name]["min"].is_array());
            assert!(bounds[name]["max"].is_array());
        }
        assert_ne!(bounds["layout_bounds"], bounds["ink_bounds"]);
        let chosen = &bounds[format!("{stage}_bounds")];
        let size = &band["properties"]["1d77434e-64ef-4de4-9e38-98bb71bb03fe"]["value"];
        for (axis, padding) in [8.0, 4.0].into_iter().enumerate() {
            assert_eq!(
                size[axis].as_f64().unwrap(),
                chosen["max"][axis].as_f64().unwrap() - chosen["min"][axis].as_f64().unwrap()
                    + padding
            );
        }
        // Legacy local layout_bounds is unchanged; canonical bounds share the
        // root Composition coordinate space for all stages and node kinds.
        assert_eq!(text["layout_bounds"]["min"], json!([0.0, 0.0]));
        assert_eq!(bounds["layout_bounds"]["min"], json!([32.0, 120.0]));
        assert_ne!(
            band["bounds"]["ink_bounds"],
            band["bounds"]["visual_bounds"]
        );
        let root = nodes
            .iter()
            .find(|n| n["key"]["instance_path"] == json!([]))
            .unwrap();
        assert!(root["evaluated"]["bounds"]["visual_bounds"]["min"].is_array());
    }
}

#[test]
fn evaluated_query_and_final_render_share_structured_width_overflow() {
    let f = Fixture::with_options(Some("ink"), Some(1.0));
    let error = f.execute(f.scene());
    assert_eq!(error["error"]["code"], "LAYOUT_OVERFLOW");
    assert_eq!(error["error"]["details"]["node"], f.text);
    assert_eq!(
        error["error"]["details"]["instance_path"],
        json!([f.instance])
    );
    assert_eq!(error["error"]["details"]["line"], 0);
    assert_eq!(error["error"]["details"]["wrap_width"], 1.0);
    let request = json!({"operation":"render.frame","time":{"num":"0","den":"1"},
        "input":{"project":f.path,"composition":f.composition,"fonts":[f.font],
            "region":{"origin":[0.0,0.0],"extent":[320.0,180.0],"pixels":[32,18]}}});
    let frame = serde_json::to_value(
        Service::new(BackendSelection::CpuReference).execute_json(&request.to_string()),
    )
    .unwrap();
    assert_eq!(frame["error"], error["error"]);
}

#[test]
fn cycle_diagnostics_keep_closed_typed_runtime_keys_in_service_details() {
    use kronello_eval::{EvaluationError, RuntimePropertyKey};
    use kronello_model::{InstancePath, NodeId, PropertyId, PropertyKey};
    let node = NodeId::new();
    let property = PropertyId::new();
    let wrap = RuntimePropertyKey::Node(PropertyKey {
        instance_path: InstancePath::root(),
        node,
        property,
    });
    let layout = RuntimePropertyKey::LayoutValue {
        instance_path: InstancePath::root(),
        text: node,
        consumer: property,
    };
    let error: kronello_service::ServiceError =
        kronello_render::RenderError::Evaluation(EvaluationError::DependencyCycle {
            path: vec![wrap.clone(), layout, wrap],
        })
        .into();
    assert_eq!(error.code, "PROPERTY_DEPENDENCY_CYCLE");
    let path = error.details.unwrap()["path"].as_array().unwrap().clone();
    assert_eq!(path[0], path[2]);
    assert_eq!(
        path[0],
        json!({"kind":"node","instance_path":[],"node":node,"property":property})
    );
    assert_eq!(
        path[1],
        json!({"kind":"layout","instance_path":[],"text":node,"consumer":property})
    );
}
