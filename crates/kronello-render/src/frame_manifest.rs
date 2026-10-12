//! Bounded, independently verifiable frame metadata for export reports.
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use kronello_time::FrameRate;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{FrameMetadata, RenderError};

pub const FRAME_METADATA_CHUNK_FRAMES: usize = 64;
pub const FRAME_METADATA_CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const FRAME_METADATA_HEADER_BYTES: usize = 8 * 1024 * 1024;
pub const FRAME_METADATA_PAGE_BYTES: usize = 16 * 1024 * 1024;
const INDEX_LINE_BYTES: usize = 1024;
const COMMON: &[&str] = &[
    "schema_version",
    "snapshot_schema_version",
    "project_schema_version",
    "snapshot_content_hash",
    "revision",
    "composition",
    "target",
    "semantic_versions",
    "font_locks",
    "design_extent",
    "region",
    "design_to_pixel",
    "working_space",
    "flatten_tolerance_px",
    "numeric",
    "display",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameMetadataIdentity {
    pub render_snapshot_hash: String,
    pub frame_rate: FrameRate,
    pub start_frame: i64,
    pub frame_count: u64,
}
impl FrameMetadataIdentity {
    fn validate(&self) -> Result<(), RenderError> {
        if !sha_valid(&self.render_snapshot_hash)
            || !(1..=1_000_000).contains(&self.frame_count)
            || self
                .start_frame
                .checked_add(self.frame_count as i64)
                .is_none()
        {
            return Err(invalid("frame metadata identity or count"));
        }
        Ok(())
    }
    fn check_frame(&self, frame: &FrameMetadata, number: u64) -> Result<(), RenderError> {
        let index = self
            .start_frame
            .checked_add(number as i64)
            .ok_or_else(|| invalid("frame index overflow"))?;
        if number >= self.frame_count
            || frame.sequence_number != Some(number)
            || frame.frame_index.as_deref() != Some(index.to_string().as_str())
            || frame.time != self.frame_rate.frame_to_time(index)?
            || frame.snapshot_content_hash != self.render_snapshot_hash
        {
            return Err(invalid("frame metadata order, time or identity"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameMetadataManifest {
    pub schema_version: u32,
    pub root: PathBuf,
    pub root_sha256: String,
    pub identity: FrameMetadataIdentity,
    pub chunk_count: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema_version: u32,
    identity: FrameMetadataIdentity,
    common: Map<String, Value>,
    chunk_count: u64,
    index_bytes: u64,
    index_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    ordinal: u64,
    start: u64,
    count: u32,
    bytes: u64,
    sha256: String,
}
#[derive(Debug)]
pub struct FrameMetadataPage {
    pub offset: u64,
    pub next_offset: Option<u64>,
    pub frames: Vec<FrameMetadata>,
    pub files: Vec<Option<crate::SequenceFrameFiles>>,
}

/// Owns a newly-created private directory. Incomplete writes remove only it.
pub struct FrameMetadataWriter {
    root: PathBuf,
    identity: FrameMetadataIdentity,
    common: Option<Map<String, Value>>,
    records: Vec<Map<String, Value>>,
    record_bytes: usize,
    count: u64,
    chunk_count: u64,
    index: Option<BufWriter<File>>,
    index_hash: Sha256,
    index_bytes: u64,
    committed: bool,
}
impl FrameMetadataWriter {
    pub fn new(root: &Path, identity: FrameMetadataIdentity) -> Result<Self, RenderError> {
        identity.validate()?;
        fs::create_dir(root)?;
        let index = match File::create(root.join("chunks.jsonl")) {
            Ok(file) => BufWriter::new(file),
            Err(error) => {
                let _ = fs::remove_dir(root);
                return Err(error.into());
            }
        };
        Ok(Self {
            root: root.into(),
            identity,
            common: None,
            records: Vec::new(),
            record_bytes: 2,
            count: 0,
            chunk_count: 0,
            index: Some(index),
            index_hash: Sha256::new(),
            index_bytes: 0,
            committed: false,
        })
    }
    pub fn buffered_frames(&self) -> usize {
        self.records.len()
    }
    pub fn buffered_bytes(&self) -> usize {
        self.record_bytes
    }
    pub fn push(&mut self, frame: FrameMetadata) -> Result<(), RenderError> {
        self.push_with_files(frame, None)
    }
    pub fn push_with_files(
        &mut self,
        frame: FrameMetadata,
        files: Option<crate::SequenceFrameFiles>,
    ) -> Result<(), RenderError> {
        self.identity.check_frame(&frame, self.count)?;
        let Value::Object(mut delta) = serde_json::to_value(frame)? else {
            unreachable!()
        };
        if let Some(files) = files {
            delta.insert("__sequence_files".into(), serde_json::to_value(files)?);
        }
        let mut common = Map::new();
        for key in COMMON {
            common.insert(
                (*key).into(),
                delta
                    .remove(*key)
                    .ok_or_else(|| invalid("missing common field"))?,
            );
        }
        if let Some(expected) = &self.common {
            if *expected != common {
                return Err(invalid("common frame metadata changed"));
            }
        } else {
            if serde_json::to_vec(&common)?.len() > FRAME_METADATA_HEADER_BYTES {
                return Err(budget("common frame metadata"));
            }
            self.common = Some(common);
        }
        let bytes = serde_json::to_vec(&delta)?.len() + 1;
        if bytes + 2 > FRAME_METADATA_CHUNK_BYTES {
            return Err(budget("single frame metadata delta"));
        }
        if self.records.len() == FRAME_METADATA_CHUNK_FRAMES
            || self.record_bytes + bytes > FRAME_METADATA_CHUNK_BYTES
        {
            self.flush_chunk()?;
        }
        self.records.push(delta);
        self.record_bytes += bytes;
        self.count += 1;
        crate::observe_execution_payload(
            "frame_metadata_chunk_serialized",
            self.record_bytes as u64,
        );
        Ok(())
    }
    fn flush_chunk(&mut self) -> Result<(), RenderError> {
        if self.records.is_empty() {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&self.records)?;
        if bytes.len() > FRAME_METADATA_CHUNK_BYTES {
            return Err(budget("frame metadata chunk"));
        }
        let mut file = File::create(self.root.join(chunk_name(self.chunk_count)))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let entry = Chunk {
            ordinal: self.chunk_count,
            start: self.count - self.records.len() as u64,
            count: self.records.len() as u32,
            bytes: bytes.len() as u64,
            sha256: hash(&bytes),
        };
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        self.index
            .as_mut()
            .expect("active index")
            .write_all(&line)?;
        self.index_hash.update(&line);
        self.index_bytes += line.len() as u64;
        self.records.clear();
        self.record_bytes = 2;
        self.chunk_count += 1;
        Ok(())
    }
    pub fn finish(mut self) -> Result<FrameMetadataManifest, RenderError> {
        if self.count != self.identity.frame_count {
            return Err(invalid("incomplete frame metadata"));
        }
        self.flush_chunk()?;
        let index = self.index.as_mut().expect("active index");
        index.flush()?;
        index.get_ref().sync_all()?;
        let header = Header {
            schema_version: 2,
            identity: self.identity.clone(),
            common: self
                .common
                .take()
                .ok_or_else(|| invalid("missing common header"))?,
            chunk_count: self.chunk_count,
            index_bytes: self.index_bytes,
            index_sha256: format!("{:x}", self.index_hash.clone().finalize()),
        };
        let bytes = serde_json::to_vec(&header)?;
        if bytes.len() > FRAME_METADATA_HEADER_BYTES {
            return Err(budget("frame metadata header"));
        }
        let mut file = File::create(self.root.join("header.json"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        self.committed = true;
        Ok(FrameMetadataManifest {
            schema_version: 2,
            root: self.root.clone(),
            root_sha256: hash(&bytes),
            identity: self.identity.clone(),
            chunk_count: self.chunk_count,
        })
    }
}
impl Drop for FrameMetadataWriter {
    fn drop(&mut self) {
        // Close the index before reclaiming its directory on every platform.
        drop(self.index.take());
        if !self.committed {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
impl FrameMetadataManifest {
    /// Paths are relocatable; the content/identity attestation is unchanged.
    pub fn relocated(&self, root: PathBuf) -> Self {
        Self {
            root,
            ..self.clone()
        }
    }
    pub fn verify(&self) -> Result<(), RenderError> {
        self.visit(None, |_, _, _| Ok(()))
    }
    pub fn for_each_frame(
        &self,
        visit: impl FnMut(
            u64,
            FrameMetadata,
            Option<crate::SequenceFrameFiles>,
        ) -> Result<(), RenderError>,
    ) -> Result<(), RenderError> {
        self.visit(None, visit)
    }
    pub fn read_page(&self, offset: u64, limit: usize) -> Result<FrameMetadataPage, RenderError> {
        if offset > self.identity.frame_count || !(1..=FRAME_METADATA_CHUNK_FRAMES).contains(&limit)
        {
            return Err(RenderError::InvalidInput(
                "frame metadata page bounds".into(),
            ));
        }
        let mut frames = Vec::new();
        let mut files = Vec::new();
        let mut bytes = 0;
        let mut full = false;
        self.visit(
            Some(offset..offset + limit as u64),
            |number, frame, refs| {
                if number >= offset && frames.len() < limit && !full {
                    let size = serde_json::to_vec(&frame)?.len() + serde_json::to_vec(&refs)?.len();
                    if size > FRAME_METADATA_PAGE_BYTES {
                        return Err(budget("single metadata page frame"));
                    }
                    if bytes + size > FRAME_METADATA_PAGE_BYTES {
                        full = true;
                    } else {
                        bytes += size;
                        frames.push(frame);
                        files.push(refs);
                    }
                }
                Ok(())
            },
        )?;
        let next = offset + frames.len() as u64;
        if files.iter().all(Option::is_none) {
            files.clear();
        }
        Ok(FrameMetadataPage {
            offset,
            next_offset: (next < self.identity.frame_count).then_some(next),
            frames,
            files,
        })
    }
    fn visit(
        &self,
        page: Option<std::ops::Range<u64>>,
        mut visit: impl FnMut(
            u64,
            FrameMetadata,
            Option<crate::SequenceFrameFiles>,
        ) -> Result<(), RenderError>,
    ) -> Result<(), RenderError> {
        self.identity.validate()?;
        if self.schema_version != 2
            || !sha_valid(&self.root_sha256)
            || !fs::symlink_metadata(&self.root)?.is_dir()
        {
            return Err(invalid("frame manifest root"));
        }
        let bytes = read_bounded(&self.root.join("header.json"), FRAME_METADATA_HEADER_BYTES)?;
        if hash(&bytes) != self.root_sha256 {
            return Err(invalid("frame metadata header hash"));
        }
        let header: Header = serde_json::from_slice(&bytes)?;
        if header.schema_version != 2
            || header.identity != self.identity
            || header.chunk_count != self.chunk_count
            || self.chunk_count == 0
            || self.chunk_count > self.identity.frame_count
            || header.index_bytes > self.chunk_count * INDEX_LINE_BYTES as u64
            || !sha_valid(&header.index_sha256)
            || header.common.len() != COMMON.len()
            || COMMON.iter().any(|key| !header.common.contains_key(*key))
        {
            return Err(invalid("frame metadata header contract"));
        }
        let path = self.root.join("chunks.jsonl");
        if !fs::symlink_metadata(&path)?.is_file()
            || fs::metadata(&path)?.len() != header.index_bytes
        {
            return Err(invalid("frame metadata index length"));
        }
        let mut input = BufReader::new(File::open(path)?);
        let mut index_hash = Sha256::new();
        let mut ordinal = 0;
        let mut number = 0;
        let mut read_bytes = 0;
        loop {
            let mut line = Vec::new();
            let count = input
                .by_ref()
                .take((INDEX_LINE_BYTES + 1) as u64)
                .read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            if count > INDEX_LINE_BYTES || line.last() != Some(&b'\n') {
                return Err(invalid("frame index line budget"));
            }
            index_hash.update(&line);
            read_bytes += count as u64;
            let entry: Chunk = serde_json::from_slice(&line)?;
            if entry.ordinal != ordinal
                || entry.start != number
                || entry.count == 0
                || entry.count as usize > FRAME_METADATA_CHUNK_FRAMES
                || entry.bytes > FRAME_METADATA_CHUNK_BYTES as u64
                || !sha_valid(&entry.sha256)
                || number + u64::from(entry.count) > self.identity.frame_count
            {
                return Err(invalid("frame chunk order or bounds"));
            }
            if page.as_ref().is_some_and(|page| {
                entry.start + u64::from(entry.count) <= page.start || entry.start >= page.end
            }) {
                // The complete index is still authenticated and checked for
                // contiguous ranges. Pages verify only their selected payload;
                // publication and recovery call verify() for every chunk.
                number += u64::from(entry.count);
                ordinal += 1;
                continue;
            }
            let data = read_bounded(
                &self.root.join(chunk_name(ordinal)),
                FRAME_METADATA_CHUNK_BYTES,
            )?;
            if data.len() as u64 != entry.bytes || hash(&data) != entry.sha256 {
                return Err(invalid("frame chunk hash or length"));
            }
            let records: Vec<Map<String, Value>> = serde_json::from_slice(&data)?;
            if records.len() != entry.count as usize {
                return Err(invalid("frame chunk count"));
            }
            for mut delta in records {
                let files: Option<crate::SequenceFrameFiles> = delta
                    .remove("__sequence_files")
                    .map(serde_json::from_value)
                    .transpose()?;
                if let Some(files) = &files
                    && (files.numeric.name != format!("frame-{number:08}.rgba16f")
                        || files.display.name != format!("frame-{number:08}.png")
                        || !sha_valid(&files.numeric.sha256)
                        || !sha_valid(&files.display.sha256))
                {
                    return Err(invalid("sequence frame file references"));
                }
                if delta.keys().any(|key| header.common.contains_key(key)) {
                    return Err(invalid("frame delta overwrites common header"));
                }
                let mut object = header.common.clone();
                object.extend(delta);
                let frame: FrameMetadata = serde_json::from_value(Value::Object(object))?;
                self.identity.check_frame(&frame, number)?;
                if let Some(files) = &files {
                    let size =
                        u64::from(frame.region.pixels[0]) * u64::from(frame.region.pixels[1]) * 8;
                    if files.numeric.bytes != size {
                        return Err(invalid("sequence numeric payload length"));
                    }
                }
                visit(number, frame, files)?;
                number += 1;
            }
            ordinal += 1;
        }
        if ordinal != self.chunk_count
            || number != self.identity.frame_count
            || read_bytes != header.index_bytes
            || format!("{:x}", index_hash.finalize()) != header.index_sha256
        {
            return Err(invalid("frame metadata index hash or final count"));
        }
        Ok(())
    }
}
fn chunk_name(ordinal: u64) -> String {
    format!("chunk-{ordinal:010}.json")
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sha_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, RenderError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(invalid("metadata file type or read budget"));
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
        return Err(invalid("metadata length changed"));
    }
    Ok(bytes)
}
fn invalid(message: &str) -> RenderError {
    RenderError::Backend {
        code: "OUTPUT_VALIDATION_FAILED",
        message: message.into(),
    }
}
fn budget(message: &str) -> RenderError {
    RenderError::Backend {
        code: "RESOURCE_LIMIT",
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(number: u64) -> FrameMetadata {
        let mut frame: FrameMetadata =
            serde_json::from_str(include_str!("../tests/fixtures/frame-metadata-v1.json")).unwrap();
        let rate = FrameRate::new(100, 1).unwrap();
        frame.time = rate.frame_to_time(number as i64).unwrap();
        frame.frame_index = Some(number.to_string());
        frame.sequence_number = Some(number);
        if let Some(stats) = frame.transfer_stats.as_mut() {
            stats.gpu_wait_operations = number;
        }
        frame
    }
    fn identity(count: u64) -> FrameMetadataIdentity {
        FrameMetadataIdentity {
            render_snapshot_hash: frame(0).snapshot_content_hash,
            frame_rate: FrameRate::new(100, 1).unwrap(),
            start_frame: 0,
            frame_count: count,
        }
    }
    #[test]
    fn bounded_chunks_restore_every_frame_across_page_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer =
            FrameMetadataWriter::new(&dir.path().join("frames"), identity(1000)).unwrap();
        for n in 0..1000 {
            writer.push(frame(n)).unwrap();
            assert!(writer.buffered_frames() <= FRAME_METADATA_CHUNK_FRAMES);
            assert!(writer.buffered_bytes() <= FRAME_METADATA_CHUNK_BYTES);
        }
        let manifest = writer.finish().unwrap();
        manifest.verify().unwrap();
        assert_eq!(manifest.chunk_count, 16);
        let mut offset = 0;
        loop {
            let page = manifest.read_page(offset, 63).unwrap();
            for (i, restored) in page.frames.iter().enumerate() {
                assert_eq!(*restored, frame(offset + i as u64));
            }
            match page.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert!(manifest.read_page(1000, 64).unwrap().frames.is_empty());
        assert!(manifest.read_page(1001, 64).is_err());
    }
    #[test]
    fn incomplete_writer_cleans_only_its_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("unrelated"), b"keep").unwrap();
        let root = dir.path().join("frames");
        let mut writer = FrameMetadataWriter::new(&root, identity(65)).unwrap();
        writer.push(frame(0)).unwrap();
        assert!(writer.finish().is_err());
        assert!(!root.exists());
        assert!(dir.path().join("unrelated").exists());
    }
    #[test]
    fn corrupted_missing_and_reordered_chunks_are_rejected() {
        for mode in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("frames");
            let mut writer = FrameMetadataWriter::new(&root, identity(65)).unwrap();
            for n in 0..65 {
                writer.push(frame(n)).unwrap();
            }
            let mut manifest = writer.finish().unwrap();
            if mode == 0 {
                fs::write(root.join(chunk_name(0)), b"[]").unwrap();
            }
            if mode == 1 {
                fs::remove_file(root.join(chunk_name(1))).unwrap();
            }
            if mode == 2 {
                let index = fs::read_to_string(root.join("chunks.jsonl")).unwrap();
                let mut lines: Vec<_> = index.lines().map(str::to_owned).collect();
                lines.swap(0, 1);
                let data = (lines.join("\n") + "\n").into_bytes();
                fs::write(root.join("chunks.jsonl"), &data).unwrap();
                let mut header: Header =
                    serde_json::from_slice(&fs::read(root.join("header.json")).unwrap()).unwrap();
                header.index_sha256 = hash(&data);
                let bytes = serde_json::to_vec(&header).unwrap();
                fs::write(root.join("header.json"), &bytes).unwrap();
                manifest.root_sha256 = hash(&bytes);
            }
            assert!(manifest.verify().is_err());
            let offset = if mode == 1 { 64 } else { 0 };
            assert!(manifest.read_page(offset, 1).is_err());
        }
    }
}
