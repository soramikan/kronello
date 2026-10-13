//! Disk-backed, immutable decoded audio. Each source retains one 4096-frame
//! window in its own speaker layout; seek/read failures propagate instead of
//! substituting silence. Layouts never fold down implicitly (ADR-0124).
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use kronello_audio::{AudioError, ChannelSourceReader};
use kronello_model::{Asset, AssetId, AssetKind, ChannelMask};

use crate::{MediaError, MediaRuntime, content_hash, resolve_asset};

const WINDOW: usize = 4096;
struct Window {
    file: std::fs::File,
    index: Option<std::fs::File>,
    cache_header: Option<std::path::PathBuf>,
    start: usize,
    bytes: Vec<u8>,
}
struct Source {
    _temporary: Option<tempfile::NamedTempFile>,
    mask: ChannelMask,
    length: usize,
    window: RefCell<Window>,
}
impl Source {
    fn frame_bytes(&self) -> usize {
        self.mask.channels() * 4
    }
}
pub(crate) struct SpoolSources {
    sources: BTreeMap<(AssetId, u32), Source>,
    pub decoded_bytes: u64,
    read_bytes: Cell<u64>,
    window_payload_bytes: Cell<u64>,
}
impl SpoolSources {
    pub fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            decoded_bytes: 0,
            read_bytes: Cell::new(0),
            window_payload_bytes: Cell::new(0),
        }
    }
    pub fn read_bytes(&self) -> u64 {
        self.read_bytes.get()
    }
    pub fn contains(&self, key: &(AssetId, u32)) -> bool {
        self.sources.contains_key(key)
    }
    pub fn decode(
        &mut self,
        runtime: &MediaRuntime,
        asset: &Asset,
        project_path: &Path,
        stream: u32,
        stage: &Path,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
            return Err(MediaError::InvalidInput("asset is not audio/video".into()));
        }
        let path = resolve_asset(asset, project_path)?;
        let fingerprint = crate::assets::FileFingerprint::of(&std::fs::metadata(&path)?);
        let lease = runtime
            .audio_spools
            .as_ref()
            .map(|cache| cache.lease(asset, stream, runtime.capabilities(), project_path))
            .transpose()?
            .flatten();
        if let Some(lease) = &lease {
            let _verify = kronello_render::ExecutionDiagnosticsCollector::from_environment()
                .time("audio_spool_cache_validate");
            if let Some((reader, index, cache_header, report)) = lease.load(checkpoint)? {
                if crate::assets::FileFingerprint::of(&std::fs::metadata(&path)?) != fingerprint
                    || resolve_asset(asset, project_path)? != path
                {
                    return Err(MediaError::AssetHashMismatch(path.display().to_string()));
                }
                kronello_render::count_execution("audio_spool_cache_hits", 1);
                self.sources.insert(
                    (asset.id, stream),
                    Source {
                        _temporary: None,
                        mask: report.mask,
                        length: report.frames,
                        window: RefCell::new(Window {
                            file: reader,
                            index: Some(index),
                            cache_header: Some(cache_header),
                            start: usize::MAX,
                            bytes: Vec::new(),
                        }),
                    },
                );
                return Ok(());
            }
        }
        if runtime.audio_spools.is_some() {
            kronello_render::count_execution("audio_spool_cache_misses", 1);
        }
        kronello_render::count_execution("audio_source_decodes", 1);
        let mut file = tempfile::NamedTempFile::new_in(stage)?;
        // The mask is constant within a stream and reported once on the
        // decode report; chunks are raw interleaved f32 in that order.
        let report = runtime.decode_audio_stream(&path, stream, &mut |chunk, _mask| {
            checkpoint(0)?;
            let bytes: Vec<_> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
            kronello_render::count_execution("audio_spool_chunks", 1);
            kronello_render::observe_execution_payload(
                "audio_decode_chunk",
                (chunk.len() as u64) * 4,
            );
            kronello_render::observe_execution_payload(
                "audio_spool_write_buffer",
                bytes.len() as u64,
            );
            file.write_all(&bytes)?;
            Ok(())
        })?;
        let length = report.frames;
        let mask = report.mask;
        if content_hash(&path)? != asset.content_hash {
            return Err(MediaError::AssetHashMismatch(path.display().to_string()));
        }
        file.flush()?;
        if crate::assets::FileFingerprint::of(&std::fs::metadata(&path)?) != fingerprint {
            return Err(MediaError::AssetHashMismatch(path.display().to_string()));
        }
        if let Some(lease) = lease {
            let _publish = kronello_render::ExecutionDiagnosticsCollector::from_environment()
                .time("audio_spool_cache_publish");
            if lease.publish(&file, &report, checkpoint)? {
                kronello_render::count_execution("audio_spool_cache_writes", 1);
            }
        }
        let reader = file.reopen()?;
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(
                (length as u64)
                    .checked_mul((mask.channels() * 4) as u64)
                    .ok_or_else(|| MediaError::InvalidInput("audio spool size overflow".into()))?,
            )
            .ok_or_else(|| MediaError::InvalidInput("audio spool total overflow".into()))?;
        self.sources.insert(
            (asset.id, stream),
            Source {
                _temporary: Some(file),
                mask,
                length,
                window: RefCell::new(Window {
                    file: reader,
                    index: None,
                    cache_header: None,
                    start: usize::MAX,
                    bytes: Vec::new(),
                }),
            },
        );
        Ok(())
    }
}
impl ChannelSourceReader for SpoolSources {
    fn layout(&self, asset: AssetId, stream: u32) -> Result<ChannelMask, AudioError> {
        Ok(self
            .sources
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .mask)
    }
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError> {
        Ok(self
            .sources
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .length)
    }
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        let source = self
            .sources
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?;
        if out.len() != source.mask.channels() {
            return Err(AudioError::UnsupportedChannelLayout(
                "spool frame sized for a different layout".into(),
            ));
        }
        if index >= source.length {
            return Err(AudioError::SourceTooShort(asset));
        }
        let frame_bytes = source.frame_bytes();
        let mut window = source.window.borrow_mut();
        let begin = index / WINDOW * WINDOW;
        if window.start != begin {
            let count = WINDOW.min(source.length - begin);
            window
                .file
                .seek(SeekFrom::Start((begin as u64) * frame_bytes as u64))
                .map_err(|e| AudioError::SourceRead(e.to_string()))?;
            let old_capacity = window.bytes.capacity() as u64;
            window.bytes.resize(count * frame_bytes, 0);
            let retained = self
                .window_payload_bytes
                .get()
                .saturating_sub(old_capacity)
                .saturating_add(window.bytes.capacity() as u64);
            self.window_payload_bytes.set(retained);
            kronello_render::observe_execution_payload("audio_source_windows", retained);
            let Window {
                file,
                bytes,
                index,
                cache_header,
                ..
            } = &mut *window;
            file.read_exact(bytes)
                .map_err(|e| AudioError::SourceRead(e.to_string()))?;
            if let Some(checksums) = index {
                use sha2::Digest;
                let mut expected = [0_u8; 32];
                checksums
                    .seek(SeekFrom::Start((begin / WINDOW) as u64 * 32))
                    .and_then(|_| checksums.read_exact(&mut expected))
                    .map_err(|e| AudioError::SourceRead(e.to_string()))?;
                if sha2::Sha256::digest(&*bytes).as_slice() != expected {
                    // Reject before returning samples; retire only this
                    // derivative header so a later attempt decodes afresh.
                    if let Some(header) = cache_header {
                        let _ = std::fs::remove_file(header);
                    }
                    return Err(AudioError::SourceRead(
                        "cached spool block checksum differs".into(),
                    ));
                }
                kronello_render::count_execution("audio_spool_cache_verified_windows", 1);
            }
            self.read_bytes.set(
                self.read_bytes
                    .get()
                    .checked_add((count as u64) * frame_bytes as u64)
                    .ok_or(AudioError::Overflow)?,
            );
            window.start = begin;
        }
        let offset = (index - begin) * frame_bytes;
        for (channel, value) in out.iter_mut().enumerate() {
            let start = offset + channel * 4;
            *value = f32::from_le_bytes(
                window.bytes[start..start + 4]
                    .try_into()
                    .expect("four bytes"),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    fn wave(path: &Path, value: i16) {
        let mut data = Vec::new();
        for i in 0..4800_i16 {
            data.extend((value.wrapping_add(i % 100)).to_le_bytes());
            data.extend((-value).to_le_bytes());
        }
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(48000_u32.to_le_bytes());
        bytes.extend(192000_u32.to_le_bytes());
        bytes.extend(4_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        std::fs::write(path, bytes).unwrap();
    }
    fn asset(path: &Path) -> Asset {
        Asset {
            id: AssetId::new(),
            kind: AssetKind::Audio,
            content_hash: content_hash(path).unwrap(),
            locator: kronello_model::AssetLocator {
                relative: None,
                absolute: Some(path.to_str().unwrap().into()),
            },
            streams: vec![],
        }
    }
    fn payloads(root: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(root.join("source-pcm-v2"))
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                (path.extension().and_then(|s| s.to_str()) == Some("pcm")).then_some(path)
            })
            .collect()
    }
    #[test]
    fn verified_spool_survives_stage_cleanup_and_rejects_corruption_and_source_rewrite() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = project.path().join("source.wav");
        wave(&path, 1000);
        let asset = asset(&path);
        let runtime = MediaRuntime::load()
            .unwrap()
            .with_audio_spool_cache(cache.path().into(), 100000, 2)
            .unwrap();
        let diagnostics = crate::ExportDiagnosticsCollector::enabled();
        let _scope = diagnostics.activate();
        let expected = {
            let stage = tempfile::tempdir().unwrap();
            let mut sources = SpoolSources::new();
            sources
                .decode(
                    &runtime,
                    &asset,
                    &project.path().join("project.kronello"),
                    0,
                    stage.path(),
                    &mut |_| Ok(()),
                )
                .unwrap();
            let mut frame = [0.0; 2];
            sources.read_frame(asset.id, 0, 137, &mut frame).unwrap();
            frame
        };
        assert_eq!(payloads(cache.path()).len(), 1);
        let stage = tempfile::tempdir().unwrap();
        let mut sources = SpoolSources::new();
        sources
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(sources.decoded_bytes, 0);
        let mut frame = [0.0; 2];
        sources.read_frame(asset.id, 0, 137, &mut frame).unwrap();
        assert_eq!(frame, expected);
        drop(sources);
        let pcm = payloads(cache.path()).pop().unwrap();
        let mut bytes = std::fs::read(&pcm).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&pcm, bytes).unwrap();
        let mut corrupted = SpoolSources::new();
        corrupted
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(corrupted.decoded_bytes, 0);
        assert!(
            corrupted
                .read_frame(asset.id, 0, 137, &mut frame)
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );
        drop(corrupted);
        let mut repaired = SpoolSources::new();
        repaired
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(repaired.decoded_bytes, 4800 * 2 * 4);
        repaired.read_frame(asset.id, 0, 137, &mut frame).unwrap();
        assert_eq!(frame, expected);
        wave(&path, 2000);
        let error = SpoolSources::new()
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap_err();
        assert_eq!(error.code(), "ASSET_HASH_MISMATCH");
        let counts = diagnostics.snapshot().unwrap().counts;
        assert_eq!(counts["audio_source_decodes"], 2);
        assert_eq!(counts["audio_spool_cache_hits"], 2);
    }
    #[test]
    fn cache_quota_and_contended_writer_keep_private_spooling_valid() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = project.path().join("source.wav");
        wave(&path, 1000);
        let asset = asset(&path);
        let runtime = MediaRuntime::load()
            .unwrap()
            .with_audio_spool_cache(cache.path().into(), 100, 1)
            .unwrap();
        let stage = tempfile::tempdir().unwrap();
        let mut sources = SpoolSources::new();
        sources
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(sources.frame_count(asset.id, 0).unwrap(), 4800);
        assert!(payloads(cache.path()).is_empty());
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(cache.path().join("source-pcm-v2/cache.lock"))
            .unwrap();
        lock.lock().unwrap();
        let mut independent = SpoolSources::new();
        independent
            .decode(
                &runtime,
                &asset,
                &project.path().join("project.kronello"),
                0,
                stage.path(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(independent.frame_count(asset.id, 0).unwrap(), 4800);
        assert!(payloads(cache.path()).is_empty());
    }
}
