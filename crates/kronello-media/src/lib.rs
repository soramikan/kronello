//! Local-file media backend. Native resources are confined to the audited FFI.
#![deny(unsafe_code)]
mod image;
pub use image::*;
mod render;
pub use render::*;
mod assets;
mod audio;
mod export;
#[allow(unsafe_code)]
mod ffi;
mod proxy;
mod video;
pub use assets::*;
pub use audio::*;
pub use export::*;
pub use proxy::*;
pub use video::*;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("ASSET_MISSING: {0}")]
    AssetMissing(String),
    #[error("ASSET_HASH_MISMATCH: {0}")]
    AssetHashMismatch(String),
    #[error("ENCODER_UNAVAILABLE: {encoder}: {reason}; FFmpeg: {ffmpeg:?}; use AV1 or ProRes")]
    EncoderUnavailable {
        encoder: String,
        reason: String,
        ffmpeg: Option<FfmpegErrorDetail>,
    },
    #[error("FFMPEG_UNAVAILABLE: {0}")]
    FfmpegUnavailable(String),
    #[error("UNSUPPORTED_FEATURE: {0}")]
    UnsupportedFeature(String),
    #[error(transparent)]
    Audio(#[from] kronello_audio::AudioError),
    #[error(transparent)]
    Render(#[from] kronello_render::RenderError),
    #[error("DECODE_ERROR: {0}")]
    Decode(String),
    #[error("ENCODE_ERROR: {0}")]
    Encode(String),
    #[error("FRAME_NOT_FOUND: {0}")]
    FrameNotFound(String),
    #[error("INVALID_MEDIA_INPUT: {0}")]
    InvalidInput(String),
    #[error("OUTPUT_EXISTS: {0}")]
    OutputExists(PathBuf),
    #[error("DISTRIBUTION_LICENSE_ERROR: {0}")]
    DistributionLicense(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Time(#[from] kronello_time::TimeError),
    #[error(transparent)]
    Project(#[from] kronello_model::ProjectError),
    #[error(transparent)]
    Flow(#[from] kronello_tracking::FlowError),
}
impl MediaError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AssetMissing(_) => "ASSET_MISSING",
            Self::AssetHashMismatch(_) => "ASSET_HASH_MISMATCH",
            Self::EncoderUnavailable { .. } => "ENCODER_UNAVAILABLE",
            Self::FfmpegUnavailable(_) => "FFMPEG_UNAVAILABLE",
            Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            Self::Audio(e) => e.code(),
            Self::Render(e) => e.code(),
            Self::Decode(_) => "DECODE_ERROR",
            Self::Encode(_) => "ENCODE_ERROR",
            Self::FrameNotFound(_) => "FRAME_NOT_FOUND",
            Self::InvalidInput(_) | Self::Project(_) => "INVALID_MEDIA_INPUT",
            Self::OutputExists(_) => "OUTPUT_EXISTS",
            Self::DistributionLicense(_) => "DISTRIBUTION_LICENSE_ERROR",
            Self::Io(_) => "MEDIA_IO_ERROR",
            Self::Json(_) => "INVALID_MEDIA_INPUT",
            Self::Time(_) => "TIME_ERROR",
            Self::Flow(e) => e.code(),
        }
    }
}
/// Original FFmpeg failure, copied before native resources are released.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfmpegErrorDetail {
    pub code: i32,
    pub operation: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodecCapability {
    pub name: String,
    pub encoder: bool,
    pub decoder: bool,
    /// FFmpeg HARDWARE or HYBRID registration; device success is checked on open.
    pub hardware: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LibraryCapability {
    pub name: String,
    pub version: u32,
    pub license: String,
    pub configuration: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCapabilities {
    pub schema_version: u32,
    pub ffmpeg_version: String,
    pub library_directory: PathBuf,
    pub substituted: bool,
    pub libraries: Vec<LibraryCapability>,
    pub distribution_eligible: bool,
    pub development_only: bool,
    pub codecs: Vec<CodecCapability>,
    /// Compiled device types; availability of a physical device is not implied.
    pub hwaccels: Vec<String>,
}
impl MediaCapabilities {
    pub fn verify_distribution(&self) -> Result<(), MediaError> {
        if !self.distribution_eligible || !self.ffmpeg_version.starts_with("9.") {
            return Err(MediaError::DistributionLicense(
                "requires FFmpeg 9.x, LGPL shared libraries without GPL/nonfree".into(),
            ));
        }
        self.select_encoder(EncodeCodec::Av1)?;
        self.select_encoder(EncodeCodec::ProRes)?;
        if !self
            .codecs
            .iter()
            .any(|c| c.encoder && c.name == "pcm_s24le")
        {
            return Err(MediaError::DistributionLicense(
                "PCM24 encoder required".into(),
            ));
        }
        if !self
            .codecs
            .iter()
            .any(|c| c.decoder && matches!(c.name.as_str(), "libdav1d" | "libaom-av1"))
        {
            return Err(MediaError::DistributionLicense(
                "AV1 software decoder required".into(),
            ));
        }
        Ok(())
    }
    pub fn select_encoder(&self, codec: EncodeCodec) -> Result<&CodecCapability, MediaError> {
        let names: &[&str] = match codec {
            EncodeCodec::Av1 => &["libsvtav1", "libaom-av1"],
            EncodeCodec::ProRes => &["prores_ks"],
            EncodeCodec::H264 => &["h264_videotoolbox"],
            EncodeCodec::Hevc => &["hevc_videotoolbox"],
            EncodeCodec::Dnx => &["dnxhd"],
        };
        names
            .iter()
            .find_map(|name| {
                self.codecs.iter().find(|c| {
                    c.encoder
                        && c.name == *name
                        && (!matches!(codec, EncodeCodec::H264 | EncodeCodec::Hevc) || c.hardware)
                })
            })
            .ok_or_else(|| MediaError::EncoderUnavailable {
                encoder: format!("{codec:?}"),
                reason: format!("no eligible registered encoder among {}", names.join(", ")),
                ffmpeg: None,
            })
    }
    /// Registered encoder availability for a closed MEDIA-004 output kind that
    /// does not pass through [`EncodeCodec`]: `gif`, `libmp3lame`, `flac`.
    pub fn require_encoder(&self, name: &str) -> Result<(), MediaError> {
        if self.codecs.iter().any(|c| c.encoder && c.name == name) {
            Ok(())
        } else {
            Err(MediaError::EncoderUnavailable {
                encoder: name.into(),
                reason: "closed-profile encoder is not registered".into(),
                ffmpeg: None,
            })
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodeCodec {
    Av1,
    ProRes,
    H264,
    Hevc,
    /// MEDIA-004: FFmpeg native DNxHD/DNxHR encoder (all versioned profiles).
    Dnx,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    Software,
    Hardware,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaTransferStats {
    pub cpu_copy_bytes: u64,
    pub cpu_conversion_input_bytes: u64,
    pub cpu_conversion_output_bytes: u64,
    pub cpu_upload_bytes: u64,
    pub cpu_readback_bytes: u64,
    pub gpu_copy_bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaPathReport {
    pub decoder: Option<String>,
    pub encoder: Option<String>,
    pub execution: ExecutionKind,
    pub input_pixel_format: String,
    pub output_pixel_format: String,
    pub transfer_path: String,
    pub transfers: MediaTransferStats,
}
/// Each instance owns its dynamic libraries. Runtime override does not mutate
/// process-global loader search paths or silently fall back to system libraries.
pub struct MediaRuntime {
    native: ffi::NativeRuntime,
    capabilities: MediaCapabilities,
}
impl MediaRuntime {
    pub fn load() -> Result<Self, MediaError> {
        match std::env::var_os("KRONELLO_FFMPEG_LIB_DIR") {
            Some(path) => Self::load_directory(PathBuf::from(path), true),
            None => {
                let executable = std::env::current_exe()?;
                let root = executable.parent().and_then(std::path::Path::parent);
                let directory = root
                    .filter(|root| root.join("package-manifest.json").is_file())
                    .map(|root| root.join("lib"))
                    .unwrap_or_else(|| PathBuf::from(env!("KRONELLO_FFMPEG_BUILD_LIB_DIR")));
                // A declared package must fail if its runtime is broken. Never
                // fall back to the development prefix or system libraries.
                Self::load_directory(directory, false)
            }
        }
    }
    pub fn load_directory(path: PathBuf, substituted: bool) -> Result<Self, MediaError> {
        let path = path
            .canonicalize()
            .map_err(|e| MediaError::FfmpegUnavailable(e.to_string()))?;
        let native = ffi::NativeRuntime::open(&path)?;
        let capabilities = native.capabilities(path, substituted);
        Ok(Self {
            native,
            capabilities,
        })
    }
    pub fn capabilities(&self) -> &MediaCapabilities {
        &self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_capabilities_select_videotoolbox_and_reject_ineligible_encoders() {
        let mut capabilities = MediaCapabilities {
            schema_version: 1,
            ffmpeg_version: String::new(),
            library_directory: PathBuf::new(),
            substituted: false,
            libraries: vec![],
            distribution_eligible: false,
            development_only: true,
            codecs: vec![],
            hwaccels: vec![],
        };
        for (codec, name, software) in [
            (EncodeCodec::H264, "h264_videotoolbox", "libx264"),
            (EncodeCodec::Hevc, "hevc_videotoolbox", "libx265"),
        ] {
            capabilities.codecs = vec![CodecCapability {
                name: software.into(),
                encoder: true,
                decoder: false,
                hardware: false,
            }];
            for registration in [None, Some((false, true)), Some((true, false))] {
                capabilities.codecs.truncate(1);
                if let Some((encoder, hardware)) = registration {
                    capabilities.codecs.push(CodecCapability {
                        name: name.into(),
                        encoder,
                        decoder: true,
                        hardware,
                    });
                }
                let error = capabilities.select_encoder(codec).unwrap_err();
                assert_eq!(error.code(), "ENCODER_UNAVAILABLE");
                assert!(matches!(
                    error,
                    MediaError::EncoderUnavailable { ffmpeg: None, .. }
                ));
            }
            capabilities.codecs.push(CodecCapability {
                name: name.into(),
                encoder: true,
                decoder: false,
                hardware: true,
            });
            assert_eq!(capabilities.select_encoder(codec).unwrap().name, name);
        }
    }
}

mod streaming;
