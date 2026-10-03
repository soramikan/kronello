//! Local, integrity-checked fixture lookup. Downloads belong to the Python fetcher.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// Shared with both Python fixture scripts; points directly to the external directory.
pub const EXTERNAL_FIXTURE_DIR_ENV: &str = "KRONELLO_FIXTURE_EXTERNAL_DIR";

#[derive(Debug)]
pub enum FixtureError {
    NotFound {
        id: String,
        path: PathBuf,
    },
    SizeMismatch {
        id: String,
        expected: u64,
        actual: u64,
    },
    HashMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    UnknownId {
        id: String,
    },
    InvalidManifest {
        path: PathBuf,
        reason: String,
    },
    UnsupportedStorage {
        id: String,
        storage: String,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl Display for FixtureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { id, path } => write!(f, "fixture {id} not found: {}", path.display()),
            Self::SizeMismatch {
                id,
                expected,
                actual,
            } => write!(f, "fixture {id}: expected {expected} bytes, got {actual}"),
            Self::HashMismatch {
                id,
                expected,
                actual,
            } => write!(f, "fixture {id}: expected SHA-256 {expected}, got {actual}"),
            Self::UnknownId { id } => write!(f, "unknown fixture ID: {id}"),
            Self::InvalidManifest { path, reason } => {
                write!(f, "invalid fixture manifest {}: {reason}", path.display())
            }
            Self::UnsupportedStorage { id, storage } => {
                write!(f, "fixture {id}: unsupported storage {storage}")
            }
            Self::Io { path, source } => write!(f, "fixture I/O {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for FixtureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Entry {
    id: String,
    storage: String,
    path: PathBuf,
    bytes: Option<u64>,
    sha256: Option<String>,
}

/// Reads `tests/fixtures/manifest.json` beneath a repository root.
/// Only bundled and external entries have manifest-pinned integrity metadata.
#[derive(Debug)]
pub struct FixtureResolver {
    root: PathBuf,
    external: PathBuf,
    entries: Vec<Entry>,
}

impl FixtureResolver {
    /// Uses the shared environment override or `<root>/target/fixtures/external`.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, FixtureError> {
        let root = root.as_ref();
        let external = std::env::var_os(EXTERNAL_FIXTURE_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target/fixtures/external"));
        Self::with_external_dir(root, external)
    }

    /// Explicit directory override, useful for isolated tests without process env mutation.
    pub fn with_external_dir(
        root: impl AsRef<Path>,
        external: impl AsRef<Path>,
    ) -> Result<Self, FixtureError> {
        let root = root.as_ref().to_path_buf();
        let path = root.join("tests/fixtures/manifest.json");
        let invalid = |reason: String| FixtureError::InvalidManifest {
            path: path.clone(),
            reason,
        };
        let data = fs::read(&path).map_err(|error| invalid(error.to_string()))?;
        let manifest: Value =
            serde_json::from_slice(&data).map_err(|error| invalid(error.to_string()))?;
        if manifest["schema_version"].as_u64() != Some(1) {
            return Err(invalid("unsupported schema_version".into()));
        }
        let fixtures = manifest["fixtures"]
            .as_array()
            .ok_or_else(|| invalid("missing fixtures array".into()))?;
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        let mut external_names = HashSet::new();
        let mut entries = Vec::new();
        for fixture in fixtures {
            let string = |field: &str| -> Result<String, FixtureError> {
                fixture[field]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(format!("missing or invalid {field}")))
            };
            let id = string("id")?;
            let storage = string("storage")?;
            let relative = string("path")?;
            let entry_path = PathBuf::from(&relative);
            if !ids.insert(id.clone()) || !paths.insert(relative.clone()) {
                return Err(invalid("duplicate fixture ID or path".into()));
            }
            if relative.contains('\\')
                || relative.contains(':')
                || entry_path
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(invalid(format!("unsafe fixture path: {relative}")));
            }
            if storage == "bundled" && !entry_path.starts_with("tests/fixtures") {
                return Err(invalid(
                    "bundled path must be beneath tests/fixtures".into(),
                ));
            }
            if storage == "external" {
                if !entry_path.starts_with("external") || entry_path.components().count() != 2 {
                    return Err(invalid("external path must be external/<filename>".into()));
                }
                if !external_names.insert(entry_path.file_name().unwrap().to_owned()) {
                    return Err(invalid("duplicate external filename".into()));
                }
            }
            let mut bytes = None;
            let mut sha256 = None;
            if matches!(storage.as_str(), "bundled" | "external") {
                bytes = Some(
                    fixture["bytes"]
                        .as_u64()
                        .filter(|value| *value > 0)
                        .ok_or_else(|| invalid(format!("invalid byte count for {id}")))?,
                );
                let hash = string("sha256")?;
                if hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(invalid(format!("invalid SHA-256 for {id}")));
                }
                sha256 = Some(hash);
            }
            entries.push(Entry {
                id,
                storage,
                path: entry_path,
                bytes,
                sha256,
            });
        }
        Ok(Self {
            root,
            external: external.as_ref().to_path_buf(),
            entries,
        })
    }

    /// Returns an absolute canonical path only after streaming byte-count and SHA-256 checks.
    /// Missing/corrupt fixtures fail; no download or fallback is attempted.
    pub fn resolve(&self, id: &str) -> Result<PathBuf, FixtureError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| FixtureError::UnknownId { id: id.into() })?;
        let path = match entry.storage.as_str() {
            "bundled" => self.root.join(&entry.path),
            "external" => self.external.join(entry.path.file_name().unwrap()),
            _ => {
                return Err(FixtureError::UnsupportedStorage {
                    id: id.into(),
                    storage: entry.storage.clone(),
                });
            }
        };
        let io_error = |source: io::Error| {
            if source.kind() == io::ErrorKind::NotFound {
                FixtureError::NotFound {
                    id: id.into(),
                    path: path.clone(),
                }
            } else {
                FixtureError::Io {
                    path: path.clone(),
                    source,
                }
            }
        };
        let mut source = File::open(&path).map_err(io_error)?;
        let mut hasher = Sha256::new();
        let mut count = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            count += read as u64;
            hasher.update(&buffer[..read]);
        }
        let expected = entry.bytes.unwrap();
        if count != expected {
            return Err(FixtureError::SizeMismatch {
                id: id.into(),
                expected,
                actual: count,
            });
        }
        let actual = format!("{:x}", hasher.finalize());
        let expected = entry.sha256.as_ref().unwrap();
        if &actual != expected {
            return Err(FixtureError::HashMismatch {
                id: id.into(),
                expected: expected.clone(),
                actual,
            });
        }
        fs::canonicalize(&path).map_err(io_error)
    }
}

/// Resolve an ID using this crate's repository manifest and the shared external override.
pub fn resolve_fixture(id: &str) -> Result<PathBuf, FixtureError> {
    FixtureResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?.resolve(id)
}
