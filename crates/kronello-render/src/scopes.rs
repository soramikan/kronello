//! COLOR-004 deterministic scope inspection on the composited working-space
//! frame (ADR-0113). All scopes are integer bin histograms over straight
//! (unpremultiplied) RGB derived from premultiplied pixels; no display
//! conversion is applied here. Every resolution is fixed by this module so a
//! query result is a pure function of `(frame, working_space)`.
use crate::RenderError;
use kronello_model::ColorSpace;
use serde::{Deserialize, Serialize};

/// Waveform/parade column resolution (horizontal sample groups).
pub const SCOPE_COLUMNS: usize = 512;
/// Waveform/parade vertical level resolution.
pub const SCOPE_LEVELS: usize = 256;
/// Vectorscope Cb/Cr plane edge resolution.
pub const SCOPE_VECTOR_SIZE: usize = 256;
/// Histogram bins per channel.
pub const SCOPE_HISTOGRAM_BINS: usize = 256;
/// Matches `kronello_gpu::color::ALPHA_EPSILON`: alpha at or below this value
/// carries no color information.
const SCOPE_ALPHA_EPSILON: f32 = 1.0 / 65536.0;

/// A `columns x levels` integer histogram; `bins[row * columns + column]`,
/// row 0 is the lowest level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeGrid {
    pub columns: u32,
    pub levels: u32,
    pub bins: Vec<u32>,
}
/// RGB parade: three `columns x levels` histograms sharing one grid shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeParade {
    pub columns: u32,
    pub levels: u32,
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
}
/// Per-channel plus luma histograms on a shared 0..levels domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeHistogram {
    pub levels: u32,
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub luma: Vec<u32>,
}
/// `size x size` Cb/Cr histogram; `bins[cr * size + cb]`, centered on neutral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeVectorscope {
    pub size: u32,
    pub bins: Vec<u32>,
}
/// All four scope families for one fixed frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeData {
    pub waveform: ScopeGrid,
    pub vectorscope: ScopeVectorscope,
    pub histogram: ScopeHistogram,
    pub parade: ScopeParade,
}
/// BT.601 luma coefficients per working space; the scope contract documents
/// that Cb/Cr use the matching normalized primaries.
fn luma_coefficients(space: ColorSpace) -> Result<[f32; 3], RenderError> {
    match space {
        ColorSpace::LinearRec709 => Ok([0.2126, 0.7152, 0.0722]),
        ColorSpace::LinearRec2020 => Ok([0.2627, 0.6780, 0.0593]),
        ColorSpace::Srgb => Err(RenderError::InvalidInput(
            "scopes require a linear working space".into(),
        )),
    }
}
/// Level-domain bin index for a straight channel value: clamp to [0,1] so
/// HDR and negative working values land in the edge bins deterministically.
fn level_bin(value: f32, levels: usize) -> usize {
    ((value.clamp(0.0, 1.0) * levels as f32) as usize).min(levels - 1)
}
/// Column index for a pixel x on a `width`-pixel frame.
fn column_bin(x: u32, width: u32, columns: usize) -> usize {
    ((x as usize * columns) / width as usize).min(columns - 1)
}
/// Compute all scope families over the composited working-space frame.
/// `pixels` is row-major premultiplied `linear` data of `size` dimensions.
/// Straight RGB uses `c / a` for alpha above the shared epsilon and zero
/// otherwise; out-of-range straight values clamp into edge bins.
pub fn compute_scopes(
    pixels: &[[f32; 4]],
    size: [u32; 2],
    working_space: ColorSpace,
) -> Result<ScopeData, RenderError> {
    let [width, height] = size;
    if width == 0 || height == 0 || pixels.len() != width as usize * height as usize {
        return Err(RenderError::InvalidInput(
            "scope frame dimensions do not match pixel data".into(),
        ));
    }
    let [kr, kg, kb] = luma_coefficients(working_space)?;
    let mut waveform = vec![0u32; SCOPE_COLUMNS * SCOPE_LEVELS];
    let mut vectorscope = vec![0u32; SCOPE_VECTOR_SIZE * SCOPE_VECTOR_SIZE];
    let mut hist: [Vec<u32>; 4] = std::array::from_fn(|_| vec![0u32; SCOPE_HISTOGRAM_BINS]);
    let mut parade: [Vec<u32>; 3] =
        std::array::from_fn(|_| vec![0u32; SCOPE_COLUMNS * SCOPE_LEVELS]);
    for (index, p) in pixels.iter().enumerate() {
        let x = index as u32 % width;
        let straight = if p[3] > SCOPE_ALPHA_EPSILON {
            [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
        } else {
            [0.0; 3]
        };
        let luma = straight[0] * kr + straight[1] * kg + straight[2] * kb;
        // Neutral chroma is 0.5; Cb/Cr are normalized to [-0.5, 0.5] and
        // shifted so the vectorscope is centered, matching BT.601 scaling.
        let cb = (straight[2] - luma) / (2.0 * (1.0 - kb)) + 0.5;
        let cr = (straight[0] - luma) / (2.0 * (1.0 - kr)) + 0.5;
        let column = column_bin(x, width, SCOPE_COLUMNS);
        waveform[column + level_bin(luma, SCOPE_LEVELS) * SCOPE_COLUMNS] += 1;
        for channel in 0..3 {
            parade[channel][column + level_bin(straight[channel], SCOPE_LEVELS) * SCOPE_COLUMNS] +=
                1;
            hist[channel][level_bin(straight[channel], SCOPE_HISTOGRAM_BINS)] += 1;
        }
        hist[3][level_bin(luma, SCOPE_HISTOGRAM_BINS)] += 1;
        vectorscope[level_bin(cb, SCOPE_VECTOR_SIZE)
            + level_bin(cr, SCOPE_VECTOR_SIZE) * SCOPE_VECTOR_SIZE] += 1;
    }
    Ok(ScopeData {
        waveform: ScopeGrid {
            columns: SCOPE_COLUMNS as u32,
            levels: SCOPE_LEVELS as u32,
            bins: waveform,
        },
        vectorscope: ScopeVectorscope {
            size: SCOPE_VECTOR_SIZE as u32,
            bins: vectorscope,
        },
        histogram: ScopeHistogram {
            levels: SCOPE_HISTOGRAM_BINS as u32,
            red: std::mem::take(&mut hist[0]),
            green: std::mem::take(&mut hist[1]),
            blue: std::mem::take(&mut hist[2]),
            luma: std::mem::take(&mut hist[3]),
        },
        parade: ScopeParade {
            columns: SCOPE_COLUMNS as u32,
            levels: SCOPE_LEVELS as u32,
            red: std::mem::take(&mut parade[0]),
            green: std::mem::take(&mut parade[1]),
            blue: std::mem::take(&mut parade[2]),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_are_deterministic_and_count_every_pixel() {
        let size = [8, 4];
        let pixels: Vec<[f32; 4]> = (0..32)
            .map(|i| {
                let v = i as f32 / 31.0;
                [v, 0.5, 1.0 - v, 1.0]
            })
            .collect();
        let a = compute_scopes(&pixels, size, ColorSpace::LinearRec709).unwrap();
        let b = compute_scopes(&pixels, size, ColorSpace::LinearRec709).unwrap();
        assert_eq!(a, b);
        for bins in [
            &a.waveform.bins,
            &a.vectorscope.bins,
            &a.parade.red,
            &a.parade.green,
            &a.parade.blue,
        ] {
            assert_eq!(bins.iter().sum::<u32>(), 32);
        }
        for bins in [
            &a.histogram.red,
            &a.histogram.green,
            &a.histogram.blue,
            &a.histogram.luma,
        ] {
            assert_eq!(bins.iter().sum::<u32>(), 32);
        }
    }
    #[test]
    fn zero_alpha_and_extreme_values_land_in_edge_bins() {
        let size = [4, 1];
        let pixels = [
            [0.0; 4],
            [-2.0, 4.0, 1.0, 1.0],
            [0.5, 0.25, 0.75, 0.0],
            [0.1; 4],
        ];
        let data = compute_scopes(&pixels, size, ColorSpace::LinearRec709).unwrap();
        assert_eq!(data.histogram.red.iter().sum::<u32>(), 4);
        // Pixel 1: straight [-2,4,1] clamps to red bin 0 and 4.0->255 edge.
        assert!(data.histogram.red[0] >= 2);
        assert!(data.histogram.red[SCOPE_HISTOGRAM_BINS - 1] >= 1);
    }
    #[test]
    fn dimension_mismatch_and_encoded_working_space_are_typed() {
        assert!(compute_scopes(&[[0.0; 4]; 3], [2, 2], ColorSpace::LinearRec709).is_err());
        assert!(compute_scopes(&[[0.0; 4]; 4], [2, 2], ColorSpace::Srgb).is_err());
    }
}
