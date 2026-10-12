//! Optional disk-backed decoded-source reuse; all entries remain derivative.
use crate::{AudioDecodeReport, MediaCapabilities, MediaError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct SpoolCache {
    root: PathBuf,
    bytes: u64,
    entries: usize,
    executable_hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    key: String,
    index_sha256: String,
    report: AudioDecodeReport,
}
pub(crate) struct Lease {
    cache: SpoolCache,
    _lock: File,
    key: String,
}
impl SpoolCache {
    pub fn new(root: PathBuf, bytes: u64, entries: usize) -> Result<Self, MediaError> {
        if !root.is_absolute() || bytes == 0 || entries == 0 || entries > 64 {
            return Err(MediaError::InvalidInput(
                "audio spool cache configuration".into(),
            ));
        }
        let root = root.join("source-pcm-v2");
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
            bytes,
            entries,
            executable_hash: crate::content_hash(&std::env::current_exe()?)?,
        })
    }
    pub fn lease(
        &self,
        asset: &kronello_model::Asset,
        stream: u32,
        capabilities: &MediaCapabilities,
        project: &Path,
    ) -> Result<Option<Lease>, MediaError> {
        let project = project.parent().unwrap_or(Path::new(".")).canonicalize()?;
        if self.root.starts_with(project) {
            return Err(MediaError::InvalidInput(
                "audio spool cache must be outside the project directory".into(),
            ));
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("cache.lock"))?;
        if lock.try_lock().is_err() {
            return Ok(None);
        }
        // A successful global writer lease proves no other cache writer owns
        // these temporary names; remove only our abandoned publications.
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".spool-payload-") || name.starts_with(".spool-header-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let mut libraries = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(&capabilities.library_directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ["avutil", "avcodec", "avformat", "swresample", "swscale"]
                .iter()
                .any(|part| name.contains(part))
                && (name.ends_with(".dylib") || name.ends_with(".dll") || name.contains(".so"))
            {
                let path = entry.path().canonicalize()?;
                if path.is_file() {
                    libraries.insert(path);
                }
            }
        }
        if libraries.is_empty() {
            return Err(MediaError::InvalidInput(
                "audio cache runtime libraries unavailable".into(),
            ));
        }
        let libraries = libraries
            .into_iter()
            .map(|path| {
                let before = crate::assets::FileFingerprint::of(&std::fs::metadata(&path)?);
                let hash = crate::content_hash(&path)?;
                if before != crate::assets::FileFingerprint::of(&std::fs::metadata(&path)?) {
                    return Err(MediaError::InvalidInput(
                        "audio cache runtime library changed".into(),
                    ));
                }
                Ok((path, hash))
            })
            .collect::<Result<Vec<_>, MediaError>>()?;
        let input = serde_json::to_vec(&(
            2_u32,
            &self.executable_hash,
            capabilities,
            libraries,
            &asset.content_hash,
            &asset.streams,
            stream,
        ))?;
        let key = format!("{:x}", Sha256::digest(input));
        Ok(Some(Lease {
            cache: self.clone(),
            _lock: lock,
            key,
        }))
    }
}
impl Lease {
    fn path(&self, extension: &str) -> PathBuf {
        self.cache.root.join(format!("{}.{}", self.key, extension))
    }
    pub fn load(
        &self,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<Option<(File, File, PathBuf, AudioDecodeReport)>, MediaError> {
        let header_path = self.path("json");
        let Ok(header_file) = File::open(header_path) else {
            return Ok(None);
        };
        if header_file.metadata()?.len() > 65536 {
            return Ok(None);
        }
        let header: Header = match serde_json::from_reader(header_file.take(65537)) {
            Ok(header) => header,
            Err(_) => return Ok(None),
        };
        if header.version != 2 || header.key != self.key || header.report.frames == 0 {
            return Ok(None);
        }
        let Some(bytes) =
            (header.report.frames as u64).checked_mul(header.report.mask.channels() as u64 * 4)
        else {
            return Ok(None);
        };
        if bytes > self.cache.bytes {
            return Ok(None);
        }
        let Ok(pcm) = File::open(self.path("pcm")) else {
            return Ok(None);
        };
        if !pcm.metadata()?.is_file() || pcm.metadata()?.len() != bytes {
            return Ok(None);
        }
        let Ok(mut index) = File::open(self.path("idx")) else {
            return Ok(None);
        };
        let expected = (header.report.frames as u64).div_ceil(4096).checked_mul(32);
        if expected != Some(index.metadata()?.len())
            || !index.metadata()?.is_file()
            || hash_reader(&mut index, checkpoint)? != header.index_sha256
        {
            return Ok(None);
        }
        index.seek(SeekFrom::Start(0))?;
        // Operational LRU only; timestamps never enter authored identity.
        let _ = pcm.set_modified(std::time::SystemTime::now());
        Ok(Some((pcm, index, self.path("json"), header.report)))
    }
    pub fn publish(
        &self,
        temporary: &tempfile::NamedTempFile,
        report: &AudioDecodeReport,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<bool, MediaError> {
        let bytes = temporary.as_file().metadata()?.len();
        if bytes > self.cache.bytes || report.frames == 0 {
            return Ok(false);
        }
        let mut owned = Vec::new();
        for entry in std::fs::read_dir(&self.cache.root)? {
            let entry = entry?;
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if path.extension().and_then(|s| s.to_str()) != Some("pcm")
                || stem.len() != 64
                || !stem.bytes().all(|b| b.is_ascii_hexdigit())
            {
                continue;
            }
            let meta = entry.metadata()?;
            if !meta.is_file() {
                continue;
            }
            owned.push((meta.modified().ok(), meta.len(), path));
        }
        owned.sort_by_key(|(modified, _, _)| *modified);
        let mut retained = owned
            .iter()
            .fold(0_u64, |sum, (_, bytes, _)| sum.saturating_add(*bytes));
        let mut count = owned.len();
        for (_, length, path) in owned {
            if retained.saturating_add(bytes) <= self.cache.bytes && count < self.cache.entries {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                let _ = std::fs::remove_file(path.with_extension("json"));
                let _ = std::fs::remove_file(path.with_extension("idx"));
                retained -= length;
                count -= 1;
            }
        }
        if retained.saturating_add(bytes) > self.cache.bytes || count >= self.cache.entries {
            return Ok(false);
        }
        // Verify the compact checksum index at open, then each PCM window
        // before returning any f32. Memory does not scale with source length.
        let mut input = temporary.reopen()?;
        let mut index_file = tempfile::Builder::new()
            .prefix(".spool-header-")
            .tempfile_in(&self.cache.root)?;
        let mut index_hash = Sha256::new();
        let mut buffer = vec![0_u8; 4096 * report.mask.channels() * 4];
        let mut remaining = bytes;
        while remaining > 0 {
            checkpoint(0)?;
            let count = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..count])?;
            let hash = Sha256::digest(&buffer[..count]);
            index_file.write_all(&hash)?;
            index_hash.update(hash);
            remaining -= count as u64;
        }
        let index_sha256 = format!("{:x}", index_hash.finalize());
        index_file.flush()?;
        index_file.as_file().sync_all()?;
        index_file
            .persist(self.path("idx"))
            .map_err(|error| MediaError::Io(error.error))?;
        // Header is published last. Readers hold the same OS lock, so they
        // never consume a partially replaced payload/header pair.
        // Remove an invalid previous entry before admission. A live Windows
        // reader can refuse removal; keep its file and use the private spool.
        if self.path("pcm").exists() && std::fs::remove_file(self.path("pcm")).is_err() {
            return Ok(false);
        }
        if std::fs::hard_link(temporary.path(), self.path("pcm")).is_err() {
            let mut payload = tempfile::Builder::new()
                .prefix(".spool-payload-")
                .tempfile_in(&self.cache.root)?;
            std::io::copy(&mut temporary.reopen()?, &mut payload)?;
            payload.flush()?;
            payload.as_file().sync_all()?;
            payload
                .persist(self.path("pcm"))
                .map_err(|error| MediaError::Io(error.error))?;
        }
        let mut header = tempfile::Builder::new()
            .prefix(".spool-header-")
            .tempfile_in(&self.cache.root)?;
        serde_json::to_writer(
            &mut header,
            &Header {
                version: 2,
                key: self.key.clone(),
                index_sha256,
                report: report.clone(),
            },
        )?;
        header.flush()?;
        header.as_file().sync_all()?;
        header
            .persist(self.path("json"))
            .map_err(|error| MediaError::Io(error.error))?;
        Ok(true)
    }
}
fn hash_reader(
    file: &mut File,
    checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
) -> Result<String, MediaError> {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        checkpoint(0)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
