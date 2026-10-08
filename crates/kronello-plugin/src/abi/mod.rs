//! Audited native plugin ABI layer — the only module where `unsafe` is
//! enabled in this crate (through `#[allow(unsafe_code)]` on `mod abi` in
//! lib.rs), and reachable only from the detached helper process entry
//! (`helper.rs`). Safe code elsewhere orchestrates processes and files.
//!
//! Layout:
//! - `dl`: minimal dynamic-library open/symbol/close wrapper;
//! - `vst3`: hand-written implementation of the public VST3
//!   COM-compatible ABI (module entry points, `IPluginFactory`,
//!   `IComponent`, `IAudioProcessor`, `IEditController`). No Steinberg SDK
//!   source or headers are vendored or linked;
//! - `au` (macOS only): AudioToolbox hosting (`AudioComponentInstanceNew` /
//!   `AudioUnitRender`) for built-in and file-pinned Audio Units.
use crate::{HelperIo, PluginFormat, PluginReport, PluginSpec};

#[cfg(target_os = "macos")]
pub(crate) mod au;
pub(crate) mod dl;
pub(crate) mod vst3;
#[cfg(not(target_os = "macos"))]
pub(crate) mod au {
    use crate::{HelperIo, PluginError, PluginReport, PluginSpec};
    pub fn describe(_spec: &PluginSpec) -> Result<PluginReport, PluginError> {
        Err(PluginError::Unsupported(
            "audio_unit hosting is only implemented on macOS".into(),
        ))
    }
    pub fn process(_spec: &PluginSpec, _io: &HelperIo) -> Result<(PluginReport, u64), PluginError> {
        Err(PluginError::Unsupported(
            "audio_unit hosting is only implemented on macOS".into(),
        ))
    }
}

/// Maximum samples per processing block handed to plugin code.
pub(crate) const MAX_BLOCK: usize = 4096;
/// Absolute cap on frames the helper will ever process in one call.
pub(crate) const MAX_FRAMES: usize = 48_000 * 3600;

pub fn describe(spec: &PluginSpec) -> Result<PluginReport, crate::PluginError> {
    match spec.format {
        PluginFormat::Vst3 => vst3::describe(spec),
        PluginFormat::AudioUnit => au::describe(spec),
    }
}
pub fn process(
    spec: &PluginSpec,
    io: &HelperIo,
) -> Result<(PluginReport, u64), crate::PluginError> {
    match spec.format {
        PluginFormat::Vst3 => vst3::process(spec, io),
        PluginFormat::AudioUnit => au::process(spec, io),
    }
}

/// Bounded f32le file I/O for the helper sample transport. Input files must
/// match the declared frame/channel count exactly; output files are written
/// with the processed frames only.
pub(crate) mod io {
    use crate::{HelperIo, PluginError};
    use std::io::Write;
    use std::path::Path;

    /// Read the interleaved little-endian f32 input; a short or oversized
    /// file is a protocol violation, never silently padded by the caller's
    /// contract (the helper zero-extends only the plugin-visible tail).
    pub fn read_interleaved(io: &HelperIo) -> Result<Vec<f32>, PluginError> {
        let expected = io.frames as usize * io.channels as usize;
        let bytes = std::fs::read(&io.input)?;
        if bytes.len() != expected * 4 {
            return Err(PluginError::Protocol(format!(
                "plugin input {} is {} bytes, expected {expected} f32 samples",
                io.input.display(),
                bytes.len()
            )));
        }
        let mut out = Vec::with_capacity(expected);
        for chunk in bytes.chunks_exact(4) {
            out.push(f32::from_le_bytes(chunk.try_into().expect("4 bytes")));
        }
        Ok(out)
    }
    pub fn write_interleaved(path: &Path, samples: &[f32]) -> Result<(), PluginError> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        let mut cursor = std::io::Cursor::new(&mut bytes);
        for sample in samples {
            cursor.write_all(&sample.to_le_bytes())?;
        }
        std::fs::write(path, &bytes)?;
        Ok(())
    }
}
