//! AUDIO-007/008/010 deterministic channel-masked processors (ADR-0117/0124).
//!
//! Coefficients are computed in f64 from constant resolved parameters and
//! every processor starts from zeroed state at the clip's placement
//! boundary. The audio evaluator feeds processors from placement start, so
//! any request range reproduces the exact samples of a continuous render;
//! realtime playback and export share this single implementation.
//! Every stage applies per channel over the bus layout; only the
//! compressor/limiter detectors and gain application exclude the LFE channel,
//! which passes through unchanged (ADR-0124).
use kronello_model::{AudioEqBand, AudioEqBandKind, ChannelMask, ResolvedAudioEffect};

const SAMPLE_RATE: f64 = 48_000.0;

/// Transposed direct-form-II multichannel biquad with f64 coefficients and
/// per-channel state.
#[derive(Debug, Clone)]
pub(crate) struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    /// `z[stage][channel]` filter state, one [z1, z2] per channel.
    z: Vec<[f64; 2]>,
}
impl Biquad {
    fn with_channels(mut self, channels: usize) -> Self {
        self.z = vec![[0.0; 2]; channels];
        self
    }
    pub(crate) fn normalized(b: [f64; 3], a: [f64; 3], channels: usize) -> Self {
        Self {
            b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]],
            a: [a[1] / a[0], a[2] / a[0]],
            z: vec![],
        }
        .with_channels(channels)
    }
    /// In-place per-channel filtering. `frame` length equals `z.len()`.
    pub(crate) fn process(&mut self, frame: &mut [f32]) {
        for (channel, value) in frame.iter_mut().enumerate() {
            let x = f64::from(*value);
            let y = self.b[0] * x + self.z[channel][0];
            self.z[channel][0] = self.b[1] * x - self.a[0] * y + self.z[channel][1];
            self.z[channel][1] = self.b[2] * x - self.a[1] * y;
            *value = y as f32;
        }
    }
}
fn w0(freq_hz: f64) -> f64 {
    std::f64::consts::TAU * freq_hz / SAMPLE_RATE
}
fn alpha(w0: f64, q: f64) -> f64 {
    w0.sin() / (2.0 * q)
}
/// RBJ audio EQ cookbook peaking section.
fn peaking(freq_hz: f64, gain_db: f64, q: f64, channels: usize) -> Biquad {
    let (w0, alpha) = (w0(freq_hz), alpha(w0(freq_hz), q));
    let amplitude = 10_f64.powf(gain_db / 40.0);
    let cos = w0.cos();
    Biquad::normalized(
        [1.0 + alpha * amplitude, -2.0 * cos, 1.0 - alpha * amplitude],
        [1.0 + alpha / amplitude, -2.0 * cos, 1.0 - alpha / amplitude],
        channels,
    )
}
/// RBJ shelf section with the Q-form alpha (fixed slope from `q`).
fn shelf(freq_hz: f64, gain_db: f64, q: f64, high: bool, channels: usize) -> Biquad {
    let w0 = w0(freq_hz);
    let alpha = alpha(w0, q);
    let amplitude = 10_f64.powf(gain_db / 40.0);
    let cos = w0.cos();
    let beta = 2.0 * amplitude.sqrt() * alpha;
    let (ap, am) = (amplitude + 1.0, amplitude - 1.0);
    if high {
        Biquad::normalized(
            [
                amplitude * (ap + am * cos + beta),
                -2.0 * amplitude * (am + ap * cos),
                amplitude * (ap + am * cos - beta),
            ],
            [
                ap - am * cos + beta,
                2.0 * (am - ap * cos),
                ap - am * cos - beta,
            ],
            channels,
        )
    } else {
        Biquad::normalized(
            [
                amplitude * (ap - am * cos + beta),
                2.0 * amplitude * (am - ap * cos),
                amplitude * (ap + am * cos - beta),
            ],
            [
                ap + am * cos + beta,
                -2.0 * amplitude * (am + ap * cos),
                ap + am * cos - beta,
            ],
            channels,
        )
    }
}
/// Second-order Butterworth section (RBJ high-/low-pass with explicit Q).
fn butterworth2(freq_hz: f64, q: f64, high: bool, channels: usize) -> Biquad {
    let w0 = w0(freq_hz);
    let alpha = alpha(w0, q);
    let cos = w0.cos();
    if high {
        Biquad::normalized(
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
            channels,
        )
    } else {
        Biquad::normalized(
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
            channels,
        )
    }
}
/// First-order Butterworth section via the bilinear tangent mapping,
/// expressed as a degenerate biquad.
fn butterworth1(freq_hz: f64, high: bool, channels: usize) -> Biquad {
    let k = (std::f64::consts::PI * freq_hz / SAMPLE_RATE).tan();
    let a1 = (k - 1.0) / (k + 1.0);
    let b = if high {
        [1.0 / (1.0 + k), -1.0 / (1.0 + k), 0.0]
    } else {
        [k / (1.0 + k), k / (1.0 + k), 0.0]
    };
    Biquad {
        b,
        a: [a1, 0.0],
        z: vec![[0.0; 2]; channels],
    }
}
/// Butterworth pole-pair Q values: order 2 -> [0.7071], order 3 -> [1.0] plus
/// the real pole, order 4 -> [1.3065, 0.5412]. Odd orders append one
/// first-order section.
fn butterworth_cascade(freq_hz: f64, order: u32, high: bool, channels: usize) -> Vec<Biquad> {
    let n = order as usize;
    let mut stages: Vec<Biquad> = (0..n / 2)
        .map(|k| {
            let theta = std::f64::consts::PI * (2 * k + n + 1) as f64 / (2.0 * n as f64);
            let q = 1.0 / (2.0 * theta.cos().abs());
            butterworth2(freq_hz, q, high, channels)
        })
        .collect();
    if order % 2 == 1 {
        stages.push(butterworth1(freq_hz, high, channels));
    }
    stages
}
fn eq_stage(band: AudioEqBand, channels: usize) -> Biquad {
    match band.kind {
        AudioEqBandKind::Peak => peaking(band.freq_hz, band.gain_db, band.q, channels),
        AudioEqBandKind::LowShelf => shelf(band.freq_hz, band.gain_db, band.q, false, channels),
        AudioEqBandKind::HighShelf => shelf(band.freq_hz, band.gain_db, band.q, true, channels),
    }
}
/// Index of the LFE channel inside a frame, if the bus layout carries one.
/// Compressor/limiter detection and gain application skip only this channel
/// (ADR-0124); filters keep processing it like every other channel.
fn lfe_index(mask: ChannelMask) -> Option<usize> {
    mask.channel_bits()
        .iter()
        .position(|bit| *bit == ChannelMask::LOW_FREQUENCY)
}
/// AUDIO-008 channel-linked feed-forward peak compressor. The envelope is a
/// one-pole peak detector on the maximum non-LFE channel magnitude; static
/// gain is computed in dB and applied identically to every non-LFE channel.
/// The LFE channel passes through unprocessed.
#[derive(Debug, Clone)]
pub(crate) struct Compressor {
    threshold_db: f64,
    slope: f64,
    makeup: f64,
    attack: f64,
    release: f64,
    envelope: f64,
    lfe: Option<usize>,
}
impl Compressor {
    fn new(
        threshold_db: f64,
        ratio: f64,
        attack_ms: f64,
        release_ms: f64,
        makeup_db: f64,
        lfe: Option<usize>,
    ) -> Self {
        Self {
            threshold_db,
            slope: 1.0 / ratio - 1.0,
            makeup: 10_f64.powf(makeup_db / 20.0),
            attack: time_constant(attack_ms),
            release: time_constant(release_ms),
            envelope: 0.0,
            lfe,
        }
    }
    fn process(&mut self, frame: &mut [f32]) {
        let detector = frame
            .iter()
            .enumerate()
            .filter(|(channel, _)| Some(*channel) != self.lfe)
            .fold(0.0_f64, |m, (_, v)| m.max(f64::from(v.abs())));
        let coefficient = if detector > self.envelope {
            self.attack
        } else {
            self.release
        };
        self.envelope = coefficient * self.envelope + (1.0 - coefficient) * detector;
        let envelope_db = 20.0 * self.envelope.max(1e-12).log10();
        let over = envelope_db - self.threshold_db;
        let reduction_db = if over > 0.0 { over * self.slope } else { 0.0 };
        let gain = 10_f64.powf(reduction_db / 20.0) * self.makeup;
        for (channel, value) in frame.iter_mut().enumerate() {
            if Some(channel) == self.lfe {
                continue;
            }
            *value = (f64::from(*value) * gain) as f32;
        }
    }
}
/// AUDIO-008 ceiling limiter: instant-attack peak holder with an exponential
/// release, applying the exact gain that keeps the non-LFE envelope at the
/// ceiling. The LFE channel passes through unprocessed.
#[derive(Debug, Clone)]
pub(crate) struct Limiter {
    ceiling: f64,
    release: f64,
    envelope: f64,
    lfe: Option<usize>,
}
impl Limiter {
    fn new(ceiling_db: f64, release_ms: f64, lfe: Option<usize>) -> Self {
        Self {
            ceiling: 10_f64.powf(ceiling_db / 20.0),
            release: time_constant(release_ms),
            envelope: 0.0,
            lfe,
        }
    }
    fn process(&mut self, frame: &mut [f32]) {
        let detector = frame
            .iter()
            .enumerate()
            .filter(|(channel, _)| Some(*channel) != self.lfe)
            .fold(0.0_f64, |m, (_, v)| m.max(f64::from(v.abs())));
        self.envelope = detector.max(self.envelope * self.release);
        let gain = if self.envelope > self.ceiling {
            self.ceiling / self.envelope
        } else {
            1.0
        };
        for (channel, value) in frame.iter_mut().enumerate() {
            if Some(channel) == self.lfe {
                continue;
            }
            *value = (f64::from(*value) * gain) as f32;
        }
    }
}
/// One-pole coefficient for a `ms`-millisecond time constant at 48 kHz.
fn time_constant(ms: f64) -> f64 {
    (-1.0 / (ms * (SAMPLE_RATE / 1_000.0))).exp()
}
/// A stateful per-sample processing stage instantiated for one mix request.
/// `process` is in-place over a bus-layout frame (any supported channel
/// count).
#[derive(Debug, Clone)]
pub(crate) enum Processor {
    /// Ordered biquad cascade (EQ bands, HPF/LPF sections).
    Filters(Vec<Biquad>),
    Compressor(Box<Compressor>),
    Limiter(Limiter),
}
impl Processor {
    pub(crate) fn process(&mut self, frame: &mut [f32]) {
        match self {
            Self::Filters(stages) => {
                for stage in stages.iter_mut() {
                    stage.process(frame);
                }
            }
            Self::Compressor(compressor) => compressor.process(frame),
            Self::Limiter(limiter) => limiter.process(frame),
        }
    }
}
/// Instantiate the deterministic chain for one resolved effect over the bus
/// layout. Fresh state on every call keeps a render request independent of
/// evaluation history.
pub(crate) fn processor(spec: &ResolvedAudioEffect, mask: ChannelMask) -> Processor {
    let channels = mask.channels();
    match spec {
        ResolvedAudioEffect::Eq { bands } => {
            Processor::Filters(bands.iter().map(|b| eq_stage(*b, channels)).collect())
        }
        ResolvedAudioEffect::Hpf { cutoff_hz, order } => {
            Processor::Filters(butterworth_cascade(*cutoff_hz, *order, true, channels))
        }
        ResolvedAudioEffect::Lpf { cutoff_hz, order } => {
            Processor::Filters(butterworth_cascade(*cutoff_hz, *order, false, channels))
        }
        ResolvedAudioEffect::Compressor {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            makeup_db,
        } => Processor::Compressor(Box::new(Compressor::new(
            *threshold_db,
            *ratio,
            *attack_ms,
            *release_ms,
            *makeup_db,
            lfe_index(mask),
        ))),
        ResolvedAudioEffect::Limiter {
            ceiling_db,
            release_ms,
        } => Processor::Limiter(Limiter::new(*ceiling_db, *release_ms, lfe_index(mask))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, samples: usize) -> Vec<[f32; 2]> {
        (0..samples)
            .map(|i| {
                let v = (std::f64::consts::TAU * freq * i as f64 / SAMPLE_RATE).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }
    fn run(p: &mut Processor, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        input
            .iter()
            .map(|f| {
                let mut frame = *f;
                p.process(&mut frame);
                frame
            })
            .collect()
    }
    fn rms(frames: &[[f32; 2]], skip: usize) -> f64 {
        let tail = &frames[skip..];
        (tail
            .iter()
            .map(|f| f64::from(f[0]) * f64::from(f[0]))
            .sum::<f64>()
            / tail.len() as f64)
            .sqrt()
    }
    #[test]
    fn peaking_eq_boosts_band_and_leaves_far_signal() {
        let spec = ResolvedAudioEffect::Eq {
            bands: vec![AudioEqBand {
                kind: AudioEqBandKind::Peak,
                freq_hz: 1_000.0,
                gain_db: 12.0,
                q: 1.0,
            }],
        };
        let input = sine(1_000.0, 48_000);
        let mut p = processor(&spec, ChannelMask::STEREO);
        let out = run(&mut p, &input);
        let gain = rms(&out, 24_000) / rms(&input, 24_000);
        assert!((20.0 * gain.log10() - 12.0).abs() < 0.5, "in-band {gain}");
        let input = sine(100.0, 48_000);
        let mut p = processor(&spec, ChannelMask::STEREO);
        let out = run(&mut p, &input);
        let gain = rms(&out, 24_000) / rms(&input, 24_000);
        assert!(gain < 1.2, "out-of-band {gain}");
    }
    #[test]
    fn hpf_attenuates_below_cutoff() {
        let spec = ResolvedAudioEffect::Hpf {
            cutoff_hz: 1_000.0,
            order: 4,
        };
        let input = sine(100.0, 48_000);
        let mut p = processor(&spec, ChannelMask::STEREO);
        let out = run(&mut p, &input);
        assert!(rms(&out, 24_000) < rms(&input, 24_000) * 0.01);
    }
    #[test]
    fn first_and_third_order_build() {
        for order in [1_u32, 3] {
            let spec = ResolvedAudioEffect::Lpf {
                cutoff_hz: 2_000.0,
                order,
            };
            let mut p = processor(&spec, ChannelMask::STEREO);
            let mut frame = [1.0, 1.0];
            p.process(&mut frame);
            assert!(frame.iter().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn compressor_reduces_hot_sine() {
        let spec = ResolvedAudioEffect::Compressor {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            makeup_db: 0.0,
        };
        let input = sine(440.0, 96_000);
        let mut p = processor(&spec, ChannelMask::STEREO);
        let out = run(&mut p, &input);
        let db = 20.0 * (rms(&out, 48_000) / rms(&input, 48_000)).log10();
        // 0.5-peak sine ≈ -6 dB peak envelope → ~14 dB over the -20 dB
        // threshold → about -10 dB gain at 4:1 (envelope dips between peaks).
        assert!((-11.5..-8.0).contains(&db), "gain {db} dB");
    }
    #[test]
    fn limiter_holds_ceiling() {
        let spec = ResolvedAudioEffect::Limiter {
            ceiling_db: -6.0,
            release_ms: 50.0,
        };
        let input = sine(440.0, 48_000);
        let mut p = processor(&spec, ChannelMask::STEREO);
        let out = run(&mut p, &input);
        let peak = out
            .iter()
            .skip(4_800)
            .flatten()
            .fold(0.0_f32, |m, v| m.max(v.abs()));
        let ceiling = 10_f64.powf(-6.0 / 20.0) as f32;
        assert!(peak <= ceiling * 1.001, "peak {peak}");
    }
    #[test]
    fn lfe_channel_passes_limiter_unchanged() {
        let spec = ResolvedAudioEffect::Limiter {
            ceiling_db: -6.0,
            release_ms: 50.0,
        };
        let mut p = processor(&spec, ChannelMask::SURROUND_5_1);
        let mut frame = [0.9, 0.9, 0.9, 0.95, 0.9, 0.9];
        p.process(&mut frame);
        // LFE (index 3) is neither detected nor scaled; the other channels
        // are limited below ceiling.
        assert_eq!(frame[3], 0.95);
        let ceiling = 10_f64.powf(-6.0 / 20.0) as f32;
        assert!(frame[0] <= ceiling);
        let envelope_gain = frame[0] / 0.9;
        for channel in [1, 2, 4, 5] {
            assert!((frame[channel] / 0.9 - envelope_gain).abs() < 1e-6);
        }
    }
    #[test]
    fn compressor_ignores_lfe_in_detection() {
        let spec = ResolvedAudioEffect::Compressor {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 1.0,
            release_ms: 100.0,
            makeup_db: 0.0,
        };
        let mut p = processor(&spec, ChannelMask::SURROUND_5_1);
        // A loud LFE alone never triggers compression on the other channels.
        let mut frame = [0.001, 0.001, 0.001, 0.99, 0.001, 0.001];
        for _ in 0..2_000 {
            p.process(&mut frame);
        }
        let quiet = frame[0];
        assert!(
            quiet > 0.0005,
            "front channel must not be compressed by LFE"
        );
    }
    #[test]
    fn filters_process_every_channel() {
        let spec = ResolvedAudioEffect::Hpf {
            cutoff_hz: 1_000.0,
            order: 4,
        };
        let mut p = processor(&spec, ChannelMask::SURROUND_5_1);
        let mut frame = [1.0; 6];
        for _ in 0..64 {
            p.process(&mut frame);
        }
        assert!(frame.iter().all(|v| v.is_finite()));
        // All six channels share the HPF, including LFE.
        let reference = frame[0];
        for &value in frame.iter().skip(1) {
            assert!((value - reference).abs() < 1e-6);
        }
    }
    #[test]
    fn identical_runs_are_bit_identical() {
        let spec = ResolvedAudioEffect::Compressor {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            makeup_db: 3.0,
        };
        let input = sine(440.0, 9_600);
        let run = || {
            let mut p = processor(&spec, ChannelMask::STEREO);
            input
                .iter()
                .map(|f| {
                    let mut frame = *f;
                    p.process(&mut frame);
                    frame
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}
