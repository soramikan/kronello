//! Asynchronous native transport using the shared service for all edits.
//! Unsafe is limited to C buffer ownership and native Metal layer interop.
#![allow(unsafe_code)]
mod audio;
mod output;
mod preview;
pub use audio::*;
use kronello_service::{
    AudioPreparationInput, AudioPrepareRequest, BackendSelection, ProjectRequest, ProjectSession,
    Request, Response, ResultData, Service, ServiceError,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    ffi::{CStr, CString, c_char, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
const MAX_JSON: usize = 16 * 1024 * 1024;
const CAPACITY: usize = 64;
struct Session {
    path: std::path::PathBuf,
    sender: mpsc::SyncSender<(u64, Work)>,
    output: Arc<Mutex<VecDeque<Value>>>,
    outstanding: Mutex<usize>,
}
enum Work {
    CaptureAudio(
        AudioPrepareRequest,
        mpsc::SyncSender<Result<AudioPreparationInput, ServiceError>>,
    ),
    Call(String),
    Subscribe(bool),
    /// GUI-011: surfaces are addressed by a slot so one session can drive the
    /// Source and Program monitors (and future panels) over the same retained
    /// store/worker. Slot 0 is the original single-surface ABI.
    Attach(u32, preview::Layer, u32, u32),
    Resize(u32, u32, u32),
    Redraw(u32, String),
}
static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    SESSIONS.get_or_init(Default::default)
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn guard<T: Default>(f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_default()
}
fn error(e: &ServiceError) -> Value {
    serde_json::to_value(Response::Error { error: e.clone() }).expect("response JSON")
}
/// Wrap a worker-side `ResultData` outcome as the shared Response shape so
/// io.output.* answers are indistinguishable from service answers.
fn respond(result: Result<ResultData, ServiceError>) -> Response {
    match result {
        Ok(result) => Response::Success { result },
        Err(error) => Response::Error { error },
    }
}
// SAFETY: caller supplies readable bytes. Copy before returning; the worker
// never borrows foreign memory. Null and oversized buffers are rejected.
unsafe fn input(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len > MAX_JSON {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}
fn enqueue(handle: u64, work: Work) -> u64 {
    let sessions = lock(sessions());
    let Some(s) = sessions.get(&handle) else {
        return 0;
    };
    let mut outstanding = lock(&s.outstanding);
    if *outstanding >= CAPACITY {
        return 0;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if s.sender.try_send((id, work)).is_err() {
        return 0;
    }
    *outstanding += 1;
    id
}
fn info(service: &Service<'_>, path: &str) -> Value {
    serde_json::to_value(service.execute(Request::ProjectInfo(ProjectRequest {
        project: path.into(),
    })))
    .expect("response JSON")
}
fn publish(output: &Mutex<VecDeque<Value>>, value: Value) {
    lock(output).push_back(value);
}
fn notify(output: &Mutex<VecDeque<Value>>, name: &str, response: Value) {
    let mut q = lock(output);
    // Coalesce snapshots without dropping accepted request completions.
    q.retain(|v| v["notification"] != name);
    q.push_back(json!({"notification":name,"response_json":response.to_string()}));
}
fn capture_session_audio(
    request: AudioPrepareRequest,
) -> Result<Option<AudioPreparationInput>, ServiceError> {
    let canonical = request.project.canonicalize().ok();
    let sender = {
        let entries = lock(sessions());
        entries
            .values()
            .find(|entry| match &canonical {
                Some(path) => entry.path.canonicalize().ok().as_ref() == Some(path),
                None => entry.path == request.project,
            })
            .map(|entry| entry.sender.clone())
    };
    let Some(sender) = sender else {
        return Ok(None);
    };
    let (reply, result) = mpsc::sync_channel(1);
    sender
        .try_send((0, Work::CaptureAudio(request, reply)))
        .map_err(|_| ServiceError::new("BACKPRESSURE", "audio snapshot queue unavailable"))?;
    result
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| {
            ServiceError::new("SESSION_CLOSED", "audio snapshot capture did not complete")
        })?
        .map(Some)
}
fn with_session<T>(session: &Option<ProjectSession>, operation: impl FnOnce() -> T) -> T {
    match session {
        Some(session) => session.scope(operation),
        None => operation(),
    }
}
fn worker(
    path: String,
    executable: Option<String>,
    receiver: mpsc::Receiver<(u64, Work)>,
    output: Arc<Mutex<VecDeque<Value>>>,
) {
    let mut service = Service::new(BackendSelection::Gpu);
    let has_worker = executable.is_some();
    if let Some(executable) = executable {
        service = service.with_worker_executable(executable.into());
    }
    let mut session = None;
    let mut open_error = None;
    match ProjectSession::open(std::path::Path::new(&path)) {
        Ok(value) => session = Some(value),
        Err(e) if e.code == "PROJECT_NOT_FOUND" => (),
        Err(e) => open_error = Some(error(&e)),
    }
    let mut previous = open_error
        .clone()
        .unwrap_or_else(|| with_session(&session, || info(&service, &path)));
    publish(
        &output,
        json!({"request_id":0,"response_json":previous.to_string()}),
    );
    let mut previous_jobs = Value::Null;
    let mut subscribed = false;
    let mut previews: HashMap<u32, preview::Preview> = HashMap::new();
    // IO-001: external output devices bound to this session's program GPU
    // context. Empty until the caller attaches/enables a route explicitly.
    let mut outputs = output::OutputSet::default();
    loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok((id, work)) => {
                if let Work::CaptureAudio(request, reply) = work {
                    let result = if let Some(value) = &open_error {
                        Err(
                            serde_json::from_value::<ServiceError>(value["error"].clone())
                                .expect("typed session error"),
                        )
                    } else {
                        catch_unwind(AssertUnwindSafe(|| {
                            with_session(&session, || service.capture_audio_input(request))
                        }))
                        .unwrap_or_else(|_| {
                            Err(ServiceError::new(
                                "FFI_PANIC",
                                "audio snapshot capture panicked",
                            ))
                        })
                    };
                    let _ = reply.send(result);
                    continue;
                }
                // A session created before project.create adopts its store on
                // the next request. An initially locked/failed session remains
                // failed even after an external holder releases its lock.
                if session.is_none()
                    && open_error.is_none()
                    && std::path::Path::new(&path).is_file()
                {
                    match ProjectSession::open(std::path::Path::new(&path)) {
                        Ok(value) => session = Some(value),
                        Err(e) => open_error = Some(error(&e)),
                    }
                }
                let result = open_error.clone().unwrap_or_else(|| {
                    with_session(&session, || {
                        catch_unwind(AssertUnwindSafe(|| match work {
                    Work::CaptureAudio(..) => unreachable!("capture handled separately"),
                    Work::Call(json) => {
                        // Use the exact shared strict Request decoder. The embedder
                        // must provide the CLI worker; a Swift executable cannot
                        // implement the service's detached worker_entry protocol.
                        let response = match serde_json::from_str::<Request>(&json) {
                            Ok(Request::RenderSubmit(_)) if !has_worker => Response::Error {
                                error: ServiceError::new(
                                    "WORKER_EXECUTABLE_REQUIRED",
                                    "open with a same-version kronello CLI to submit jobs",
                                ),
                            },
                            // IO-001: output commands run on this worker's
                            // device set, not the service's headless path.
                            Ok(Request::IoOutputList(_)) => respond(Ok(
                                ResultData::OutputDevices(outputs.list()),
                            )),
                            Ok(Request::IoOutputEnable(r)) => respond(
                                outputs
                                    .enable(&r, previews.get(&0))
                                    .map(ResultData::OutputState),
                            ),
                            Ok(Request::IoOutputDisable(r)) => respond(Ok(
                                ResultData::OutputState(outputs.disable(r.kind)),
                            )),
                            Ok(request) => service.execute(request),
                            Err(e) => Response::Error { error: e.into() },
                        };
                        serde_json::to_value(response).expect("response JSON")
                    }
                    Work::Subscribe(enable) => {
                        subscribed = enable;
                        json!({"status":"success"})
                    }
                    // IO-001: the reference-monitor slot binds its surface to
                    // the program preview's device so the same frame can fan
                    // out without cross-device copies.
                    Work::Attach(slot, layer, w, h) if slot == output::REF_MONITOR_SLOT => {
                        match previews.get(&0) {
                            Some(program) => match program.output_presentation(layer, w, h) {
                                Ok(presentation) => {
                                    outputs.attach_ref_monitor(presentation);
                                    json!({"status":"success"})
                                }
                                Err(e) => error(&e),
                            },
                            None => error(&ServiceError::new(
                                "SURFACE_UNAVAILABLE",
                                "attach the program monitor surface before the reference monitor",
                            )),
                        }
                    }
                    Work::Attach(slot, layer, w, h) => match preview::Preview::attach(layer, w, h) {
                        Ok(p) => {
                            // Replacing a slot releases the previous surface
                            // only after its queued work has finished (FIFO).
                            previews.insert(slot, p);
                            json!({"status":"success"})
                        }
                        Err(e) => error(&e),
                    },
                    Work::Resize(slot, w, h) if slot == output::REF_MONITOR_SLOT => {
                        match previews.get(&0) {
                            Some(program) => outputs
                                .resize_ref_monitor(program, w, h)
                                .map(|()| json!({"status":"success"}))
                                .unwrap_or_else(|e| error(&e)),
                            None => error(&ServiceError::new(
                                "SURFACE_UNAVAILABLE",
                                "attach the program monitor surface before the reference monitor",
                            )),
                        }
                    }
                    Work::Resize(slot, w, h) => match previews.get_mut(&slot) {
                        Some(p) => p
                            .resize(w, h)
                            .map(|()| json!({"status":"success"}))
                            .unwrap_or_else(|e| error(&e)),
                        None => error(&ServiceError::new("SURFACE_NOT_ATTACHED", "attach a surface first")),
                    },
                    // IO-001: the program redraw is the single frame source.
                    // After the program present, every explicitly enabled
                    // output presents the identical frame; per-route outcomes
                    // are reported without failing the monitor itself.
                    Work::Redraw(0, ref json) => match previews.get_mut(&0) {
                        Some(p) => match p.render_frame(&service, json) {
                            Ok(source) => {
                                let mut response = p
                                    .present(&source)
                                    .unwrap_or_else(|e| error(&e));
                                if outputs.has_active() {
                                    let report = outputs.present_all(&source, p);
                                    if let Some(preview) = response
                                        .get_mut("preview")
                                        .and_then(Value::as_object_mut)
                                    {
                                        preview.insert("outputs".into(), report);
                                    }
                                }
                                response
                            }
                            Err(e) => error(&e),
                        },
                        None => error(&ServiceError::new("SURFACE_NOT_ATTACHED", "attach a surface first")),
                    },
                    Work::Redraw(slot, json) => match previews.get_mut(&slot) {
                        Some(p) => p
                            .redraw(&service, &json)
                            .unwrap_or_else(|e| error(&e)),
                        None => error(&ServiceError::new("SURFACE_NOT_ATTACHED", "attach a surface first")),
                    },
                }))
                .unwrap_or_else(|_| error(&ServiceError::new("FFI_PANIC", "native worker panicked; request failed")))
                    })
                });
                publish(
                    &output,
                    json!({"request_id":id,"response_json":result.to_string()}),
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if subscribed {
            let current = open_error
                .clone()
                .unwrap_or_else(|| with_session(&session, || info(&service, &path)));
            if current != previous {
                notify(&output, "revision_changed", current.clone());
                previous = current;
            }
            let jobs = serde_json::to_value(with_session(&session, || {
                service.execute(Request::JobList(Default::default()))
            }))
            .expect("response JSON");
            if jobs != previous_jobs {
                notify(&output, "job_progress", jobs.clone());
                previous_jobs = jobs;
            }
        }
    }
}
/// Start a session; completion 0 contains shared project.info. All subsequent
/// requests still carry explicit project paths. Optional same-version CLI
/// executable enables detached jobs. Returns zero on input/thread failure.
/// # Safety
/// Non-null pointers must be readable for their specified lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_open(
    path: *const u8,
    len: usize,
    executable: *const u8,
    executable_len: usize,
) -> u64 {
    guard(|| {
        // SAFETY: the C caller guarantees readable path bytes for this call.
        let Some(path) = (unsafe { input(path, len) }) else {
            return 0;
        };
        let executable = if executable.is_null() && executable_len == 0 {
            None
        } else {
            // SAFETY: the same C buffer contract applies to the worker path.
            let Some(value) = (unsafe { input(executable, executable_len) }) else {
                return 0;
            };
            Some(value)
        };
        let session_path = std::path::PathBuf::from(&path);
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        let output = Arc::new(Mutex::new(VecDeque::new()));
        let worker_output = output.clone();
        if std::thread::Builder::new()
            .name("kronello-ffi".into())
            .spawn(move || worker(path, executable, receiver, worker_output))
            .is_err()
        {
            return 0;
        }
        let handle = NEXT.fetch_add(1, Ordering::Relaxed);
        lock(sessions()).insert(
            handle,
            Session {
                path: session_path,
                sender,
                output,
                outstanding: Mutex::new(0),
            },
        );
        handle
    })
}
/// Stop accepting work; queued work finishes and then releases the retained
/// layer. Does not join the worker or block on disk/GPU operations.
#[unsafe(no_mangle)]
pub extern "C" fn kronello_close(handle: u64) {
    guard(|| {
        lock(sessions()).remove(&handle);
    });
}
/// Queue the exact shared Request JSON. Returns zero on invalid input/handle
/// or backpressure (64 unpolled requests maximum).
/// # Safety
/// json must be readable for len bytes during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_call(handle: u64, json: *const u8, len: usize) -> u64 {
    guard(|| {
        // SAFETY: caller-owned JSON stays readable until this copy returns.
        unsafe { input(json, len) }
            .map(|s| enqueue(handle, Work::Call(s)))
            .unwrap_or(0)
    })
}
/// Poll subscription: coalesced revision/job snapshots at idle 250ms intervals
/// and after requests. All notifications include the shared response JSON.
#[unsafe(no_mangle)]
pub extern "C" fn kronello_subscribe(handle: u64, enable: bool) -> u64 {
    guard(|| enqueue(handle, Work::Subscribe(enable)))
}
/// Return owned NUL-terminated UTF-8 JSON or null if empty/closed. Each result
/// must be freed exactly once with kronello_free.
#[unsafe(no_mangle)]
pub extern "C" fn kronello_poll(handle: u64) -> *mut c_char {
    guard(|| {
        let sessions = lock(sessions());
        let Some(s) = sessions.get(&handle) else {
            return std::ptr::null_mut();
        };
        let Some(value) = lock(&s.output).pop_front() else {
            return std::ptr::null_mut();
        };
        if value["request_id"].as_u64().is_some_and(|id| id != 0) {
            *lock(&s.outstanding) -= 1;
        }
        CString::new(value.to_string())
            .expect("escaped JSON has no NUL")
            .into_raw()
    })
}
/// # Safety
/// ptr must be null or an unfreed allocation from kronello_poll.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_free(ptr: *mut c_char) {
    guard(|| {
        if !ptr.is_null() {
            // SAFETY: the caller returns an unfreed poll allocation once.
            drop(unsafe { CString::from_raw(ptr) });
        }
    });
}
/// Retain a CAMetalLayer, then queue GPU initialization. Release on replacement
/// or worker shutdown, after surface use has ended.
/// # Safety
/// layer must be a live CAMetalLayer on macOS. Call on the main thread after
/// installing it on an NSView; do not mutate device/pixelFormat afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_surface_attach(
    handle: u64,
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> u64 {
    // SAFETY: same layer contract; slot 0 keeps the original surface.
    unsafe { kronello_surface_attach_at(handle, 0, layer, width, height) }
}
#[unsafe(no_mangle)]
pub extern "C" fn kronello_surface_resize(handle: u64, width: u32, height: u32) -> u64 {
    kronello_surface_resize_at(handle, 0, width, height)
}
/// Same render.frame JSON as CLI/MCP; returns presentation metadata only.
/// # Safety
/// json must be readable for len bytes during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_surface_redraw(handle: u64, json: *const u8, len: usize) -> u64 {
    // SAFETY: same JSON buffer contract; slot 0 keeps the original surface.
    unsafe { kronello_surface_redraw_at(handle, 0, json, len) }
}
/// GUI-011: slotted variants address an independent preview surface on the
/// same session/worker, so the Source and Program monitors share the retained
/// store, subscriptions and serialized request pipeline.
/// # Safety
/// Same contracts as the unslotted surface functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_surface_attach_at(
    handle: u64,
    surface: u32,
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> u64 {
    guard(|| {
        if layer.is_null() || width == 0 || height == 0 {
            return 0;
        }
        // SAFETY: the caller supplies a live installed CAMetalLayer. Retain
        // synchronously so queued work never borrows caller-owned lifetime.
        enqueue(
            handle,
            Work::Attach(
                surface,
                unsafe { preview::Layer::retain(layer) },
                width,
                height,
            ),
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn kronello_surface_resize_at(
    handle: u64,
    surface: u32,
    width: u32,
    height: u32,
) -> u64 {
    guard(|| enqueue(handle, Work::Resize(surface, width, height)))
}
/// # Safety
/// json must be readable for len bytes during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_surface_redraw_at(
    handle: u64,
    surface: u32,
    json: *const u8,
    len: usize,
) -> u64 {
    guard(|| {
        // SAFETY: caller-owned request bytes are copied before returning.
        unsafe { input(json, len) }
            .map(|s| enqueue(handle, Work::Redraw(surface, s)))
            .unwrap_or(0)
    })
}
/// Rust-only test convenience exercising the allocation/free contract.
pub fn poll_json(handle: u64) -> Option<Value> {
    let ptr = kronello_poll(handle);
    if ptr.is_null() {
        return None;
    }
    // SAFETY: freshly returned allocation, freed once after decoding.
    let mut value: Value =
        unsafe { serde_json::from_slice(CStr::from_ptr(ptr).to_bytes()).expect("poll JSON") };
    // SAFETY: ptr is the unfreed allocation acquired above.
    unsafe { kronello_free(ptr) };
    // Tests inspect decoded responses; C clients receive exact JSON text so
    // their transport decoder cannot round arbitrary-precision project data.
    value["response"] =
        serde_json::from_str(value["response_json"].as_str().expect("response text"))
            .expect("response JSON");
    Some(value)
}
