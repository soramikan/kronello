//! Hash-pinned plugin identity shared by the service API, the fixed job
//! input and the worker→helper protocol (ADR-0131). The document and job
//! records carry only a locator, a manifest hash and a version string —
//! never bundle bytes.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::PluginError;

/// Host ABI family. `vst3` uses the hand-written COM-compatible loader in
/// `abi::vst3`; `audio_unit` uses AudioToolbox through `abi::au` (macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    Vst3,
    AudioUnit,
}

/// One plugin parameter assignment. VST3 takes `value` as the normalized
/// 0..=1 domain of `IEditController::setParamNormalized`; AudioUnit takes the
/// unit's native parameter value domain (`AudioUnitSetParameter`, Float32).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginParameter {
    /// VST3 ParamID or AudioUnit parameter id.
    pub id: u32,
    pub value: f64,
}

/// Hash-pinned plugin binding. `path` + `sha256` pin the bundle content for
/// file-backed plugins; a built-in (file-less) Audio Unit is pinned by its
/// exact component triplet plus `version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginSpec {
    pub format: PluginFormat,
    /// Bundle directory or single-module file (`.vst3`, `.component`,
    /// `.dylib`/`.so`). Required for `vst3`; optional for `audio_unit`
    /// (absent selects a built-in system component).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Lowercase hex SHA-256 of [`bundle_manifest_hash`]; required whenever
    /// `path` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// VST3: 32-hex component class id (TUID bytes). AudioUnit:
    /// `type:subtype:manufacturer` as three 4-character codes, e.g.
    /// `aufx:dely:appl`.
    pub component: String,
    /// Recorded plugin version. AudioUnit pins `AudioComponentGetVersion`
    /// as hex (e.g. `00010600`); VST3 records the class version string for
    /// audit when the bundle reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub parameters: Vec<PluginParameter>,
}
impl PluginSpec {
    /// Structural validation shared by submit, worker re-validation and the
    /// helper. Content verification is [`verify_spec_pin`].
    pub fn validate(&self) -> Result<(), PluginError> {
        if let Some(hash) = &self.sha256 {
            let valid = hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !valid {
                return Err(PluginError::InvalidInput(
                    "plugin sha256 must be 64 lowercase hex".into(),
                ));
            }
        }
        if self.parameters.len() > 1024 {
            return Err(PluginError::InvalidInput(
                "plugin parameter table exceeds 1024 rows".into(),
            ));
        }
        match self.format {
            PluginFormat::Vst3 => {
                if self.path.is_none() {
                    return Err(PluginError::Missing(
                        "vst3 requires a pinned bundle path".into(),
                    ));
                }
                if self.sha256.is_none() {
                    return Err(PluginError::InvalidInput(
                        "vst3 requires a pinned bundle sha256".into(),
                    ));
                }
                parse_class_id(&self.component)?;
                for parameter in &self.parameters {
                    if !(0.0..=1.0).contains(&parameter.value) || !parameter.value.is_finite() {
                        return Err(PluginError::InvalidInput(format!(
                            "vst3 parameter {} value must be normalized 0..=1",
                            parameter.id
                        )));
                    }
                }
            }
            PluginFormat::AudioUnit => {
                if cfg!(not(target_os = "macos")) {
                    return Err(PluginError::Unsupported(
                        "audio_unit hosting is only implemented on macOS".into(),
                    ));
                }
                parse_au_component(&self.component)?;
                if self.path.is_some() && self.sha256.is_none() {
                    return Err(PluginError::InvalidInput(
                        "a file-based audio_unit requires a pinned bundle sha256".into(),
                    ));
                }
                if let Some(version) = &self.version
                    && u32::from_str_radix(version, 16).is_err()
                {
                    return Err(PluginError::InvalidInput(
                        "audio_unit version must be an 8-digit hex version number".into(),
                    ));
                }
                for parameter in &self.parameters {
                    if !parameter.value.is_finite() {
                        return Err(PluginError::InvalidInput(format!(
                            "audio_unit parameter {} value must be finite",
                            parameter.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

/// VST3 component class id (TUID) from 32 lowercase/uppercase hex digits.
pub fn parse_class_id(text: &str) -> Result<[u8; 16], PluginError> {
    if text.len() != 32 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PluginError::InvalidInput(
            "vst3 component must be a 32-hex class id".into(),
        ));
    }
    let mut id = [0u8; 16];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16).expect("hex") as u8;
        let lo = (pair[1] as char).to_digit(16).expect("hex") as u8;
        id[index] = hi << 4 | lo;
    }
    Ok(id)
}
/// AudioUnit component triplet `type:subtype:manufacturer`; each field is
/// exactly four ASCII bytes interpreted as a big-endian OSType.
pub fn parse_au_component(text: &str) -> Result<[u32; 3], PluginError> {
    let invalid = || {
        PluginError::InvalidInput(
            "audio_unit component must be type:subtype:manufacturer four-character codes".into(),
        )
    };
    let mut out = [0u32; 3];
    let mut parts = text.split(':');
    for slot in &mut out {
        let part = parts.next().ok_or_else(invalid)?;
        if part.len() != 4 || !part.is_ascii() {
            return Err(invalid());
        }
        *slot = u32::from_be_bytes(part.as_bytes().try_into().expect("4 bytes"));
    }
    if parts.next().is_some() {
        return Err(invalid());
    }
    Ok(out)
}

fn hash_file_content(path: &Path) -> Result<String, PluginError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Deterministic pin for a plugin input. A regular file hashes its bytes; a
/// bundle directory hashes a manifest over every entry (sorted relative
/// path, kind tag, and file content hash — symlinks hash their target text
/// and are never followed). Missing paths are `PLUGIN_MISSING`.
pub fn bundle_manifest_hash(path: &Path) -> Result<String, PluginError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PluginError::Missing(path.display().to_string())
        } else {
            PluginError::Io(e)
        }
    })?;
    if metadata.is_file() {
        return hash_file_content(path);
    }
    if !metadata.is_dir() {
        return Err(PluginError::Missing(format!(
            "plugin bundle is not a file or directory: {}",
            path.display()
        )));
    }
    let mut entries: Vec<(String, u8, String)> = Vec::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut children = std::fs::read_dir(&dir)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let rel = child
                .path()
                .strip_prefix(path)
                .map_err(|e| PluginError::InvalidInput(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() {
                continue;
            }
            let kind = child.file_type()?;
            if kind.is_symlink() {
                // Record the link text; the resolved module image also lands
                // in the manifest through the "x" record below.
                let target = std::fs::read_link(child.path())?;
                let digest = Sha256::digest(target.to_string_lossy().as_bytes());
                entries.push((rel, b'l', format!("{:x}", digest)));
            } else if kind.is_dir() {
                entries.push((rel.clone(), b'd', String::new()));
                pending.push(child.path());
            } else if kind.is_file() {
                entries.push((rel, b'f', hash_file_content(&child.path())?));
            } else {
                return Err(PluginError::Unsupported(format!(
                    "plugin bundle entry is not a file/dir/symlink: {rel}"
                )));
            }
        }
    }
    entries.sort();
    let mut digest = Sha256::new();
    digest.update(b"kronello.plugin-manifest-v1");
    for (rel, kind, content) in &entries {
        digest.update((rel.len() as u32).to_be_bytes());
        digest.update(rel.as_bytes());
        digest.update([*kind]);
        digest.update(content.as_bytes());
    }
    // The image actually handed to dlopen gets its own record so a symlinked
    // or differently-named module inside a matching tree still pins bytes.
    if let Ok(module) = resolve_vst3_module(path) {
        digest.update(b"module");
        digest.update(hash_file_content(&module)?.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Content pin check executed at submit, at worker start, and inside the
/// helper immediately before loading. `path`-less built-in components have
/// no bytes to pin; their identity check is the component triplet + version.
pub fn verify_spec_pin(spec: &PluginSpec) -> Result<(), PluginError> {
    let Some(path) = &spec.path else {
        return Ok(());
    };
    let actual = bundle_manifest_hash(path)?;
    let expected = spec.sha256.as_deref().unwrap_or_default();
    if actual != expected {
        return Err(PluginError::HashMismatch(format!(
            "plugin bundle {} hashed {actual}, expected {expected}",
            path.display()
        )));
    }
    Ok(())
}

/// Locate the loadable module image inside a plugin bundle. A plain file
/// loads directly; a bundle directory resolves the single image in the
/// platform's `Contents/<platform-dir>` payload directory.
pub fn resolve_vst3_module(bundle: &Path) -> Result<PathBuf, PluginError> {
    let metadata = std::fs::symlink_metadata(bundle).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PluginError::Missing(bundle.display().to_string())
        } else {
            PluginError::Io(e)
        }
    })?;
    if metadata.is_file() {
        return Ok(bundle.to_path_buf());
    }
    if !metadata.is_dir() {
        return Err(PluginError::Missing(format!(
            "plugin path is not a file or bundle: {}",
            bundle.display()
        )));
    }
    // Platform payload directory, per the VST3 bundle layout.
    #[cfg(target_os = "macos")]
    let platform_dir = "MacOS";
    #[cfg(target_os = "linux")]
    let platform_dir = {
        if cfg!(target_arch = "aarch64") {
            "aarch64-linux"
        } else {
            "x86_64-linux"
        }
    };
    #[cfg(target_os = "windows")]
    let platform_dir = {
        if cfg!(target_arch = "aarch64") {
            "arm64_64-win"
        } else {
            "x86_64-win"
        }
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(PluginError::Unsupported(
        "vst3 hosting has no module layout for this platform".into(),
    ));
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let payload = bundle.join("Contents").join(platform_dir);
        let mut modules = Vec::new();
        match std::fs::read_dir(&payload) {
            Ok(read) => {
                for entry in read.collect::<Result<Vec<_>, _>>()? {
                    // dlopen follows the link; the manifest already pinned the
                    // link target text plus the resolved image bytes.
                    if entry.metadata()?.is_file() {
                        modules.push(entry.path());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PluginError::Unsupported(format!(
                    "vst3 bundle has no Contents/{platform_dir} payload"
                )));
            }
            Err(e) => return Err(PluginError::Io(e)),
        }
        match modules.len() {
            0 => Err(PluginError::Missing(format!(
                "vst3 bundle Contents/{platform_dir} is empty"
            ))),
            1 => Ok(modules.remove(0)),
            _ => Err(PluginError::Unsupported(format!(
                "vst3 bundle carries {} module images; exactly one is supported",
                modules.len()
            ))),
        }
    }
}
