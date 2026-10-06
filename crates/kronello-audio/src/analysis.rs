//! Bounded offline FFT analysis; the immutable result is consumed without PCM.
use crate::{AudioBuffer, AudioError};
use kronello_model::{
    AssetId, AudioAnalysisConfig, AudioAnalysisDataAsset, AudioAnalysisFrame, AudioAnalysisSource,
};
use kronello_time::Time;

/// Rectangular windows; stereo RMS and channel-averaged one-sided FFT energy.
/// The final window is zero padded; energy is normalized by window squared.
pub fn analyze_audio(
    id: AssetId,
    source: AudioAnalysisSource,
    config: AudioAnalysisConfig,
    start_sample: i64,
    buffer: &AudioBuffer,
) -> Result<AudioAnalysisDataAsset, AudioError> {
    config
        .validate()
        .map_err(|e| AudioError::InvalidInput(e.to_string()))?;
    if start_sample < 0 {
        return Err(AudioError::InvalidInput("negative analysis start".into()));
    }
    let n = config.window as usize;
    let hop = config.hop as usize;
    let count = buffer.frames().len().div_ceil(hop);
    if count > 65536
        || config
            .work_for_samples(buffer.frames().len())
            .is_none_or(|w| w > 100_000_000)
    {
        return Err(AudioError::BudgetExceeded(
            "analysis FFT operation budget".into(),
        ));
    }
    let mut frames = Vec::with_capacity(count);
    let mut previous_rms = 0.0;
    let mut last_beat: Option<usize> = None;
    for start in (0..buffer.frames().len()).step_by(hop) {
        let mut energy = vec![0.0; config.bands.len()];
        let mut rms_squared = 0.0;
        for channel in 0..2 {
            let mut re = vec![0.0; n];
            let mut im = vec![0.0; n];
            for (j, frame) in buffer.frames()[start..].iter().take(n).enumerate() {
                re[j] = f64::from(frame[channel]);
                rms_squared += re[j] * re[j] / (2 * n) as f64;
            }
            fft(&mut re, &mut im);
            for k in 0..=n / 2 {
                let weight = if k == 0 || k == n / 2 { 1.0 } else { 2.0 };
                let power = weight * (re[k] * re[k] + im[k] * im[k]) / (2 * n * n) as f64;
                for (band, [low, high]) in config.bands.iter().enumerate() {
                    let frequency = k as u64 * u64::from(config.sample_rate);
                    if frequency >= u64::from(*low) * n as u64
                        && frequency < u64::from(*high) * n as u64
                    {
                        energy[band] += power;
                    }
                }
            }
        }
        let rms = rms_squared.sqrt();
        let onset = (rms - previous_rms).max(0.0);
        let beat = onset >= 0.05 && last_beat.is_none_or(|s| start - s >= 4800);
        if beat {
            last_beat = Some(start);
        }
        previous_rms = rms;
        frames.push(AudioAnalysisFrame {
            time: Time::new(
                start_sample
                    .checked_add(start as i64)
                    .ok_or(AudioError::Overflow)?,
                48000,
            )?,
            rms,
            band_energy: energy,
            onset,
            beat,
        });
    }
    let result = AudioAnalysisDataAsset {
        id,
        source,
        config,
        start_sample,
        sample_count: buffer.frames().len() as u32,
        frames,
    };
    result
        .validate()
        .map_err(|e| AudioError::InvalidInput(e.to_string()))?;
    Ok(result)
}
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - n.ilog2());
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut length = 2;
    while length <= n {
        let angle = -std::f64::consts::TAU / length as f64;
        let (wi, wr) = angle.sin_cos();
        for base in (0..n).step_by(length) {
            let (mut ur, mut ui) = (1.0, 0.0);
            for j in 0..length / 2 {
                let a = base + j;
                let b = a + length / 2;
                let tr = re[b] * ur - im[b] * ui;
                let ti = re[b] * ui + im[b] * ur;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (ur, ui) = (ur * wr - ui * wi, ur * wi + ui * wr);
            }
        }
        length *= 2;
    }
}
