use crate::MediaError;
use kronello_model::{Asset, AssetLocator, DocumentObject, Project};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn content_hash(path: &Path) -> Result<String, MediaError> {
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(MediaError::InvalidInput(
            "asset must be a regular file".into(),
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
/// Verify every resolve, without mtime/size-based shortcuts. If a relative file
/// exists with wrong content, do not silently substitute the absolute candidate.
pub fn resolve_asset(asset: &Asset, project_path: &Path) -> Result<PathBuf, MediaError> {
    let located = locate_asset(asset, project_path)?;
    let actual = content_hash(&located.path)?;
    if actual != asset.content_hash {
        return Err(MediaError::AssetHashMismatch(
            located.path.display().to_string(),
        ));
    }
    Ok(located.path)
}

/// Stat-level rewrite detector for a located file. Any metadata-visible change
/// (size, mtime, ctime, or inode/file-index replacement) produces a different
/// fingerprint; it detects ordinary overwrites and atomic replaces but is not
/// a substitute for the content hash itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileFingerprint {
    len: u64,
    modified: Option<std::time::SystemTime>,
    unique: u64,
    changed_ns: i128,
}
impl FileFingerprint {
    pub(crate) fn of(meta: &fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            return Self {
                len: meta.len(),
                modified: meta.modified().ok(),
                unique: meta.ino(),
                changed_ns: i128::from(meta.ctime()) * 1_000_000_000
                    + i128::from(meta.ctime_nsec()),
            };
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            // file_index() is unstable (issue #63010); creation time still
            // distinguishes an atomic replace from an in-place overwrite.
            return Self {
                len: meta.len(),
                modified: meta.modified().ok(),
                unique: meta.creation_time(),
                changed_ns: (meta.last_write_time() as i128) << 32
                    | i128::from(meta.file_attributes()),
            };
        }
        #[allow(unreachable_code)]
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            unique: 0,
            changed_ns: 0,
        }
    }
}

/// Cheap availability only. A located regular file is not a verified hash match.
/// Rendering and collection must continue to use `resolve_asset`.
pub struct LocatedAsset {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub(crate) fingerprint: FileFingerprint,
}
pub fn locate_asset(asset: &Asset, project_path: &Path) -> Result<LocatedAsset, MediaError> {
    asset.validate()?;
    let base = project_path.parent().unwrap_or(Path::new("."));
    let candidates = asset
        .locator
        .relative
        .iter()
        .map(|p| base.join(p))
        .chain(asset.locator.absolute.iter().map(PathBuf::from));
    for path in candidates {
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => {
                return Ok(LocatedAsset {
                    path: path.canonicalize()?,
                    size_bytes: meta.len(),
                    fingerprint: FileFingerprint::of(&meta),
                });
            }
            Ok(_) => {
                return Err(MediaError::InvalidInput(
                    "asset locator is not a regular file".into(),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(MediaError::AssetMissing(asset.id.to_string()))
}
fn scan(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), MediaError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let kind = entry.file_type()?;
        // Never follow directory symlinks or cycles during relink scans.
        if kind.is_dir() {
            scan(&entry.path(), out)?;
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}
/// Explicit search by content, never by name. Deterministic lexical first match.
/// The original Asset is unchanged unless a complete matching candidate exists.
pub fn relink_asset(
    asset: &Asset,
    project_path: &Path,
    directory: &Path,
) -> Result<Asset, MediaError> {
    asset.validate()?;
    let mut paths = Vec::new();
    scan(directory, &mut paths)?;
    for path in paths {
        if content_hash(&path)? == asset.content_hash {
            let canonical = path.canonicalize()?;
            let base = project_path
                .parent()
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            let relative = canonical
                .strip_prefix(base)
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let mut linked = asset.clone();
            linked.locator = AssetLocator {
                relative,
                absolute: Some(canonical.to_string_lossy().into_owned()),
            };
            return Ok(linked);
        }
    }
    Err(MediaError::AssetMissing(format!(
        "no hash match for {} in {}",
        asset.id,
        directory.display()
    )))
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CollectedProject {
    pub directory: PathBuf,
    pub project: PathBuf,
    pub asset_count: usize,
}
/// Stage an immutable project copy and all verified assets. The supplied writer
/// persists the same Project via the service/store boundary (no store dependency
/// in this backend). It runs before destination publication.
pub fn collect_project<E: From<MediaError>>(
    project: &Project,
    source_project: &Path,
    output: &Path,
    writer: impl FnOnce(&Path, &Project) -> Result<(), E>,
) -> Result<CollectedProject, E> {
    project.ensure_editable().map_err(MediaError::from)?;
    if output.exists() {
        return Err(MediaError::OutputExists(output.into()).into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(MediaError::from)?;
    let media = stage.path().join("assets");
    fs::create_dir(&media).map_err(MediaError::from)?;
    let mut copy = project.clone();
    for object in &mut copy.assets {
        let DocumentObject::Known(asset) = object else {
            return Err(
                MediaError::InvalidInput("opaque assets cannot be collected".into()).into(),
            );
        };
        let source = resolve_asset(asset, source_project)?;
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("bin");
        let name = format!("{}.{}", asset.id, extension);
        let target = media.join(&name);
        fs::copy(&source, &target).map_err(MediaError::from)?;
        if content_hash(&target)? != asset.content_hash {
            return Err(MediaError::AssetHashMismatch(source.display().to_string()).into());
        }
        asset.locator = AssetLocator {
            relative: Some(format!("assets/{name}")),
            absolute: None,
        };
    }
    let project_file = stage.path().join("project.kronello");
    writer(&project_file, &copy)?;
    // Reserve the final name atomically, refusing pre-existing output folders.
    fs::create_dir(output).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            MediaError::OutputExists(output.into())
        } else {
            MediaError::Io(e)
        }
    })?;
    if let Err(e) = fs::rename(stage.path(), output) {
        let _ = fs::remove_dir(output);
        return Err(MediaError::Io(e).into());
    }
    Ok(CollectedProject {
        directory: output.into(),
        project: output.join("project.kronello"),
        asset_count: copy.assets.len(),
    })
}
