//! Bounded immutable converted sources, after presentation/lock validation.
use kronello_render::VideoSource;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFrameCacheLimits {
    pub entries: usize,
    pub bytes: usize,
}
impl Default for SourceFrameCacheLimits {
    fn default() -> Self {
        Self {
            entries: 8,
            bytes: 256 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SourceFrameCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    /// Converted pixel/sampler/key payload, excluding allocator/driver overhead.
    pub retained_bytes: usize,
    pub peak_retained_bytes: usize,
}
#[derive(PartialEq, Eq)]
pub(crate) struct SourceCacheKey {
    pub path: PathBuf,
    pub fingerprint: crate::assets::FileFingerprint,
    pub identity: String,
}
#[derive(Default)]
pub(crate) struct SourceFrameCache {
    pub limits: SourceFrameCacheLimits,
    entries: VecDeque<(SourceCacheKey, VideoSource, usize)>,
    pub stats: SourceFrameCacheStats,
}
impl SourceFrameCache {
    pub fn configure(&mut self, limits: SourceFrameCacheLimits) {
        self.limits = limits;
        while !self.entries.is_empty()
            && (self.entries.len() > limits.entries || self.stats.retained_bytes > limits.bytes)
        {
            self.evict();
        }
    }
    fn evict(&mut self) {
        let (_, _, bytes) = self.entries.pop_front().expect("retained entry");
        self.stats.retained_bytes -= bytes;
        self.stats.evictions += 1;
        self.stats.entries = self.entries.len();
        kronello_render::count_execution("source_frame_cache_evictions", 1);
    }
    pub fn get(&mut self, key: &SourceCacheKey) -> Option<VideoSource> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _, _)| candidate == key)
        {
            let entry = self.entries.remove(index).expect("located source");
            let source = entry.1.clone();
            self.entries.push_back(entry);
            self.stats.hits += 1;
            kronello_render::count_execution("source_frame_cache_hits", 1);
            Some(source)
        } else {
            self.stats.misses += 1;
            kronello_render::count_execution("source_frame_cache_misses", 1);
            None
        }
    }
    pub fn insert(&mut self, key: SourceCacheKey, source: VideoSource, payload_bytes: usize) {
        let bytes = payload_bytes
            .saturating_add(key.identity.len())
            .saturating_add(key.path.as_os_str().len());
        if self.limits.entries == 0 || bytes > self.limits.bytes {
            return;
        }
        while self.entries.len() >= self.limits.entries
            || self.stats.retained_bytes > self.limits.bytes - bytes
        {
            self.evict();
        }
        self.entries.push_back((key, source, bytes));
        self.stats.entries = self.entries.len();
        self.stats.retained_bytes += bytes;
        self.stats.peak_retained_bytes = self
            .stats
            .peak_retained_bytes
            .max(self.stats.retained_bytes);
        kronello_render::observe_execution_payload(
            "converted_source_cache",
            self.stats.retained_bytes as u64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converted_source_lru_enforces_bytes_and_counts_without_invalidating_readers() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let fingerprint = crate::assets::FileFingerprint::of(&temp.as_file().metadata().unwrap());
        let key = |identity: &str| SourceCacheKey {
            path: path.clone(),
            fingerprint,
            identity: identity.into(),
        };
        let source = |value| VideoSource::new([1; 2], move |_, _| [value; 4]);
        let mut cache = SourceFrameCache::default();
        cache.configure(SourceFrameCacheLimits {
            entries: 2,
            bytes: 1024,
        });
        cache.insert(key("a"), source(0.25), 128);
        cache.insert(key("b"), source(0.5), 128);
        let reader = cache.get(&key("a")).unwrap();
        cache.insert(key("c"), source(0.75), 128);
        assert!(cache.get(&key("b")).is_none());
        assert_eq!(cache.get(&key("a")).unwrap().sample(0, 0), [0.25; 4]);
        cache.insert(key("large"), source(1.0), 700);
        assert!(cache.stats.retained_bytes <= 1024);
        assert!(cache.stats.entries <= 2);
        cache.configure(SourceFrameCacheLimits {
            entries: 1,
            bytes: 1,
        });
        assert_eq!((cache.stats.entries, cache.stats.retained_bytes), (0, 0));
        cache.insert(key("oversized"), source(1.0), 700);
        assert!(cache.get(&key("oversized")).is_none());
        assert_eq!(reader.sample(0, 0), [0.25; 4]);
    }
}
