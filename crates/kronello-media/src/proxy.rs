//! Deterministic single-pass ProRes mezzanine transcode for preview proxies
//! (ADR-0119). Decode is the shared `next_rgba` SDR path, scale is the
//! fixed-point `scale_rgba8`, and encode is the fixed ProRes encoder profile.
//! The staged destination write never clobbers an existing file.
use crate::*;
use kronello_time::Rational;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Verified outcome of `MediaRuntime::encode_proxy`: the published container
/// probe plus the byte identity the proxy link registers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyEncodeResult {
    pub width: u32,
    pub height: u32,
    /// Encoded presentation-order frame count.
    pub frames: u64,
    /// Source stream time base carried into the proxy container.
    pub time_base: Rational,
    /// Probe duration of the written video stream.
    pub duration: Option<Rational>,
    /// SHA-256 of the published file.
    pub content_hash: String,
    /// Probe of the written container (video stream index 0).
    pub probe: MediaProbe,
}

impl MediaRuntime {
    /// Transcode `source` stream `stream_index` into a ProRes proxy at
    /// `width`x`height` (from `kronello_model::proxy_dimensions`). PTS ticks
    /// are re-based at zero in the source stream time base; non-increasing or
    /// non-integral source timestamps fail instead of being conformed.
    /// `progress` runs after each encoded frame for checkpoint/cancel hooks.
    pub fn encode_proxy(
        &self,
        source: &Path,
        stream_index: u32,
        width: u32,
        height: u32,
        output: &Path,
        progress: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<ProxyEncodeResult, MediaError> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(MediaError::InvalidInput(
                "proxy dimensions must be positive and even".into(),
            ));
        }
        let codec = self.capabilities.select_encoder(EncodeCodec::ProRes)?;
        let mut decoder = self.open_video_stream(source, stream_index)?;
        let time_base = decoder.native.time_base;
        if time_base <= Rational::ZERO {
            return Err(MediaError::InvalidInput(
                "non-positive source time base".into(),
            ));
        }
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if output.exists() {
            return Err(MediaError::OutputExists(output.into()));
        }
        let stage = tempfile::NamedTempFile::new_in(parent)?;
        let mut encoder = ffi::NativeEncoder::open_color(
            &self.native,
            stage.path(),
            codec,
            i32::try_from(width)
                .map_err(|_| MediaError::InvalidInput("proxy width overflow".into()))?,
            i32::try_from(height)
                .map_err(|_| MediaError::InvalidInput("proxy height overflow".into()))?,
            time_base,
            None,
        )?;
        let mut first: Option<Rational> = None;
        let mut previous: Option<i64> = None;
        let mut frames = 0u64;
        let result = (|| -> Result<(), MediaError> {
            while let Some(frame) = decoder.next_rgba()? {
                let origin = *first.get_or_insert(frame.pts);
                let tick = frame.pts.checked_sub(origin)?.checked_div(time_base)?;
                if tick.denominator() != 1
                    || tick.numerator() < 0
                    || previous.is_some_and(|p| tick.numerator() <= p)
                {
                    return Err(MediaError::InvalidInput(
                        "proxy source must yield strictly increasing integral ticks".into(),
                    ));
                }
                // Packet spans come from the demuxed frame duration; sources
                // that publish no span on a frame (common on the tail) inherit
                // the spacing established by the preceding frame.
                let span = frame.duration.checked_div(time_base)?;
                let duration = if span.denominator() == 1 && span.numerator() > 0 {
                    span.numerator()
                } else {
                    match previous {
                        Some(p) => tick.numerator() - p,
                        None => {
                            return Err(MediaError::InvalidInput(
                                "proxy source must yield positive integral frame durations".into(),
                            ));
                        }
                    }
                };
                previous = Some(tick.numerator());
                let rgba = scale_rgba8(&frame.rgba, frame.width, frame.height, width, height)?;
                encoder.frame(&rgba, tick.numerator(), duration)?;
                frames = frames
                    .checked_add(1)
                    .ok_or_else(|| MediaError::InvalidInput("proxy frame overflow".into()))?;
                progress(frames)?;
            }
            if frames == 0 {
                return Err(MediaError::Decode("empty video stream".into()));
            }
            encoder.finish()
        })();
        drop(encoder);
        result?;
        stage.as_file().sync_all()?;
        stage.persist_noclobber(output).map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                MediaError::OutputExists(output.into())
            } else {
                MediaError::Io(e.error)
            }
        })?;
        let probe = self.probe(output)?;
        let video = probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Video)
            .ok_or_else(|| MediaError::Encode("proxy container has no video stream".into()))?;
        if video.codec != "prores"
            || video.width != Some(width)
            || video.height != Some(height)
            || video.index != 0
            || video.start != Some(Rational::ZERO)
        {
            return Err(MediaError::Encode(format!(
                "proxy verification failed: codec={} {:?}x{:?} index={} start={:?}",
                video.codec, video.width, video.height, video.index, video.start
            )));
        }
        Ok(ProxyEncodeResult {
            width,
            height,
            frames,
            time_base,
            duration: video.duration.or(probe.duration),
            content_hash: content_hash(output)?,
            probe,
        })
    }
}
