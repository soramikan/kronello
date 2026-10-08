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
    start: usize,
    bytes: Vec<u8>,
}
struct Source {
    _temporary: tempfile::NamedTempFile,
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
}
impl SpoolSources {
    pub fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            decoded_bytes: 0,
            read_bytes: Cell::new(0),
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
        let mut file = tempfile::NamedTempFile::new_in(stage)?;
        // The mask is constant within a stream and reported once on the
        // decode report; chunks are raw interleaved f32 in that order.
        let report = runtime.decode_audio_stream(&path, stream, &mut |chunk, _mask| {
            checkpoint(0)?;
            let bytes: Vec<_> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
            file.write_all(&bytes)?;
            Ok(())
        })?;
        let length = report.frames;
        let mask = report.mask;
        if content_hash(&path)? != asset.content_hash {
            return Err(MediaError::AssetHashMismatch(path.display().to_string()));
        }
        file.flush()?;
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
                _temporary: file,
                mask,
                length,
                window: RefCell::new(Window {
                    file: reader,
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
            window.bytes.resize(count * frame_bytes, 0);
            let Window { file, bytes, .. } = &mut *window;
            file.read_exact(bytes)
                .map_err(|e| AudioError::SourceRead(e.to_string()))?;
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
