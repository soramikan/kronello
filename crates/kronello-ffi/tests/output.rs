//! IO-001 FFI boundary coverage (ADR-0134): `io.output.*` requests cross the
//! same worker ABI as every other shared operation, the reference-monitor
//! slot binds only to an attached program surface, and every missing
//! precondition is a typed error — never a silent no-op.
#![allow(unsafe_code)]
use kronello_ffi::*;
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::time::{Duration, Instant};

fn wait(handle: u64, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = poll_json(handle)
            && v["request_id"] == id
        {
            return v["response"].clone();
        }
        assert!(Instant::now() < deadline, "request {id} timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn open(path: &str) -> u64 {
    // SAFETY: live UTF-8 bytes, borrowed only during the call.
    let h = unsafe { kronello_open(path.as_ptr(), path.len(), std::ptr::null(), 0) };
    assert_ne!(h, 0);
    h
}
fn call(h: u64, request: &str) -> u64 {
    // SAFETY: live UTF-8 bytes copied by the ABI.
    let id = unsafe { kronello_call(h, request.as_ptr(), request.len()) };
    assert_ne!(id, 0);
    id
}

#[test]
fn io_output_list_and_typed_rejects_cross_the_worker() {
    let h = open("missing-io001.kronello");
    wait(h, 0);
    let list = wait(h, call(h, r#"{"operation":"io.output.list"}"#));
    assert_eq!(list["status"], "success");
    let devices = list["result"]["value"]["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 4);
    let kinds: Vec<_> = devices
        .iter()
        .map(|d| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["ref_monitor", "syphon", "sdi", "ndi"]);
    for device in devices {
        assert_eq!(device["active"], false);
    }
    // Vendor SDK kinds are a typed boundary, never silently routed elsewhere.
    for kind in ["sdi", "ndi"] {
        let response = wait(
            h,
            call(
                h,
                &json!({"operation":"io.output.enable","kind":kind}).to_string(),
            ),
        );
        assert_eq!(response["error"]["code"], "UNSUPPORTED_FEATURE", "{kind}");
        let response = wait(
            h,
            call(
                h,
                &json!({"operation":"io.output.disable","kind":kind}).to_string(),
            ),
        );
        // Disable of a never-active kind still answers typed state.
        assert_eq!(response["result"]["value"]["active"], false, "{kind}");
    }
    // The reference monitor needs its attached surface first.
    let response = wait(
        h,
        call(
            h,
            r#"{"operation":"io.output.enable","kind":"ref_monitor"}"#,
        ),
    );
    assert_eq!(response["error"]["code"], "SURFACE_UNAVAILABLE");
    kronello_close(h);
}

#[test]
fn io_output_syphon_enable_reports_detection_or_missing_surface() {
    let h = open("missing-io001-syphon.kronello");
    wait(h, 0);
    // Without the program surface attached, enable is either the typed
    // "program surface required" reject or "framework absent" — both typed.
    let response = wait(
        h,
        call(
            h,
            r#"{"operation":"io.output.enable","kind":"syphon","name":"Kronello Test"}"#,
        ),
    );
    let code = response["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "UNSUPPORTED_FEATURE" || code == "SURFACE_UNAVAILABLE",
        "unexpected code {code}: {response}"
    );
    let list = wait(h, call(h, r#"{"operation":"io.output.list"}"#));
    let syphon = list["result"]["value"]["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "syphon")
        .unwrap();
    // Detection is a runtime dlopen probe; it is reported, never assumed.
    // With the framework present the missing program surface is the reject.
    if syphon["detected"].as_bool().unwrap() {
        assert_eq!(code, "SURFACE_UNAVAILABLE");
    }
    kronello_close(h);
}

#[cfg(target_os = "macos")]
#[test]
fn ref_monitor_slot_requires_the_program_surface_first() {
    use objc2::rc::Retained;
    use objc2_quartz_core::CAMetalLayer;
    let h = open("missing-io001-ref.kronello");
    wait(h, 0);
    let layer = CAMetalLayer::new();
    let ptr = Retained::as_ptr(&layer).cast::<c_void>().cast_mut();
    // SAFETY: layer is a live CAMetalLayer retained by the FFI.
    let id = unsafe { kronello_surface_attach_at(h, 8, ptr, 64, 32) };
    let response = wait(h, id);
    assert_eq!(response["error"]["code"], "SURFACE_UNAVAILABLE");
    kronello_close(h);
}
