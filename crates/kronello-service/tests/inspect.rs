use kronello_service::{BackendSelection, Response, Service};
use serde_json::{Value, json};

fn document() -> Value {
    serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap()
}
fn execute(request: Value) -> Value {
    let response = Service::new(BackendSelection::Gpu).execute_json(&request.to_string());
    let wire = serde_json::to_value(&response).unwrap();
    let _: Response = serde_json::from_str(&wire.to_string()).unwrap();
    wire
}
fn ok(request: Value) -> Value {
    let response = execute(request);
    assert_eq!(response["status"], "success", "{response}");
    let schema = kronello_service::api_json_schema();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&response)
        .unwrap();
    response["result"]["value"].clone()
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    document: Value,
}
impl Fixture {
    fn new(document: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inspect.kronello");
        ok(json!({"operation":"project.create","project":path,"document":document}));
        Self {
            _dir: dir,
            path,
            document,
        }
    }
    fn node(&self, node: &Value, time: i64) -> Value {
        json!({"operation":"node.explain","project":self.path,"composition":self.document["compositions"][0]["id"],"key":{"instance_path":[],"node":node},"time":{"num":time.to_string(),"den":"1"}})
    }
    fn render(&self, pixels: [u32; 2]) -> Value {
        json!({"operation":"render.explain","input":{"project":self.path,"composition":self.document["compositions"][0]["id"],"region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":pixels}},"time":{"num":"0","den":"1"}})
    }
    fn export(&self) -> Value {
        ok(json!({"operation":"project.export","project":self.path}))
    }
}
fn property(key: &str, kind: &str, value: Value) -> Value {
    json!({"id":uuid::Uuid::new_v4(),"descriptor":{"key":key,"version":1},"source":{"kind":"constant","value":{"kind":kind,"value":value}},"modifiers":[]})
}
fn opacity(document: &mut Value, value: f64) {
    let p = document["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();
    p["source"]["value"]["value"] = json!(value);
}
fn parent(document: &mut Value, containment: bool, value: f64) -> Value {
    let id = json!(uuid::Uuid::new_v4());
    let child = document["compositions"][0]["nodes"][0]["id"].clone();
    let node = json!({"id":id,"kind":{"kind":"group"},"containment_parent":null,"transform_parent":null,"child_order":if containment {vec![child.clone()]} else {vec![]},"active_range":{"start":{"num":"-2","den":"1"},"end":{"num":"3","den":"1"}},"properties":[property("kronello.opacity","scalar",json!(value))]});
    document["compositions"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(node);
    if containment {
        document["compositions"][0]["root_nodes"] = json!([id]);
        document["compositions"][0]["nodes"][0]["containment_parent"] = id.clone();
    } else {
        document["compositions"][0]["root_nodes"]
            .as_array_mut()
            .unwrap()
            .push(id.clone());
        document["compositions"][0]["nodes"][0]["transform_parent"] = id.clone();
    }
    id
}
fn has(result: &Value, code: &str) -> bool {
    result["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["code"] == code)
}

#[test]
fn expression_dependencies_and_typed_failures_are_returned_without_fallback() {
    let mut doc = document();
    let expression = uuid::Uuid::new_v4();
    let upstream = property("kronello.effect.opacity", "scalar", json!(0.0));
    let node = doc["compositions"][0]["nodes"][0]["id"].clone();
    doc["expressions"] = json!([{"id":expression,"version":1,"value_type":"scalar","nodes":[{"property":{"node":node,"property":upstream["id"],"value_type":"scalar"}}]}]);
    doc["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap()["source"] = json!({"kind":"expression","value":expression});
    doc["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .push(upstream.clone());
    let f = Fixture::new(doc.clone());
    let result = ok(f.node(&node, 0));
    assert!(has(&result, "OPACITY_ZERO"));
    assert!(
        result["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "property" && d["upstream"]["property"] == upstream["id"])
    );
    doc["expressions"][0]["nodes"] = json!([{"literal":{"kind":"scalar","value":1.0}},{"literal":{"kind":"scalar","value":0.0}},{"divide":{"left":0,"right":1}}]);
    let f = Fixture::new(doc);
    let result = ok(f.node(&node, 0));
    assert!(has(&result, "EVALUATION_FAILED"));
    assert!(
        result["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["error"]["code"] == "EVALUATION_ERROR")
    );
    assert!(result["opacity"].is_null());
    assert_eq!(result["assessment"], "blocked");
}

#[test]
fn transparent_paint_and_unrelated_compiler_failure_are_not_mistaken_for_visible_pixels() {
    let mut doc = document();
    doc["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["descriptor"]["key"] == "kronello.fill_color")
        .unwrap()["source"]["value"]["value"]["components"]["alpha"] = json!(0.0);
    let f = Fixture::new(doc);
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][0]["id"], 0));
    assert!(has(&result, "PAINT_ALPHA_ZERO"));
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let f = Fixture::new(doc);
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][0]["id"], 0));
    assert_eq!(result["assessment"], "indeterminate");
    assert!(!has(&result, "FONT_MISSING"));
    assert_eq!(result["render_diagnostics"][0]["code"], "FONT_MISSING");
}

#[test]
fn sequence_plan_uses_the_same_lowering_and_sequence_working_space() {
    let mut doc = document();
    let sequence = uuid::Uuid::new_v4();
    doc["sequences"] = json!([{"id":sequence,"extent":{"width":64.0,"height":32.0},"frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec2020","tracks":[{"id":uuid::Uuid::new_v4(),"kind":"video","clips":[{"id":uuid::Uuid::new_v4(),"source_ref":{"kind":"composition","composition":doc["compositions"][0]["id"]},"timeline_range":{"start":{"num":"1","den":"1"},"end":{"num":"2","den":"1"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[]}]}]}]);
    let f = Fixture::new(doc);
    let mut request = f.render([8, 4]);
    request["input"]
        .as_object_mut()
        .unwrap()
        .remove("composition");
    request["input"]["target"] = json!({"kind":"sequence","sequence":sequence});
    let gap = ok(request.clone());
    request["time"] = json!({"num":"1","den":"1"});
    let active = ok(request);
    assert_eq!(
        active["target"],
        json!({"kind":"sequence","sequence":sequence})
    );
    assert_eq!(active["plan"]["profile"]["working_space"], "linear_rec2020");
    assert!(
        active["plan"]["tiles"][0]["stages"]
            .as_array()
            .unwrap()
            .len()
            > gap["plan"]["tiles"][0]["stages"].as_array().unwrap().len()
    );
}

#[test]
fn inspection_separates_own_opacity_half_open_range_and_hidden_ancestors() {
    let mut doc = document();
    opacity(&mut doc, 0.0);
    let group = parent(&mut doc, true, 0.0);
    let f = Fixture::new(doc);
    let before = f.export();
    let child = &f.document["compositions"][0]["nodes"][0]["id"];
    let active = ok(f.node(child, 0));
    assert_eq!(active["assessment"], "hidden");
    assert!(has(&active, "OPACITY_ZERO") && has(&active, "ANCESTOR_OPACITY_ZERO"));
    let boundary = ok(f.node(child, 3));
    assert!(
        has(&boundary, "OUTSIDE_ACTIVE_RANGE") && has(&boundary, "ANCESTOR_OUTSIDE_ACTIVE_RANGE")
    );
    assert!(
        boundary["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["subject"]["node"] == group && r["category"] == "parent")
    );
    assert_eq!(active["pixel_visibility_observed"], false);
    assert_eq!(f.export(), before);
}

#[test]
fn transform_parent_activity_and_opacity_do_not_hide_the_child_but_zero_scale_does() {
    let mut doc = document();
    let group = parent(&mut doc, false, 0.0);
    doc["compositions"][0]["nodes"][1]["active_range"]["end"] = json!({"num":"0","den":"1"});
    let f = Fixture::new(doc.clone());
    let result = ok(f.node(&doc["compositions"][0]["nodes"][0]["id"], 0));
    assert_eq!(result["assessment"], "potentially_visible", "{result}");
    assert!(
        result["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["kind"] == "transform_parent" && d["node"]["node"] == group)
    );
    doc["compositions"][0]["nodes"][1]["properties"]
        .as_array_mut()
        .unwrap()
        .push(property(
            "kronello.transform.scale",
            "vec2",
            json!([0.0, 1.0]),
        ));
    let f = Fixture::new(doc);
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][0]["id"], 0));
    assert!(has(&result, "TRANSFORM_COLLAPSED"));
}

#[test]
fn mattes_report_consumption_zero_opacity_inactivity_and_unobserved_coverage() {
    let mut doc = document();
    let matte = parent(&mut doc, false, 0.0);
    doc["compositions"][0]["nodes"][0]["transform_parent"] = Value::Null;
    let f = Fixture::new(doc);
    let source = f.document["compositions"][0]["nodes"][0]["id"].clone();
    let bindings = json!([{"source":{"instance_path":[],"node":source},"matte":{"instance_path":[],"node":matte},"kind":"alpha","visible":false}]);
    let mut request = f.node(&source, 0);
    request["mattes"] = bindings.clone();
    let result = ok(request.clone());
    assert!(has(&result, "MASK_ZERO_OPACITY"));
    request["key"]["node"] = matte.clone();
    assert!(has(&ok(request), "MATTE_ONLY"));
    let mut doc = f.document.clone();
    doc["compositions"][0]["nodes"][1]["active_range"]["end"] = json!({"num":"0","den":"1"});
    let inactive = Fixture::new(doc);
    let mut request = inactive.node(&source, 0);
    request["mattes"] = bindings.clone();
    let explanation = ok(request);
    assert!(
        explanation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "MASK_INACTIVE"
                && reason["subject"]["node"] == matte
                && reason["details"]["source"]["node"] == source
                && reason["impact"] == "blocks")
    );
    let before = inactive.export();
    let mut request = inactive.render([8, 4]);
    request["mattes"] = bindings.clone();
    let result = ok(request);
    assert!(result["plan"].is_null());
    assert_eq!(result["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(result["diagnostics"][0]["code"], "MATTE_MISSING");
    assert_eq!(
        result["diagnostics"][0]["message"],
        "MATTE_MISSING: duplicate source or missing active matte binding"
    );
    // The read-only plan preserves the same failure as the final render DAG.
    let project = serde_json::from_value(inactive.document.clone()).unwrap();
    let composition =
        serde_json::from_value(inactive.document["compositions"][0]["id"].clone()).unwrap();
    let snapshot =
        kronello_render::RenderSnapshot::new(&project, composition, 0, Default::default())
            .unwrap()
            .with_mattes(serde_json::from_value(bindings).unwrap());
    let scene = kronello_render::build_scene_ir(&snapshot, kronello_time::Time::ZERO, &[]).unwrap();
    let error = kronello_render::build_render_dag(
        &scene,
        Default::default(),
        kronello_render::OutputRegion {
            origin: [0.; 2],
            extent: [64., 32.],
            pixels: [8, 4],
        },
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        result["diagnostics"][0]["message"].as_str().unwrap()
    );
    assert!(matches!(
        error,
        kronello_render::RenderError::Backend {
            code: "MATTE_MISSING",
            ..
        }
    ));
    assert_eq!(inactive.export(), before);
}

#[test]
fn missing_content_font_and_opaque_effect_are_structured_and_do_not_edit() {
    let mut doc = document();
    doc["shapes"] = json!([]);
    let f = Fixture::new(doc);
    let before = f.export();
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][0]["id"], 0));
    assert!(has(&result, "ASSET_MISSING"));
    assert_eq!(result["assessment"], "blocked");
    assert!(
        result["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "ASSET_MISSING" && r["details"]["content"].is_string())
    );
    assert_eq!(f.export(), before);
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let f = Fixture::new(doc);
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][1]["id"], 0));
    assert!(has(&result, "FONT_MISSING"));
    let mut doc = document();
    doc["compositions"][0]["nodes"][0]["effects"] =
        json!([{"id":uuid::Uuid::new_v4(),"unknown":"kept"}]);
    let f = Fixture::new(doc);
    let result = ok(f.node(&f.document["compositions"][0]["nodes"][0]["id"], 0));
    assert!(has(&result, "UNSUPPORTED_FEATURE"));
}

#[test]
fn alpha_and_luminance_mattes_distinguish_proven_zero_from_unobserved_coverage() {
    let mut doc = document();
    let source = parent(&mut doc, true, 1.0);
    let matte = doc["compositions"][0]["nodes"][0]["id"].clone();
    let f = Fixture::new(doc.clone());
    let mut request = f.node(&source, 0);
    request["mattes"] = json!([{"source":{"instance_path":[],"node":source},"matte":{"instance_path":[],"node":matte},"kind":"alpha","visible":false}]);
    let result = ok(request.clone());
    assert!(has(&result, "MASK_COVERAGE_UNRESOLVED"));
    assert_eq!(result["assessment"], "indeterminate");
    let fill = doc["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["descriptor"]["key"] == "kronello.fill_color")
        .unwrap();
    fill["source"]["value"]["value"]["components"] = json!({"r":0.0,"g":0.0,"b":0.0,"alpha":1.0});
    let f = Fixture::new(doc.clone());
    request["project"] = json!(f.path);
    request["mattes"][0]["kind"] = json!("luminance");
    assert!(has(&ok(request.clone()), "MASK_ZERO_LUMINANCE"));
    let fill = doc["compositions"][0]["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["descriptor"]["key"] == "kronello.fill_color")
        .unwrap();
    fill["source"]["value"]["value"]["components"]["alpha"] = json!(0.0);
    let f = Fixture::new(doc);
    request["project"] = json!(f.path);
    request["mattes"][0]["kind"] = json!("alpha");
    assert!(has(&ok(request), "MASK_ZERO_COVERAGE"));
}

#[test]
fn render_plan_uses_tiling_real_compile_counters_and_labeled_transfer_estimates() {
    let mut doc = document();
    opacity(&mut doc, 0.0);
    let f = Fixture::new(doc);
    let before = f.export();
    let request = f.render([1024, 512]);
    let result = ok(request.clone());
    assert_eq!(ok(request.clone()), result);
    let plan = &result["plan"];
    assert_eq!(plan["executed"], false);
    assert_eq!(plan["backend"], "gpu");
    assert_eq!(plan["tiles"].as_array().unwrap().len(), 2);
    assert_eq!(plan["output_host_bytes_estimate"], 1024u64 * 512 * 32);
    assert_eq!(plan["cache_scope"], "isolated_query_compilation");
    assert_eq!(plan["raster_cache_observed"], false);
    assert!(
        plan["compilation_cache"]["geometry"]["hits"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        plan["compilation_cache"]["geometry"]["misses"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(plan["compilation_cache"]["raster"]["misses"], 0);
    assert!(
        plan["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "SINGLE_GRAPH_LINEAR_DISPLAY_OUTPUT")
    );
    assert!(
        plan["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "ZERO_OPACITY_STILL_PROCESSED")
    );
    let readback = plan["transfers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["direction"] == "gpu_to_cpu")
        .unwrap();
    assert_eq!(readback["bytes_estimate"], 512u64 * 512 * 8 * 4 + 8);
    assert_eq!(readback["operations_estimate"], 6);
    let cpu = serde_json::to_value(
        Service::new(BackendSelection::CpuReference).execute_json(&request.to_string()),
    )
    .unwrap();
    assert!(
        cpu["result"]["value"]["plan"]["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["bytes_estimate"] == 0)
    );
    assert_eq!(f.export(), before);
}

#[test]
fn instance_paths_map_local_time_and_an_inactive_placement_skips_out_of_domain_map() {
    let mut doc = document();
    let definition = doc["compositions"][0].clone();
    let node = definition["nodes"][0]["id"].clone();
    let root = json!(uuid::Uuid::new_v4());
    let instance = uuid::Uuid::new_v4();
    let placement = uuid::Uuid::new_v4();
    let mut composition = definition.clone();
    composition["id"] = root.clone();
    composition["root_nodes"] = json!([placement]);
    composition["nodes"] = json!([{"id":placement,"kind":{"kind":"composition_instance","value":{"id":instance,"definition_ref":definition["id"],"input_bindings":{},"local_time_map":{"kind":"linear","offset":{"num":"-1","den":"1"},"speed":{"num":"2","den":"1"}},"seed":0}},"containment_parent":null,"transform_parent":null,"child_order":[],"active_range":{"start":{"num":"1","den":"1"},"end":{"num":"2","den":"1"}},"properties":[]}]);
    doc["compositions"]
        .as_array_mut()
        .unwrap()
        .insert(0, composition);
    doc["compositions"][0]["nodes"][0]["kind"]["value"]["local_time_map"] = json!({"kind":"piecewise_linear","points":[{"parent":{"num":"1","den":"1"},"local":{"num":"1","den":"1"}},{"parent":{"num":"3","den":"2"},"local":{"num":"2","den":"1"}}]});
    let f = Fixture::new(doc);
    let mut request = f.node(&node, 1);
    request["key"]["instance_path"] = json!([instance]);
    let result = ok(request.clone());
    assert_eq!(result["local_time"], json!({"num":"1","den":"1"}));
    request["time"] = json!({"num":"2","den":"1"});
    let result = ok(request);
    assert!(has(&result, "ANCESTOR_OUTSIDE_ACTIVE_RANGE"));
    assert!(result["local_time"].is_null());
}

#[test]
fn explain_rejects_unknown_fields_urls_and_missing_runtime_node_without_creating_project() {
    let f = Fixture::new(document());
    let node = &f.document["compositions"][0]["nodes"][0]["id"];
    let mut request = f.node(node, 0);
    request["shell"] = json!("ignored?");
    assert_eq!(execute(request)["error"]["code"], "INVALID_REQUEST");
    let mut request = f.node(node, 0);
    request["project"] = json!("https://example.invalid/file");
    assert_eq!(execute(request)["error"]["code"], "INVALID_REQUEST");
    let mut request = f.node(node, 0);
    request["key"]["node"] = json!(uuid::Uuid::new_v4());
    assert_eq!(execute(request)["error"]["code"], "INVALID_REQUEST");
    let path = f._dir.path().join("missing.kronello");
    let mut request = f.render([8, 4]);
    request["input"]["project"] = json!(path);
    assert_eq!(execute(request)["error"]["code"], "PROJECT_NOT_FOUND");
    assert!(!path.exists());
}

#[test]
fn disabled_node_and_disabled_containment_parent_are_explained() {
    let mut doc = document();
    let node = doc["compositions"][0]["nodes"][0]["id"].clone();
    doc["compositions"][0]["nodes"][0]["enabled"] = json!(false);
    let f = Fixture::new(doc);
    let result = ok(f.node(&node, 0));
    assert!(has(&result, "DISABLED"), "{result}");
    assert_eq!(result["assessment"], "hidden", "{result}");
    let mut doc = document();
    let node = doc["compositions"][0]["nodes"][0]["id"].clone();
    let group = parent(&mut doc, true, 1.0);
    let index = doc["compositions"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|n| n["id"] == group)
        .unwrap();
    doc["compositions"][0]["nodes"][index]["enabled"] = json!(false);
    let f = Fixture::new(doc);
    let result = ok(f.node(&node, 0));
    assert!(has(&result, "ANCESTOR_DISABLED"), "{result}");
    assert!(!has(&result, "DISABLED"), "{result}");
    assert_eq!(result["assessment"], "hidden", "{result}");
}

#[test]
fn inspect002_temporal_explain_plans_actual_samples_without_executing_or_mutating() {
    let f = Fixture::new(document());
    let before = f.export();
    let mut request = f.render([1024, 512]);
    let settings = kronello_render::TemporalSettings {
        frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
        shutter_angle: kronello_time::Rational::from_integer(180),
        shutter_phase: kronello_time::Rational::ZERO,
        samples: 3,
        cut_policy: kronello_render::CutPolicy::AllowCrossing,
    };
    request["input"]["profile"] = serde_json::to_value(kronello_render::RenderProfile {
        temporal: Some(settings),
        ..Default::default()
    })
    .unwrap();
    let result = ok(request.clone());
    assert_eq!(result["plan"]["executed"], false);
    assert_eq!(
        result["plan"]["temporal_samples"].as_array().unwrap().len(),
        3
    );
    assert_eq!(result["plan"]["tiles"].as_array().unwrap().len(), 6);
    assert_eq!(result["plan"]["graph_executions_estimate"], 6);
    assert_eq!(result["plan"]["native_preview_supported"], false);
    assert_eq!(result["plan"]["compilation_cache"]["raster"]["misses"], 0);
    assert_eq!(ok(request), result);
    assert_eq!(f.export(), before);
}
