use kronello_service::{BackendSelection, Request, Response, Service};

#[test]
fn json_order_service_request() {
    let input = r#"{"commands":[{"property_source_set":{"source":{"value":{"value":1.5,"kind":"scalar"},"kind":"constant"},"property":"00000000-0000-0000-0000-000000000002","object":"00000000-0000-0000-0000-000000000001"}}],"base_revision":"0","project":"missing-json-order.kronello","operation":"edit.plan"}"#;
    let _: Request = serde_json::from_str(input).unwrap();
    let Response::Error { error } =
        Service::new(BackendSelection::CpuReference).execute_json(input)
    else {
        panic!()
    };
    assert_eq!(error.code, "PROJECT_NOT_FOUND");
}

#[test]
fn json_order_successful_plan_and_invalid_requests() {
    use serde_json::{Value, json};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.kronello");
    let document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let service = Service::new(BackendSelection::CpuReference);
    let create =
        json!({"operation":"project.create","project":path,"document":document}).to_string();
    assert!(matches!(
        service.execute_json(&create),
        Response::Success { .. }
    ));
    let node = &document["compositions"][0]["nodes"][0];
    let property = node["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();
    let request = json!({"operation":"edit.plan","project":path,"base_revision":"1","commands":[
        {"property_source_set":{"object":node["id"],"property":property["id"],"source":{"kind":"constant","value":{"kind":"scalar","value":0.5}}}}
    ]}).to_string();
    let reversed = request.replace(
        r#""kind":"scalar","value":0.5"#,
        r#""value":0.5,"kind":"scalar""#,
    );
    assert_ne!(request, reversed);
    let canonical = serde_json::to_value(service.execute_json(&request)).unwrap();
    let actual = serde_json::to_value(service.execute_json(&reversed)).unwrap();
    assert_eq!(actual["status"], "success", "{actual}");
    assert_eq!(actual, canonical);
    for invalid in [
        reversed.replace(
            r#""value":0.5,"kind":"scalar""#,
            r#""value":"bad","kind":"scalar""#,
        ),
        reversed.replace(
            r#""value":0.5,"kind":"scalar""#,
            r#""value":0.5,"kind":"scalar","extra":true"#,
        ),
        reversed.replace(
            r#""value":0.5,"kind":"scalar""#,
            r#""value":0.5,"value":0.6,"kind":"scalar""#,
        ),
        reversed.replace(
            r#""value":0.5,"kind":"scalar""#,
            r#""value":0.5,"kind":"future""#,
        ),
    ] {
        let Response::Error { error } = service.execute_json(&invalid) else {
            panic!("invalid request accepted")
        };
        assert_eq!(error.code, "INVALID_REQUEST");
    }
}

#[test]
fn json_order_nested_commands_and_response() {
    use serde_json::json;
    let commands = [
        r#"{"keyframe_upsert":{"curve":"00000000-0000-0000-0000-000000000001","key":{"interpolation":{"value":{"control2":[0.75,0.75],"control1":[0.25,0.25]},"kind":"cubic"},"value":{"value":[1.5,2.5],"kind":"vec2"},"time":{"den":"2","num":"1"}}}}"#,
        r#"{"property_source_set":{"source":{"value":{"value":{"components":{"r":0.5,"g":0.25,"b":0.75,"alpha":0.5},"space":"srgb"},"kind":"color"},"kind":"constant"},"property":"00000000-0000-0000-0000-000000000002","object":"00000000-0000-0000-0000-000000000001"}}"#,
    ];
    for command in commands {
        let request = format!(
            r#"{{"commands":[{command}],"project":"missing-json-order.kronello","base_revision":"0","operation":"edit.plan"}}"#
        );
        let _: Request = serde_json::from_str(&request).unwrap();
        let Response::Error { error } =
            Service::new(BackendSelection::CpuReference).execute_json(&request)
        else {
            panic!()
        };
        assert_eq!(error.code, "PROJECT_NOT_FOUND");
    }
    let response = json!({"result":{"value":{"revision":"1","composition":"00000000-0000-0000-0000-000000000003","samples":[{"value_type":"scalar","unit":"dimensionless","key":{"property":"00000000-0000-0000-0000-000000000002","node":"00000000-0000-0000-0000-000000000001","instance_path":[],"kind":"node"},"values":[{"value":1.5,"kind":"scalar"}]}],"times":[{"den":"2","num":"1"}]},"kind":"samples"},"status":"success"});
    // Reverse the adjacent Value tag explicitly: Value's default map order is canonical.
    let text = response.to_string().replace(
        r#""kind":"scalar","value":1.5"#,
        r#""value":1.5,"kind":"scalar""#,
    );
    let _: Response = serde_json::from_str(&text).unwrap();
}
