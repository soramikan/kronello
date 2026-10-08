//! Typed errors for the plugin hosting boundary. Every failure a worker or
//! helper can produce maps to one of these codes; nothing falls back to
//! unprocessed audio.

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Pinned bundle path does not exist or has no loadable module.
    #[error("PLUGIN_MISSING: {0}")]
    Missing(String),
    /// Recorded pin (bundle manifest hash) differs from the bytes on disk.
    #[error("ASSET_HASH_MISMATCH: {0}")]
    HashMismatch(String),
    /// ABI, platform payload, sample format or layout the host cannot run.
    #[error("UNSUPPORTED_FEATURE: {0}")]
    Unsupported(String),
    /// Plugin call returned an error, or the helper exited abnormally.
    #[error("PLUGIN_FAILED: {0}")]
    Failed(String),
    /// Helper exceeded its processing deadline and was terminated.
    #[error("PLUGIN_TIMEOUT: {0}")]
    Timeout(String),
    /// Helper output did not match the protocol contract.
    #[error("PLUGIN_PROTOCOL: {0}")]
    Protocol(String),
    /// Recorded plugin identity differs from the probed bundle/component.
    #[error("PLUGIN_VERSION_MISMATCH: {0}")]
    VersionMismatch(String),
    /// Caller input (spec, parameters, io contract) is malformed.
    #[error("INVALID_REQUEST: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl PluginError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing(_) => "PLUGIN_MISSING",
            Self::HashMismatch(_) => "ASSET_HASH_MISMATCH",
            Self::Unsupported(_) => "UNSUPPORTED_FEATURE",
            Self::Failed(_) => "PLUGIN_FAILED",
            Self::Timeout(_) => "PLUGIN_TIMEOUT",
            Self::Protocol(_) => "PLUGIN_PROTOCOL",
            Self::VersionMismatch(_) => "PLUGIN_VERSION_MISMATCH",
            Self::InvalidInput(_) => "INVALID_REQUEST",
            Self::Io(_) => "IO_ERROR",
            Self::Json(_) => "PLUGIN_PROTOCOL",
        }
    }
    /// Rebuild a typed error from a helper-reported code/message pair.
    /// Unknown codes collapse into PLUGIN_FAILED rather than being dropped.
    pub fn from_code(code: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        match code {
            "PLUGIN_MISSING" => Self::Missing(message),
            "ASSET_HASH_MISMATCH" => Self::HashMismatch(message),
            "UNSUPPORTED_FEATURE" => Self::Unsupported(message),
            "PLUGIN_TIMEOUT" => Self::Timeout(message),
            "PLUGIN_PROTOCOL" => Self::Protocol(message),
            "PLUGIN_VERSION_MISMATCH" => Self::VersionMismatch(message),
            "INVALID_REQUEST" => Self::InvalidInput(message),
            _ => Self::Failed(if code == "PLUGIN_FAILED" {
                message
            } else {
                format!("{code}: {message}")
            }),
        }
    }
}
