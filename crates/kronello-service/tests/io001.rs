//! IO-001 service coverage (ADR-0134): the shared registry exposes
//! `io.output.*` enumeration plus explicit enable/disable. Headless
//! transports report honest runtime detection and typed rejects — they can
//! never activate an external output, and vendor SDK kinds always reject
//! `UNSUPPORTED_FEATURE` through the adapter boundary.
use kronello_service::*;
use serde_json::{Value as Json, json};

fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn execute(request: Json) -> Json {
    serde_json::to_value(service().execute_json(&request.to_string())).unwrap()
}
fn error(request: Json) -> ServiceError {
    service()
        .dispatch(serde_json::from_value(request).unwrap())
        .unwrap_err()
}

#[test]
fn io_output_list_reports_all_kinds_with_detection() {
    let response = execute(json!({"operation":"io.output.list"}));
    assert_eq!(response["status"], "success");
    let result = response["result"]["value"].clone();
    let devices = result["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 4);
    let kinds: Vec<_> = devices
        .iter()
        .map(|d| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["ref_monitor", "syphon", "sdi", "ndi"]);
    for device in devices {
        // Every entry reports detection/availability/activity plus the
        // contract transport, without platform types leaking to the wire.
        assert!(device.get("detected").is_some());
        assert!(device.get("available").is_some());
        assert!(device.get("transport").is_some());
        assert_eq!(device["active"], false);
        // Headless: nothing can be activated from this transport.
        assert_eq!(device["available"], false);
    }
}

#[test]
fn io_output_enable_disable_are_typed_rejects_headless() {
    for kind in ["ref_monitor", "syphon", "sdi", "ndi"] {
        let err = error(json!({"operation":"io.output.enable","kind":kind}));
        assert_eq!(err.code, "UNSUPPORTED_FEATURE", "{kind}");
        let err = error(json!({"operation":"io.output.disable","kind":kind}));
        assert_eq!(err.code, "UNSUPPORTED_FEATURE", "{kind}");
    }
}

#[test]
fn vendor_sdk_boundary_rejects_with_detection_in_message() {
    for (kind, sdk) in [
        (OutputDeviceKind::Sdi, "DeckLink"),
        (OutputDeviceKind::Ndi, "NDI"),
    ] {
        let adapter = UnimplementedVendorAdapter::new(kind, kind == OutputDeviceKind::Ndi);
        let err = vendor_output_open(&adapter).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_FEATURE");
        assert!(err.message.contains(sdk), "{}", err.message);
        assert!(err.message.contains("sdk detected"), "{}", err.message);
    }
}

#[test]
fn io_output_requests_roundtrip_the_wire_codec() {
    // The strict Request decoder and ResultData codec are the single wire
    // contract for CLI/MCP/FFI.
    let request: Request = serde_json::from_str(
        r#"{"operation":"io.output.enable","kind":"syphon","name":"Program Out"}"#,
    )
    .unwrap();
    let Request::IoOutputEnable(r) = request else {
        panic!("wrong variant")
    };
    assert_eq!(r.kind, OutputDeviceKind::Syphon);
    assert_eq!(r.name.as_deref(), Some("Program Out"));
    let response = Response::Success {
        result: ResultData::OutputState(IoOutputStateResult {
            kind: OutputDeviceKind::Syphon,
            active: true,
        }),
    };
    let encoded = serde_json::to_string(&response).unwrap();
    assert!(encoded.contains("output_state"));
    let decoded: Response = serde_json::from_str(&encoded).unwrap();
    let Response::Success {
        result: ResultData::OutputState(state),
    } = decoded
    else {
        panic!("wrong variant")
    };
    assert!(state.active);
    assert_eq!(state.kind, OutputDeviceKind::Syphon);
    // Unknown kinds are invalid wire input, not silent defaults.
    assert!(
        serde_json::from_str::<Request>(r#"{"operation":"io.output.enable","kind":"hdmi"}"#)
            .is_err()
    );
}
