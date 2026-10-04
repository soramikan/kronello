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
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("query.kronello");
        let document: Value = serde_json::from_str(include_str!(
            "../../../examples/integration-001.project.json"
        ))
        .unwrap();
        let definition: Value = serde_json::from_str(include_str!(
            "../../../examples/integration-001.definition.json"
        ))
        .unwrap();
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
        fixture.ok(
            json!({"operation":"template.define","project":fixture.path,"base_revision":"1",
            "session_id":session,"idempotency_key":"define","definition":definition}),
        );
        fixture.ok(json!({"operation":"template.instantiate","project":fixture.path,"base_revision":"2",
            "session_id":session,"idempotency_key":"instantiate","composition":fixture.composition,
            "node":uuid::Uuid::new_v4(),"index":0,"instance":{"id":instance,
            "definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"8","den":"1"},
            "inputs":{"headline":{"kind":"string","value":"長い日本語字幕"},
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
