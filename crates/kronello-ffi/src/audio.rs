//! Producer-only binary preview resources; never call from an audio callback.
use kronello_service::{
    AudioPrepareRequest, MAX_PLAYBACK_BLOCK_FRAMES, PreparedAudio, ServiceError,
};
use std::{
    ffi::{CString, c_char, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
};

fn failure(result: Result<(), ServiceError>, error: *mut *mut c_char) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            // SAFETY: C contract requires a writable error output.
            unsafe {
                *error = CString::new(serde_json::to_string(&e).expect("error JSON"))
                    .expect("escaped JSON")
                    .into_raw();
            }
            false
        }
    }
}
fn panicked() -> ServiceError {
    ServiceError::new("FFI_PANIC", "audio producer request panicked")
}

/// Synchronous preparation on a non-realtime thread, independent of the session FIFO.
/// Returns an owned opaque pointer; error JSON uses kronello_free. No process registry handle.
/// # Safety
/// json must be readable for len bytes; has_audio/error are writable outputs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_audio_prepare(
    json: *const u8,
    len: usize,
    has_audio: *mut bool,
    error: *mut *mut c_char,
) -> *mut c_void {
    if error.is_null() || has_audio.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: required writable outputs.
    unsafe {
        *error = std::ptr::null_mut();
        *has_audio = false;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller provides readable request bytes.
        let text = unsafe { crate::input(json, len) }
            .ok_or_else(|| ServiceError::invalid("audio request buffer"))?;
        let r: AudioPrepareRequest = serde_json::from_str(&text)?;
        let prepared = match crate::capture_session_audio(r.clone())? {
            Some(input) => input.prepare()?,
            None => PreparedAudio::prepare(&r.project, r.target, &r.expected_revision)?,
        };
        Ok::<_, ServiceError>(prepared)
    }))
    .unwrap_or_else(|_| Err(panicked()));
    match result {
        Ok(p) => {
            // SAFETY: writable output; owned allocation is returned to this caller.
            unsafe {
                *has_audio = p.has_audio();
            }
            Box::into_raw(Box::new(p)).cast()
        }
        Err(e) => {
            failure(Err(e), error);
            std::ptr::null_mut()
        }
    }
}
/// Render 1..4096 stereo frames into caller-owned interleaved f32 memory.
/// # Safety
/// resource is live from prepare, exclusively owned by the producer; output has
/// frames*2 writable floats, error is writable. Free must not race this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_audio_render(
    resource: *const c_void,
    start_sample: i64,
    frames: usize,
    output: *mut f32,
    error: *mut *mut c_char,
) -> bool {
    if error.is_null() {
        return false;
    }
    // SAFETY: C contract guarantees writable output.
    unsafe {
        *error = std::ptr::null_mut();
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        if resource.is_null()
            || output.is_null()
            || !(1..=MAX_PLAYBACK_BLOCK_FRAMES).contains(&frames)
        {
            return Err(ServiceError::new(
                "INVALID_AUDIO_INPUT",
                "audio resource/buffer/block size",
            ));
        }
        // SAFETY: caller guarantees the resource lifetime and buffer size, checked bounds.
        let prepared = unsafe { &*resource.cast::<PreparedAudio>() };
        let output = unsafe { std::slice::from_raw_parts_mut(output, frames * 2) };
        prepared.render_block(start_sample, output)
    }))
    .unwrap_or_else(|_| Err(panicked()));
    failure(result, error)
}
/// # Safety
/// resource is null or the still-owned pointer from prepare, freed exactly once
/// after all producer calls finish. Does not run on the realtime callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kronello_audio_free(resource: *mut c_void) {
    if !resource.is_null() {
        // SAFETY: caller returns the owned allocation once, without concurrent calls.
        drop(unsafe { Box::from_raw(resource.cast::<PreparedAudio>()) });
    }
}
