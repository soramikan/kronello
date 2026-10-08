//! Worker→helper wire protocol (ADR-0131, "helper process contract").
//!
//! The request is one JSON document on the helper's stdin; the response is
//! one JSON document on its stdout. Audio payloads never cross the pipe —
//! float32 buffers travel through caller-created stage files referenced by
//! [`HelperIo`], and the helper reads/writes them under bounded lengths.
//! Every field is validated in the helper before plugin code runs.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::PluginSpec;

pub const HELPER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HelperOp {
    /// Load, enumerate classes, initialize, terminate, unload.
    Describe,
    /// Load, initialize, process `io.frames` samples, unload.
    Process,
}

/// File-based sample transport. `input`/`output` are raw little-endian
/// f32, interleaved `channels`-wide, exactly `frames` samples per channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HelperIo {
    pub input: PathBuf,
    pub output: PathBuf,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HelperRequest {
    pub schema_version: u32,
    pub op: HelperOp,
    pub spec: PluginSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io: Option<HelperIo>,
    /// Helper-side watchdog budget in milliseconds; the helper aborts if
    /// plugin code is still running past this deadline.
    pub deadline_ms: u64,
}
impl HelperRequest {
    pub fn describe(spec: PluginSpec, deadline_ms: u64) -> Self {
        Self {
            schema_version: HELPER_PROTOCOL_VERSION,
            op: HelperOp::Describe,
            spec,
            io: None,
            deadline_ms,
        }
    }
    pub fn process(spec: PluginSpec, io: HelperIo, deadline_ms: u64) -> Self {
        Self {
            schema_version: HELPER_PROTOCOL_VERSION,
            op: HelperOp::Process,
            spec,
            io: Some(io),
            deadline_ms,
        }
    }
}

/// Host-visible plugin identity returned by describe/process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginReport {
    /// Declared plugin name (PClassInfo2 / AudioComponentCopyName), or the
    /// pinned identity when the ABI cannot report one.
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// All classes the bundle exports; `component` selects among them.
    #[serde(default)]
    pub classes: Vec<PluginClassInfo>,
    /// Declared processing latency (`IAudioProcessor::getLatencySamples`).
    #[serde(default)]
    pub latency_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginClassInfo {
    /// VST3 class id hex; AudioUnit component triplet.
    pub class_id: String,
    pub name: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperResponse {
    /// `ok` carries `report` (+ `frames` for process); `error` carries a
    /// typed `code` + `message`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<PluginReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
impl HelperResponse {
    pub fn ok(report: PluginReport, frames: Option<u64>) -> Self {
        Self {
            status: "ok".into(),
            report: Some(report),
            frames,
            code: None,
            message: None,
        }
    }
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            status: "error".into(),
            report: None,
            frames: None,
            code: Some(code.into()),
            message: Some(message.into()),
        }
    }
}
