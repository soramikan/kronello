#![allow(unsafe_code)]
use kronello_ffi::*;
use kronello_service::{BackendSelection, Request, Service};
use serde_json::{Value, json};
use std::{
    ffi::{c_char, c_void},
    time::{Duration, Instant},
};

#[test]
fn binary_audio_invalid_buffers_return_owned_typed_errors() {
    let mut error = std::ptr::null_mut();
    let mut has_audio = true;
    // SAFETY: output pointers are writable; null input is rejected before reading.
    let resource =
        unsafe { kronello_audio_prepare(std::ptr::null(), 0, &mut has_audio, &mut error) };
    assert!(resource.is_null());
    assert!(!has_audio);
    assert!(!error.is_null());
    // SAFETY: live owned C string from prepare, freed once after decoding.
    let value: Value =
        unsafe { serde_json::from_slice(std::ffi::CStr::from_ptr(error).to_bytes()).unwrap() };
    assert_eq!(value["code"], "INVALID_REQUEST");
    unsafe { kronello_free(error) };
    let mut output = [1.0; 2];
    // SAFETY: invalid resource is null, error/output are writable.
    assert!(!unsafe {
        kronello_audio_render(std::ptr::null(), 0, 1, output.as_mut_ptr(), &mut error)
    });
    let value: Value =
        unsafe { serde_json::from_slice(std::ffi::CStr::from_ptr(error).to_bytes()).unwrap() };
    assert_eq!(value["code"], "INVALID_AUDIO_INPUT");
    assert_eq!(output, [1.0; 2]);
    unsafe {
        kronello_free(error);
        kronello_audio_free(std::ptr::null_mut());
    }
}
#[test]
fn explain_queries_cross_the_worker_abi_with_the_shared_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inspect.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    assert_eq!(
        service(json!({"operation":"project.create","project":path,"document":doc}))["status"],
        "success"
    );
    let handle = open(path.to_str().unwrap());
    wait(handle, 0);
    let node = json!({"operation":"node.explain","project":path,"composition":doc["compositions"][0]["id"],"key":{"instance_path":[],"node":doc["compositions"][0]["nodes"][0]["id"]},"time":{"num":"0","den":"1"}});
    assert_eq!(wait(handle, call(handle, &node.to_string())), service(node));
    let render = json!({"operation":"render.explain","input":{"project":path,"composition":doc["compositions"][0]["id"],"region":{"origin":[0,0],"extent":[64,32],"pixels":[8,4]}},"time":{"num":"0","den":"1"}});
    assert_eq!(
        wait(handle, call(handle, &render.to_string())),
        service(render)
    );
    kronello_close(handle);
}

#[test]
fn detached_submit_requires_explicit_cli_worker_before_creating_job_state() {
    let h = open("missing.kronello");
    wait(h, 0);
    let request = json!({"operation":"render.submit","render":{
        "input":{"project":"missing.kronello","composition":"e706c050-13fa-4654-9350-dcef30f1d792","region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]}},
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"24","den":"1"},"output_directory":"unused"
    }});
    let id = call(h, &request.to_string());
    assert_eq!(wait(h, id)["error"]["code"], "WORKER_EXECUTABLE_REQUIRED");
    kronello_close(h);
}

#[test]
fn preview_shader_validates_without_gpu() {
    for source in [
        include_str!("../src/preview.wgsl"),
        include_str!("../src/preview_scaled.wgsl"),
    ] {
        let module = naga::front::wgsl::parse_str(source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn preview_dag_uses_shared_saved_revision_without_initializing_gpu() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    assert_eq!(
        service(json!({"operation":"project.create","project":path,"document":doc}))["status"],
        "success"
    );
    let request = json!({"operation":"render.frame","input":{"project":path,"composition":doc["compositions"][0]["id"],"region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]}},"time":{"num":"0","den":"1"}});
    let Request::RenderFrame(request) = serde_json::from_value(request).unwrap() else {
        panic!("wrong request");
    };
    let service = Service::new(BackendSelection::Gpu);
    let (revision, dag) = service.preview_dag(&request).unwrap();
    assert_eq!(revision, "1");
    assert_eq!(dag.region(), request.input.region);
    assert!(!dag.nodes().is_empty());
    let mut invalid = request;
    invalid.input.project = dir.path().join("missing.kronello");
    assert_eq!(
        service.preview_dag(&invalid).unwrap_err().code,
        "PROJECT_NOT_FOUND"
    );
}

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
fn service(request: Value) -> Value {
    serde_json::to_value(Service::new(BackendSelection::Gpu).execute_json(&request.to_string()))
        .unwrap()
}

#[test]
fn shared_requests_errors_and_explicit_project_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.kronello");
    let path = path.to_str().unwrap();
    let h = open(path);
    assert_eq!(wait(h, 0)["error"]["code"], "PROJECT_NOT_FOUND");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let request = json!({"operation":"project.create","project":path,"document":doc}).to_string();
    let id = call(h, &request);
    assert_eq!(wait(h, id)["result"]["value"]["revision"], "1");
    let request = json!({"operation":"project.info","project":path});
    let id = call(h, &request.to_string());
    assert_eq!(wait(h, id), service(request));
    for request in [
        r#"{"operation":"project.info"}"#,
        r#"{"operation":"capabilities.get","shell":"touch forbidden"}"#,
        r#"{"operation":"capabilities.get","operation":"job.list"}"#,
        r#"not JSON"#,
    ] {
        let id = call(h, request);
        let expected =
            serde_json::to_value(Service::new(BackendSelection::Gpu).execute_json(request))
                .unwrap();
        assert_eq!(wait(h, id), expected);
    }
    kronello_close(h);
    assert!(kronello_poll(h).is_null());
}

#[test]
fn invalid_inputs_backpressure_and_handle_lifetime() {
    let h = open("missing.kronello");
    wait(h, 0);
    // SAFETY: null input is rejected before dereference; no allocation is freed.
    unsafe {
        assert_eq!(kronello_call(h, std::ptr::null(), 0), 0);
        assert_eq!(kronello_open(std::ptr::null(), 0, std::ptr::null(), 0), 0);
        kronello_free(std::ptr::null_mut());
        let bytes = [0xffu8];
        assert_eq!(kronello_call(h, bytes.as_ptr(), bytes.len()), 0);
    }
    let request = r#"{"operation":"capabilities.get"}"#;
    let ids: Vec<_> = (0..64).map(|_| call(h, request)).collect();
    // SAFETY: readable request bytes; rejection is independent of processing
    // speed, because outstanding completions have not been polled.
    assert_eq!(
        unsafe { kronello_call(h, request.as_ptr(), request.len()) },
        0
    );
    for id in ids {
        assert_eq!(wait(h, id)["status"], "success");
    }
    let id = call(h, request);
    wait(h, id);
    kronello_close(h);
    kronello_close(h);
    // SAFETY: valid buffer with an invalid handle.
    assert_eq!(
        unsafe { kronello_call(h, request.as_ptr(), request.len()) },
        0
    );
}

#[test]
fn subscription_detects_external_revision_and_job_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notify.kronello");
    let path = path.to_str().unwrap();
    let mut doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    assert_eq!(
        service(json!({"operation":"project.create","project":path,"document":doc}))["status"],
        "success"
    );
    let h = open(path);
    wait(h, 0);
    wait(h, kronello_subscribe(h, true));
    doc["name"] = json!("External change");
    assert_eq!(
        service(
            json!({"operation":"project.import","project":path,"base_revision":"1","document":doc})
        )["status"],
        "success"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut revision = false;
    let mut jobs = false;
    while !(revision && jobs) {
        if let Some(v) = poll_json(h) {
            if v["notification"] == "revision_changed" {
                assert_eq!(
                    v["response"],
                    service(json!({"operation":"project.info","project":path}))
                );
                assert_eq!(v["response"]["result"]["value"]["revision"], "2");
                revision = true;
            }
            if v["notification"] == "job_progress" {
                assert_eq!(v["response"]["status"], "success");
                assert_eq!(v["response"], service(json!({"operation":"job.list"})));
                jobs = true;
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing subscription notification"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    wait(h, kronello_subscribe(h, false));
    kronello_close(h);
}

#[test]
fn preview_controls_fail_without_an_attached_surface() {
    let h = open("missing.kronello");
    wait(h, 0);
    assert_eq!(
        wait(h, kronello_surface_resize(h, 64, 32))["error"]["code"],
        "SURFACE_NOT_ATTACHED"
    );
    let json = r#"{"operation":"capabilities.get"}"#;
    // SAFETY: valid request buffer; no native layer is passed.
    let id = unsafe { kronello_surface_redraw(h, json.as_ptr(), json.len()) };
    assert_eq!(wait(h, id)["error"]["code"], "SURFACE_NOT_ATTACHED");
    // SAFETY: null layer is rejected without Objective-C access.
    assert_eq!(
        unsafe { kronello_surface_attach(h, std::ptr::null_mut(), 64, 32) },
        0
    );
    // GUI-011: every extra monitor slot tracks its own surface state.
    assert_eq!(
        wait(h, kronello_surface_resize_at(h, 1, 64, 32))["error"]["code"],
        "SURFACE_NOT_ATTACHED"
    );
    let id = unsafe { kronello_surface_redraw_at(h, 2, json.as_ptr(), json.len()) };
    assert_eq!(wait(h, id)["error"]["code"], "SURFACE_NOT_ATTACHED");
    assert_eq!(
        unsafe { kronello_surface_attach_at(h, 1, std::ptr::null_mut(), 64, 32) },
        0
    );
    kronello_close(h);
}

#[test]
fn header_matches_every_exported_function_signature() {
    // Rust type checks and matching C declarations lock down both sides of ABI.
    let _: unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> u64 = kronello_open;
    let _: extern "C" fn(u64) = kronello_close;
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> u64 = kronello_call;
    let _: extern "C" fn(u64, bool) -> u64 = kronello_subscribe;
    let _: extern "C" fn(u64) -> *mut c_char = kronello_poll;
    let _: unsafe extern "C" fn(*mut c_char) = kronello_free;
    let _: unsafe extern "C" fn(u64, *mut c_void, u32, u32) -> u64 = kronello_surface_attach;
    let _: extern "C" fn(u64, u32, u32) -> u64 = kronello_surface_resize;
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> u64 = kronello_surface_redraw;
    let _: unsafe extern "C" fn(u64, u32, *mut c_void, u32, u32) -> u64 =
        kronello_surface_attach_at;
    let _: extern "C" fn(u64, u32, u32, u32) -> u64 = kronello_surface_resize_at;
    let _: unsafe extern "C" fn(u64, u32, *const u8, usize) -> u64 = kronello_surface_redraw_at;
    let _: unsafe extern "C" fn(*const u8, usize, *mut bool, *mut *mut c_char) -> *mut c_void =
        kronello_audio_prepare;
    let _: unsafe extern "C" fn(*const c_void, i64, usize, *mut f32, *mut *mut c_char) -> bool =
        kronello_audio_render;
    let _: unsafe extern "C" fn(
        *const c_void,
        i64,
        usize,
        *mut f32,
        *mut *mut c_char,
        *mut *mut c_char,
    ) -> bool = kronello_audio_render_metered;
    let _: unsafe extern "C" fn(*mut c_void) = kronello_audio_free;
    let header = include_str!("../../../apps/macos/Sources/CKronelloFFI/include/kronello.h");
    for declaration in [
        "uint64_t kronello_open(const uint8_t *path, size_t len, const uint8_t *worker_executable, size_t worker_executable_len);",
        "void kronello_close(uint64_t handle);",
        "uint64_t kronello_call(uint64_t handle, const uint8_t *json, size_t len);",
        "uint64_t kronello_subscribe(uint64_t handle, bool enable);",
        "char *kronello_poll(uint64_t handle);",
        "void kronello_free(char *ptr);",
        "uint64_t kronello_surface_attach(uint64_t handle, void *metal_layer, uint32_t width, uint32_t height);",
        "uint64_t kronello_surface_resize(uint64_t handle, uint32_t width, uint32_t height);",
        "uint64_t kronello_surface_redraw(uint64_t handle, const uint8_t *json, size_t len);",
        "uint64_t kronello_surface_attach_at(uint64_t handle, uint32_t surface, void *metal_layer, uint32_t width, uint32_t height);",
        "uint64_t kronello_surface_resize_at(uint64_t handle, uint32_t surface, uint32_t width, uint32_t height);",
        "uint64_t kronello_surface_redraw_at(uint64_t handle, uint32_t surface, const uint8_t *json, size_t len);",
        "void *kronello_audio_prepare(const uint8_t *json, size_t len, bool *has_audio, char **error);",
        "bool kronello_audio_render(const void *resource, int64_t start_sample, size_t frames, float *output, char **error);",
        "bool kronello_audio_render_metered(const void *resource, int64_t start_sample, size_t frames, float *output, char **meters, char **error);",
        "void kronello_audio_free(void *resource);",
    ] {
        assert!(
            header.lines().any(|l| l == declaration),
            "missing {declaration}"
        );
    }
    assert_eq!(
        header
            .lines()
            .filter(|l| l.starts_with("uint64_t kronello_")
                || l.starts_with("void kronello_")
                || l.starts_with("void *kronello_")
                || l.starts_with("bool kronello_")
                || l.starts_with("char *kronello_"))
            .count(),
        16
    );
}

#[test]
fn json_order_ffi_worker_boundary() {
    let handle = open("missing-json-order.kronello");
    wait(handle, 0);
    let response = wait(
        handle,
        call(
            handle,
            r#"{"commands":[{"property_source_set":{"source":{"value":{"value":1.5,"kind":"scalar"},"kind":"constant"},"property":"00000000-0000-0000-0000-000000000002","object":"00000000-0000-0000-0000-000000000001"}}],"base_revision":"0","project":"missing-json-order.kronello","operation":"edit.plan"}"#,
        ),
    );
    kronello_close(handle);
    assert_eq!(response["error"]["code"], "PROJECT_NOT_FOUND");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("json-order.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let node = &doc["compositions"][0]["nodes"][0];
    let property = node["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();

    assert_eq!(
        service(json!({"operation":"project.create","project":path,"document":doc}))["status"],
        "success"
    );
    let handle = open(path.to_str().unwrap());
    wait(handle, 0);
    let canonical = json!({"operation":"edit.plan","project":path,"base_revision":"1","commands":[{"property_source_set":{"object":node["id"],"property":property["id"],"source":{"kind":"constant","value":{"kind":"scalar","value":0.5}}}}]});
    let raw = canonical.to_string().replace(
        r#""kind":"scalar","value":0.5"#,
        r#""value":0.5,"kind":"scalar""#,
    );
    assert_ne!(raw, canonical.to_string());
    assert_eq!(wait(handle, call(handle, &raw)), service(canonical));
    let invalid = raw.replace(r#""value":0.5"#, r#""value":"bad""#);
    let response = wait(handle, call(handle, &invalid));
    kronello_close(handle);
    assert_eq!(response["error"]["code"], "INVALID_REQUEST");
}
