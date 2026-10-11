//! Producer-owned exact audio state. PCM bindings are weak, never extra retention.
use crate::{RenderTarget, ServiceError, playback::SharedChannelSources};
use kronello_audio::{AudioPlaybackCheckpoint, AudioPlaybackSession, DocumentAudioPlan};
use kronello_model::AssetId;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

/// Runtime retention only; never stored in a project or preparation request.
#[derive(Debug, Clone, Copy)]
pub struct AudioCheckpointCacheLimits {
    pub entries: usize,
    pub state_bytes: usize,
    pub interval_samples: i64,
}
impl Default for AudioCheckpointCacheLimits {
    fn default() -> Self {
        Self {
            entries: 128,
            state_bytes: 32 * 1024 * 1024,
            interval_samples: 2 * 48_000,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct AudioCheckpointCacheStats {
    pub retained_plans: usize,
    pub retained_checkpoints: usize,
    pub retained_state_bytes: usize,
    pub peak_retained_state_bytes: usize,
    pub saves: u64,
    pub restores: u64,
    pub misses: u64,
    pub evictions: u64,
    pub invalidations: u64,
    pub bypasses: u64,
    pub plan_compilations: u64,
    pub plan_hits: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AudioCheckpointKey {
    pub project: PathBuf,
    pub revision: String,
    pub target: RenderTarget,
}
struct Saved {
    checkpoint: AudioPlaybackCheckpoint,
    used: u64,
}
pub(crate) struct AudioCheckpointCache {
    limits: AudioCheckpointCacheLimits,
    key: Option<AudioCheckpointKey>,
    // One structurally bounded compiled plan; its immutable properties are
    // shared with sessions/checkpoints and separate from copied state payload.
    plan: Option<Arc<DocumentAudioPlan>>,
    sources: BTreeMap<(AssetId, u32), Weak<kronello_media::DecodedAudio>>,
    generation: Arc<()>,
    saved: BTreeMap<i64, Saved>,
    tick: u64,
    stats: AudioCheckpointCacheStats,
}
impl Default for AudioCheckpointCache {
    fn default() -> Self {
        Self::new(AudioCheckpointCacheLimits::default()).expect("valid default checkpoint limits")
    }
}
pub(crate) struct AudioCheckpointResume {
    pub session: AudioPlaybackSession,
    pub start_sample: i64,
    pub generation: Option<Arc<()>>,
}
impl AudioCheckpointCache {
    pub(crate) fn new(limits: AudioCheckpointCacheLimits) -> Result<Self, ServiceError> {
        if limits.entries > 4096
            || limits.state_bytes > 256 * 1024 * 1024
            || limits.interval_samples <= 0
        {
            return Err(ServiceError::new(
                "INVALID_AUDIO_INPUT",
                "audio checkpoint retention limits",
            ));
        }
        Ok(Self {
            limits,
            key: None,
            plan: None,
            sources: BTreeMap::new(),
            generation: Arc::new(()),
            saved: BTreeMap::new(),
            tick: 0,
            stats: AudioCheckpointCacheStats::default(),
        })
    }
    fn enabled(&self) -> bool {
        self.limits.entries > 0 && self.limits.state_bytes > 0
    }
    pub(crate) fn stats(&self) -> AudioCheckpointCacheStats {
        self.stats
    }
    pub(crate) fn interval_samples(&self) -> i64 {
        self.limits.interval_samples
    }
    pub(crate) fn plan(&mut self, key: &AudioCheckpointKey) -> Option<Arc<DocumentAudioPlan>> {
        if self.enabled() && self.key.as_ref() == Some(key) {
            self.stats.plan_hits += 1;
            self.plan.clone()
        } else {
            self.stats.plan_compilations += 1;
            None
        }
    }
    fn bindings_equal(&self, sources: &SharedChannelSources) -> bool {
        self.sources.len() == sources.0.len()
            && sources.0.iter().all(|(key, source)| {
                self.sources
                    .get(key)
                    .and_then(Weak::upgrade)
                    .is_some_and(|old| Arc::ptr_eq(&old, source))
            })
    }
    /// Called only after all current sources passed the normal hash/locator/
    /// stream/ABI fence. A retained plan alone never authorizes state restore.
    pub(crate) fn resume(
        &mut self,
        key: AudioCheckpointKey,
        plan: Arc<DocumentAudioPlan>,
        sources: &SharedChannelSources,
        limit: i64,
    ) -> AudioCheckpointResume {
        if !self.enabled() {
            self.stats.bypasses += 1;
            return AudioCheckpointResume {
                session: plan.playback_session(),
                start_sample: 0,
                generation: None,
            };
        }
        let same = self.key.as_ref() == Some(&key)
            && self
                .plan
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, &plan))
            && self.bindings_equal(sources);
        if !same {
            if self.key.is_some() {
                self.stats.invalidations += 1;
            }
            self.saved.clear();
            self.stats.retained_checkpoints = 0;
            self.stats.retained_state_bytes = 0;
            self.key = Some(key);
            self.plan = Some(plan.clone());
            self.stats.retained_plans = 1;
            self.sources = sources
                .0
                .iter()
                .map(|(key, source)| (*key, Arc::downgrade(source)))
                .collect();
            self.generation = Arc::new(());
        }
        self.tick = self.tick.saturating_add(1);
        let (session, start_sample) =
            if let Some((&sample, saved)) = self.saved.range_mut(..=limit).next_back() {
                saved.used = self.tick;
                self.stats.restores += 1;
                (saved.checkpoint.playback_session(), sample)
            } else {
                self.stats.misses += 1;
                (plan.playback_session(), 0)
            };
        AudioCheckpointResume {
            session,
            start_sample,
            generation: Some(self.generation.clone()),
        }
    }
    pub(crate) fn save(
        &mut self,
        generation: Option<&Arc<()>>,
        boundary: i64,
        session: &AudioPlaybackSession,
    ) {
        if !self.enabled()
            || !generation.is_some_and(|generation| Arc::ptr_eq(generation, &self.generation))
            || boundary < self.limits.interval_samples
            || self.saved.contains_key(&boundary)
        {
            return;
        }
        // Keep sparse actual block boundaries. Both neighbors matter when a
        // rewind fills an evicted region; max-entry/byte limits remain global.
        if self
            .saved
            .range(..boundary)
            .next_back()
            .is_some_and(|(&sample, _)| boundary - sample < self.limits.interval_samples)
            || self
                .saved
                .range(boundary..)
                .next()
                .is_some_and(|(&sample, _)| sample - boundary < self.limits.interval_samples)
        {
            return;
        }
        let Some(bytes) = session
            .checkpoint_payload_bytes(boundary)
            .filter(|bytes| *bytes <= self.limits.state_bytes)
        else {
            self.stats.bypasses += 1;
            return;
        };
        while self.saved.len() >= self.limits.entries
            || self.stats.retained_state_bytes > self.limits.state_bytes - bytes
        {
            let sample = *self
                .saved
                .iter()
                .min_by_key(|(_, saved)| saved.used)
                .expect("retained state eviction")
                .0;
            let old = self.saved.remove(&sample).expect("retained state");
            self.stats.retained_state_bytes -= old.checkpoint.payload_bytes();
            self.stats.evictions += 1;
        }
        let checkpoint = session
            .checkpoint(boundary, self.limits.state_bytes)
            .expect("unchanged preflighted checkpoint state");
        self.tick = self.tick.saturating_add(1);
        self.saved.insert(
            boundary,
            Saved {
                checkpoint,
                used: self.tick,
            },
        );
        self.stats.saves += 1;
        self.stats.retained_checkpoints = self.saved.len();
        self.stats.retained_state_bytes += bytes;
        self.stats.peak_retained_state_bytes = self
            .stats
            .peak_retained_state_bytes
            .max(self.stats.retained_state_bytes);
    }
}
