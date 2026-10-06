//! Rec.2100 transfer functions. One linear working unit is 203 cd/m².
use serde::{Deserialize, Serialize};

pub const HDR_REFERENCE_WHITE_NITS: f64 = 203.0;
pub const HDR_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HdrTransfer {
    Pq,
    Hlg,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HdrSettings {
    pub transfer: HdrTransfer,
}
impl HdrTransfer {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Pq => "smpte2084",
            Self::Hlg => "arib-std-b67",
        }
    }
    pub fn decode(self, encoded: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Pq => encoded.map(|v| {
                let p = v.powf(32.0 / 2523.0);
                let l = ((p - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p))
                    .powf(16384.0 / 2610.0);
                l * 10000.0 / HDR_REFERENCE_WHITE_NITS
            }),
            Self::Hlg => {
                let a = 0.17883277_f64;
                let b = 1.0 - 4.0 * a;
                let c = 0.5 - a * (4.0 * a).ln();
                let scene = encoded.map(|v| {
                    if v <= 0.5 {
                        v * v / 3.0
                    } else {
                        (((v - c) / a).exp() + b) / 12.0
                    }
                });
                let gain = luminance(scene).powf(0.2) * 1000.0 / HDR_REFERENCE_WHITE_NITS;
                scene.map(|v| v * gain)
            }
        }
    }
    pub fn encode(self, linear: [f64; 3]) -> Option<[f64; 3]> {
        let peak = match self {
            Self::Pq => 10000.0,
            Self::Hlg => 1000.0,
        } / HDR_REFERENCE_WHITE_NITS;
        if linear
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || *v > peak)
        {
            return None;
        }
        let encoded = match self {
            Self::Pq => linear.map(|v| {
                let p = (v * HDR_REFERENCE_WHITE_NITS / 10000.0).powf(2610.0 / 16384.0);
                ((3424.0 / 4096.0 + 2413.0 / 128.0 * p) / (1.0 + 2392.0 / 128.0 * p))
                    .powf(2523.0 / 32.0)
            }),
            Self::Hlg => {
                let display = linear.map(|v| v * HDR_REFERENCE_WHITE_NITS / 1000.0);
                let y = luminance(display);
                let gain = if y == 0.0 {
                    1.0
                } else {
                    y.powf(1.0 / 1.2).powf(0.2)
                };
                let a = 0.17883277_f64;
                let b = 1.0 - 4.0 * a;
                let c = 0.5 - a * (4.0 * a).ln();
                display.map(|v| {
                    let s = v / gain;
                    if s <= 1.0 / 12.0 {
                        (3.0 * s).sqrt()
                    } else {
                        a * (12.0 * s - b).ln() + c
                    }
                })
            }
        };
        // Only floating-point roundoff at a transfer endpoint is tolerated.
        // A physically valid display RGB can still be outside HLG signal gamut.
        if encoded
            .iter()
            .any(|v| !v.is_finite() || *v < -1e-12 || *v > 1.0 + 1e-12)
        {
            None
        } else {
            Some(encoded.map(|v| v.clamp(0.0, 1.0)))
        }
    }
}
fn luminance(rgb: [f64; 3]) -> f64 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn white_and_roundtrip() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            for rgb in [[0.0; 3], [1.0; 3], [0.25, 1.2, 3.0]] {
                let encoded = transfer.encode(rgb).unwrap();
                let decoded = transfer.decode(encoded);
                for i in 0..3 {
                    assert!((rgb[i] - decoded[i]).abs() < 1e-6);
                }
            }
            assert!(transfer.encode([-0.01, 0.0, 0.0]).is_none());
            assert!(transfer.encode([100.0; 3]).is_none());
        }
        assert!((HdrTransfer::Pq.encode([1.0; 3]).unwrap()[0] - 0.58068888).abs() < 1e-6);
        assert!((HdrTransfer::Hlg.decode([0.75; 3])[0] - 1.0).abs() < 0.001);
        assert!(
            HdrTransfer::Hlg
                .encode([0.0, 0.0, 1000.0 / 203.0])
                .is_none()
        );
        assert!(HdrTransfer::Pq.encode([0.0, 0.0, 1000.0 / 203.0]).is_some());
        let representable = HdrTransfer::Hlg.decode([0.0, 0.0, 1.0]);
        assert!((HdrTransfer::Hlg.encode(representable).unwrap()[2] - 1.0).abs() < 1e-12);
    }
}

/// Explicit SDR view conversion, after compositing. HDR working values stay untouched.
pub fn hdr_to_sdr_linear(pixel: [f32; 4]) -> [f32; 4] {
    let alpha = f64::from(pixel[3]);
    if alpha == 0.0 {
        return [0.0; 4];
    }
    let rgb = [0, 1, 2].map(|i| f64::from(pixel[i]) / alpha);
    let rgb = [
        1.660491 * rgb[0] - 0.587641 * rgb[1] - 0.072850 * rgb[2],
        -0.124550 * rgb[0] + 1.132900 * rgb[1] - 0.008349 * rgb[2],
        -0.018151 * rgb[0] - 0.100579 * rgb[1] + 1.118730 * rgb[2],
    ];
    let mapped = rgb.map(|v| {
        let v = v.max(0.0);
        v / (1.0 + v)
    });
    [
        (mapped[0] * alpha) as f32,
        (mapped[1] * alpha) as f32,
        (mapped[2] * alpha) as f32,
        pixel[3],
    ]
}
pub(crate) fn hdr_sdr_display(linear: &[[f32; 4]]) -> Vec<[f32; 4]> {
    linear
        .iter()
        .copied()
        .map(|p| {
            let p = hdr_to_sdr_linear(p);
            let a = p[3];
            let rgb = [0, 1, 2].map(|i| {
                if a == 0.0 {
                    0.0
                } else {
                    let v = p[i] / a;
                    if v <= 0.0031308 {
                        12.92 * v
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                }
            });
            [rgb[0], rgb[1], rgb[2], a]
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HdrMetadata {
    pub transfer: HdrTransfer,
    pub reference_white_nits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hlg_peak_nits: Option<u32>,
}
impl From<HdrSettings> for HdrMetadata {
    fn from(settings: HdrSettings) -> Self {
        Self {
            transfer: settings.transfer,
            reference_white_nits: 203,
            hlg_peak_nits: (settings.transfer == HdrTransfer::Hlg).then_some(1000),
        }
    }
}
