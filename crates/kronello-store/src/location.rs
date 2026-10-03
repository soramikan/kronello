use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

use crate::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenMode {
    #[default]
    Auto,
    ForceNormal,
    ForceSafe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectedLocation {
    Local,
    SyncFolder,
    NetworkFileSystem,
}

pub trait LocationDetector {
    fn detect(&self, path: &Path) -> Result<DetectedLocation, StoreError>;
}

#[derive(Debug, Default)]
pub struct SystemLocationDetector;
impl LocationDetector for SystemLocationDetector {
    fn detect(&self, path: &Path) -> Result<DetectedLocation, StoreError> {
        let home = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        let existing = if path.exists() {
            path
        } else {
            path.parent()
                .ok_or_else(|| StoreError::InvalidLocation("missing parent".into()))?
        };
        let filesystem = filesystem_name(existing)?;
        Ok(detect_location(
            path,
            home.as_deref(),
            filesystem.as_deref(),
        ))
    }
}

/// Pure, injectable heuristic. OS mount discovery is kept outside this function.
pub fn detect_location(
    path: &Path,
    home: Option<&Path>,
    filesystem: Option<&str>,
) -> DetectedLocation {
    if filesystem.is_some_and(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "smbfs" | "cifs" | "nfs" | "nfs4" | "afpfs" | "webdav" | "davfs" | "davfs2" | "fuse"
        )
    }) {
        return DetectedLocation::NetworkFileSystem;
    }
    if let Some(relative) = home.and_then(|home| path.strip_prefix(home).ok())
        && (relative.starts_with("Library/CloudStorage")
            || relative.starts_with("Library/Mobile Documents")
            || relative.components().any(|part| {
                let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
                ["dropbox", "onedrive", "google drive", "googledrive"]
                    .iter()
                    .any(|prefix| name == *prefix || name.starts_with(&format!("{prefix} -")))
            }))
    {
        return DetectedLocation::SyncFolder;
    }
    DetectedLocation::Local
}

#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    pub mode: OpenMode,
}

/// OS cache root, shared by content identity, never a table or project sibling.
/// Rejects projects stored inside the cache namespace to preserve separation.
pub fn render_cache_location(project_path: &Path) -> Result<PathBuf, StoreError> {
    let dirs = ProjectDirs::from("org", "kronello", "kronello")
        .ok_or_else(|| StoreError::InvalidLocation("OS cache directory is unavailable".into()))?;
    let cache = dirs.cache_dir().join("render");
    let actual = if project_path.exists() {
        project_path.canonicalize()?
    } else {
        project_path.to_path_buf()
    };
    let parent = actual
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    // Canonicalize existing ancestors so symlink aliases cannot bypass separation.
    let mut ancestor = cache.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .ok_or_else(|| StoreError::InvalidLocation("invalid cache path".into()))?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| StoreError::InvalidLocation("invalid cache root".into()))?;
    }
    let mut resolved = ancestor.canonicalize()?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    if resolved.starts_with(&parent) || parent.starts_with(&resolved) {
        return Err(StoreError::InvalidLocation(
            "project and render cache overlap".into(),
        ));
    }
    Ok(resolved)
}

#[cfg(unix)]
fn filesystem_name(path: &Path) -> Result<Option<String>, StoreError> {
    let info = nix::sys::statfs::statfs(path)
        .map_err(|error| std::io::Error::from_raw_os_error(error as i32))?;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    return Ok(Some(info.filesystem_type_name().to_owned()));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use nix::sys::statfs::{FUSE_SUPER_MAGIC, FsType, NFS_SUPER_MAGIC, SMB_SUPER_MAGIC};
        let kind = info.filesystem_type();
        let name = if kind == NFS_SUPER_MAGIC {
            "nfs"
        } else if kind == SMB_SUPER_MAGIC
            || kind == FsType(0xFF53_4D42u32 as _)
            || kind == FsType(0xFE53_4D42u32 as _)
        {
            "cifs"
        } else if kind == FUSE_SUPER_MAGIC {
            "fuse"
        } else {
            "local"
        };
        Ok(Some(name.into()))
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        let _ = info;
        Ok(None)
    }
}

#[cfg(not(unix))]
fn filesystem_name(path: &Path) -> Result<Option<String>, StoreError> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::UNC(_,_) | Prefix::VerbatimUNC(_,_)))
        {
            return Ok(Some("cifs".into()));
        }
    }
    let _ = path;
    Ok(None)
}
