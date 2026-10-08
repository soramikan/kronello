//! Hand-written host for the public VST3 COM-compatible ABI (ADR-0131).
//! Only the documented binary surface is implemented: module entry points,
//! `FUnknown`/`IPluginFactory`/`IPluginFactory2`/`IPluginBase`/`IComponent`/
//! `IAudioProcessor`/`IEditController`. No Steinberg SDK code or headers are
//! vendored; every vtable below is spelled out field-for-field so the ABI
//! contract is auditable in one place.
//!
//! Scope (all enforced as typed errors, never silently bypassed):
//! - kSample32 processing, mono→stereo handled as N-channel up to 2;
//! - kRealtime is honored, kOffline preferred when accepted;
//! - IEditController parameters via `setParamNormalized` only;
//! - bundles without the platform payload, missing exports, QI failures or
//!   non-stereo bus configurations → `UNSUPPORTED_FEATURE`/`PLUGIN_FAILED`.
use std::ffi::c_void;
use std::path::Path;
use std::ptr;

use crate::abi::MAX_BLOCK;
use crate::abi::dl::Library;
use crate::abi::io as fio;
use crate::{HelperIo, PluginClassInfo, PluginError, PluginReport, PluginSpec};
use crate::{parse_class_id, resolve_vst3_module};

type TResult = i32;
const K_RESULT_OK: TResult = 0;

// Interface IDs (Steinberg::FUID values converted by the documented
// INLINE_UID byte swap: l1 bytes reversed, l2/l4 nibble-swapped, l3 plain).
// FUnknown / IPluginBase / IPluginFactory ids are only queried by plugins,
// not by this host; the fixture carries them for completeness.
const IID_IPLUGINFACTORY2: [u8; 16] = [
    0x50, 0xB6, 0x07, 0x00, 0x4B, 0xF2, 0x0B, 0x4C, 0xA4, 0x64, 0xED, 0xB9, 0x0B, 0xF0, 0xBB, 0x2A,
];
const IID_ICOMPONENT: [u8; 16] = [
    0x31, 0xFF, 0x31, 0xE8, 0xD5, 0xF2, 0x01, 0x43, 0x92, 0x8E, 0xBB, 0xEE, 0x69, 0x25, 0x02, 0x78,
];
const IID_IAUDIOPROCESSOR: [u8; 16] = [
    0x99, 0x3F, 0x04, 0x42, 0xDA, 0xB7, 0x3C, 0x45, 0xA5, 0x69, 0xE7, 0x9D, 0xAE, 0x9A, 0x3D, 0xC3,
];
const IID_IEDITCONTROLLER: [u8; 16] = [
    0xE3, 0xBB, 0xD7, 0xDC, 0x42, 0x77, 0x8D, 0x44, 0xA8, 0x74, 0xAA, 0xCC, 0x9C, 0x97, 0x9E, 0x75,
];

const K_AUDIO: i32 = 0;
const K_INPUT: i32 = 0;
const K_OUTPUT: i32 = 1;
const K_MAIN: i32 = 0;
const K_SAMPLE32: i32 = 0;
const K_REALTIME: i32 = 0;
const K_OFFLINE: i32 = 2;

// ---------------------------------------------------------------- ABI ----
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
// Compile-time ABI sanity: sizes mirror the documented C layouts.
const _: () = assert!(size_of::<PClassInfo>() == 116);
const _: () = assert!(size_of::<PClassInfo2>() == 380);
const _: () = assert!(size_of::<BusInfo>() == 276);
const _: () = assert!(size_of::<ProcessSetup>() == 24);
const _: () = assert!(size_of::<AudioBusBuffers>() == 24);
const _: () = assert!(size_of::<ProcessData>() == 80);
const _: () = assert!(size_of::<ParameterInfo>() == 792);

// ------------------------------------------------------------ helpers ----
unsafe fn vt<T>(obj: *mut c_void) -> &'static T {
    unsafe { &*ptr::read(obj as *const *const T) }
}
unsafe fn qi(obj: *mut c_void, iid: &[u8; 16]) -> *mut c_void {
    let mut out: *mut c_void = ptr::null_mut();
    let result = unsafe { (vt::<FUnknownVtbl>(obj).query_interface)(obj, iid.as_ptr(), &mut out) };
    if result == K_RESULT_OK {
        out
    } else {
        ptr::null_mut()
    }
}
unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        unsafe { (vt::<FUnknownVtbl>(obj).release)(obj) };
    }
}
fn clean(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}
fn class_hex(cid: &[u8; 16]) -> String {
    cid.iter().map(|b| format!("{b:02x}")).collect()
}
fn failed<T>(what: &str, result: TResult) -> Result<T, PluginError> {
    Err(PluginError::Failed(format!(
        "vst3 {what} returned {result}"
    )))
}
fn unsupported<T>(what: impl Into<String>) -> Result<T, PluginError> {
    Err(PluginError::Unsupported(format!("vst3 {}", what.into())))
}

/// Tear-down guard. Records which lifecycle steps ran so Drop unwinds the
/// exact spec order: setProcessing(0) → setActive(0) → busses off →
/// terminate → release objects → ModuleExit → dlclose.
struct Session {
    _lib: Library,
    module_exit: Option<unsafe extern "C" fn() -> bool>,
    factory: *mut c_void,
    component: *mut c_void,
    controller: *mut c_void,
    controller_separate: bool,
    processor: *mut c_void,
    component_init: bool,
    controller_init: bool,
    active: bool,
    processing: bool,
    busses_on: bool,
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if self.processing && !self.processor.is_null() {
                (vt::<IAudioProcessorVtbl>(self.processor).set_processing)(self.processor, 0);
            }
            if self.active && !self.component.is_null() {
                (vt::<IComponentVtbl>(self.component).set_active)(self.component, 0);
            }
            if self.busses_on && !self.component.is_null() {
                for dir in [K_INPUT, K_OUTPUT] {
                    (vt::<IComponentVtbl>(self.component).activate_bus)(
                        self.component,
                        K_AUDIO,
                        dir,
                        0,
                        0,
                    );
                }
            }
            if self.controller_init && self.controller_separate {
                (vt::<IPluginBaseVtbl>(self.controller).terminate)(self.controller);
            }
            if self.component_init {
                (vt::<IPluginBaseVtbl>(self.component).terminate)(self.component);
            }
            release(self.processor);
            if self.controller_separate {
                release(self.controller);
            }
            release(self.component);
            release(self.factory);
            if let Some(module_exit) = self.module_exit {
                module_exit();
            }
        }
    }
}

fn open_session(module: &Path) -> Result<Session, PluginError> {
    let lib = Library::open(module)?;
    let module_entry: Option<unsafe extern "C" fn(*mut c_void) -> bool> = unsafe {
        lib.symbol("ModuleEntry").map(|p| {
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> bool>(p)
        })
    };
    let module_exit: Option<unsafe extern "C" fn() -> bool> = unsafe {
        lib.symbol("ModuleExit")
            .map(|p| std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> bool>(p))
    };
    if let Some(entry) = module_entry
        && unsafe { !entry(ptr::null_mut()) }
    {
        return Err(PluginError::Failed(
            "vst3 ModuleEntry returned false".into(),
        ));
    }
    let factory_sym = unsafe { lib.symbol("GetPluginFactory") };
    let Some(factory_sym) = factory_sym else {
        return unsupported("module exports no GetPluginFactory (not a VST3 plugin)");
    };
    let get_factory: unsafe extern "C" fn() -> *mut c_void =
        unsafe { std::mem::transmute(factory_sym) };
    let factory = unsafe { get_factory() };
    if factory.is_null() {
        return Err(PluginError::Failed(
            "vst3 GetPluginFactory returned null".into(),
        ));
    }
    Ok(Session {
        _lib: lib,
        module_exit,
        factory,
        component: ptr::null_mut(),
        controller: ptr::null_mut(),
        controller_separate: false,
        processor: ptr::null_mut(),
        component_init: false,
        controller_init: false,
        active: false,
        processing: false,
        busses_on: false,
    })
}

fn enumerate_classes(factory: *mut c_void) -> Result<Vec<PluginClassInfo>, PluginError> {
    unsafe {
        let v1 = vt::<IPluginFactoryVtbl>(factory);
        let count = (v1.count_classes)(factory);
        if count < 0 {
            return failed("countClasses", count);
        }
        if count > 1024 {
            return unsupported("bundle declares more than 1024 classes");
        }
        let factory2 = qi(factory, &IID_IPLUGINFACTORY2);
        let mut out = Vec::new();
        for index in 0..count {
            let mut info = PluginClassInfo {
                class_id: String::new(),
                name: String::new(),
                category: String::new(),
            };
            if !factory2.is_null() {
                let mut raw = PClassInfo2 {
                    cid: [0; 16],
                    cardinality: 0,
                    category: [0; 32],
                    name: [0; 64],
                    class_flags: 0,
                    sub_categories: [0; 128],
                    vendor: [0; 64],
                    version: [0; 64],
                    sdk_version: 0,
                };
                let res = (vt::<IPluginFactory2Vtbl>(factory2).get_class_info2)(
                    factory2, index, &mut raw,
                );
                if res == K_RESULT_OK {
                    info.class_id = class_hex(&raw.cid);
                    info.category = clean(&raw.category);
                    info.name = clean(&raw.name);
                    // richer identity rides on the report below
                }
            }
            if info.class_id.is_empty() {
                let mut raw = PClassInfo {
                    cid: [0; 16],
                    cardinality: 0,
                    category: [0; 32],
                    name: [0; 64],
                };
                let res = (v1.get_class_info)(factory, index, &mut raw);
                if res != K_RESULT_OK {
                    release(factory2);
                    return failed("getClassInfo", res);
                }
                info.class_id = class_hex(&raw.cid);
                info.category = clean(&raw.category);
                info.name = clean(&raw.name);
            }
            out.push(info);
        }
        release(factory2);
        Ok(out)
    }
}

/// Vendor/version strings for the selected class when IPluginFactory2 is
/// available (empty strings otherwise).
fn class_identity(factory: *mut c_void, cid: &[u8; 16]) -> (String, String, String) {
    unsafe {
        let factory2 = qi(factory, &IID_IPLUGINFACTORY2);
        if factory2.is_null() {
            return (String::new(), String::new(), String::new());
        }
        let count = (vt::<IPluginFactory2Vtbl>(factory2).factory.count_classes)(factory2);
        let mut out = (String::new(), String::new(), String::new());
        for index in 0..count.min(1024) {
            let mut raw = PClassInfo2 {
                cid: [0; 16],
                cardinality: 0,
                category: [0; 32],
                name: [0; 64],
                class_flags: 0,
                sub_categories: [0; 128],
                vendor: [0; 64],
                version: [0; 64],
                sdk_version: 0,
            };
            if (vt::<IPluginFactory2Vtbl>(factory2).get_class_info2)(factory2, index, &mut raw)
                != K_RESULT_OK
            {
                continue;
            }
            if &raw.cid == cid {
                out = (clean(&raw.name), clean(&raw.vendor), clean(&raw.version));
                break;
            }
        }
        release(factory2);
        out
    }
}

pub fn describe(spec: &PluginSpec) -> Result<PluginReport, PluginError> {
    let module = resolve_vst3_module(
        spec.path
            .as_deref()
            .ok_or_else(|| PluginError::Missing("vst3 spec carries no bundle path".into()))?,
    )?;
    let cid = parse_class_id(&spec.component)?;
    let session = open_session(&module)?;
    let classes = enumerate_classes(session.factory)?;
    if !classes
        .iter()
        .any(|c| c.class_id == spec.component.to_ascii_lowercase())
    {
        return Err(PluginError::Missing(format!(
            "vst3 bundle exports no class {}",
            spec.component
        )));
    }
    let (name, vendor, version) = class_identity(session.factory, &cid);
    Ok(PluginReport {
        name,
        vendor,
        version,
        classes,
        latency_samples: 0,
    })
}

/// Load, initialize, process `io` frames and unload — the full roundtrip.
/// Input/out transport is interleaved f32; the plugin sees planar buffers.
pub fn process(spec: &PluginSpec, io: &HelperIo) -> Result<(PluginReport, u64), PluginError> {
    if io.channels < 1 || io.channels > 2 {
        return unsupported("vst3 processing requires 1 or 2 channels");
    }
    if io.frames == 0 || io.frames as usize > crate::abi::MAX_FRAMES {
        return Err(PluginError::InvalidInput(format!(
            "vst3 frame count {} out of bounds",
            io.frames
        )));
    }
    if io.sample_rate != 48_000 {
        return unsupported("vst3 processing runs at the 48000 Hz project rate");
    }
    let module = resolve_vst3_module(
        spec.path
            .as_deref()
            .ok_or_else(|| PluginError::Missing("vst3 spec carries no bundle path".into()))?,
    )?;
    let cid = parse_class_id(&spec.component)?;
    let channels = io.channels as usize;
    let total = io.frames as usize;
    let interleaved = fio::read_interleaved(io)?;

    let mut session = open_session(&module)?;
    let classes = enumerate_classes(session.factory)?;
    if !classes
        .iter()
        .any(|c| c.class_id == spec.component.to_ascii_lowercase())
    {
        return Err(PluginError::Missing(format!(
            "vst3 bundle exports no class {}",
            spec.component
        )));
    }
    let (name, vendor, version) = class_identity(session.factory, &cid);
    if let Some(recorded) = &spec.version
        && !version.is_empty()
        && recorded != &version
    {
        return Err(PluginError::VersionMismatch(format!(
            "vst3 class {} reports version {version}, pinned {recorded}",
            spec.component
        )));
    }

    unsafe {
        // ---- instantiate component ----------------------------------
        let mut component: *mut c_void = ptr::null_mut();
        let res = (vt::<IPluginFactoryVtbl>(session.factory).create_instance)(
            session.factory,
            cid.as_ptr(),
            IID_ICOMPONENT.as_ptr(),
            &mut component,
        );
        if res != K_RESULT_OK || component.is_null() {
            return failed("createInstance(IComponent)", res);
        }
        session.component = component;
        let res = (vt::<IPluginBaseVtbl>(component).initialize)(component, ptr::null_mut());
        if res != K_RESULT_OK {
            return failed("component initialize", res);
        }
        session.component_init = true;

        // ---- processor ------------------------------------------------
        let processor = qi(component, &IID_IAUDIOPROCESSOR);
        if processor.is_null() {
            return unsupported("component does not implement IAudioProcessor");
        }
        session.processor = processor;
        let res =
            (vt::<IAudioProcessorVtbl>(processor).can_process_sample_size)(processor, K_SAMPLE32);
        if res != K_RESULT_OK {
            return unsupported("plugin cannot process kSample32");
        }

        // ---- controller (same object, or factory-created) -------------
        let mut controller = qi(component, &IID_IEDITCONTROLLER);
        if controller.is_null() {
            let mut ctrl_cid = [0u8; 16];
            let res = (vt::<IComponentVtbl>(component).get_controller_class_id)(
                component,
                ctrl_cid.as_mut_ptr(),
            );
            if res == K_RESULT_OK && ctrl_cid != [0; 16] {
                let mut created: *mut c_void = ptr::null_mut();
                let res = (vt::<IPluginFactoryVtbl>(session.factory).create_instance)(
                    session.factory,
                    ctrl_cid.as_ptr(),
                    IID_IEDITCONTROLLER.as_ptr(),
                    &mut created,
                );
                if res == K_RESULT_OK && !created.is_null() {
                    let res2 =
                        (vt::<IPluginBaseVtbl>(created).initialize)(created, ptr::null_mut());
                    if res2 == K_RESULT_OK {
                        session.controller_init = true;
                        session.controller_separate = true;
                        controller = created;
                    } else {
                        release(created);
                    }
                }
            }
        }
        session.controller = controller;
        if !controller.is_null() && !session.controller_separate {
            // same-object controller shares the component's initialize
        }
        if !spec.parameters.is_empty() {
            if controller.is_null() {
                return unsupported("plugin exposes no IEditController for parameters");
            }
            let cv = vt::<IEditControllerVtbl>(controller);
            for parameter in &spec.parameters {
                let res = (cv.set_param_normalized)(controller, parameter.id, parameter.value);
                if res != K_RESULT_OK {
                    return failed("setParamNormalized", res);
                }
            }
        }

        // ---- bus arrangement: exactly one stereo audio bus each way ---
        let arrangements = [3u64];
        let res = (vt::<IAudioProcessorVtbl>(processor).set_bus_arrangements)(
            processor,
            arrangements.as_ptr(),
            1,
            arrangements.as_ptr(),
            1,
        );
        if res != K_RESULT_OK {
            return unsupported("plugin refused a stereo kMain input+output arrangement");
        }
        let mut bus = BusInfo {
            media_type: 0,
            direction: 0,
            channel_count: 0,
            name: [0; 128],
            bus_type: 0,
            flags: 0,
        };
        let res = (vt::<IComponentVtbl>(component).get_bus_info)(
            component, K_AUDIO, K_OUTPUT, 0, &mut bus,
        );
        if res != K_RESULT_OK {
            return failed("getBusInfo(output)", res);
        }
        if bus.bus_type != K_MAIN || bus.channel_count != channels as i32 {
            return unsupported(format!(
                "plugin output bus 0 is {} channels ({} required)",
                bus.channel_count, channels
            ));
        }
        for dir in [K_INPUT, K_OUTPUT] {
            let res = (vt::<IComponentVtbl>(component).activate_bus)(component, K_AUDIO, dir, 0, 1);
            if res != K_RESULT_OK {
                return failed("activateBus", res);
            }
        }
        session.busses_on = true;
        let res = (vt::<IComponentVtbl>(component).set_active)(component, 1);
        if res != K_RESULT_OK {
            return failed("setActive", res);
        }
        session.active = true;
        let setup = ProcessSetup {
            process_mode: K_OFFLINE,
            symbolic_sample_size: K_SAMPLE32,
            max_samples_per_block: MAX_BLOCK as i32,
            sample_rate: io.sample_rate as f64,
        };
        let res = (vt::<IAudioProcessorVtbl>(processor).setup_processing)(processor, &setup);
        if res != K_RESULT_OK {
            // Some plugins only accept kRealtime; retry once before failing.
            let retry = ProcessSetup {
                process_mode: K_REALTIME,
                ..setup
            };
            let res2 = (vt::<IAudioProcessorVtbl>(processor).setup_processing)(processor, &retry);
            if res2 != K_RESULT_OK {
                return failed("setupProcessing", res);
            }
        }
        let res = (vt::<IAudioProcessorVtbl>(processor).set_processing)(processor, 1);
        if res != K_RESULT_OK {
            return failed("setProcessing", res);
        }
        session.processing = true;

        // ---- block loop over planar buffers ---------------------------
        let mut planar_in: Vec<Vec<f32>> = vec![Vec::new(); channels];
        let mut planar_out: Vec<Vec<f32>> = vec![Vec::new(); channels];
        let mut interleaved_out = vec![0f32; total * channels];
        let mut offset = 0usize;
        while offset < total {
            let block = (total - offset).min(MAX_BLOCK);
            for (ch, buffer) in planar_in.iter_mut().enumerate() {
                buffer.clear();
                buffer.extend((0..block).map(|i| interleaved[(offset + i) * channels + ch]));
            }
            for buffer in planar_out.iter_mut() {
                buffer.clear();
                buffer.resize(block, 0.0);
            }
            let mut in_ptrs: Vec<*mut f32> = planar_in.iter_mut().map(|b| b.as_mut_ptr()).collect();
            let mut out_ptrs: Vec<*mut f32> =
                planar_out.iter_mut().map(|b| b.as_mut_ptr()).collect();
            let mut input_bus = AudioBusBuffers {
                num_channels: channels as i32,
                silence_flags: 0,
                channel_buffers32: in_ptrs.as_mut_ptr(),
            };
            let mut output_bus = AudioBusBuffers {
                num_channels: channels as i32,
                silence_flags: 0,
                channel_buffers32: out_ptrs.as_mut_ptr(),
            };
            let mut data = ProcessData {
                process_mode: setup.process_mode,
                symbolic_sample_size: K_SAMPLE32,
                num_samples: block as i32,
                num_inputs: 1,
                num_outputs: 1,
                inputs: &mut input_bus,
                outputs: &mut output_bus,
                input_parameter_changes: ptr::null_mut(),
                output_parameter_changes: ptr::null_mut(),
                input_events: ptr::null_mut(),
                output_events: ptr::null_mut(),
                process_context: ptr::null_mut(),
            };
            let res = (vt::<IAudioProcessorVtbl>(processor).process)(processor, &mut data);
            if res != K_RESULT_OK {
                return failed("process", res);
            }
            for ch in 0..channels {
                for (index, sample) in planar_out[ch].iter().enumerate() {
                    interleaved_out[(offset + index) * channels + ch] = *sample;
                }
            }
            offset += block;
        }
        let latency = (vt::<IAudioProcessorVtbl>(processor).get_latency_samples)(processor) as u64;
        (vt::<IAudioProcessorVtbl>(processor).set_processing)(processor, 0);
        session.processing = false;

        fio::write_interleaved(&io.output, &interleaved_out)?;
        Ok((
            PluginReport {
                name,
                vendor,
                version,
                classes,
                latency_samples: latency,
            },
            io.frames,
        ))
    }
}
