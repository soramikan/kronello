//! AUDIO-010 deterministic WSOLA pitch-preserving retiming (ADR-0124).
//!
//! Waveform-similarity overlap-add runs fully inside this process: an
//! integer-sample normalized cross-correlation search picks each segment's
//! input offset, then Hann overlap-add emits `HOP` output samples per hop.
//! The nominal input position follows the clip's rational `TimeMap`
//! (`PiecewiseTimeMap` / `Linear` speed), so duration and source alignment
//! stay exact while pitch is preserved. There is no randomness, no external
//! process and no platform DSP; the tie-break ordering and f64 accumulation
//! are fully specified so every render reproduces bit-identical samples.
//!
//! Speed 1.0 bypasses overlap-add entirely and reproduces the `resample_v1`
//! linear interpolation exactly. A hold segment (slope 0) emits silence per
//! the NLE-006 contract. Reads outside the source window [0, frame_count)
//! inside the correlation context are zero-filled; the nominal read position
//! itself beyond the source is `AUDIO_SOURCE_TOO_SHORT`.
use kronello_model::{AssetId, AudioRetimePolicy, Clip, SourceRef};
use kronello_time::{Rational, Time, TimeMap};

use crate::channels::MAX_CHANNELS;
use crate::{AudioError, ChannelSourceReader};

/// Analysis/synthesis window length: ~21 ms at 48 kHz.
pub(crate) const WSOLA_WINDOW: usize = 1024;
/// Synthesis hop: 512 samples (~11 ms); `WSOLA_WINDOW - WSOLA_HOP` overlap.
pub(crate) const WSOLA_HOP: usize = 512;
/// Correlation search half-width in input samples (±~5 ms at 48 kHz).
pub(crate) const WSOLA_SEARCH: usize = 256;
const OVERLAP: usize = WSOLA_WINDOW - WSOLA_HOP;

fn invalid(detail: &str) -> AudioError {
    AudioError::InvalidInput(detail.into())
}

/// Local source position (in samples) of the hop anchored at `sample`,
/// derived from the clip's rational time map — never accumulated state.
fn nominal_position(clip: &Clip, anchor: i64) -> Result<(Time, i64, Time), AudioError> {
    let local = super::advanced::local_time(clip, anchor)?;
    let position = local.checked_mul(Time::from_integer(48_000))?;
    if position < Time::ZERO {
        let SourceRef::Asset { asset, .. } = clip.source_ref else {
            unreachable!()
        };
        return Err(AudioError::SourceTooShort(asset));
    }
    let base = position.floor();
    let fraction = position.checked_sub(Time::from_integer(base))?;
    Ok((position, base, fraction))
}

/// Rational instantaneous source-time rate at absolute `sample`.
fn map_slope(clip: &Clip, sample: i64) -> Result<Rational, AudioError> {
    let parent = Time::new(sample, 48_000)?.checked_sub(clip.timeline_range.start())?;
    Ok(match &clip.time_map {
        TimeMap::Linear(map) => map.speed(),
        TimeMap::PiecewiseLinear(map) => map.slope_at(parent)?,
        _ => {
            return Err(AudioError::Unsupported(
                "pitch_preserve_v1 supports positive linear / piecewise-linear maps".into(),
            ));
        }
    })
}

/// Streaming WSOLA cursor for one asset entry inside one mix call. State is
/// recreated per evaluation; hops anchor at `placement_start` so a partitioned
/// render reproduces the exact continuous-render samples (ADR-0117/0124).
pub(crate) struct Wsola {
    channels: usize,
    asset: AssetId,
    stream: u32,
    window: Vec<f64>,
    /// Accumulated overlap from prior segments over the next `OVERLAP`
    /// positions of the upcoming hop.
    tail: Vec<[f64; MAX_CHANNELS]>,
    /// Scratch input window covering `[base - SEARCH, base + SEARCH + WINDOW)`.
    input: Vec<[f64; MAX_CHANNELS]>,
    /// Pending emitted samples from the current hop, interleaved.
    out: Vec<f32>,
    out_pos: usize,
    /// Absolute output sample index where the next hop starts.
    next_anchor: i64,
}
impl Wsola {
    pub(crate) fn new(clip: &Clip, asset: AssetId, stream: u32, channels: usize) -> Self {
        let window = (0..WSOLA_WINDOW)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / WSOLA_WINDOW as f64).cos())
            .collect();
        let placement_start = clip
            .timeline_range
            .start()
            .checked_mul(Time::from_integer(48_000))
            .map(|t| t.floor())
            .unwrap_or(0);
        Self {
            channels,
            asset,
            stream,
            window,
            tail: vec![[0.0; MAX_CHANNELS]; OVERLAP],
            input: vec![[0.0; MAX_CHANNELS]; 2 * WSOLA_SEARCH + WSOLA_WINDOW],
            out: Vec::new(),
            out_pos: 0,
            next_anchor: placement_start,
        }
    }
    /// Emit the mixed-layout frame for absolute `sample`. `out` must have
    /// exactly `channels` entries (the source layout — conversion happens in
    /// the caller's bus layout stage).
    pub(crate) fn frame(
        &mut self,
        clip: &Clip,
        sources: &dyn ChannelSourceReader,
        sample: i64,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        if out.len() != self.channels {
            return Err(AudioError::UnsupportedChannelLayout(
                "wsola frame channel count mismatch".into(),
            ));
        }
        // `out` holds WSOLA_HOP interleaved frames per hop; `out_pos` counts
        // frames, so the hop boundary is `out_pos == WSOLA_HOP` (an empty
        // buffer marks the first call).
        if self.out_pos == WSOLA_HOP || self.out.is_empty() {
            if sample != self.next_anchor {
                return Err(invalid("wsola frames must evaluate sequentially"));
            }
            self.produce_hop(clip, sources)?;
        }
        out.copy_from_slice(&self.out[self.out_pos * self.channels..][..self.channels]);
        self.out_pos += 1;
        Ok(())
    }
    fn read(&self, sources: &dyn ChannelSourceReader, index: i64, out: &mut [f64]) {
        if index < 0 {
            out.fill(0.0);
            return;
        }
        let Ok(index) = usize::try_from(index) else {
            out.fill(0.0);
            return;
        };
        let mut frame = [0.0_f32; MAX_CHANNELS];
        match sources.read_frame(self.asset, self.stream, index, &mut frame[..self.channels]) {
            Ok(()) => {
                for (channel, value) in frame[..self.channels].iter().enumerate() {
                    out[channel] = f64::from(*value);
                }
            }
            // Outside [0, frame_count) the correlation context is zero-filled;
            // the nominal position itself is bounds-checked by the caller.
            Err(_) => out.fill(0.0),
        }
    }
    fn produce_hop(
        &mut self,
        clip: &Clip,
        sources: &dyn ChannelSourceReader,
    ) -> Result<(), AudioError> {
        debug_assert_eq!(clip.audio_retime, AudioRetimePolicy::PitchPreserveV1);
        let anchor = self.next_anchor;
        self.next_anchor += WSOLA_HOP as i64;
        self.out.clear();
        self.out_pos = 0;
        self.out.resize(WSOLA_HOP * self.channels, 0.0);
        let slope = map_slope(clip, anchor)?;
        if slope <= Rational::ZERO {
            // NLE-006 hold: the source clock is pinned; emit silence.
            self.tail.fill([0.0; MAX_CHANNELS]);
            return Ok(());
        }
        let (_, base, fraction) = nominal_position(clip, anchor)?;
        if base < 0 {
            return Err(AudioError::SourceTooShort(self.asset));
        }
        let base = usize::try_from(base).map_err(|_| AudioError::SourceTooShort(self.asset))?;
        if slope == Rational::ONE {
            // Unity speed reproduces resample_v1 exactly; no windowing.
            self.tail.fill([0.0; MAX_CHANNELS]);
            let len = sources.frame_count(self.asset, self.stream)?;
            let fraction = number(&fraction);
            for i in 0..WSOLA_HOP {
                let index = base
                    .checked_add(i)
                    .ok_or(AudioError::SourceTooShort(self.asset))?;
                let mut a = [0.0_f32; MAX_CHANNELS];
                sources.read_frame(self.asset, self.stream, index, &mut a[..self.channels])?;
                if fraction == 0.0 || index + 1 >= len {
                    if fraction != 0.0 && index + 1 >= len {
                        return Err(AudioError::SourceTooShort(self.asset));
                    }
                    self.out[i * self.channels..(i + 1) * self.channels]
                        .copy_from_slice(&a[..self.channels]);
                    continue;
                }
                let mut b = [0.0_f32; MAX_CHANNELS];
                sources.read_frame(self.asset, self.stream, index + 1, &mut b[..self.channels])?;
                for (channel, out) in self.out[i * self.channels..(i + 1) * self.channels]
                    .iter_mut()
                    .enumerate()
                {
                    *out = (f64::from(a[channel]) * (1.0 - fraction)
                        + f64::from(b[channel]) * fraction) as f32;
                }
            }
            return Ok(());
        }
        // Windowed path: gather the search context, then pick the candidate
        // offset with the best normalized cross-correlation against the
        // pending overlap. Iteration order 0, -1, +1, -2, +2, ... and a
        // strict `>` comparison make the selection fully deterministic.
        for offset in 0..self.input.len() {
            let mut frame = [0.0_f64; MAX_CHANNELS];
            self.read(
                sources,
                base as i64 - WSOLA_SEARCH as i64 + offset as i64,
                &mut frame,
            );
            self.input[offset] = frame;
        }
        let mut energy_tail = 0.0_f64;
        for frame in &self.tail {
            for &value in frame.iter().take(self.channels) {
                energy_tail += value * value;
            }
        }
        let mut best_delta = 0_i64;
        let mut best_score = f64::NEG_INFINITY;
        for magnitude in 0..=WSOLA_SEARCH as i64 {
            for delta in if magnitude == 0 {
                [0, 0]
            } else {
                [-magnitude, magnitude]
            } {
                let mut dot = 0.0_f64;
                let mut energy_in = 0.0_f64;
                let start = (WSOLA_SEARCH as i64 + delta) as usize;
                for i in 0..OVERLAP {
                    for channel in 0..self.channels {
                        let value = self.input[start + i][channel];
                        dot += value * self.tail[i][channel];
                        energy_in += value * value;
                    }
                }
                let score = if energy_tail <= 0.0 || energy_in <= 0.0 {
                    0.0
                } else {
                    dot / (energy_tail * energy_in).sqrt()
                };
                if score > best_score {
                    best_score = score;
                    best_delta = delta;
                }
            }
        }
        let segment = (WSOLA_SEARCH as i64 + best_delta) as usize;
        for i in 0..WSOLA_HOP {
            for (channel, out) in self.out[i * self.channels..(i + 1) * self.channels]
                .iter_mut()
                .enumerate()
            {
                *out = (self.tail[i][channel] + self.window[i] * self.input[segment + i][channel])
                    as f32;
            }
        }
        // The next hop's pending overlap is this window's second half. With
        // WINDOW = 2·HOP no earlier segment reaches that far, so the tail is
        // replaced — not accumulated — here.
        for j in 0..OVERLAP {
            for channel in 0..self.channels {
                self.tail[j][channel] =
                    self.window[WSOLA_HOP + j] * self.input[segment + WSOLA_HOP + j][channel];
            }
        }
        Ok(())
    }
}
fn number(time: &Time) -> f64 {
    time.numerator() as f64 / time.denominator() as f64
}
