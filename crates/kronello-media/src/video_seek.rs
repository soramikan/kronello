//! Bounded references to exact presentations observed by canonical decoding.
use crate::MediaError;
use kronello_render::DecodedVideoFrame;
use kronello_time::Time;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};

/// Runtime-only policy, never stored in an authored project or render snapshot.
#[derive(Debug, Clone, Copy)]
pub struct VideoSeekIndexLimits {
    pub max_entries: usize,
    /// Certify only requests that actually decode at least this many frames.
    /// Sequential playback avoids a full native-plane digest per frame.
    pub min_decode_frames: u64,
}
impl Default for VideoSeekIndexLimits {
    fn default() -> Self {
        Self {
            max_entries: if cfg!(unix) { 4096 } else { 0 },
            min_decode_frames: 128,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct VideoSeekIndexStats {
    pub entries: usize,
    /// Logical descriptors and FIFO timestamps; excludes allocator overhead,
    /// capacities, decoded frames and native codec memory.
    pub metadata_payload_bytes: usize,
    pub evictions: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct Reference {
    pub end: Time,
    pub seek_pts: Time,
    signature: [u8; 32],
    seekable: bool,
}
pub(crate) struct VideoSeekIndex {
    limits: VideoSeekIndexLimits,
    entries: BTreeMap<Time, Reference>,
    fifo: VecDeque<Time>,
    evictions: u64,
}
impl Default for VideoSeekIndex {
    fn default() -> Self {
        Self::new(VideoSeekIndexLimits::default()).expect("valid default seek limits")
    }
}
impl VideoSeekIndex {
    pub(crate) fn new(limits: VideoSeekIndexLimits) -> Result<Self, MediaError> {
        if limits.max_entries > 65_536 {
            return Err(MediaError::InvalidInput("seek index entry limit".into()));
        }
        if !cfg!(unix) && limits.max_entries > 0 {
            return Err(MediaError::UnsupportedFeature(
                "verified seek index requires the Unix source identity fence".into(),
            ));
        }
        Ok(Self {
            limits,
            entries: BTreeMap::new(),
            fifo: VecDeque::new(),
            evictions: 0,
        })
    }
    pub(crate) fn enabled(&self) -> bool {
        self.limits.max_entries > 0
    }
    pub(crate) fn stats(&self) -> VideoSeekIndexStats {
        VideoSeekIndexStats {
            entries: self.entries.len(),
            metadata_payload_bytes: self.entries.len()
                * (std::mem::size_of::<Reference>() + 2 * std::mem::size_of::<Time>()),
            evictions: self.evictions,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.fifo.clear();
    }
    pub(crate) fn find(&self, time: Time, reverse: bool) -> Option<(Time, Reference)> {
        let entry = if reverse {
            self.entries.range(..time).next_back()
        } else {
            self.entries.range(..=time).next_back()
        }?;
        let (&pts, &reference) = entry;
        let within = if reverse {
            time <= reference.end
        } else {
            time < reference.end
        };
        (within && reference.seekable).then_some((pts, reference))
    }
    pub(crate) fn insert(&mut self, frame: &DecodedVideoFrame, decoded_frames: u64) {
        if !self.enabled() || decoded_frames < self.limits.min_decode_frames {
            return;
        }
        // Failed native restart points remain disabled even after an exact
        // origin fallback certifies the same presentation again.
        if self.entries.contains_key(&frame.pts) {
            return;
        }
        if self.entries.len() == self.limits.max_entries {
            let oldest = self.fifo.pop_front().expect("indexed FIFO entry");
            self.entries.remove(&oldest);
            self.evictions += 1;
        }
        self.entries.insert(
            frame.pts,
            Reference {
                end: frame.end,
                seek_pts: frame.pts,
                signature: signature(frame),
                seekable: true,
            },
        );
        self.fifo.push_back(frame.pts);
    }
    pub(crate) fn reject(&mut self, pts: Time) {
        if let Some(reference) = self.entries.get_mut(&pts) {
            reference.seekable = false;
        }
    }
    pub(crate) fn verified_anchor(&mut self, pts: Time, seek_pts: Time) {
        if let Some(reference) = self.entries.get_mut(&pts) {
            reference.seek_pts = seek_pts;
        }
    }
}
impl Reference {
    pub(crate) fn matches(self, pts: Time, frame: &DecodedVideoFrame) -> bool {
        frame.pts == pts && frame.end == self.end && signature(frame) == self.signature
    }
}
fn signature(frame: &DecodedVideoFrame) -> [u8; 32] {
    let mut hash = Sha256::new();
    for time in [frame.pts, frame.end] {
        hash.update(time.numerator().to_le_bytes());
        hash.update(time.denominator().to_le_bytes());
    }
    hash.update(frame.width.to_le_bytes());
    hash.update(frame.height.to_le_bytes());
    for label in [
        &frame.pixel_format,
        &frame.color_primaries,
        &frame.color_transfer,
        &frame.color_matrix,
        &frame.color_range,
    ] {
        hash.update((label.len() as u64).to_le_bytes());
        hash.update(label.as_bytes());
    }
    hash.update((frame.pixels.len() as u64).to_le_bytes());
    hash.update(&frame.pixels);
    hash.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaRuntime;
    fn path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/generated/media")
            .join(name)
    }
    #[test]
    fn cheap_forward_work_is_not_certified() {
        let runtime = MediaRuntime::load().unwrap();
        let mut decoder = runtime.open_video(&path("cfr-30000-1001.nut")).unwrap();
        let frame = decoder.decode_at(Time::ZERO).unwrap();
        assert_eq!(decoder.seek_index_stats().entries, 0);
        let mut index = VideoSeekIndex::default();
        index.insert(&frame, 127);
        assert_eq!(index.stats().entries, 0);
        index.insert(&frame, 128);
        assert_eq!(index.stats().entries, usize::from(cfg!(unix)));
    }
    #[cfg(unix)]
    #[test]
    fn certified_seek_preserves_native_intervals_reverse_edges_and_eviction() {
        let runtime = MediaRuntime::load().unwrap();
        for (name, times) in [
            (
                "cfr-30000-1001.nut",
                (0..6)
                    .map(|i| Time::new(i * 1001, 30000).unwrap())
                    .collect::<Vec<_>>(),
            ),
            (
                "vfr.nut",
                [0, 1, 3, 6, 10, 15]
                    .into_iter()
                    .map(|i| Time::new(i, 30).unwrap())
                    .collect(),
            ),
            (
                "bframes.nut",
                (1..7).map(|i| Time::new(i, 24).unwrap()).collect(),
            ),
        ] {
            let source = path(name);
            let mut decoder = runtime
                .open_video(&source)
                .unwrap()
                .with_seek_index_limits(VideoSeekIndexLimits {
                    max_entries: 2,
                    min_decode_frames: 0,
                })
                .unwrap();
            for time in &times {
                assert_eq!(
                    decoder.decode_at(*time).unwrap(),
                    runtime
                        .open_video(&source)
                        .unwrap()
                        .decode_at(*time)
                        .unwrap()
                );
            }
            assert_eq!(decoder.seek_index_stats().entries, 2);
            assert_eq!(decoder.seek_index_stats().evictions, 4);
            let time = times[4];
            let expected = runtime
                .open_video(&source)
                .unwrap()
                .decode_at(time)
                .unwrap();
            assert_eq!(decoder.decode_at(time).unwrap(), expected);
            assert_eq!(decoder.decode_at_reverse(expected.end).unwrap(), expected);
            assert_eq!(
                decoder.decode_at_reverse(time).unwrap(),
                runtime
                    .open_video(&source)
                    .unwrap()
                    .decode_at_reverse(time)
                    .unwrap()
            );
            assert!(decoder.decode_stats().indexed_seek_attempts > 0);
            assert!(decoder.seek_index_stats().entries <= 2);
            assert_eq!(
                decoder
                    .decode_at(Time::from_integer(-1))
                    .unwrap_err()
                    .code(),
                "FRAME_NOT_FOUND"
            );
            assert_eq!(
                decoder.decode_at(times[3]).unwrap(),
                runtime
                    .open_video(&source)
                    .unwrap()
                    .decode_at(times[3])
                    .unwrap()
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn failed_certificate_replays_origin_and_disables_repeated_native_restart() {
        let runtime = MediaRuntime::load().unwrap();
        let source = path("cfr-30000-1001.nut");
        let mut decoder = runtime
            .open_video(&source)
            .unwrap()
            .with_seek_index_limits(VideoSeekIndexLimits {
                max_entries: 2,
                min_decode_frames: 0,
            })
            .unwrap();
        let expected = decoder.decode_at(Time::ZERO).unwrap();
        decoder.decode_at(Time::new(1001, 30000).unwrap()).unwrap();
        decoder
            .seek_index
            .entries
            .get_mut(&Time::ZERO)
            .unwrap()
            .signature[0] ^= 1;
        assert_eq!(decoder.decode_at(Time::ZERO).unwrap(), expected);
        assert!(
            decoder.canonical,
            "fallback must actually replay the origin, not return its cached candidate"
        );
        assert_eq!(decoder.decode_stats().indexed_seek_fallbacks, 1);
        assert_eq!(decoder.decode_stats().verified_seek_hits, 0);
        decoder.next_rgba().unwrap();
        assert_eq!(decoder.decode_at(Time::ZERO).unwrap(), expected);
        assert_eq!(decoder.decode_stats().indexed_seek_attempts, 1);
        assert!(
            runtime
                .open_video(&source)
                .unwrap()
                .with_seek_index_limits(VideoSeekIndexLimits {
                    max_entries: 65_537,
                    min_decode_frames: 0,
                })
                .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn source_identity_change_fences_presentation_and_reference_cache_then_recovers() {
        let runtime = MediaRuntime::load().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("video.nut");
        std::fs::copy(path("cfr-30000-1001.nut"), &source).unwrap();
        let mut decoder = runtime
            .open_video(&source)
            .unwrap()
            .with_seek_index_limits(VideoSeekIndexLimits {
                max_entries: 2,
                min_decode_frames: 0,
            })
            .unwrap();
        let expected = decoder.decode_at(Time::ZERO).unwrap();
        let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
        std::fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            decoder.decode_at(Time::ZERO).unwrap_err().code(),
            "DECODE_ERROR"
        );
        assert_eq!(decoder.seek_index_stats().entries, 0);
        assert_eq!(decoder.decode_at(Time::ZERO).unwrap(), expected);
        assert_eq!(decoder.seek_index_stats().entries, 1);
    }
}
