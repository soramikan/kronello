//! Deterministic in-repository VST3 fixture (AUDIO-011 acceptance).
//!
//! `FIXTURE_SOURCE` is a hand-written Rust plugin implementing the public
//! COM-compatible VST3 ABI — one stereo `Kronello Test Gain` class with a
//! single normalized parameter (id `0` = gain). It is compiled on the fly
//! with the same `rustc` that built the test (`rustc --edition 2021
//! --crate-type cdylib`) and laid out as a `.vst3` bundle, so the host →
//! load → process → unload roundtrip is exercised without any Steinberg
//! SDK code and without a commercial plugin.
//!
//! Fixture test hooks (read inside `process`):
//! - `KRONELLO_PLUGIN_FIXTURE_CRASH=1` → `std::process::abort()`;
//! - `KRONELLO_PLUGIN_FIXTURE_HANG_MS=<n>` → spin-sleep per block;
//! - `KRONELLO_PLUGIN_FIXTURE_LOG=<path>` → append lifecycle markers
//!   (`module_entry`, `process`, `module_exit`) for unload verification.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Component class id of the fixture: hex of `b"kronellotestgain"`.
pub const FIXTURE_CLASS_ID: &str = "6b726f6e656c6c6f746573746761696e";

fn platform_dir() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "MacOS"
    }
    #[cfg(target_os = "linux")]
    {
        if cfg!(target_arch = "aarch64") {
            "aarch64-linux"
        } else {
            "x86_64-linux"
        }
    }
    #[cfg(target_os = "windows")]
    {
        if cfg!(target_arch = "aarch64") {
            "arm64_64-win"
        } else {
            "x86_64-win"
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    compile_error!("no vst3 module layout for this platform")
}

/// Compile the fixture and lay it out as `<dir>/kronello-test-gain.vst3`.
/// Returns the bundle directory.
pub fn build_fixture_bundle(dir: &Path) -> std::io::Result<PathBuf> {
    let source_path = dir.join("kronello_test_gain.rs");
    std::fs::write(&source_path, FIXTURE_SOURCE)?;
    let bundle = dir.join("kronello-test-gain.vst3");
    let module_dir = bundle.join("Contents").join(platform_dir());
    std::fs::create_dir_all(&module_dir)?;
    std::fs::write(
        bundle.join("Contents").join("Info.plist"),
        b"<?xml version=\"1.0\"?><plist><dict/></plist>\n",
    )?;
    let module = module_dir.join(format!(
        "kronello-test-gain.{}",
        std::env::consts::DLL_EXTENSION
    ));
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let status = Command::new(rustc)
        .args([
            "--edition",
            "2021",
            "-C",
            "opt-level=2",
            "--crate-type",
            "cdylib",
            "-o",
        ])
        .arg(&module)
        .arg(&source_path)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "rustc exited {status} building the vst3 fixture"
        )));
    }
    Ok(bundle)
}

/// Plugin source compiled by [`build_fixture_bundle`]. Kept as a string so
/// the fixture and the host ABI stay in one audited diff.
pub const FIXTURE_SOURCE: &str = r#"
// Deterministic hand-written VST3 test plugin (Kronello "Test Gain").
// Implements the public COM-compatible ABI directly — no SDK code.
#![allow(non_snake_case, dead_code)]
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;

type TResult = i32;
const OK: TResult = 0;
const FALSE: TResult = 1;
const INVALID_ARG: TResult = 2;
const NOT_IMPL: TResult = 3;
const NO_INTERFACE: TResult = -1;

const IID_FUNKNOWN: [u8; 16] =
    [0, 0, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0x46, 0];
const IID_IPLUGINBASE: [u8; 16] =
    [0xDB, 0x8D, 0x88, 0x22, 0x6E, 0x15, 0xAE, 0x45, 0x83, 0x58, 0xB3, 0x48, 0x19, 0x08, 0x25, 0x06];
const IID_IPLUGINFACTORY: [u8; 16] =
    [0x60, 0xCE, 0xA7, 0x7A, 0x59, 0xCB, 0x3B, 0x4F, 0x8F, 0x65, 0x8F, 0x16, 0xE5, 0x33, 0xF0, 0x34];
const IID_IPLUGINFACTORY2: [u8; 16] =
    [0x50, 0xB6, 0x07, 0x00, 0x4B, 0xF2, 0x0B, 0x4C, 0xA4, 0x64, 0xED, 0xB9, 0x0B, 0xF0, 0xBB, 0x2A];
const IID_ICOMPONENT: [u8; 16] =
    [0x31, 0xFF, 0x31, 0xE8, 0xD5, 0xF2, 0x01, 0x43, 0x92, 0x8E, 0xBB, 0xEE, 0x69, 0x25, 0x02, 0x78];
const IID_IAUDIO: [u8; 16] =
    [0x99, 0x3F, 0x04, 0x42, 0xDA, 0xB7, 0x3C, 0x45, 0xA5, 0x69, 0xE7, 0x9D, 0xAE, 0x9A, 0x3D, 0xC3];
const IID_IEDIT: [u8; 16] =
    [0xE3, 0xBB, 0xD7, 0xDC, 0x42, 0x77, 0x8D, 0x44, 0xA8, 0x74, 0xAA, 0xCC, 0x9C, 0x97, 0x9E, 0x75];
const CLASS_ID: [u8; 16] = *b"kronellotestgain";

fn mark(tag: &str) {
    if let Ok(path) = std::env::var("KRONELLO_PLUGIN_FIXTURE_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{tag}");
        }
    }
}

// ------------------------------------------------------------- layout ----
#[repr(C)]
struct FUnknownVtbl {
    query_interface: unsafe extern "C" fn(*mut c_void, *const u8, *mut *mut c_void) -> TResult,
    add_ref: unsafe extern "C" fn(*mut c_void) -> u32,
    release: unsafe extern "C" fn(*mut c_void) -> u32,
}
#[repr(C)]
struct PClassInfo {
    cid: [u8; 16],
    cardinality: i32,
    category: [u8; 32],
    name: [u8; 64],
}
#[repr(C)]
struct PClassInfo2 {
    cid: [u8; 16],
    cardinality: i32,
    category: [u8; 32],
    name: [u8; 64],
    class_flags: i32,
    sub_categories: [u8; 128],
    vendor: [u8; 64],
    version: [u8; 64],
    sdk_version: i32,
}
#[repr(C)]
struct IPluginFactoryVtbl {
    base: FUnknownVtbl,
    count_classes: unsafe extern "C" fn(*mut c_void) -> i32,
    get_class_info: unsafe extern "C" fn(*mut c_void, i32, *mut PClassInfo) -> TResult,
    create_instance:
        unsafe extern "C" fn(*mut c_void, *const u8, *const u8, *mut *mut c_void) -> TResult,
}
#[repr(C)]
struct IPluginFactory2Vtbl {
    factory: IPluginFactoryVtbl,
    get_class_info2: unsafe extern "C" fn(*mut c_void, i32, *mut PClassInfo2) -> TResult,
    get_class_info_unicode: unsafe extern "C" fn(*mut c_void, i32, *mut c_void) -> TResult,
    set_host_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
}
#[repr(C)]
struct IPluginBaseVtbl {
    base: FUnknownVtbl,
    initialize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    terminate: unsafe extern "C" fn(*mut c_void) -> TResult,
}
#[repr(C)]
struct BusInfo {
    media_type: i32,
    direction: i32,
    channel_count: i32,
    name: [u16; 128],
    bus_type: i32,
    flags: u32,
}
#[repr(C)]
struct IComponentVtbl {
    plugin_base: IPluginBaseVtbl,
    get_controller_class_id: unsafe extern "C" fn(*mut c_void, *mut u8) -> TResult,
    set_io_mode: unsafe extern "C" fn(*mut c_void, i32) -> TResult,
    get_bus_count: unsafe extern "C" fn(*mut c_void, i32, i32) -> i32,
    get_bus_info: unsafe extern "C" fn(*mut c_void, i32, i32, i32, *mut BusInfo) -> TResult,
    get_routing_info: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> TResult,
    activate_bus: unsafe extern "C" fn(*mut c_void, i32, i32, i32, u8) -> TResult,
    set_active: unsafe extern "C" fn(*mut c_void, u8) -> TResult,
    set_state: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    get_state: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
}
#[repr(C)]
struct ProcessSetup {
    process_mode: i32,
    symbolic_sample_size: i32,
    max_samples_per_block: i32,
    sample_rate: f64,
}
#[repr(C)]
struct AudioBusBuffers {
    num_channels: i32,
    silence_flags: u64,
    channel_buffers32: *mut *mut f32,
}
#[repr(C)]
struct ProcessData {
    process_mode: i32,
    symbolic_sample_size: i32,
    num_samples: i32,
    num_inputs: i32,
    num_outputs: i32,
    inputs: *mut AudioBusBuffers,
    outputs: *mut AudioBusBuffers,
    input_parameter_changes: *mut c_void,
    output_parameter_changes: *mut c_void,
    input_events: *mut c_void,
    output_events: *mut c_void,
    process_context: *mut c_void,
}
#[repr(C)]
struct IAudioProcessorVtbl {
    base: FUnknownVtbl,
    set_bus_arrangements:
        unsafe extern "C" fn(*mut c_void, *const u64, i32, *const u64, i32) -> TResult,
    get_bus_arrangement: unsafe extern "C" fn(*mut c_void, i32, i32, *mut u64) -> TResult,
    can_process_sample_size: unsafe extern "C" fn(*mut c_void, i32) -> TResult,
    get_latency_samples: unsafe extern "C" fn(*mut c_void) -> u32,
    setup_processing: unsafe extern "C" fn(*mut c_void, *const ProcessSetup) -> TResult,
    set_processing: unsafe extern "C" fn(*mut c_void, u8) -> TResult,
    process: unsafe extern "C" fn(*mut c_void, *mut ProcessData) -> TResult,
    get_tail_samples: unsafe extern "C" fn(*mut c_void) -> u32,
}
#[repr(C)]
struct ParameterInfo {
    id: u32,
    title: [u16; 128],
    short_title: [u16; 128],
    units: [u16; 128],
    step_count: i32,
    default_normalized_value: f64,
    unit_id: i32,
    flags: i32,
}
#[repr(C)]
struct IEditControllerVtbl {
    plugin_base: IPluginBaseVtbl,
    set_component_state: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    set_state: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    get_state: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    get_parameter_count: unsafe extern "C" fn(*mut c_void) -> i32,
    get_parameter_info: unsafe extern "C" fn(*mut c_void, i32, *mut ParameterInfo) -> TResult,
    get_param_string_by_value: unsafe extern "C" fn(*mut c_void, u32, f64, *mut u16) -> TResult,
    get_param_value_by_string: unsafe extern "C" fn(*mut c_void, *const u16, *mut f64) -> TResult,
    normalized_param_to_plain: unsafe extern "C" fn(*mut c_void, u32, f64) -> f64,
    plain_param_to_normalized: unsafe extern "C" fn(*mut c_void, u32, f64) -> f64,
    set_param_normalized: unsafe extern "C" fn(*mut c_void, u32, f64) -> TResult,
    set_component_handler: unsafe extern "C" fn(*mut c_void, *mut c_void) -> TResult,
    create_view: unsafe extern "C" fn(*mut c_void, *const i8) -> *mut c_void,
}

// -------------------------------------------------------- plugin object --
#[repr(C)]
struct Obj {
    comp_vt: *const IComponentVtbl,
    audio_vt: *const IAudioProcessorVtbl,
    edit_vt: *const IEditControllerVtbl,
    refs: Cell<u32>,
    gain: Cell<f64>,
}
unsafe fn base(this: *mut c_void, off: usize) -> *mut Obj {
    (this as *mut u8).sub(off) as *mut Obj
}
unsafe fn addref_at(b: *mut Obj) -> u32 {
    let o = unsafe { &*b };
    let n = o.refs.get() + 1;
    o.refs.set(n);
    n
}
unsafe fn release_at(b: *mut Obj) -> u32 {
    let o = unsafe { &*b };
    let n = o.refs.get().saturating_sub(1);
    o.refs.set(n);
    if n == 0 {
        drop(unsafe { Box::from_raw(b) });
    }
    n
}
unsafe extern "C" fn comp_addref(this: *mut c_void) -> u32 {
    unsafe { addref_at(base(this, 0)) }
}
unsafe extern "C" fn comp_release(this: *mut c_void) -> u32 {
    unsafe { release_at(base(this, 0)) }
}
unsafe extern "C" fn audio_addref(this: *mut c_void) -> u32 {
    unsafe { addref_at(base(this, 8)) }
}
unsafe extern "C" fn audio_release(this: *mut c_void) -> u32 {
    unsafe { release_at(base(this, 8)) }
}
unsafe extern "C" fn edit_addref(this: *mut c_void) -> u32 {
    unsafe { addref_at(base(this, 16)) }
}
unsafe extern "C" fn edit_release(this: *mut c_void) -> u32 {
    unsafe { release_at(base(this, 16)) }
}
unsafe fn qi_impl(base_ptr: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    let iid = unsafe { std::slice::from_raw_parts(iid, 16) };
    let target = if iid == IID_FUNKNOWN || iid == IID_IPLUGINBASE || iid == IID_ICOMPONENT {
        base_ptr
    } else if iid == IID_IAUDIO {
        unsafe { (base_ptr as *mut u8).add(8) as *mut c_void }
    } else if iid == IID_IEDIT {
        unsafe { (base_ptr as *mut u8).add(16) as *mut c_void }
    } else {
        unsafe { *out = ptr::null_mut() };
        return NO_INTERFACE;
    };
    unsafe {
        *out = target;
        comp_addref(base_ptr);
    }
    OK
}
unsafe extern "C" fn comp_qi(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    unsafe { qi_impl(this, iid, out) }
}
unsafe extern "C" fn audio_qi(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    unsafe { qi_impl(base(this, 8) as *mut c_void, iid, out) }
}
unsafe extern "C" fn edit_qi(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    unsafe { qi_impl(base(this, 16) as *mut c_void, iid, out) }
}

// ------------------------------------------------------------- IComponent -
unsafe extern "C" fn comp_initialize(_this: *mut c_void, _ctx: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn comp_terminate(_this: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn comp_get_controller_class_id(_this: *mut c_void, out: *mut u8) -> TResult {
    // Single component: IEditController is reached by QI on the component.
    unsafe { ptr::copy_nonoverlapping(CLASS_ID.as_ptr(), out, 16) };
    FALSE
}
unsafe extern "C" fn comp_set_io_mode(_this: *mut c_void, _mode: i32) -> TResult {
    OK
}
unsafe extern "C" fn comp_get_bus_count(_this: *mut c_void, media_type: i32, _dir: i32) -> i32 {
    if media_type == 0 { 1 } else { 0 }
}
unsafe extern "C" fn comp_get_bus_info(
    _this: *mut c_void,
    media_type: i32,
    dir: i32,
    index: i32,
    info: *mut BusInfo,
) -> TResult {
    if media_type != 0 || index != 0 {
        return INVALID_ARG;
    }
    let mut name = [0u16; 128];
    for (i, c) in "Stereo".encode_utf16().enumerate() {
        name[i] = c;
    }
    unsafe {
        *info = BusInfo {
            media_type,
            direction: dir,
            channel_count: 2,
            name,
            bus_type: 0,
            flags: 1,
        };
    }
    OK
}
unsafe extern "C" fn comp_get_routing_info(
    _this: *mut c_void,
    _a: *mut c_void,
    _b: *mut c_void,
) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn comp_activate_bus(
    _this: *mut c_void,
    media_type: i32,
    _dir: i32,
    index: i32,
    _state: u8,
) -> TResult {
    if media_type == 0 && index == 0 { OK } else { INVALID_ARG }
}
unsafe extern "C" fn comp_set_active(_this: *mut c_void, _state: u8) -> TResult {
    OK
}
unsafe extern "C" fn comp_set_state(_this: *mut c_void, _s: *mut c_void) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn comp_get_state(_this: *mut c_void, _s: *mut c_void) -> TResult {
    NOT_IMPL
}

// -------------------------------------------------------- IAudioProcessor -
unsafe extern "C" fn ap_set_bus_arrangements(
    _this: *mut c_void,
    ins: *const u64,
    n_in: i32,
    outs: *const u64,
    n_out: i32,
) -> TResult {
    unsafe {
        if n_in == 1 && n_out == 1 && *ins == 3 && *outs == 3 { OK } else { FALSE }
    }
}
unsafe extern "C" fn ap_get_bus_arrangement(
    _this: *mut c_void,
    _dir: i32,
    _index: i32,
    out: *mut u64,
) -> TResult {
    unsafe { *out = 3 };
    OK
}
unsafe extern "C" fn ap_can_process_sample_size(_this: *mut c_void, size: i32) -> TResult {
    if size == 0 { OK } else { FALSE }
}
unsafe extern "C" fn ap_get_latency_samples(_this: *mut c_void) -> u32 {
    0
}
unsafe extern "C" fn ap_setup_processing(_this: *mut c_void, setup: *const ProcessSetup) -> TResult {
    let s = unsafe { &*setup };
    if s.symbolic_sample_size == 0 && (s.sample_rate - 48000.0).abs() < f64::EPSILON {
        OK
    } else {
        FALSE
    }
}
unsafe extern "C" fn ap_set_processing(_this: *mut c_void, _state: u8) -> TResult {
    OK
}
unsafe extern "C" fn ap_process(this: *mut c_void, data: *mut ProcessData) -> TResult {
    if std::env::var("KRONELLO_PLUGIN_FIXTURE_CRASH").is_ok() {
        std::process::abort();
    }
    if let Ok(ms) = std::env::var("KRONELLO_PLUGIN_FIXTURE_HANG_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    mark("process");
    let gain = unsafe { (*base(this, 8)).gain.get() } as f32;
    let d = unsafe { &*data };
    let inputs = unsafe { &*d.inputs };
    let outputs = unsafe { &mut *d.outputs };
    for ch in 0..outputs.num_channels.min(inputs.num_channels) as usize {
        let src = unsafe { *inputs.channel_buffers32.add(ch) };
        let dst = unsafe { *outputs.channel_buffers32.add(ch) };
        if src.is_null() || dst.is_null() {
            continue;
        }
        for i in 0..d.num_samples as usize {
            unsafe { *dst.add(i) = *src.add(i) * gain };
        }
    }
    OK
}
unsafe extern "C" fn ap_get_tail_samples(_this: *mut c_void) -> u32 {
    0
}

// -------------------------------------------------------- IEditController -
unsafe extern "C" fn ec_initialize(_this: *mut c_void, _ctx: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn ec_terminate(_this: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn ec_set_component_state(_this: *mut c_void, _s: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn ec_set_state(_this: *mut c_void, _s: *mut c_void) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn ec_get_state(_this: *mut c_void, _s: *mut c_void) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn ec_get_parameter_count(_this: *mut c_void) -> i32 {
    1
}
fn to_utf16(text: &str) -> [u16; 128] {
    let mut out = [0u16; 128];
    for (i, c) in text.encode_utf16().enumerate().take(128) {
        out[i] = c;
    }
    out
}
unsafe extern "C" fn ec_get_parameter_info(
    _this: *mut c_void,
    index: i32,
    info: *mut ParameterInfo,
) -> TResult {
    if index != 0 {
        return INVALID_ARG;
    }
    unsafe {
        *info = ParameterInfo {
            id: 0,
            title: to_utf16("Gain"),
            short_title: to_utf16("Gain"),
            units: [0; 128],
            step_count: 0,
            default_normalized_value: 1.0,
            unit_id: 0,
            flags: 0,
        };
    }
    OK
}
unsafe extern "C" fn ec_get_param_string_by_value(
    _this: *mut c_void,
    _id: u32,
    _v: f64,
    _s: *mut u16,
) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn ec_get_param_value_by_string(
    _this: *mut c_void,
    _s: *const u16,
    _v: *mut f64,
) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn ec_normalized_to_plain(_this: *mut c_void, _id: u32, v: f64) -> f64 {
    v
}
unsafe extern "C" fn ec_plain_to_normalized(_this: *mut c_void, _id: u32, v: f64) -> f64 {
    v
}
unsafe extern "C" fn ec_set_param_normalized(this: *mut c_void, id: u32, v: f64) -> TResult {
    if id != 0 {
        return INVALID_ARG;
    }
    unsafe { (*base(this, 16)).gain.set(v) };
    OK
}
unsafe extern "C" fn ec_set_component_handler(_this: *mut c_void, _h: *mut c_void) -> TResult {
    OK
}
unsafe extern "C" fn ec_create_view(_this: *mut c_void, _name: *const i8) -> *mut c_void {
    ptr::null_mut()
}

// --------------------------------------------------------------- factory --
#[repr(C)]
struct Factory {
    vt1: *const IPluginFactoryVtbl,
    vt2: *const IPluginFactory2Vtbl,
    refs: Cell<u32>,
}
unsafe fn factory_base(this: *mut c_void, off: usize) -> *mut Factory {
    (this as *mut u8).sub(off) as *mut Factory
}
unsafe extern "C" fn f_addref(this: *mut c_void) -> u32 {
    unsafe { (*factory_base(this, 0)).refs.set((*factory_base(this, 0)).refs.get() + 1) };
    unsafe { (*factory_base(this, 0)).refs.get() }
}
unsafe extern "C" fn f_release(this: *mut c_void) -> u32 {
    let f = unsafe { factory_base(this, 0) };
    let n = unsafe { (*f).refs.get() }.saturating_sub(1);
    unsafe { (*f).refs.set(n) };
    if n == 0 {
        drop(unsafe { Box::from_raw(f) });
    }
    n
}
unsafe extern "C" fn f_qi(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    let iid = unsafe { std::slice::from_raw_parts(iid, 16) };
    let target = if iid == IID_FUNKNOWN || iid == IID_IPLUGINFACTORY {
        this
    } else if iid == IID_IPLUGINFACTORY2 {
        unsafe { (this as *mut u8).add(8) as *mut c_void }
    } else {
        unsafe { *out = ptr::null_mut() };
        return NO_INTERFACE;
    };
    unsafe {
        *out = target;
        f_addref(this);
    }
    OK
}
unsafe extern "C" fn f2_addref(this: *mut c_void) -> u32 {
    unsafe { f_addref(factory_base(this, 8) as *mut c_void) }
}
unsafe extern "C" fn f2_release(this: *mut c_void) -> u32 {
    unsafe { f_release(factory_base(this, 8) as *mut c_void) }
}
unsafe extern "C" fn f2_qi(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> TResult {
    unsafe { f_qi(factory_base(this, 8) as *mut c_void, iid, out) }
}
unsafe extern "C" fn f_count_classes(_this: *mut c_void) -> i32 {
    1
}
fn class_name() -> [u8; 64] {
    let mut out = [0u8; 64];
    out[.."Kronello Test Gain".len()].copy_from_slice(b"Kronello Test Gain");
    out
}
fn class_category() -> [u8; 32] {
    let mut out = [0u8; 32];
    out[.."Audio Module Class".len()].copy_from_slice(b"Audio Module Class");
    out
}
unsafe extern "C" fn f_get_class_info(
    _this: *mut c_void,
    index: i32,
    info: *mut PClassInfo,
) -> TResult {
    if index != 0 {
        return INVALID_ARG;
    }
    unsafe {
        *info = PClassInfo {
            cid: CLASS_ID,
            cardinality: 0x7fffffff,
            category: class_category(),
            name: class_name(),
        };
    }
    OK
}
unsafe extern "C" fn f_create_instance(
    _this: *mut c_void,
    cid: *const u8,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> TResult {
    let cid = unsafe { std::slice::from_raw_parts(cid, 16) };
    if cid != CLASS_ID.as_slice() {
        return INVALID_ARG;
    }
    let iid = unsafe { std::slice::from_raw_parts(iid, 16) };
    if !(iid == IID_FUNKNOWN || iid == IID_IPLUGINBASE || iid == IID_ICOMPONENT) {
        unsafe { *obj = ptr::null_mut() };
        return NO_INTERFACE;
    }
    let boxed = Box::new(Obj {
        comp_vt: &COMP_VT,
        audio_vt: &AUDIO_VT,
        edit_vt: &EDIT_VT,
        refs: Cell::new(1),
        gain: Cell::new(1.0),
    });
    unsafe { *obj = Box::into_raw(boxed) as *mut c_void };
    OK
}
fn string32(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}
fn string64(text: &str) -> [u8; 64] {
    let mut out = [0u8; 64];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}
fn string128(text: &str) -> [u8; 128] {
    let mut out = [0u8; 128];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}
unsafe extern "C" fn f2_get_class_info2(
    this: *mut c_void,
    index: i32,
    info: *mut PClassInfo2,
) -> TResult {
    if index != 0 {
        return INVALID_ARG;
    }
    let _ = this;
    unsafe {
        *info = PClassInfo2 {
            cid: CLASS_ID,
            cardinality: 0x7fffffff,
            category: string32("Audio Module Class"),
            name: string64("Kronello Test Gain"),
            class_flags: 0,
            sub_categories: string128("Fx|Test"),
            vendor: string64("Kronello"),
            version: string64("1.0.0"),
            sdk_version: 0,
        };
    }
    OK
}
unsafe extern "C" fn f2_get_class_info_unicode(
    _this: *mut c_void,
    _index: i32,
    _info: *mut c_void,
) -> TResult {
    NOT_IMPL
}
unsafe extern "C" fn f2_set_host_context(_this: *mut c_void, _ctx: *mut c_void) -> TResult {
    OK
}

static FACTORY_VT: IPluginFactoryVtbl = IPluginFactoryVtbl {
    base: FUnknownVtbl {
        query_interface: f_qi,
        add_ref: f_addref,
        release: f_release,
    },
    count_classes: f_count_classes,
    get_class_info: f_get_class_info,
    create_instance: f_create_instance,
};
static FACTORY2_VT: IPluginFactory2Vtbl = IPluginFactory2Vtbl {
    factory: IPluginFactoryVtbl {
        base: FUnknownVtbl {
            query_interface: f2_qi,
            add_ref: f2_addref,
            release: f2_release,
        },
        count_classes: f_count_classes,
        get_class_info: f_get_class_info,
        create_instance: f_create_instance,
    },
    get_class_info2: f2_get_class_info2,
    get_class_info_unicode: f2_get_class_info_unicode,
    set_host_context: f2_set_host_context,
};
static COMP_VT: IComponentVtbl = IComponentVtbl {
    plugin_base: IPluginBaseVtbl {
        base: FUnknownVtbl {
            query_interface: comp_qi,
            add_ref: comp_addref,
            release: comp_release,
        },
        initialize: comp_initialize,
        terminate: comp_terminate,
    },
    get_controller_class_id: comp_get_controller_class_id,
    set_io_mode: comp_set_io_mode,
    get_bus_count: comp_get_bus_count,
    get_bus_info: comp_get_bus_info,
    get_routing_info: comp_get_routing_info,
    activate_bus: comp_activate_bus,
    set_active: comp_set_active,
    set_state: comp_set_state,
    get_state: comp_get_state,
};
static AUDIO_VT: IAudioProcessorVtbl = IAudioProcessorVtbl {
    base: FUnknownVtbl {
        query_interface: audio_qi,
        add_ref: audio_addref,
        release: audio_release,
    },
    set_bus_arrangements: ap_set_bus_arrangements,
    get_bus_arrangement: ap_get_bus_arrangement,
    can_process_sample_size: ap_can_process_sample_size,
    get_latency_samples: ap_get_latency_samples,
    setup_processing: ap_setup_processing,
    set_processing: ap_set_processing,
    process: ap_process,
    get_tail_samples: ap_get_tail_samples,
};
static EDIT_VT: IEditControllerVtbl = IEditControllerVtbl {
    plugin_base: IPluginBaseVtbl {
        base: FUnknownVtbl {
            query_interface: edit_qi,
            add_ref: edit_addref,
            release: edit_release,
        },
        initialize: ec_initialize,
        terminate: ec_terminate,
    },
    set_component_state: ec_set_component_state,
    set_state: ec_set_state,
    get_state: ec_get_state,
    get_parameter_count: ec_get_parameter_count,
    get_parameter_info: ec_get_parameter_info,
    get_param_string_by_value: ec_get_param_string_by_value,
    get_param_value_by_string: ec_get_param_value_by_string,
    normalized_param_to_plain: ec_normalized_to_plain,
    plain_param_to_normalized: ec_plain_to_normalized,
    set_param_normalized: ec_set_param_normalized,
    set_component_handler: ec_set_component_handler,
    create_view: ec_create_view,
};

#[no_mangle]
pub extern "C" fn ModuleEntry(_lib: *mut c_void) -> bool {
    mark("module_entry");
    true
}
#[no_mangle]
pub extern "C" fn ModuleExit() -> bool {
    mark("module_exit");
    true
}
#[no_mangle]
pub extern "C" fn GetPluginFactory() -> *mut c_void {
    mark("get_factory");
    let factory = Box::new(Factory {
        vt1: &FACTORY_VT,
        vt2: &FACTORY2_VT,
        refs: Cell::new(1),
    });
    Box::into_raw(factory) as *mut c_void
}
"#;
