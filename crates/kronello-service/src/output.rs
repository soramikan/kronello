//! IO-001 (ADR-0134): external monitor output ops. Output devices are
//! process-local runtime resources owned by the native output session, never
//! document state; the shared registry exposes enumeration plus explicit
//! enable/disable only. Detection results are transport-independent `dlopen`
//! probes; activation additionally requires the FFI output session, so these
//! commands fail typed on headless transports.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::ServiceError;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OutputDeviceKind {
    RefMonitor,
    Syphon,
    Sdi,
    Ndi,
}
impl OutputDeviceKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::RefMonitor => "ref_monitor",
            Self::Syphon => "syphon",
            Self::Sdi => "sdi",
            Self::Ndi => "ndi",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IoOutputListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IoOutputEnableRequest {
    pub kind: OutputDeviceKind,
    /// Optional display name for the published output (for example the Syphon
    /// server name). Never a locator or an arbitrary command string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IoOutputDisableRequest {
    pub kind: OutputDeviceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputDeviceInfo {
    pub kind: OutputDeviceKind,
    pub name: String,
    /// The contract an activation would use: `native_surface`,
    /// `syphon`, or `vendor_sdk`.
    pub transport: String,
    /// Runtime probe: the platform/framework/SDK is present. Detection never
    /// implies an adapter can be opened.
    pub detected: bool,
    /// This transport can activate the output right now (native session for
    /// ref_monitor/syphon; a contributed adapter for vendor outputs).
    pub available: bool,
    /// An output of this kind is currently enabled in this session.
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IoOutputListResult {
    pub devices: Vec<OutputDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IoOutputStateResult {
    pub kind: OutputDeviceKind,
    pub active: bool,
}

fn info(
    kind: OutputDeviceKind,
    name: &str,
    transport: &str,
    detected: bool,
    available: bool,
    detail: impl Into<String>,
) -> OutputDeviceInfo {
    OutputDeviceInfo {
        kind,
        name: name.into(),
        transport: transport.into(),
        detected,
        available,
        active: false,
        detail: Some(detail.into()),
    }
}

/// Boundary for proprietary vendor SDK output adapters (SDI, NDI, deck
/// control). SDKs are never bundled or linked; an adapter reports runtime
/// detection only, and any enable attempt without a contributed adapter is a
/// typed `UNSUPPORTED_FEATURE` — never a silent no-op or a fallback to a
/// different output kind.
pub trait VendorOutputAdapter {
    fn kind(&self) -> OutputDeviceKind;
    /// Marketing name of the required SDK for diagnostics.
    fn sdk_name(&self) -> &'static str;
    /// Runtime probe: the SDK/framework was found on this machine.
    fn detected(&self) -> bool;
}
/// The detection-only adapter boundary shipped for SDI and NDI.
pub struct UnimplementedVendorAdapter {
    kind: OutputDeviceKind,
    detected: bool,
}
impl UnimplementedVendorAdapter {
    pub fn new(kind: OutputDeviceKind, detected: bool) -> Self {
        Self { kind, detected }
    }
}
impl VendorOutputAdapter for UnimplementedVendorAdapter {
    fn kind(&self) -> OutputDeviceKind {
        self.kind
    }
    fn sdk_name(&self) -> &'static str {
        match self.kind {
            OutputDeviceKind::Sdi => "Blackmagic Desktop Video SDK (DeckLink)",
            OutputDeviceKind::Ndi => "NDI SDK",
            _ => "vendor SDK",
        }
    }
    fn detected(&self) -> bool {
        self.detected
    }
}
/// Vendor outputs are a defined boundary, not an implementation: every open
/// is rejected with a typed error naming the SDK and detection result.
pub fn vendor_output_open(adapter: &dyn VendorOutputAdapter) -> Result<(), ServiceError> {
    Err(ServiceError::new(
        "UNSUPPORTED_FEATURE",
        format!(
            "{} output requires the {} adapter, which is not implemented in this build (sdk detected: {})",
            adapter.kind().wire_name(),
            adapter.sdk_name(),
            adapter.detected(),
        ),
    ))
}

/// Enumerate the external output contract. `native_session` is true only when
/// the caller is the FFI output session that can actually drive surfaces and
/// publish; headless transports still report honest detection results.
pub fn output_device_list(native_session: bool) -> IoOutputListResult {
    let syphon = kronello_framebridge::output::syphon_detected();
    let sdi = kronello_framebridge::output::decklink_detected();
    let ndi = kronello_framebridge::output::ndi_detected();
    let display = cfg!(target_os = "macos");
    IoOutputListResult {
        devices: vec![
            info(
                OutputDeviceKind::RefMonitor,
                "Reference monitor",
                "native_surface",
                display,
                display && native_session,
                if display {
                    "fullscreen CAMetalLayer on a non-main display; the native session enumerates NSScreen, applies the target display color space, and reuses the program-monitor presentation transform"
                } else {
                    "reference monitor output requires macOS native surfaces"
                },
            ),
            info(
                OutputDeviceKind::Syphon,
                "Syphon",
                "syphon",
                syphon,
                syphon && native_session,
                if syphon {
                    "SyphonMetalServer detected; activation publishes BGRA8 program frames"
                } else {
                    "Syphon.framework not detected at runtime"
                },
            ),
            info(
                OutputDeviceKind::Sdi,
                "SDI (Blackmagic DeckLink)",
                "vendor_sdk",
                sdi,
                false,
                "vendor SDK adapter boundary only; no SDI adapter is implemented in this build",
            ),
            info(
                OutputDeviceKind::Ndi,
                "NDI",
                "vendor_sdk",
                ndi,
                false,
                "vendor SDK adapter boundary only; no NDI adapter is implemented in this build",
            ),
        ],
    }
}

/// Headless transports cannot drive external outputs. This reject is the
/// typed boundary; enabling is only possible through the FFI output session.
pub fn external_output_requires_native_session(kind: OutputDeviceKind) -> ServiceError {
    ServiceError::new(
        "UNSUPPORTED_FEATURE",
        format!(
            "{} output activation requires the native output session (GUI FFI attach); headless transports cannot drive external outputs",
            kind.wire_name(),
        ),
    )
}

/// Kinds currently active in a session, folded into the shared list shape.
pub fn with_active(
    mut result: IoOutputListResult,
    active: &BTreeSet<OutputDeviceKind>,
) -> IoOutputListResult {
    for device in &mut result.devices {
        device.active = active.contains(&device.kind);
    }
    result
}
