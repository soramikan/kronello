//! AUDIO-008: ITU-R BS.1770-4 K-weighted loudness measurement plus
//! 4x-oversampled true-peak estimation on the 48 kHz stereo bus (ADR-0117).
//! Deterministic: fixed coefficients, fixed block grid, no wall clock.
use crate::{AudioError, MAX_AUDIO_FRAMES, dsp::Biquad};

/// ITU-R BS.1770-4 integration windows on the 48 kHz sample grid.
const MOMENTARY: usize = 19_200; // 400 ms
const HOP: usize = 4_800; // 100 ms (75% overlap)
const SHORT_TERM: usize = 144_000; // 3 s
const OFFSET: f64 = -0.691;
const ABSOLUTE_GATE: f64 = -70.0;

/// Deterministic BS.1770-4 summary for one rendered range. `None` means the
/// measurement is undefined for the input (too short or fully gated out).
#[derive(Debug, Clone, PartialEq)]
pub struct LoudnessReport {
    /// Gated integrated programme loudness (momentary-block gating).
    pub integrated_lufs: Option<f64>,
    /// Maximum 400 ms momentary loudness inside the range.
    pub momentary_lufs: Option<f64>,
    /// Maximum 3 s short-term loudness inside the range.
    pub short_term_lufs: Option<f64>,
    /// 4x-oversampled true peak in dBTP; `None` for a silent range.
    pub true_peak_dbtp: Option<f64>,
}
/// BS.1770-4 K-weighting at 48 kHz: stage-1 high-shelf then stage-2 RLB
/// high-pass, both applied independently to left and right.
fn k_weighting() -> [Biquad; 2] {
    [
        Biquad::normalized(
            [
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85,
            ],
            [1.0, -1.690_659_293_182_41, 0.732_480_774_215_85],
        ),
        Biquad::normalized(
            [1.0, -2.0, 1.0],
            [1.0, -1.990_047_454_833_98, 0.990_072_250_366_21],
        ),
    ]
}
/// Measure a finite interleaved stereo range. Caller bounds memory; this
/// refuses inputs above the shared audio budget.
pub fn loudness(frames: &[[f32; 2]]) -> Result<LoudnessReport, AudioError> {
    if frames.len() > MAX_AUDIO_FRAMES || frames.iter().flatten().any(|v| !v.is_finite()) {
        return Err(AudioError::InvalidInput(
            "loudness input budget or finite sample violation".into(),
        ));
    }
    // K-weight the full range once; block statistics read prefix energies.
    let mut filter = k_weighting();
    let mut prefix = Vec::with_capacity(frames.len() + 1);
    prefix.push(0.0_f64);
    for frame in frames {
        let mut y = *frame;
        for stage in &mut filter {
            y = stage.process(y);
        }
        let energy = f64::from(y[0]) * f64::from(y[0]) + f64::from(y[1]) * f64::from(y[1]);
        prefix.push(prefix.last().copied().unwrap_or(0.0) + energy);
    }
    let mean_square = |start: usize, len: usize| (prefix[start + len] - prefix[start]) / len as f64;
    // Momentary blocks on the fixed 400 ms / 100 ms hop grid.
    let mut blocks = Vec::new();
    let mut start = 0_usize;
    while start + MOMENTARY <= frames.len() {
        let z = mean_square(start, MOMENTARY);
        blocks.push((z, OFFSET + 10.0 * z.max(1e-30).log10()));
        start += HOP;
    }
    let momentary_lufs = blocks.iter().map(|(_, l)| *l).reduce(f64::max);
    let short_term_lufs = if frames.len() >= SHORT_TERM {
        let mut best = f64::NEG_INFINITY;
        let mut start = 0_usize;
        while start + SHORT_TERM <= frames.len() {
            let z = mean_square(start, SHORT_TERM);
            best = best.max(OFFSET + 10.0 * z.max(1e-30).log10());
            start += HOP;
        }
        Some(best)
    } else {
        None
    };
    // BS.1770-4 two-stage gating: absolute -70 LUFS, then mean - 10 LU.
    let integrated_lufs = {
        let gated: Vec<(f64, f64)> = blocks
            .iter()
            .copied()
            .filter(|(_, l)| *l > ABSOLUTE_GATE)
            .collect();
        if gated.is_empty() {
            None
        } else {
            let mean_z = gated.iter().map(|(z, _)| z).sum::<f64>() / gated.len() as f64;
            let relative_gate = OFFSET + 10.0 * mean_z.max(1e-30).log10() - 10.0;
            let selected: Vec<f64> = gated
                .iter()
                .filter(|(_, l)| *l > relative_gate)
                .map(|(z, _)| *z)
                .collect();
            if selected.is_empty() {
                None
            } else {
                Some(
                    OFFSET
                        + 10.0
                            * (selected.iter().sum::<f64>() / selected.len() as f64)
                                .max(1e-30)
                                .log10(),
                )
            }
        }
    };
    Ok(LoudnessReport {
        integrated_lufs,
        momentary_lufs,
        short_term_lufs,
        true_peak_dbtp: true_peak(frames),
    })
}
const UPSAMPLE: usize = 4;
const PHASE_TAPS: usize = 8;
/// 4x linear-phase interpolating FIR: 32-tap windowed sinc at pi/4, split
/// into four phases each DC-normalized to unity gain. Deterministic
/// coefficients are computed at call time from integer arithmetic only.
fn interpolation_phases() -> [[f64; PHASE_TAPS]; UPSAMPLE] {
    const TAPS: usize = UPSAMPLE * PHASE_TAPS;
    let center = (TAPS as f64 - 1.0) / 2.0;
    let fc = std::f64::consts::FRAC_PI_4;
    let mut h = [0.0_f64; TAPS];
    for (i, v) in h.iter_mut().enumerate() {
        let m = i as f64 - center;
        let ideal = if m == 0.0 {
            fc / std::f64::consts::PI
        } else {
            (fc * m).sin() / (std::f64::consts::PI * m)
        };
        let x = i as f64 / (TAPS as f64 - 1.0);
        let window = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * x).cos()
            + 0.08 * (4.0 * std::f64::consts::PI * x).cos();
        *v = ideal * window;
    }
    let mut phases = [[0.0_f64; PHASE_TAPS]; UPSAMPLE];
    for (p, taps) in phases.iter_mut().enumerate() {
        for (k, tap) in taps.iter_mut().enumerate() {
            *tap = h[UPSAMPLE * k + p];
        }
        let sum: f64 = taps.iter().sum();
        for tap in taps.iter_mut() {
            *tap /= sum;
        }
    }
    phases
}
/// Peak over the 4x-oversampled signal and the raw samples, in dBTP.
fn true_peak(frames: &[[f32; 2]]) -> Option<f64> {
    if frames.is_empty() {
        return None;
    }
    let phases = interpolation_phases();
    let mut peak = 0.0_f64;
    for channel in 0..2 {
        let read = |i: i64| -> f64 {
            if i < 0 || i as usize >= frames.len() {
                0.0
            } else {
                f64::from(frames[i as usize][channel])
            }
        };
        for i in 0..frames.len() as i64 {
            for taps in &phases {
                let mut y = 0.0;
                for (k, coefficient) in taps.iter().enumerate() {
                    y += coefficient * read(i + 4 - k as i64);
                }
                peak = peak.max(y.abs());
            }
            peak = peak.max(read(i).abs());
        }
    }
    (peak > 0.0).then(|| 20.0 * peak.log10())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sine(freq: f64, amplitude: f64, seconds: f64) -> Vec<[f32; 2]> {
        let n = (seconds * 48_000.0) as usize;
        (0..n)
            .map(|i| {
                (amplitude * (std::f64::consts::TAU * freq * i as f64 / 48_000.0).sin()) as f32
            })
            .map(|v| [v, v])
            .collect()
    }
    #[test]
    fn sine_at_minus_20_dbtp_measures_about_minus_20_lufs() {
        // Dual-channel sine at amplitude 0.1: both channels contribute mean
        // square 0.005, the K filter adds ~+0.7 dB near 1 kHz, yielding the
        // documented ≈ -20.0 LUFS reference level.
        let report = loudness(&sine(997.0, 0.1, 10.0)).expect("loudness");
        let integrated = report.integrated_lufs.expect("integrated");
        assert!((integrated - -20.0).abs() < 0.1, "integrated {integrated}");
        let momentary = report.momentary_lufs.expect("momentary");
        assert!(
            (momentary - integrated).abs() < 0.2,
            "momentary {momentary}"
        );
        let short_term = report.short_term_lufs.expect("short term");
        assert!(
            (short_term - integrated).abs() < 0.2,
            "short term {short_term}"
        );
        let tp = report.true_peak_dbtp.expect("true peak");
        assert!((tp - -20.0).abs() < 0.2, "true peak {tp}");
    }
    #[test]
    fn silence_reports_no_loudness() {
        let report = loudness(&vec![[0.0; 2]; 48_000]).expect("loudness");
        assert_eq!(report.integrated_lufs, None);
        assert_eq!(report.true_peak_dbtp, None);
    }
    #[test]
    fn short_input_reports_partial_fields() {
        let report = loudness(&sine(440.0, 0.5, 0.2)).expect("loudness");
        assert_eq!(report.integrated_lufs, None);
        assert!(report.true_peak_dbtp.is_some());
    }
    #[test]
    fn determinism() {
        let input = sine(440.0, 0.3, 5.0);
        assert_eq!(loudness(&input).unwrap(), loudness(&input).unwrap());
    }
}
