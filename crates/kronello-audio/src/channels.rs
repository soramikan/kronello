//! ADR-0124 multichannel buffers, sources and explicit ITU layout conversion.
//!
//! Every audio stage carries a [`ChannelMask`]; nothing ever derives a layout
//! from a channel count and nothing folds down implicitly. Conversion between
//! supported layouts is an explicit, deterministic matrix: same-position
//! channels pass at unity, center and surrounds fold at −3 dB (1/√2), and the
//! LFE channel is excluded unless the caller explicitly opts in.
use std::collections::BTreeMap;

use kronello_model::{AssetId, ChannelMask};

use crate::{AudioError, MAX_AUDIO_FRAMES};

/// Largest channel count in the closed layout set (7.1).
pub const MAX_CHANNELS: usize = 8;
/// ITU BS.775 style fold-down coefficient: −3 dB for center and each surround.
pub const ITU_CENTER_COEFFICIENT: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Surrounds contribute to mono at half the ITU coefficient (−9 dB each).
pub const ITU_SURROUND_MONO_COEFFICIENT: f64 = std::f64::consts::FRAC_1_SQRT_2 / 2.0;

fn unsupported_layout(detail: impl Into<String>) -> AudioError {
    AudioError::UnsupportedChannelLayout(detail.into())
}

/// Finite interleaved f32 frames carrying an explicit [`ChannelMask`].
/// `samples.len()` is always a multiple of `mask.channels()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBuffer {
    mask: ChannelMask,
    samples: Vec<f32>,
}
impl ChannelBuffer {
    pub fn new(mask: ChannelMask, samples: Vec<f32>) -> Result<Self, AudioError> {
        let channels = mask.channels();
        if !samples.len().is_multiple_of(channels)
            || samples.len() / channels > MAX_AUDIO_FRAMES
            || samples.iter().any(|v| !v.is_finite())
        {
            return Err(AudioError::InvalidInput(
                "channel buffer budget or finite sample violation".into(),
            ));
        }
        Ok(Self { mask, samples })
    }
    /// Synthesizes a silent buffer of `frames` frames.
    pub fn silent(mask: ChannelMask, frames: usize) -> Result<Self, AudioError> {
        if frames > MAX_AUDIO_FRAMES {
            return Err(AudioError::InvalidInput(
                "channel buffer frame budget".into(),
            ));
        }
        Self::new(mask, vec![0.0; frames * mask.channels()])
    }
    /// Adapts a legacy stereo frame buffer into a stereo [`ChannelBuffer`].
    pub fn from_stereo(frames: &[[f32; 2]]) -> Result<Self, AudioError> {
        let samples: Vec<f32> = frames.iter().flatten().copied().collect();
        Self::new(ChannelMask::STEREO, samples)
    }
    /// Stereo frames view; `None` for any other layout.
    pub fn stereo_frames(&self) -> Option<Vec<[f32; 2]>> {
        if self.mask != ChannelMask::STEREO {
            return None;
        }
        Some(self.samples.chunks_exact(2).map(|f| [f[0], f[1]]).collect())
    }
    pub fn mask(&self) -> ChannelMask {
        self.mask
    }
    pub fn channels(&self) -> usize {
        self.mask.channels()
    }
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels()
    }
    /// Interleaved samples in native channel order.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    pub fn frame(&self, index: usize) -> Option<&[f32]> {
        let channels = self.channels();
        self.samples
            .get(index * channels..index * channels + channels)
    }
}

/// A rendered range on the absolute sample grid with an explicit layout.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBus {
    pub(crate) start_sample: i64,
    pub(crate) buffer: ChannelBuffer,
}
impl ChannelBus {
    pub fn start_sample(&self) -> i64 {
        self.start_sample
    }
    pub fn buffer(&self) -> &ChannelBuffer {
        &self.buffer
    }
    /// Same PCM24 quantization rules as [`crate::Bus`], per channel.
    pub fn quantize_pcm24(
        &self,
        policy: crate::ClippingPolicy,
    ) -> Result<crate::QuantizedAudio, AudioError> {
        crate::quantize_samples(self.buffer.samples(), policy)
    }
    /// Stereo bus view; an explicit layout error for any other mask.
    pub fn into_stereo_bus(self) -> Result<crate::Bus, AudioError> {
        if self.buffer.mask() != ChannelMask::STEREO {
            return Err(unsupported_layout(format!(
                "cannot expose {} output as stereo Bus",
                self.buffer.mask().name()
            )));
        }
        crate::Bus::from_frames(self.start_sample, self.buffer)
    }
}
impl From<crate::Bus> for ChannelBus {
    fn from(bus: crate::Bus) -> Self {
        Self {
            start_sample: bus.start_sample(),
            buffer: ChannelBuffer {
                mask: ChannelMask::STEREO,
                samples: bus.buffer().frames().iter().flatten().copied().collect(),
            },
        }
    }
}

/// Multichannel source store keyed like [`crate::AudioSources`].
pub type ChannelSources = BTreeMap<(AssetId, u32), ChannelBuffer>;

/// Read-only indexed source contract carrying an explicit layout.
/// Implementations may page samples without exposing files or codecs to this
/// pure evaluator. Missing samples are errors; `out` must hold exactly the
/// layout's channel count.
pub trait ChannelSourceReader {
    fn layout(&self, asset: AssetId, stream: u32) -> Result<ChannelMask, AudioError>;
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError>;
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError>;
}
impl ChannelSourceReader for crate::AudioSources {
    fn layout(&self, asset: AssetId, _stream: u32) -> Result<ChannelMask, AudioError> {
        self.get(&(asset, _stream))
            .ok_or(AudioError::AssetMissing(asset))?;
        Ok(ChannelMask::STEREO)
    }
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError> {
        Ok(self
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .frames()
            .len())
    }
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        if out.len() != 2 {
            return Err(unsupported_layout("stereo sources read two channels"));
        }
        let frame = self
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .frames()
            .get(index)
            .copied()
            .ok_or(AudioError::SourceTooShort(asset))?;
        out.copy_from_slice(&frame);
        Ok(())
    }
}
impl ChannelSourceReader for ChannelSources {
    fn layout(&self, asset: AssetId, stream: u32) -> Result<ChannelMask, AudioError> {
        Ok(self
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .mask())
    }
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError> {
        Ok(self
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?
            .frame_count())
    }
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        let buffer = self
            .get(&(asset, stream))
            .ok_or(AudioError::AssetMissing(asset))?;
        if out.len() != buffer.channels() {
            return Err(unsupported_layout("source frame channel count mismatch"));
        }
        let frame = buffer
            .frame(index)
            .ok_or(AudioError::SourceTooShort(asset))?;
        out.copy_from_slice(frame);
        Ok(())
    }
}

/// Deterministic layout conversion matrix. `weights[dst][src]` is applied in
/// f64 so repeated application matches a direct table lookup exactly.
#[derive(Debug, Clone)]
pub struct LayoutMatrix {
    from: ChannelMask,
    to: ChannelMask,
    /// `weights[to_index][from_index]` in native channel order.
    weights: [[f64; MAX_CHANNELS]; MAX_CHANNELS],
}
impl LayoutMatrix {
    pub fn from(&self) -> ChannelMask {
        self.from
    }
    pub fn to(&self) -> ChannelMask {
        self.to
    }
    /// Weight applied from `src` index to `dst` index in native order.
    pub fn weight(&self, dst: usize, src: usize) -> f64 {
        self.weights[dst][src]
    }
    /// Converts one frame. Input/output lengths must equal the layouts'
    /// channel counts; a mismatch is a typed layout error, never a panic.
    pub fn apply(&self, input: &[f32], output: &mut [f32]) -> Result<(), AudioError> {
        if input.len() != self.from.channels() || output.len() != self.to.channels() {
            return Err(unsupported_layout("layout matrix channel count mismatch"));
        }
        for (dst, value) in output.iter_mut().enumerate() {
            let mut sum = 0.0_f64;
            for (src, sample) in input.iter().enumerate() {
                sum += self.weights[dst][src] * f64::from(*sample);
            }
            *value = sum as f32;
        }
        Ok(())
    }
}

/// Explicit layout conversion table between two supported masks.
///
/// Rules (all deterministic constants, ADR-0124):
/// - identical speaker positions pass at unity;
/// - mono sources duplicate to L/R at unity and to C at unity for surround
///   targets, matching the decode-side "duplicate at unity" rule;
/// - center folds into L/R at `FRAC_1_SQRT_2` (−3 dB); each surround folds
///   into the nearer L/R at the same coefficient; a 7.1 source folding to a
///   layout without the matching surround position adds the folded surround
///   at −3 dB onto the surviving surround;
/// - mono targets take L/R at −6 dB, C at −3 dB and each surround at −9 dB;
/// - LFE passes only when present in both layouts; with `include_lfe` it also
///   folds into L/R/mono at the center coefficient, otherwise it is dropped.
/// - target positions with no rule remain silent; no channel is synthesized.
pub fn layout_matrix(
    from: ChannelMask,
    to: ChannelMask,
    include_lfe: bool,
) -> Result<LayoutMatrix, AudioError> {
    // Both masks come from the closed set; the checks keep the signature total
    // for callers holding raw bits.
    let source = from.channel_bits();
    let target = to.channel_bits();
    let mut weights = [[0.0_f64; MAX_CHANNELS]; MAX_CHANNELS];
    let src_index = |bit: u64| -> Option<usize> { source.iter().position(|b| *b == bit) };
    let dst_index = |bit: u64| -> Option<usize> { target.iter().position(|b| *b == bit) };
    if from == to {
        for (index, row) in weights.iter_mut().enumerate().take(source.len()) {
            row[index] = 1.0;
        }
        return Ok(LayoutMatrix { from, to, weights });
    }
    if from == ChannelMask::MONO {
        // Mono duplicates at unity: onto L/R for stereo, or onto the center
        // speaker for surround targets (dialogue convention). No surrounds
        // or LFE are ever synthesized.
        if let Some(dst) = dst_index(ChannelMask::FRONT_CENTER) {
            weights[dst][0] = 1.0;
        } else {
            for (dst, bit) in target.iter().enumerate() {
                if matches!(*bit, ChannelMask::FRONT_LEFT | ChannelMask::FRONT_RIGHT) {
                    weights[dst][0] = 1.0;
                }
            }
        }
        return Ok(LayoutMatrix { from, to, weights });
    }
    let k = ITU_CENTER_COEFFICIENT;
    if to == ChannelMask::MONO {
        // Explicit mono fold-down: L/R at −6 dB, center at −3 dB, each
        // surround at −9 dB; LFE excluded unless `include_lfe`.
        let dst = 0;
        for fold in [ChannelMask::FRONT_LEFT, ChannelMask::FRONT_RIGHT] {
            if let Some(src) = src_index(fold) {
                weights[dst][src] += 0.5;
            }
        }
        if let Some(src) = src_index(ChannelMask::FRONT_CENTER) {
            weights[dst][src] += k;
        }
        for fold in [
            ChannelMask::SIDE_LEFT,
            ChannelMask::SIDE_RIGHT,
            ChannelMask::BACK_LEFT,
            ChannelMask::BACK_RIGHT,
        ] {
            if let Some(src) = src_index(fold) {
                weights[dst][src] += ITU_SURROUND_MONO_COEFFICIENT;
            }
        }
        if include_lfe && let Some(src) = src_index(ChannelMask::LOW_FREQUENCY) {
            weights[dst][src] += 0.5;
        }
        return Ok(LayoutMatrix { from, to, weights });
    }
    // Fold preference for a source speaker the target does not carry: the
    // surviving surround position first, then the nearer front. Coefficients
    // stay at the documented −3 dB constant.
    let fold_target =
        |bit: u64| -> Option<usize> {
            match bit {
                ChannelMask::SIDE_LEFT => {
                    dst_index(ChannelMask::BACK_LEFT).or_else(|| dst_index(ChannelMask::FRONT_LEFT))
                }
                ChannelMask::SIDE_RIGHT => dst_index(ChannelMask::BACK_RIGHT)
                    .or_else(|| dst_index(ChannelMask::FRONT_RIGHT)),
                ChannelMask::BACK_LEFT => {
                    dst_index(ChannelMask::SIDE_LEFT).or_else(|| dst_index(ChannelMask::FRONT_LEFT))
                }
                ChannelMask::BACK_RIGHT => dst_index(ChannelMask::SIDE_RIGHT)
                    .or_else(|| dst_index(ChannelMask::FRONT_RIGHT)),
                _ => None,
            }
        };
    for (src, bit) in source.iter().enumerate() {
        if let Some(dst) = dst_index(*bit) {
            weights[dst][src] = 1.0;
            continue;
        }
        match *bit {
            ChannelMask::FRONT_CENTER => {
                // Missing target center folds to L/R at −3 dB (mono handled).
                for bit in [ChannelMask::FRONT_LEFT, ChannelMask::FRONT_RIGHT] {
                    if let Some(dst) = dst_index(bit) {
                        weights[dst][src] += k;
                    }
                }
            }
            ChannelMask::LOW_FREQUENCY => {
                // LFE is dropped by default; `include_lfe` folds it onto the
                // fronts at the center coefficient.
                if include_lfe {
                    for bit in [ChannelMask::FRONT_LEFT, ChannelMask::FRONT_RIGHT] {
                        if let Some(dst) = dst_index(bit) {
                            weights[dst][src] += k;
                        }
                    }
                }
            }
            _ => {
                if let Some(dst) = fold_target(*bit) {
                    weights[dst][src] += k;
                }
            }
        }
    }
    Ok(LayoutMatrix { from, to, weights })
}

/// Explicitly converts an entire buffer to `target` using [`layout_matrix`].
/// No implicit fold-down exists: callers pick the target mask.
pub fn convert_layout(
    buffer: &ChannelBuffer,
    target: ChannelMask,
    include_lfe: bool,
) -> Result<ChannelBuffer, AudioError> {
    let matrix = layout_matrix(buffer.mask(), target, include_lfe)?;
    let channels = target.channels();
    let mut output = vec![0.0_f32; buffer.frame_count() * channels];
    let mut input = [0.0_f32; MAX_CHANNELS];
    let mut converted = [0.0_f32; MAX_CHANNELS];
    for index in 0..buffer.frame_count() {
        input[..buffer.channels()]
            .copy_from_slice(buffer.frame(index).expect("ChannelBuffer frame in range"));
        matrix.apply(&input[..buffer.channels()], &mut converted[..channels])?;
        output[index * channels..index * channels + channels]
            .copy_from_slice(&converted[..channels]);
    }
    ChannelBuffer::new(target, output)
}

use serde::{Deserialize, Serialize};

/// Per-channel peak/RMS for one explicitly-laid-out rendered range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelMeter {
    pub layout: ChannelMask,
    /// Maximum absolute sample per channel in native order.
    pub peak: Vec<f32>,
    /// Root-mean-square level per channel in native order.
    pub rms: Vec<f32>,
}
/// A track's contribution meters in the bus layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelTrackMeter {
    pub track: kronello_model::TrackId,
    pub peak: Vec<f32>,
    pub rms: Vec<f32>,
}
/// Multichannel counterpart of [`crate::BusMeters`].
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBusMeters {
    pub tracks: Vec<ChannelTrackMeter>,
    pub master: ChannelMeter,
}
/// Compute per-channel peak/RMS over interleaved frames.
pub fn channel_meter(mask: ChannelMask, samples: &[f32]) -> Result<ChannelMeter, AudioError> {
    let channels = mask.channels();
    if !samples.len().is_multiple_of(channels) {
        return Err(unsupported_layout("meter input is not whole frames"));
    }
    let mut peak = vec![0.0_f32; channels];
    let mut energy = vec![0.0_f64; channels];
    for frame in samples.chunks_exact(channels) {
        for (channel, value) in frame.iter().enumerate() {
            peak[channel] = peak[channel].max(value.abs());
            energy[channel] += f64::from(*value) * f64::from(*value);
        }
    }
    let count = (samples.len() / channels).max(1) as f64;
    Ok(ChannelMeter {
        layout: mask,
        peak,
        rms: energy.iter().map(|e| (e / count).sqrt() as f32).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_downmix_coefficients() {
        let matrix = layout_matrix(ChannelMask::SURROUND_5_1, ChannelMask::STEREO, false).unwrap();
        // dst L <- FL(1) + C(K) + SL(K); LFE excluded.
        assert_eq!(matrix.weight(0, 0), 1.0);
        assert_eq!(matrix.weight(0, 2), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(0, 4), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(0, 3), 0.0);
        assert_eq!(matrix.weight(1, 1), 1.0);
        assert_eq!(matrix.weight(1, 2), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(1, 5), std::f64::consts::FRAC_1_SQRT_2);
    }
    #[test]
    fn lfe_opt_in_folds_into_fronts() {
        let matrix = layout_matrix(ChannelMask::SURROUND_5_1, ChannelMask::STEREO, true).unwrap();
        assert_eq!(matrix.weight(0, 3), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(1, 3), std::f64::consts::FRAC_1_SQRT_2);
    }
    #[test]
    fn seven_one_folds_backs_onto_sides() {
        let matrix =
            layout_matrix(ChannelMask::SURROUND_7_1, ChannelMask::SURROUND_5_1, false).unwrap();
        // dst SL <- SL(1) + BL(K); dst SR <- SR(1) + BR(K).
        assert_eq!(matrix.weight(4, 6), 1.0);
        assert_eq!(matrix.weight(4, 4), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(5, 7), 1.0);
        assert_eq!(matrix.weight(5, 5), std::f64::consts::FRAC_1_SQRT_2);
    }
    #[test]
    fn side_surrounds_fold_onto_rears_for_back_target() {
        let matrix = layout_matrix(
            ChannelMask::SURROUND_5_1,
            ChannelMask::SURROUND_5_1_BACK,
            false,
        )
        .unwrap();
        assert_eq!(matrix.weight(4, 4), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(5, 5), std::f64::consts::FRAC_1_SQRT_2);
        let seven = layout_matrix(
            ChannelMask::SURROUND_7_1,
            ChannelMask::SURROUND_5_1_BACK,
            false,
        )
        .unwrap();
        assert_eq!(seven.weight(4, 4), 1.0);
        assert_eq!(seven.weight(4, 6), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(seven.weight(4, 7), 0.0);
        assert_eq!(seven.weight(5, 5), 1.0);
        assert_eq!(seven.weight(5, 7), std::f64::consts::FRAC_1_SQRT_2);
    }
    #[test]
    fn seven_one_to_stereo_folds_all_surrounds() {
        let matrix = layout_matrix(ChannelMask::SURROUND_7_1, ChannelMask::STEREO, false).unwrap();
        let k = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(matrix.weight(0, 0), 1.0);
        assert_eq!(matrix.weight(0, 2), k);
        assert_eq!(matrix.weight(0, 4), k);
        assert_eq!(matrix.weight(0, 6), k);
        assert_eq!(matrix.weight(0, 3), 0.0);
        assert_eq!(matrix.weight(1, 5), k);
        assert_eq!(matrix.weight(1, 7), k);
    }
    #[test]
    fn mono_duplicates_at_unity() {
        let stereo = layout_matrix(ChannelMask::MONO, ChannelMask::STEREO, false).unwrap();
        assert_eq!(stereo.weight(0, 0), 1.0);
        assert_eq!(stereo.weight(1, 0), 1.0);
        let surround = layout_matrix(ChannelMask::MONO, ChannelMask::SURROUND_5_1, false).unwrap();
        assert_eq!(surround.weight(2, 0), 1.0);
        assert_eq!(surround.weight(0, 0), 0.0);
        assert_eq!(surround.weight(3, 0), 0.0);
    }
    #[test]
    fn mono_target_coefficients() {
        let matrix = layout_matrix(ChannelMask::SURROUND_5_1, ChannelMask::MONO, false).unwrap();
        assert_eq!(matrix.weight(0, 0), 0.5);
        assert_eq!(matrix.weight(0, 1), 0.5);
        assert_eq!(matrix.weight(0, 2), std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(matrix.weight(0, 4), std::f64::consts::FRAC_1_SQRT_2 / 2.0);
        assert_eq!(matrix.weight(0, 3), 0.0);
    }
    #[test]
    fn stereo_to_surround_keeps_sparse_channels() {
        let matrix = layout_matrix(ChannelMask::STEREO, ChannelMask::SURROUND_5_1, false).unwrap();
        assert_eq!(matrix.weight(0, 0), 1.0);
        assert_eq!(matrix.weight(1, 1), 1.0);
        for dst in 2..6 {
            assert_eq!(matrix.weight(dst, 0), 0.0);
            assert_eq!(matrix.weight(dst, 1), 0.0);
        }
    }
    #[test]
    fn identity_matrix() {
        let matrix =
            layout_matrix(ChannelMask::SURROUND_5_1, ChannelMask::SURROUND_5_1, false).unwrap();
        for dst in 0..6 {
            for src in 0..6 {
                assert_eq!(matrix.weight(dst, src), if dst == src { 1.0 } else { 0.0 });
            }
        }
    }
}
