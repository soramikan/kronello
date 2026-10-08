//! NLE-007 deterministic multicam audio sync estimation (ADR-0127).
//! Bounded normalized cross-correlation over decoded 48 kHz stereo buffers;
//! no codecs, clocks, or mutable state. The returned `lag` satisfies
//! `candidate[m] ~= reference[m + lag]` over the overlapping window, so a
//! positive lag means the candidate recorded the same content earlier in its
//! own buffer — its sync_offset is `-lag` converted to seconds plus the
//! difference of stream start times.
use crate::{AudioBuffer, AudioError};

/// Minimum stereo frames of genuine overlap required for a scored lag.
const MIN_OVERLAP: i64 = 4_800;
/// Normalized-correlation floor below which the estimate is rejected as
/// unrelated media rather than a mistimed pair.
const MIN_PEAK: f64 = 0.6;
/// Second-best relative gap within which distinct lags count as ambiguous.
const AMBIGUITY_RATIO: f64 = 0.999;
/// Correlation work bound, shared with the mixer operation budget.
const MAX_SYNC_OPERATIONS: u64 = 100_000_000;
/// Hard bound on the caller-selected search radius: ten minutes at 48 kHz.
pub const MAX_SYNC_LAG: i64 = crate::MAX_AUDIO_FRAMES as i64;
/// Largest search radius the operation budget allows for two decoded
/// buffers: `estimate_sync_lag` costs `max_len·(2·lag+1)` operations. A
/// radius of zero still tests lag 0 only; negative input yields zero.
pub fn max_search_lag(reference_frames: i64, candidate_frames: i64) -> i64 {
    let max_len = reference_frames.max(candidate_frames).max(1) as u64;
    let feasible = ((MAX_SYNC_OPERATIONS / max_len).saturating_sub(1) / 2) as i64;
    feasible
        .min(reference_frames.min(candidate_frames).saturating_sub(1))
        .clamp(0, MAX_SYNC_LAG)
}

fn mono(buffer: &AudioBuffer) -> Vec<f64> {
    buffer
        .frames()
        .iter()
        .map(|frame| (f64::from(frame[0]) + f64::from(frame[1])) * 0.5)
        .collect()
}
/// Deterministic lag enumeration order: |k| ascending with the positive lag
/// first, so equal scores resolve to the smallest shift in a fixed order.
fn lag_order(max_lag: i64) -> impl Iterator<Item = i64> {
    (0..=max_lag).flat_map(|magnitude| {
        if magnitude == 0 {
            [Some(0_i64), None]
        } else {
            [Some(magnitude), Some(-magnitude)]
        }
        .into_iter()
        .flatten()
    })
}
/// Estimate `lag` in `candidate[m] ~= reference[m + lag]` by normalized
/// cross-correlation. Both buffers must be nonempty 48 kHz stereo; empty or
/// zero-energy input, too little overlap, weak or ambiguous peaks fail with
/// `MULTICAM_SYNC_FAILED`.
pub fn estimate_sync_lag(
    reference: &AudioBuffer,
    candidate: &AudioBuffer,
    max_lag_samples: i64,
) -> Result<i64, AudioError> {
    if !(0..=MAX_SYNC_LAG).contains(&max_lag_samples) {
        return Err(AudioError::InvalidInput(
            "multicam sync lag window outside bounds".into(),
        ));
    }
    let reference = mono(reference);
    let candidate = mono(candidate);
    if reference.is_empty() || candidate.is_empty() {
        return Err(AudioError::SyncFailed("empty audio input".into()));
    }
    let reference_len = reference.len() as i64;
    let candidate_len = candidate.len() as i64;
    let operations = (reference_len.max(candidate_len) as u64)
        .checked_mul(2 * max_lag_samples as u64 + 1)
        .ok_or(AudioError::Overflow)?;
    if operations > MAX_SYNC_OPERATIONS {
        return Err(AudioError::BudgetExceeded(
            "multicam sync correlation window".into(),
        ));
    }
    let reference_energy: f64 = reference.iter().map(|v| v * v).sum();
    let candidate_energy: f64 = candidate.iter().map(|v| v * v).sum();
    // Non-finite energies (NaN/inf samples) must not reach the loop either.
    if !reference_energy.is_finite()
        || !candidate_energy.is_finite()
        || reference_energy <= 0.0
        || candidate_energy <= 0.0
    {
        return Err(AudioError::SyncFailed("silent audio input".into()));
    }
    let mut best: Option<(i64, f64)> = None;
    let mut runner_up = f64::NEG_INFINITY;
    for lag in lag_order(max_lag_samples) {
        let overlap_start = (-lag).max(0);
        let overlap_end = candidate_len.min(reference_len - lag);
        let overlap = overlap_end - overlap_start;
        if overlap < MIN_OVERLAP {
            continue;
        }
        let (mut dot, mut reference_norm, mut candidate_norm) = (0.0, 0.0, 0.0);
        for m in overlap_start..overlap_end {
            // Invariant: overlap keeps `m + lag` inside the reference range.
            let r = reference[(m + lag) as usize];
            let sample = candidate[m as usize];
            dot += r * sample;
            reference_norm += r * r;
            candidate_norm += sample * sample;
        }
        if !reference_norm.is_finite()
            || !candidate_norm.is_finite()
            || reference_norm <= 0.0
            || candidate_norm <= 0.0
        {
            continue;
        }
        let score = dot / (reference_norm * candidate_norm).sqrt();
        match best {
            Some((_, peak)) if score <= peak => {
                if score > runner_up {
                    runner_up = score;
                }
            }
            _ => {
                if let Some((_, peak)) = best {
                    runner_up = peak;
                }
                best = Some((lag, score));
            }
        }
    }
    let Some((lag, peak)) = best else {
        return Err(AudioError::SyncFailed(
            "no overlapping audio for synchronization".into(),
        ));
    };
    if peak < MIN_PEAK {
        return Err(AudioError::SyncFailed("weak audio correlation".into()));
    }
    if peak > 0.0 && runner_up >= peak * AMBIGUITY_RATIO {
        return Err(AudioError::SyncFailed("ambiguous audio correlation".into()));
    }
    Ok(lag)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Deterministic fixed-seed noise: decorrelated between independent
    /// seeds and indices, perfectly correlated when shifted. A splitmix64
    /// hash of (index, seed); no clocks or entropy.
    fn noise(frames: usize, seed: u64) -> Vec<f64> {
        (0..frames)
            .map(|i| {
                let mut z = (i as u64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(seed);
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                ((z >> 40) as f64 / (1u64 << 24) as f64) - 0.5
            })
            .collect()
    }
    fn buffer(mono_samples: &[f64]) -> AudioBuffer {
        AudioBuffer::new(
            mono_samples
                .iter()
                .map(|v| [*v as f32, *v as f32])
                .collect(),
        )
        .unwrap()
    }
    #[test]
    fn estimates_positive_lag() {
        let reference = noise(48_000, 7);
        // candidate[m] = reference[m + 2400] for the overlapping prefix.
        // The operation budget bounds 48k-frame buffers to |lag| <= ~1041.
        let candidate = reference[600..].to_vec();
        let lag = estimate_sync_lag(&buffer(&reference), &buffer(&candidate), 1_000).unwrap();
        assert_eq!(lag, 600);
    }
    #[test]
    fn estimates_negative_lag() {
        let reference = noise(48_000, 11);
        // candidate[m] = reference[m - 400] for m >= 400.
        let mut candidate = vec![0.0; 400];
        candidate.extend_from_slice(&reference[..48_000 - 400]);
        let lag = estimate_sync_lag(&buffer(&reference), &buffer(&candidate), 1_000).unwrap();
        assert_eq!(lag, -400);
    }
    #[test]
    fn fails_on_silent_input() {
        let reference = noise(48_000, 3);
        let silence = vec![0.0; 48_000];
        let error = estimate_sync_lag(&buffer(&reference), &buffer(&silence), 1_000).unwrap_err();
        assert_eq!(error.code(), "MULTICAM_SYNC_FAILED");
    }
    #[test]
    fn fails_on_empty_input() {
        let reference = noise(4_800, 5);
        let error = estimate_sync_lag(&buffer(&reference), &buffer(&[]), 1_000).unwrap_err();
        assert_eq!(error.code(), "MULTICAM_SYNC_FAILED");
    }
    #[test]
    fn fails_on_weak_correlation() {
        let reference = noise(48_000, 13);
        let unrelated = noise(48_000, 4_294_967_291);
        let error = estimate_sync_lag(&buffer(&reference), &buffer(&unrelated), 1_000).unwrap_err();
        assert_eq!(error.code(), "MULTICAM_SYNC_FAILED");
    }
    #[test]
    fn fails_on_ambiguous_correlation() {
        // A periodic carrier scores ~1 at every period lag, so no distinct
        // peak is admissible.
        let reference: Vec<f64> = (0..48_000)
            .map(|n| (n as f64 * 0.314).sin() * 0.5)
            .collect();
        let error = estimate_sync_lag(&buffer(&reference), &buffer(&reference), 1_000).unwrap_err();
        assert_eq!(error.code(), "MULTICAM_SYNC_FAILED");
    }
    #[test]
    fn fails_when_overlap_below_minimum() {
        // Buffers shorter than the 4800-frame minimum never score a lag.
        let reference = noise(4_700, 17);
        let candidate = reference.clone();
        let error = estimate_sync_lag(&buffer(&reference), &buffer(&candidate), 100).unwrap_err();
        assert_eq!(error.code(), "MULTICAM_SYNC_FAILED");
    }
    #[test]
    fn rejects_invalid_lag_bounds() {
        let data = noise(4_800, 19);
        let input = buffer(&data);
        assert!(matches!(
            estimate_sync_lag(&input, &input, -1),
            Err(AudioError::InvalidInput(_))
        ));
        assert!(matches!(
            estimate_sync_lag(&input, &input, MAX_SYNC_LAG + 1),
            Err(AudioError::InvalidInput(_))
        ));
    }
    #[test]
    fn enforces_operation_budget() {
        let data = noise(9_600, 23);
        let input = buffer(&data);
        assert!(matches!(
            estimate_sync_lag(&input, &input, MAX_SYNC_LAG),
            Err(AudioError::BudgetExceeded(_))
        ));
    }
    #[test]
    fn max_search_lag_stays_within_budget() {
        let lag = max_search_lag(48_000 * 600, 48_000 * 600);
        assert!((0..=MAX_SYNC_LAG).contains(&lag));
        let data = noise(48_000, 29);
        let input = buffer(&data);
        assert!(estimate_sync_lag(&input, &input, lag).is_ok());
    }
}
