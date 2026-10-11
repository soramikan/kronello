//! Preparation-owned immutable PCM reuse. No native handles cross threads.
use crate::{
    DecodedAudio, MediaError, MediaRuntime,
    assets::{FileFingerprint, locate_asset},
};
use kronello_audio::{AudioError, MAX_AUDIO_FRAMES};
use kronello_model::Asset;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

#[derive(Debug, Clone, Copy)]
pub struct AudioSourceCacheLimits {
    pub entries: usize,
    pub pcm_bytes: usize,
}
impl Default for AudioSourceCacheLimits {
    fn default() -> Self {
        Self {
            entries: 32,
            pcm_bytes: 128 * 1024 * 1024,
        }
    }
}
/// Retained PCM payload, not allocator/RSS or all active Arc owners.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct AudioSourceCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub bypasses: u64,
    pub decoded_sources: u64,
    pub retained_sources: usize,
    pub retained_pcm_bytes: usize,
    pub peak_retained_pcm_bytes: usize,
}
struct Entry {
    descriptor: String,
    path: PathBuf,
    fingerprint: FileFingerprint,
    decoded: Arc<DecodedAudio>,
    bytes: usize,
}
/// A caller owns and bounds this cache; there is no global authored state.
/// Native decoding and file verification belong on a preparation thread.
pub struct AudioSourceCache {
    limits: AudioSourceCacheLimits,
    entries: VecDeque<Entry>,
    stats: AudioSourceCacheStats,
}
impl Default for AudioSourceCache {
    fn default() -> Self {
        Self::new(AudioSourceCacheLimits::default())
    }
}
impl AudioSourceCache {
    pub fn new(limits: AudioSourceCacheLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            stats: AudioSourceCacheStats::default(),
        }
    }
    pub fn stats(&self) -> AudioSourceCacheStats {
        self.stats
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.refresh();
    }
    fn refresh(&mut self) {
        self.stats.retained_sources = self.entries.len();
        self.stats.retained_pcm_bytes = self.entries.iter().map(|entry| entry.bytes).sum();
        self.stats.peak_retained_pcm_bytes = self
            .stats
            .peak_retained_pcm_bytes
            .max(self.stats.retained_pcm_bytes);
    }
    /// Cold decodes verify full content before and after decode. Unix warm
    /// reuse requires the same canonical locator and size/mtime/ctime/inode
    /// twice. Other platforms decode fresh until an equally strong fence exists.
    pub fn decode(
        &mut self,
        runtime: &MediaRuntime,
        asset: &Asset,
        project: &Path,
        stream: u32,
        frame_budget: usize,
    ) -> Result<Arc<DecodedAudio>, MediaError> {
        if !matches!(
            asset.kind,
            kronello_model::AssetKind::Audio | kronello_model::AssetKind::Video
        ) {
            return Err(MediaError::InvalidInput("asset is not audio/video".into()));
        }
        let located = locate_asset(asset, project)?;
        let descriptor = descriptor(runtime, asset, stream)?;
        // Stale paths cannot keep consuming the retained payload budget.
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.path != located.path || entry.fingerprint == located.fingerprint);
        self.stats.evictions += (before - self.entries.len()) as u64;
        self.refresh();
        if cfg!(unix)
            && let Some(index) = self.entries.iter().position(|entry| {
                entry.descriptor == descriptor
                    && entry.path == located.path
                    && entry.fingerprint == located.fingerprint
            })
        {
            let confirmed = locate_asset(asset, project)?;
            if confirmed.path != located.path || confirmed.fingerprint != located.fingerprint {
                return Err(MediaError::InvalidInput(
                    "audio source changed during preparation".into(),
                ));
            }
            let entry = self.entries.remove(index).expect("located cache entry");
            let decoded = entry.decoded.clone();
            self.entries.push_back(entry);
            check_budget(&decoded, frame_budget)?;
            self.stats.hits += 1;
            return Ok(decoded);
        }
        self.stats.misses += 1;
        let decoded =
            Arc::new(runtime.decode_asset_audio_bounded(asset, project, stream, frame_budget)?);
        self.stats.decoded_sources += 1;
        let confirmed = locate_asset(asset, project)?;
        if confirmed.path != located.path || confirmed.fingerprint != located.fingerprint {
            return Err(MediaError::InvalidInput(
                "audio source changed during preparation".into(),
            ));
        }
        let bytes = std::mem::size_of_val(decoded.buffer.samples());
        if !cfg!(unix) || self.limits.entries == 0 || bytes > self.limits.pcm_bytes {
            self.stats.bypasses += 1;
            return Ok(decoded);
        }
        while self.entries.len() >= self.limits.entries
            || bytes > self.limits.pcm_bytes - self.stats.retained_pcm_bytes
        {
            self.entries
                .pop_front()
                .expect("nonempty over-budget cache");
            self.stats.evictions += 1;
            self.refresh();
        }
        self.entries.push_back(Entry {
            descriptor,
            path: located.path,
            fingerprint: located.fingerprint,
            decoded: decoded.clone(),
            bytes,
        });
        self.refresh();
        Ok(decoded)
    }
}
fn check_budget(decoded: &DecodedAudio, budget: usize) -> Result<(), MediaError> {
    if decoded.buffer.frame_count() > budget.min(MAX_AUDIO_FRAMES) {
        return Err(
            AudioError::BudgetExceeded("decoded audio sample budget exceeded".into()).into(),
        );
    }
    Ok(())
}
fn descriptor(runtime: &MediaRuntime, asset: &Asset, stream: u32) -> Result<String, MediaError> {
    static CODE: OnceLock<[u8; 32]> = OnceLock::new();
    let code = CODE.get_or_init(|| {
        let mut digest = Sha256::new();
        digest.update(include_bytes!("audio.rs"));
        digest.update(include_bytes!("../native/media.c"));
        digest.finalize().into()
    });
    let caps = runtime.capabilities();
    let value = (
        "audio-pcm48k-v1",
        code,
        &caps.ffmpeg_version,
        &caps.library_directory,
        &caps.libraries,
        &asset.content_hash,
        &asset.kind,
        stream,
        &asset.streams,
    );
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}
