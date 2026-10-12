//! Streaming publication artifact index; root receipts stay independent of file count.
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::JobError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_FILES: u64 = 4_000_001;
const MAX_LINE: usize = 32768;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactIndex {
    pub version: u32,
    pub bytes: u64,
    pub count: u64,
    pub roots: usize,
    pub sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    ordinal: u64,
    root: usize,
    path: PathBuf,
    bytes: u64,
    sha256: String,
}
fn invalid(message: &str) -> JobError {
    JobError::new("OUTPUT_VALIDATION_FAILED", message)
}
fn artifact_path(root: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        root.into()
    } else {
        root.join(relative)
    }
}
fn regular(path: &Path) -> Result<fs::Metadata, JobError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid("artifact is not a regular file"));
    }
    Ok(metadata)
}
fn digest_file(path: &Path) -> Result<(u64, String), JobError> {
    let before = regular(path)?;
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    if bytes != before.len() || bytes != file.metadata()?.len() {
        return Err(invalid("artifact length changed"));
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}
fn walk(
    root: &Path,
    relative: &Path,
    visit: &mut impl FnMut(&Path) -> Result<(), JobError>,
) -> Result<(), JobError> {
    if relative.components().count() > 4 || relative.as_os_str().len() > 4096 {
        return Err(invalid("artifact path budget"));
    }
    let path = artifact_path(root, relative);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.is_file() {
        visit(relative)?;
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            walk(root, &relative.join(entry?.file_name()), visit)?;
        }
    } else {
        return Err(invalid("artifact tree contains a non-regular entry"));
    }
    Ok(())
}
pub(crate) fn write(path: &Path, roots: &[PathBuf]) -> Result<ArtifactIndex, JobError> {
    if roots.is_empty() || roots.len() > 9 {
        return Err(invalid("publication root budget"));
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut out = BufWriter::with_capacity(65536, file);
    let mut count = 0;
    let result = (|| {
        for (root_index, root) in roots.iter().enumerate() {
            let start = count;
            walk(root, Path::new(""), &mut |relative| {
                if count >= MAX_FILES {
                    return Err(invalid("publication artifact count budget"));
                }
                let (bytes, sha256) = digest_file(&artifact_path(root, relative))?;
                let entry = Entry {
                    ordinal: count,
                    root: root_index,
                    path: relative.into(),
                    bytes,
                    sha256,
                };
                let mut line = serde_json::to_vec(&entry)?;
                line.push(b'\n');
                if line.len() > MAX_LINE {
                    return Err(invalid("artifact index line budget"));
                }
                out.write_all(&line)?;
                count += 1;
                Ok(())
            })?;
            if start == count {
                return Err(invalid("empty publication root"));
            }
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(())
    })();
    drop(out);
    if let Err(error) = result {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    let (bytes, sha256) = digest_file(path)?;
    Ok(ArtifactIndex {
        version: 2,
        bytes,
        count,
        roots: roots.len(),
        sha256,
    })
}
pub(crate) fn verify(
    path: &Path,
    index: &ArtifactIndex,
    roots: &[PathBuf],
) -> Result<(), JobError> {
    if index.version != 2
        || index.roots != roots.len()
        || roots.is_empty()
        || roots.len() > 9
        || index.count == 0
        || index.count > MAX_FILES
        || index.bytes > index.count * MAX_LINE as u64
        || regular(path)?.len() != index.bytes
    {
        return Err(invalid("artifact index contract"));
    }
    let mut input = BufReader::new(File::open(path)?);
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut bytes = 0;
    let mut root_counts = vec![0_u64; roots.len()];
    let mut last_root = 0;
    loop {
        let mut line = Vec::new();
        let length = input
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if length == 0 {
            break;
        }
        if length > MAX_LINE || line.last() != Some(&b'\n') {
            return Err(invalid("artifact index line bounds"));
        }
        hash.update(&line);
        bytes += length as u64;
        let entry: Entry = serde_json::from_slice(&line)?;
        if entry.ordinal != count
            || entry.root >= roots.len()
            || entry.root < last_root
            || entry.path.components().count() > 4
            || entry.path.as_os_str().len() > 4096
            || entry
                .path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(invalid("artifact index order or relative path"));
        }
        let actual = digest_file(&artifact_path(&roots[entry.root], &entry.path))?;
        if actual != (entry.bytes, entry.sha256) {
            return Err(invalid("artifact differs from publication index"));
        }
        root_counts[entry.root] += 1;
        last_root = entry.root;
        count += 1;
        if count > index.count {
            return Err(invalid("artifact index count"));
        }
    }
    if count != index.count
        || bytes != index.bytes
        || format!("{:x}", hash.finalize()) != index.sha256
    {
        return Err(invalid("artifact index hash or final count"));
    }
    for (root, expected) in roots.iter().zip(root_counts) {
        let mut actual = 0;
        walk(root, Path::new(""), &mut |_| {
            actual += 1;
            if actual > MAX_FILES {
                return Err(invalid("artifact tree count"));
            }
            Ok(())
        })?;
        if expected == 0 || actual != expected {
            return Err(invalid("publication root has missing or extra files"));
        }
    }
    Ok(())
}
