//! macOS Audio Unit hosting through AudioToolbox (ADR-0131). Reached only
//! from the detached helper. Scope is deliberately narrow and every limit
//! is a typed error:
//! - built-in or file-pinned components located via `AudioComponentFindNext`
//!   on an exact `type:subtype:manufacturer` triplet;
//! - 48 kHz, linear-PCM float32, non-interleaved, ≤2 channels;
//! - input delivered through `kAudioUnitProperty_SetRenderCallback` on the
//!   input scope; output pulled via `AudioUnitRender` in ≤4096-frame slices;
//! - parameters via `AudioUnitSetParameter` (global scope);
//! - `spec.version` pins `AudioComponentGetVersion` exactly (hex).
use std::ffi::c_void;
use std::ptr;

use crate::abi::MAX_BLOCK;
use crate::abi::io as fio;
use crate::{HelperIo, PluginClassInfo, PluginError, PluginReport, PluginSpec, parse_au_component};

type OsStatus = i32;
const NO_ERR: OsStatus = 0;

const K_AUDIO_UNIT_SCOPE_GLOBAL: u32 = 0;
const K_AUDIO_UNIT_SCOPE_INPUT: u32 = 1;
const K_AUDIO_UNIT_SCOPE_OUTPUT: u32 = 0;
const K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT: u32 = 8;
const K_AUDIO_UNIT_PROPERTY_MAXIMUM_FRAMES_PER_SLICE: u32 = 14;
const K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK: u32 = 23;
const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
/// kAudioFormatFlagIsFloat(1) | IsPacked(1<<3) | IsNonInterleaved(1<<5)
/// — kAudioFormatFlagsNativeFloatPacked|kAudioFormatFlagIsNonInterleaved.
const K_AUDIO_FORMAT_FLAGS_F32_NON_INTERLEAVED: u32 = (1 << 0) | (1 << 3) | (1 << 5);
const K_AUDIO_TIME_STAMP_SAMPLE_TIME_VALID: u32 = 1 << 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[repr(C)]
struct AudioComponentDescription {
    component_type: u32,
    component_sub_type: u32,
    component_manufacturer: u32,
    component_flags: u32,
    component_flags_mask: u32,
}
#[repr(C)]
struct SmpteTime {
    subframes: i16,
    subframe_divisor: u32,
    counter: u32,
    smpte_type: u32,
    flags: u32,
    hours: i16,
    minutes: i16,
    seconds: i16,
    frames: i16,
}
#[repr(C)]
struct AudioTimeStamp {
    sample_time: f64,
    host_time: u64,
    rate_scalar: f64,
    word_clock_time: u64,
    smpte_time: SmpteTime,
    flags: u32,
    reserved: u32,
}
#[repr(C)]
struct AuBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}
#[repr(C)]
struct AudioBufferList {
    number_buffers: u32,
    buffers: [AuBuffer; 2],
}
#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}
#[repr(C)]
struct AuRenderCallback {
    input_proc: unsafe extern "C" fn(
        *mut c_void,
        *mut u32,
        *const AudioTimeStamp,
        u32,
        u32,
        *mut AudioBufferList,
    ) -> OsStatus,
    input_proc_ref_con: *mut c_void,
}
const _: () = assert!(size_of::<AudioStreamBasicDescription>() == 40);
const _: () = assert!(size_of::<AudioTimeStamp>() == 72);

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(
        component: *mut c_void,
        description: *const AudioComponentDescription,
    ) -> *mut c_void;
    fn AudioComponentCopyName(component: *mut c_void, name: *mut *mut c_void) -> OsStatus;
    fn AudioComponentGetVersion(component: *mut c_void, version: *mut u32) -> OsStatus;
    fn AudioComponentInstanceNew(component: *mut c_void, instance: *mut *mut c_void) -> OsStatus;
    fn AudioComponentInstanceDispose(instance: *mut c_void) -> OsStatus;
    fn AudioUnitInitialize(unit: *mut c_void) -> OsStatus;
    fn AudioUnitUninitialize(unit: *mut c_void) -> OsStatus;
    fn AudioUnitSetProperty(
        unit: *mut c_void,
        id: u32,
        scope: u32,
        element: u32,
        data: *const c_void,
        size: u32,
    ) -> OsStatus;
    fn AudioUnitSetParameter(
        unit: *mut c_void,
        id: u32,
        scope: u32,
        element: u32,
        value: f32,
        offset_in_frames: u32,
    ) -> OsStatus;
    fn AudioUnitRender(
        unit: *mut c_void,
        action_flags: *mut u32,
        time_stamp: *const AudioTimeStamp,
        bus_number: u32,
        number_frames: u32,
        data: *mut AudioBufferList,
    ) -> OsStatus;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFStringGetLength(the_string: *const c_void) -> i64;
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut u8,
        buffer_size: i64,
        encoding: u32,
    ) -> bool;
}

fn au_failed<T>(what: &str, status: OsStatus) -> Result<T, PluginError> {
    Err(PluginError::Failed(format!(
        "audio_unit {what} returned OSStatus {status}"
    )))
}
fn find_component(spec: &PluginSpec) -> Result<*mut c_void, PluginError> {
    let [ty, sub, man] = parse_au_component(&spec.component)?;
    let description = AudioComponentDescription {
        component_type: ty,
        component_sub_type: sub,
        component_manufacturer: man,
        component_flags: 0,
        component_flags_mask: 0,
    };
    let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &description) };
    if component.is_null() {
        return Err(PluginError::Missing(format!(
            "no audio unit matches component {}",
            spec.component
        )));
    }
    Ok(component)
}
fn component_name(component: *mut c_void) -> String {
    unsafe {
        let mut name: *mut c_void = ptr::null_mut();
        if AudioComponentCopyName(component, &mut name) != NO_ERR || name.is_null() {
            return String::new();
        }
        let len = CFStringGetLength(name);
        let size = (len * 4 + 1).clamp(64, 4096) as usize;
        let mut buffer = vec![0u8; size];
        let text = if CFStringGetCString(
            name,
            buffer.as_mut_ptr(),
            size as i64,
            K_CF_STRING_ENCODING_UTF8,
        ) {
            let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
            String::from_utf8_lossy(&buffer[..end]).into_owned()
        } else {
            String::new()
        };
        CFRelease(name);
        text
    }
}
fn check_version(component: *mut c_void, spec: &PluginSpec) -> Result<String, PluginError> {
    let mut version: u32 = 0;
    let status = unsafe { AudioComponentGetVersion(component, &mut version) };
    if status != NO_ERR {
        return au_failed("AudioComponentGetVersion", status);
    }
    let text = format!("{version:08x}");
    if let Some(pinned) = &spec.version
        && pinned != &text
    {
        return Err(PluginError::VersionMismatch(format!(
            "audio unit {} reports version {text}, pinned {pinned}",
            spec.component
        )));
    }
    Ok(text)
}

pub fn describe(spec: &PluginSpec) -> Result<PluginReport, PluginError> {
    let component = find_component(spec)?;
    let version = check_version(component, spec)?;
    let name = component_name(component);
    Ok(PluginReport {
        name,
        vendor: String::new(),
        version,
        classes: vec![PluginClassInfo {
            class_id: spec.component.clone(),
            name: String::new(),
            category: "audio_unit".into(),
        }],
        latency_samples: 0,
    })
}

struct InputFeed {
    /// Planar input channels plus the shared read cursor (in frames).
    channels: Vec<Vec<f32>>,
    cursor: usize,
}
unsafe extern "C" fn feed_input(
    ref_con: *mut c_void,
    _flags: *mut u32,
    _ts: *const AudioTimeStamp,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> OsStatus {
    let feed = unsafe { &mut *(ref_con as *mut InputFeed) };
    let list = unsafe { &mut *data };
    for index in 0..list.number_buffers as usize {
        let buffer = &mut list.buffers[index];
        let needed = frames as usize;
        buffer.data_byte_size = (needed * 4) as u32;
        if buffer.data.is_null() {
            continue;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(buffer.data as *mut f32, needed) };
        let source = feed.channels.get(index);
        let available = source.map_or(0, |ch| ch.len().saturating_sub(feed.cursor));
        let take = available.min(needed);
        if let Some(ch) = source {
            out[..take].copy_from_slice(&ch[feed.cursor..feed.cursor + take]);
        }
        for slot in out[take..].iter_mut() {
            *slot = 0.0;
        }
    }
    feed.cursor += frames as usize;
    NO_ERR
}

struct UnitGuard {
    unit: *mut c_void,
    initialized: bool,
}
impl Drop for UnitGuard {
    fn drop(&mut self) {
        unsafe {
            if self.initialized {
                AudioUnitUninitialize(self.unit);
            }
            if !self.unit.is_null() {
                AudioComponentInstanceDispose(self.unit);
            }
        }
    }
}

pub fn process(spec: &PluginSpec, io: &HelperIo) -> Result<(PluginReport, u64), PluginError> {
    if !(1..=2).contains(&io.channels) {
        return Err(PluginError::Unsupported(
            "audio_unit processing requires 1 or 2 channels".into(),
        ));
    }
    if io.frames == 0 || io.frames as usize > crate::abi::MAX_FRAMES {
        return Err(PluginError::InvalidInput(format!(
            "audio_unit frame count {} out of bounds",
            io.frames
        )));
    }
    if io.sample_rate != 48_000 {
        return Err(PluginError::Unsupported(
            "audio_unit processing runs at the 48000 Hz project rate".into(),
        ));
    }
    let component = find_component(spec)?;
    let version = check_version(component, spec)?;
    let name = component_name(component);
    let total = io.frames as usize;
    let channels = io.channels as usize;
    let interleaved = fio::read_interleaved(io)?;

    // Declared before `guard` so it outlives every unit teardown path —
    // the render callback dereferences `feed` until the unit is disposed.
    let mut planar: Vec<Vec<f32>> = vec![Vec::with_capacity(total); channels];
    for frame in 0..total {
        for ch in 0..channels {
            planar[ch].push(interleaved[frame * channels + ch]);
        }
    }
    let mut feed = InputFeed {
        channels: planar,
        cursor: 0,
    };

    unsafe {
        let mut unit: *mut c_void = ptr::null_mut();
        let status = AudioComponentInstanceNew(component, &mut unit);
        if status != NO_ERR || unit.is_null() {
            return au_failed("AudioComponentInstanceNew", status);
        }
        let mut guard = UnitGuard {
            unit,
            initialized: false,
        };

        let stream = AudioStreamBasicDescription {
            sample_rate: io.sample_rate as f64,
            format_id: K_AUDIO_FORMAT_LINEAR_PCM,
            format_flags: K_AUDIO_FORMAT_FLAGS_F32_NON_INTERLEAVED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            // Canonical non-interleaved layout: channelsPerFrame is the bus
            // width; each AudioBuffer in the list carries one channel.
            channels_per_frame: io.channels,
            bits_per_channel: 32,
            reserved: 0,
        };
        for (scope, label) in [
            (K_AUDIO_UNIT_SCOPE_INPUT, "input"),
            (K_AUDIO_UNIT_SCOPE_OUTPUT, "output"),
        ] {
            let status = AudioUnitSetProperty(
                unit,
                K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT,
                scope,
                0,
                (&stream as *const AudioStreamBasicDescription).cast(),
                size_of::<AudioStreamBasicDescription>() as u32,
            );
            if status != NO_ERR {
                return au_failed(&format!("SetProperty(StreamFormat,{label})"), status);
            }
        }
        let max_frames: u32 = MAX_BLOCK as u32;
        let status = AudioUnitSetProperty(
            unit,
            K_AUDIO_UNIT_PROPERTY_MAXIMUM_FRAMES_PER_SLICE,
            K_AUDIO_UNIT_SCOPE_GLOBAL,
            0,
            (&max_frames as *const u32).cast(),
            4,
        );
        if status != NO_ERR {
            return au_failed("SetProperty(MaximumFramesPerSlice)", status);
        }
        let callback = AuRenderCallback {
            input_proc: feed_input,
            input_proc_ref_con: (&mut feed as *mut InputFeed).cast(),
        };
        let status = AudioUnitSetProperty(
            unit,
            K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK,
            K_AUDIO_UNIT_SCOPE_INPUT,
            0,
            (&callback as *const AuRenderCallback).cast(),
            size_of::<AuRenderCallback>() as u32,
        );
        if status != NO_ERR {
            return au_failed("SetProperty(SetRenderCallback)", status);
        }
        let status = AudioUnitInitialize(unit);
        if status != NO_ERR {
            return au_failed("AudioUnitInitialize", status);
        }
        guard.initialized = true;
        for parameter in &spec.parameters {
            let status = AudioUnitSetParameter(
                unit,
                parameter.id,
                K_AUDIO_UNIT_SCOPE_GLOBAL,
                0,
                parameter.value as f32,
                0,
            );
            if status != NO_ERR {
                return au_failed("AudioUnitSetParameter", status);
            }
        }

        let mut out_planar: Vec<Vec<f32>> = vec![vec![0.0; total]; channels];
        let mut offset = 0usize;
        while offset < total {
            let block = (total - offset).min(MAX_BLOCK);
            let mut list = AudioBufferList {
                number_buffers: channels as u32,
                buffers: [
                    AuBuffer {
                        number_channels: 1,
                        data_byte_size: (block * 4) as u32,
                        data: ptr::null_mut(),
                    },
                    AuBuffer {
                        number_channels: 1,
                        data_byte_size: (block * 4) as u32,
                        data: ptr::null_mut(),
                    },
                ],
            };
            for (buffer, planar) in list
                .buffers
                .iter_mut()
                .zip(out_planar.iter_mut())
                .take(channels)
            {
                buffer.data = planar[offset..].as_mut_ptr().cast();
            }
            let stamp = AudioTimeStamp {
                sample_time: offset as f64,
                host_time: 0,
                rate_scalar: 0.0,
                word_clock_time: 0,
                smpte_time: SmpteTime {
                    subframes: 0,
                    subframe_divisor: 0,
                    counter: 0,
                    smpte_type: 0,
                    flags: 0,
                    hours: 0,
                    minutes: 0,
                    seconds: 0,
                    frames: 0,
                },
                flags: K_AUDIO_TIME_STAMP_SAMPLE_TIME_VALID,
                reserved: 0,
            };
            let mut action_flags: u32 = 0;
            let status =
                AudioUnitRender(unit, &mut action_flags, &stamp, 0, block as u32, &mut list);
            if status != NO_ERR {
                return au_failed("AudioUnitRender", status);
            }
            offset += block;
        }

        // Explicit teardown while `feed` is still alive; the guard only
        // cleans up on early-return paths from here on.
        AudioUnitUninitialize(unit);
        guard.initialized = false;
        AudioComponentInstanceDispose(unit);
        guard.unit = ptr::null_mut();

        let mut interleaved_out = vec![0f32; total * channels];
        for frame in 0..total {
            for ch in 0..channels {
                interleaved_out[frame * channels + ch] = out_planar[ch][frame];
            }
        }
        fio::write_interleaved(&io.output, &interleaved_out)?;
        Ok((
            PluginReport {
                name,
                vendor: String::new(),
                version,
                classes: vec![PluginClassInfo {
                    class_id: spec.component.clone(),
                    name: String::new(),
                    category: "audio_unit".into(),
                }],
                latency_samples: 0,
            },
            io.frames,
        ))
    }
}
