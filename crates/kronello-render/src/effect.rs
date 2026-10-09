//! Output-space effect contract, kernel and backward region requests.
use crate::RenderError;
use kronello_eval::Affine2;
use kronello_model::{Color, ResolvedEffect};
use serde::{Deserialize, Serialize};

pub const AFFINE_EFFECT_KERNEL_VERSION: &str = "fx002-affine-ellipse-lattice-rne16-v2";
pub const AFFINE_KERNEL_MAX_CANDIDATES: usize = 65_536;
pub const EFFECT_KERNEL_VERSION: &str = "fx001-separable-gaussian-transparent-rne16-v1";
/// Half-open rectangle on the original requested output pixel lattice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}
impl PixelBounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
    pub fn expand(self, halo: [f64; 2]) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i] - halo[i]),
            max: std::array::from_fn(|i| self.max[i] + halo[i]),
        }
    }
    pub fn translate(self, offset: [f64; 2]) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i] + offset[i]),
            max: std::array::from_fn(|i| self.max[i] + offset[i]),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NodeBounds {
    pub ink_bounds: Option<PixelBounds>,
    pub visual_bounds: Option<PixelBounds>,
}
/// Sigma/offset in output pixels. Colors retain straight tags until execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PixelEffect {
    /// Symmetric covariance [xx, xy, yy] in output pixels squared.
    AffineGaussianBlur {
        covariance: [f64; 3],
    },
    AffineDropShadow {
        covariance: [f64; 3],
        offset: [f32; 2],
        color: Color,
        opacity: f32,
    },
    GaussianBlur {
        sigma: [f32; 2],
    },
    DropShadow {
        sigma: [f32; 2],
        offset: [f32; 2],
        color: Color,
        opacity: f32,
    },
    /// COLOR-002 pointwise correction in premultiplied working space:
    /// rgb = rgb * 2^exposure + offset. Alpha is preserved; HDR and
    /// negative values are never clamped (ADR-0108).
    ColorExposure {
        exposure: f32,
        offset: f32,
    },
    /// in_white > in_black and gamma > 0 are enforced at resolution and
    /// re-checked in validate.
    ColorLevels {
        in_black: f32,
        in_white: f32,
        gamma: f32,
        out_black: f32,
        out_white: f32,
    },
    /// Monotone-cubic control points, x strictly increasing within [0,1].
    ColorCurves {
        points: Vec<[f32; 2]>,
    },
    /// Working-space HSL: hue_shift in degrees, saturation multiplier,
    /// additive lightness.
    ColorHsl {
        hue_shift: f32,
        saturation: f32,
        lightness: f32,
    },
    /// FX-005 chroma key (ADR-0115): alpha becomes the keyed Cb/Cr matte,
    /// eroded by `edge_shrink` output pixels then Gaussian-feathered by
    /// `edge_feather`. `key_color` retains its straight tag until execution.
    ChromaKey {
        key_color: Color,
        similarity: f32,
        edge_shrink: [f32; 2],
        edge_feather: [f32; 2],
        spill: f32,
    },
    /// FX-005 luma key (ADR-0115): straight working-space luminance distance
    /// to `key_luma`, normalized by `tolerance`, drives the alpha matte.
    LumaKey {
        key_luma: f32,
        tolerance: f32,
        edge_shrink: [f32; 2],
        edge_feather: [f32; 2],
    },
    /// FX-006 glow (ADR-0115): straight luminance above `threshold` is
    /// extracted, blurred by a Gaussian of sigma `radius` in output pixels,
    /// and added back at `intensity` strength.
    Glow {
        threshold: f32,
        radius: [f32; 2],
        intensity: f32,
    },
    /// FX-006 unsharp mask (ADR-0115): out = source + amount * (source -
    /// blur(source)) on all four premultiplied channels; alpha clamps to [0,1].
    Sharpen {
        amount: f32,
        radius: [f32; 2],
    },
    /// FX-006 vignette (ADR-0115): multiplies premultiplied RGB by a
    /// smoothstep corner falloff; alpha is preserved.
    Vignette {
        amount: f32,
        midpoint: f32,
        feather: f32,
        roundness: f32,
    },
    /// FX-006 corner pin (ADR-0115): inverse-mapped quad warp. `pins` are the
    /// destination corners in output-pixel edge coordinates, ordered
    /// top-left, top-right, bottom-right, bottom-left. `source` is the input
    /// content's bounds rectangle on the same lattice; the DAG builder fills
    /// it once input bounds are known, so a directly constructed value must
    /// carry it explicitly.
    CornerPin {
        pins: [[f32; 2]; 4],
        source: Option<PixelBounds>,
    },
    /// COLOR-003 pointwise `.cube` LUT in working space (ADR-0113). The
    /// lattice was content-verified before reaching the DAG: `size` is
    /// document-bounded and `data` keeps red-fastest row order. Alpha is
    /// preserved; domain-normalized samples clamp to lattice endpoints.
    ColorLut {
        lut: kronello_model::CubeLut,
        intensity: f32,
    },
    /// TRACK-002 (ADR-0122): inverse-warp stabilization in the source-pixel
    /// frame. `frame` maps output-lattice pixels onto corrected source-lattice
    /// positions (the tracked inverse); `unmap` maps them back to input raster
    /// positions; `size` is the source frame extent in pixels. `border` is
    /// the uncovered-region policy with `fill` for `Fill`.
    Stabilize {
        frame: [[f32; 3]; 2],
        unmap: [[f32; 3]; 2],
        size: [f32; 2],
        border: kronello_model::StabilizeBorder,
        fill: Color,
        sampling: kronello_model::StabilizeSampling,
    },
    /// FX-008 (ADR-0137): deterministic cell noise on the output lattice.
    /// `size` is the noise-cell edge in output pixels; `seed` is the signed
    /// 31-bit authored seed folded with the scene's rational time by the DAG
    /// builder. The noise perturbs straight working RGB by
    /// `amount * (noise - 0.5)`; alpha is preserved.
    Grain {
        amount: f32,
        size: f32,
        monochrome: bool,
        seed: i64,
    },
    /// FX-008: pixelation over axis-aligned `block_size`-pixel blocks on the
    /// output lattice. `Center` samples the block's center texel, `Edge` the
    /// block's top-left texel.
    Mosaic {
        block_size: f32,
        basis: kronello_model::MosaicBasis,
    },
    /// FX-008: inversion. `Rgb`/`Red`/`Green`/`Blue` invert the selected
    /// straight working channels and keep alpha; `Alpha` inverts alpha and
    /// keeps straight RGB.
    Invert {
        channel: kronello_model::InvertChannel,
    },
    /// FX-008: premultiplied 4x4 channel matrix, row-major (row = output
    /// channel). Results outside the half-float premultiplied contract are
    /// caught by surface validation, never clamped.
    ChannelMixer {
        matrix: [[f32; 4]; 4],
    },
    /// FX-008: straight luminance mapped onto `map_black`..`map_white`,
    /// blended by `amount` against the straight source; alpha is preserved.
    /// Colors retain straight tags until execution.
    Tint {
        map_black: Color,
        map_white: Color,
        amount: f32,
    },
    /// FX-008: uniform box blur along `direction` (unit vector on the output
    /// lattice) spanning `length` output pixels, bilinear midpoint taps.
    DirectionalBlur {
        direction: [f32; 2],
        length: f32,
    },
    /// FX-008: deterministic spin/zoom blur around `center` in output
    /// pixels. `Spin` rotates samples over [-amount/2, +amount/2] degrees;
    /// `Zoom` scales toward `center` over (1-amount, 1]. Both use the fixed
    /// [`FX008_RADIAL_TAPS`]-tap midpoint kernel.
    RadialBlur {
        mode: kronello_model::RadialBlurMode,
        amount: f32,
        center: [f32; 2],
    },
    /// FX-008: displacement warp driven by a second input surface
    /// (`DagNode::EffectMap`). `displacement` column `c` is the output-pixel
    /// sample offset contributed by channel `c` at unit signed value; a map
    /// channel value `v` in [0,1] contributes `2v - 1`.
    Displace {
        channel_x: kronello_model::DisplaceChannel,
        channel_y: kronello_model::DisplaceChannel,
        displacement: [[f32; 2]; 2],
    },
    /// FX-008: source-free procedural raster covering the whole execution
    /// surface (`DagNode::Generate`). Points are output pixels; colors retain
    /// straight tags until execution.
    Generate {
        generator: kronello_model::GenerateKind,
        color_a: Color,
        color_b: Color,
        point_a: [f32; 2],
        point_b: [f32; 2],
        cell_size: f32,
        line_width: f32,
    },
}
/// Kernel tag shared by all COLOR-002 v1 pointwise passes.
pub const COLOR002_KERNEL_VERSION: &str = "color002-pointwise-f16-v1";
/// Kernel tag shared by all FX-005/FX-006 v1 passes (ADR-0115).
pub const STANDARD_KERNEL_VERSION: &str = "fx005006-keying-standard-f16-v1";
/// Maximum corner-pin / edge-adjust kernel support, matching the Gaussian
/// budget so CPU and GPU rejects stay identical.
const STANDARD_KERNEL_MAX_RADIUS: f32 = 1024.0;
/// Kernel tag for the COLOR-003 tetrahedral LUT pass.
pub const COLOR003_KERNEL_VERSION: &str = "color003-tetrahedral-f16-v1";
/// Kernel tag for the TRACK-002 stabilize warp pass.
pub const STABILIZE_KERNEL_VERSION: &str = "track002-stabilize-warp-f16-v1";
/// Kernel tag shared by all FX-008 v1 passes (ADR-0137).
pub const FX008_KERNEL_VERSION: &str = "fx008-standard-synthesis-f16-v1";
/// Fixed midpoint tap count for the FX-008 radial blur kernel, identical on
/// the CPU reference and WGSL paths.
pub const FX008_RADIAL_TAPS: usize = 64;
/// Maximum FX-008 sampling extent in output pixels, matching the shared
/// Gaussian-class budget: mosaic block edge, grain cell edge, and the
/// directional blur's half-length.
const FX008_KERNEL_MAX_RADIUS: f32 = 1024.0;
/// Deterministic 32-bit integer hash used by `kronello.grain` on both the
/// CPU reference and the WGSL kernel (WGSL has no u64). `cell` is the
/// signed 32-bit clamped lattice coordinate plus the channel index;
/// returns [0, 1).
pub fn grain_noise(cell: [i32; 3], seed: i64) -> f32 {
    // lowbias32 (Chris Wellons): three multiply-xorshift rounds, portable
    // word-for-word to WGSL u32 arithmetic.
    let mix = |mut h: u32| {
        h ^= h >> 16;
        h = h.wrapping_mul(0x7feb_352d);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846c_a68b);
        h ^= h >> 16;
        h
    };
    let mut h = (seed as u64 ^ ((seed >> 32) as u64)) as u32;
    for v in cell {
        h = mix(h ^ v as u32);
    }
    mix(h) as f32 / 4_294_967_296.0
}
/// Cell coordinate clamp shared by the CPU/WGSL grain lattice so signed
/// 32-bit hashing never hits an out-of-range conversion.
pub fn grain_cell(position: f64, size: f64) -> i32 {
    (position / size)
        .floor()
        .clamp(-2_147_000_000.0, 2_147_000_000.0) as i32
}
/// FX-008: fold the scene's rational sample time into the authored seed so
/// the grain field is temporal but deterministic.
pub fn fold_grain_seed(seed: i64, time: kronello_time::Time) -> i64 {
    let n = time.numerator();
    let d = time.denominator();
    seed ^ (n.wrapping_mul(6_364_136_223_846_793_005) ^ d)
}
/// Inverse of a row-major 2x3 affine in f64; `None` when degenerate or
/// non-finite. Used only by stabilize bound estimation.
fn stabilize_inverse(m: &[[f32; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    let (a, b, tx) = (f64::from(m[0][0]), f64::from(m[0][1]), f64::from(m[0][2]));
    let (d, e, ty) = (f64::from(m[1][0]), f64::from(m[1][1]), f64::from(m[1][2]));
    let det = a * e - b * d;
    if !det.is_finite() || det.abs() <= 1e-12 {
        return None;
    }
    let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
    let out = [
        [ia, ib, -(ia * tx + ib * ty)],
        [id, ie, -(id * tx + ie * ty)],
    ];
    if out.iter().flatten().all(|v| v.is_finite()) {
        Some(out)
    } else {
        None
    }
}
impl PixelEffect {
    /// COLOR-002 corrections are pointwise: no kernel, no neighborhood input.
    pub fn is_pointwise_color(&self) -> bool {
        matches!(
            self,
            Self::ColorExposure { .. }
                | Self::ColorLevels { .. }
                | Self::ColorCurves { .. }
                | Self::ColorHsl { .. }
                | Self::ColorLut { .. }
        )
    }
    /// FX-005/FX-006 keying and standard effects (ADR-0115). They carry their
    /// own multi-pass execution path rather than the Gaussian/shadow one.
    pub fn is_standard(&self) -> bool {
        matches!(
            self,
            Self::ChromaKey { .. }
                | Self::LumaKey { .. }
                | Self::Glow { .. }
                | Self::Sharpen { .. }
                | Self::Vignette { .. }
                | Self::CornerPin { .. }
                | Self::Stabilize { .. }
        )
    }
    /// FX-008 remaining standard effects (ADR-0137). Grain through
    /// `DirectionalBlur` are single-input; `Displace` binds a second input
    /// surface and `Generate` is source-free.
    pub fn is_fx008(&self) -> bool {
        matches!(
            self,
            Self::Grain { .. }
                | Self::Mosaic { .. }
                | Self::Invert { .. }
                | Self::ChannelMixer { .. }
                | Self::Tint { .. }
                | Self::DirectionalBlur { .. }
                | Self::RadialBlur { .. }
                | Self::Displace { .. }
                | Self::Generate { .. }
        )
    }
    /// Scale-only lowering retains the historic meaning: the map is a pure
    /// diagonal transform with no translation. `from_design_mapped` covers
    /// effects with absolute position parameters.
    pub fn from_design(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
        Self::from_design_mapped(
            effect,
            Affine2([[scale[0], 0.0, 0.0], [0.0, scale[1], 0.0]]),
        )
    }
    /// Lower a design-space resolved effect through the region's
    /// design-to-output-pixel map. Positions (corner pins) use the full map;
    /// lengths use the diagonal scale only.
    pub fn from_design_mapped(
        effect: &ResolvedEffect,
        design_to_pixel: Affine2,
    ) -> Result<Self, RenderError> {
        let scale = [design_to_pixel.0[0][0], design_to_pixel.0[1][1]];
        if scale.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(RenderError::InvalidInput(
                "invalid effect output scale".into(),
            ));
        }
        // FX-005/FX-006 variants: resolution-checked in the model crate; the
        // pixel form converts precision and maps length/position parameters.
        let standard = match effect {
            ResolvedEffect::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => Some(Self::ChromaKey {
                key_color: *key_color,
                similarity: *similarity as f32,
                edge_shrink: [0, 1].map(|i| (*edge_shrink * scale[i]) as f32),
                edge_feather: [0, 1].map(|i| (*edge_feather * scale[i]) as f32),
                spill: *spill as f32,
            }),
            ResolvedEffect::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => Some(Self::LumaKey {
                key_luma: *key_luma as f32,
                tolerance: *tolerance as f32,
                edge_shrink: [0, 1].map(|i| (*edge_shrink * scale[i]) as f32),
                edge_feather: [0, 1].map(|i| (*edge_feather * scale[i]) as f32),
            }),
            ResolvedEffect::Glow {
                threshold,
                radius,
                intensity,
            } => Some(Self::Glow {
                threshold: *threshold as f32,
                radius: [0, 1].map(|i| (*radius * scale[i]) as f32),
                intensity: *intensity as f32,
            }),
            ResolvedEffect::Sharpen { amount, radius } => Some(Self::Sharpen {
                amount: *amount as f32,
                radius: [0, 1].map(|i| (*radius * scale[i]) as f32),
            }),
            ResolvedEffect::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => Some(Self::Vignette {
                amount: *amount as f32,
                midpoint: *midpoint as f32,
                feather: *feather as f32,
                roundness: *roundness as f32,
            }),
            ResolvedEffect::CornerPin { corners } => {
                let pins = corners.map(|p| design_to_pixel.transform_point(p));
                if pins
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Err(RenderError::InvalidInput(
                        "corner pin outside finite output range".into(),
                    ));
                }
                Some(Self::CornerPin {
                    pins: pins.map(|p| p.map(|v| v as f32)),
                    source: None,
                })
            }
            // TRACK-002 builds from the scene-resolved tracking transform and
            // the node's video extent, not from the generic design map.
            ResolvedEffect::Stabilize { .. } => {
                return Err(RenderError::InvalidInput(
                    "stabilize effects require scene-resolved tracking data".into(),
                ));
            }
            _ => None,
        };
        if let Some(result) = standard {
            result.validate()?;
            return Ok(result);
        }
        // FX-008 (ADR-0137): map_effect already rotated/composed the node
        // transform into these resolved values; the region's design-to-pixel
        // map now produces output-pixel geometry.
        let linear = [
            [design_to_pixel.0[0][0], design_to_pixel.0[0][1]],
            [design_to_pixel.0[1][0], design_to_pixel.0[1][1]],
        ];
        let fx008 = match effect {
            ResolvedEffect::Grain {
                amount,
                size,
                monochrome,
                seed,
            } => Some(Self::Grain {
                amount: *amount as f32,
                size: (*size * scale[0]) as f32,
                monochrome: *monochrome,
                seed: *seed,
            }),
            ResolvedEffect::Mosaic { block_size, basis } => Some(Self::Mosaic {
                block_size: (*block_size * scale[0]) as f32,
                basis: *basis,
            }),
            ResolvedEffect::Invert { channel } => Some(Self::Invert { channel: *channel }),
            ResolvedEffect::ChannelMixer { matrix } => Some(Self::ChannelMixer {
                matrix: matrix.map(|row| row.map(|v| v as f32)),
            }),
            ResolvedEffect::Tint {
                map_black,
                map_white,
                amount,
            } => Some(Self::Tint {
                map_black: *map_black,
                map_white: *map_white,
                amount: *amount as f32,
            }),
            ResolvedEffect::DirectionalBlur {
                angle_degrees,
                length,
            } => {
                // The mapped segment vector through the full linear part, so
                // non-uniform output scale keeps the true blur direction.
                let rad = angle_degrees.to_radians();
                let segment = [rad.cos() * *length, rad.sin() * *length];
                let v = [0, 1].map(|r| linear[r][0] * segment[0] + linear[r][1] * segment[1]);
                let len = v[0].hypot(v[1]);
                let direction = if len > 0.0 {
                    [(v[0] / len) as f32, (v[1] / len) as f32]
                } else {
                    [1.0, 0.0]
                };
                Some(Self::DirectionalBlur {
                    direction,
                    length: len as f32,
                })
            }
            ResolvedEffect::RadialBlur {
                mode,
                amount,
                center,
            } => Some(Self::RadialBlur {
                mode: *mode,
                amount: *amount as f32,
                center: design_to_pixel.transform_point(*center).map(|v| v as f32),
            }),
            ResolvedEffect::Displace {
                channel_x,
                channel_y,
                displacement,
            } => {
                // Rows scale by the output map's linear part: each column's
                // authored design-px offset becomes an output-pixel offset.
                let mapped = [0, 1].map(|r| {
                    [0, 1].map(|c| {
                        (linear[r][0] * displacement[0][c] + linear[r][1] * displacement[1][c])
                            as f32
                    })
                });
                Some(Self::Displace {
                    channel_x: *channel_x,
                    channel_y: *channel_y,
                    displacement: mapped,
                })
            }
            ResolvedEffect::Generate {
                generator,
                color_a,
                color_b,
                point_a,
                point_b,
                cell_size,
                line_width,
            } => Some(Self::Generate {
                generator: *generator,
                color_a: *color_a,
                color_b: *color_b,
                point_a: design_to_pixel.transform_point(*point_a).map(|v| v as f32),
                point_b: design_to_pixel.transform_point(*point_b).map(|v| v as f32),
                cell_size: (*cell_size * scale[0]) as f32,
                line_width: (*line_width * scale[0]) as f32,
            }),
            _ => None,
        };
        if let Some(result) = fx008 {
            result.validate()?;
            return Ok(result);
        }
        Self::from_design_spatial(effect, scale)
    }
    /// TRACK-002 (ADR-0122): build the pixel warp from the scene-resolved
    /// output→corrected-source inverse (`frame`) and `unmap`, the map from
    /// output pixels back onto the drawn input raster. `size` is the authored
    /// video extent in pixels.
    pub fn stabilize(
        frame: Affine2,
        unmap: Affine2,
        size: [f64; 2],
        border: kronello_model::StabilizeBorder,
        fill: Color,
        sampling: kronello_model::StabilizeSampling,
    ) -> Result<Self, RenderError> {
        let cast = |m: [[f64; 3]; 2]| m.map(|r| r.map(|v| v as f32));
        let result = Self::Stabilize {
            frame: cast(frame.0),
            unmap: cast(unmap.0),
            size: size.map(|v| v as f32),
            border,
            fill,
            sampling,
        };
        result.validate()?;
        Ok(result)
    }
    fn from_design_spatial(effect: &ResolvedEffect, scale: [f64; 2]) -> Result<Self, RenderError> {
        // COLOR-003 carries an asset reference, not lattice bytes; the DAG
        // builder resolves it against the snapshot luts input.
        if let ResolvedEffect::ColorLut { .. } = effect {
            return Err(RenderError::InvalidInput(
                "color lut effects require scene-resolved lattice data".into(),
            ));
        }
        // TRACK-002 likewise requires its scene-resolved inverse transform.
        if let ResolvedEffect::Stabilize { .. } = effect {
            return Err(RenderError::InvalidInput(
                "stabilize effects require scene-resolved tracking data".into(),
            ));
        }
        // COLOR-002 parameters are resolution-checked in the model crate; the
        // pixel-space form only converts precision since nothing is spatial.
        let pointwise = match effect {
            ResolvedEffect::ColorExposure { exposure, offset } => Some(Self::ColorExposure {
                exposure: *exposure as f32,
                offset: *offset as f32,
            }),
            ResolvedEffect::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => Some(Self::ColorLevels {
                in_black: *in_black as f32,
                in_white: *in_white as f32,
                gamma: *gamma as f32,
                out_black: *out_black as f32,
                out_white: *out_white as f32,
            }),
            ResolvedEffect::ColorCurves { curve } => Some(Self::ColorCurves {
                points: curve.iter().map(|p| p.map(|v| v as f32)).collect(),
            }),
            ResolvedEffect::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => Some(Self::ColorHsl {
                hue_shift: *hue_shift as f32,
                saturation: *saturation as f32,
                lightness: *lightness as f32,
            }),
            _ => None,
        };
        if let Some(result) = pointwise {
            result.validate()?;
            return Ok(result);
        }
        if let ResolvedEffect::AffineGaussianBlur { sigma, linear }
        | ResolvedEffect::AffineDropShadow { sigma, linear, .. } = effect
        {
            validate_affine_linear(*linear)?;
            if !sigma.is_finite()
                || *sigma < 0.0
                || scale.iter().any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err(RenderError::InvalidInput(
                    "invalid affine effect scale/sigma".into(),
                ));
            }
            let rows = [0, 1].map(|i| linear[i].map(|v| v * scale[i] * sigma));
            let covariance = [
                rows[0][0] * rows[0][0] + rows[0][1] * rows[0][1],
                rows[0][0] * rows[1][0] + rows[0][1] * rows[1][1],
                rows[1][0] * rows[1][0] + rows[1][1] * rows[1][1],
            ];
            if *sigma > 0.0 && covariance == [0.0; 3] {
                return Err(RenderError::UnsupportedFeature(
                    "affine Gaussian covariance underflow".into(),
                ));
            }
            let result = match effect {
                ResolvedEffect::AffineGaussianBlur { .. } => {
                    Self::AffineGaussianBlur { covariance }
                }
                ResolvedEffect::AffineDropShadow {
                    offset,
                    color,
                    opacity,
                    ..
                } => Self::AffineDropShadow {
                    covariance,
                    offset: [0, 1].map(|i| (offset[i] * scale[i]) as f32),
                    color: *color,
                    opacity: *opacity as f32,
                },
                _ => unreachable!(),
            };
            result.validate()?;
            return Ok(result);
        }
        let sigma = match effect {
            ResolvedEffect::GaussianBlur { sigma } | ResolvedEffect::DropShadow { sigma, .. } => {
                *sigma
            }
            _ => unreachable!(),
        };
        if !sigma.is_finite() || sigma < 0.0 || scale.iter().any(|s| !s.is_finite() || *s <= 0.0) {
            return Err(RenderError::InvalidInput(
                "invalid effect scale/sigma".into(),
            ));
        }
        let sigma = scale.map(|s| (sigma * s) as f32);
        let result = match effect {
            ResolvedEffect::GaussianBlur { .. } => Self::GaussianBlur { sigma },
            ResolvedEffect::DropShadow {
                offset,
                color,
                opacity,
                ..
            } => Self::DropShadow {
                sigma,
                offset: std::array::from_fn(|i| (offset[i] * scale[i]) as f32),
                color: *color,
                opacity: *opacity as f32,
            },
            _ => unreachable!(),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn sigma(&self) -> [f32; 2] {
        match self {
            Self::GaussianBlur { sigma } | Self::DropShadow { sigma, .. } => *sigma,
            Self::AffineGaussianBlur { covariance } | Self::AffineDropShadow { covariance, .. } => {
                [covariance[0].sqrt() as f32, covariance[2].sqrt() as f32]
            }
            _ => [0.0; 2],
        }
    }
    pub fn covariance(&self) -> Option<[f64; 3]> {
        match self {
            Self::AffineGaussianBlur { covariance } | Self::AffineDropShadow { covariance, .. } => {
                Some(*covariance)
            }
            _ => None,
        }
    }
    pub fn kernel_version(&self) -> &'static str {
        if matches!(self, Self::Stabilize { .. }) {
            STABILIZE_KERNEL_VERSION
        } else if matches!(self, Self::ColorLut { .. }) {
            COLOR003_KERNEL_VERSION
        } else if self.is_pointwise_color() {
            COLOR002_KERNEL_VERSION
        } else if self.is_standard() {
            STANDARD_KERNEL_VERSION
        } else if self.is_fx008() {
            FX008_KERNEL_VERSION
        } else if self.covariance().is_some() {
            AFFINE_EFFECT_KERNEL_VERSION
        } else {
            EFFECT_KERNEL_VERSION
        }
    }
    pub fn semantic_version(&self) -> u32 {
        if matches!(self, Self::Stabilize { .. }) {
            kronello_model::STABILIZE_VERSION
        } else if self.is_pointwise_color() {
            kronello_model::COLOR_EFFECT_VERSION
        } else if self.is_standard() {
            kronello_model::STANDARD_EFFECT_VERSION
        } else if self.is_fx008() {
            kronello_model::FX008_EFFECT_VERSION
        } else if self.covariance().is_some() {
            kronello_model::AFFINE_EFFECT_VERSION
        } else {
            kronello_model::EFFECT_VERSION
        }
    }
    pub fn shadow(&self) -> Option<([f32; 2], Color, f32)> {
        match self {
            Self::DropShadow {
                offset,
                color,
                opacity,
                ..
            }
            | Self::AffineDropShadow {
                offset,
                color,
                opacity,
                ..
            } => Some((*offset, *color, *opacity)),
            _ => None,
        }
    }
    /// Per-axis read support in output pixels: Gaussian 3-sigma plus matte
    /// erosion for keying; zero for pointwise and warp effects.
    pub fn halo(&self) -> [f64; 2] {
        if let Some(c) = self.covariance() {
            return [c[0], c[2]].map(|v| (3.0 * v.sqrt()).ceil());
        }
        // FX-008 extents are exact, not Gaussian.
        match self {
            Self::Mosaic { block_size, .. } => {
                return [f64::from(block_size.ceil()); 2];
            }
            Self::DirectionalBlur {
                direction, length, ..
            } => {
                return [0, 1].map(|i| {
                    (f64::from(direction[i].abs()) * f64::from(*length) / 2.0).ceil() + 1.0
                });
            }
            Self::Displace { displacement, .. } => {
                return [0, 1].map(|r| {
                    (f64::from(displacement[r][0].abs()) + f64::from(displacement[r][1].abs()))
                        .ceil()
                        + 1.0
                });
            }
            _ => {}
        }
        let mut halo: [f64; 2] = [0.0; 2];
        for sigma in self.kernel_sigmas() {
            for (i, s) in sigma.iter().enumerate() {
                halo[i] = halo[i].max((3.0 * f64::from(*s)).ceil());
            }
        }
        if let Self::ChromaKey { edge_shrink, .. } | Self::LumaKey { edge_shrink, .. } = self {
            for (h, s) in halo.iter_mut().zip(edge_shrink) {
                *h += f64::from(s.ceil());
            }
        }
        halo
    }
    /// Gaussian sigma equivalents for the separable kernel paths: blur sigma
    /// for blur/shadow, `radius` for glow/sharpen, `edge_feather` for keying.
    /// Warp kernels (stabilize) have no Gaussian support.
    fn kernel_sigmas(&self) -> Vec<[f32; 2]> {
        match self {
            Self::Glow { radius, .. } | Self::Sharpen { radius, .. } => vec![*radius],
            Self::ChromaKey { edge_feather, .. } | Self::LumaKey { edge_feather, .. } => {
                vec![*edge_feather]
            }
            Self::Stabilize { .. } => vec![],
            _ => vec![self.sigma()],
        }
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.is_pointwise_color() {
            return self.validate_color();
        }
        if self.is_standard() {
            return self.validate_standard();
        }
        if self.is_fx008() {
            return self.validate_fx008();
        }
        if let Some(covariance) = self.covariance() {
            affine_gaussian_kernel(covariance)?;
        } else {
            for sigma in self.sigma() {
                gaussian_kernel(sigma)?;
            }
        }
        if let Some((offset, _, opacity)) = self.shadow()
            && (offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(&opacity))
        {
            return Err(RenderError::InvalidInput(
                "invalid shadow offset/opacity".into(),
            ));
        }
        Ok(())
    }
    /// FX-005/FX-006 parameter contract mirrors the model-side checks and the
    /// shared 1024-output-pixel kernel budget (ADR-0115).
    fn validate_standard(&self) -> Result<(), RenderError> {
        let invalid = || RenderError::InvalidInput("invalid standard effect parameters".into());
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        let nonnegative = |v: f32| v.is_finite() && v >= 0.0;
        let lengths = |v: &[f32; 2]| v.iter().all(|v| nonnegative(*v));
        match self {
            Self::ChromaKey {
                similarity,
                edge_shrink,
                edge_feather,
                spill,
                ..
            } => {
                if !unit(*similarity)
                    || !unit(*spill)
                    || !lengths(edge_shrink)
                    || !lengths(edge_feather)
                {
                    return Err(invalid());
                }
            }
            Self::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => {
                if !unit(*key_luma)
                    || !unit(*tolerance)
                    || !lengths(edge_shrink)
                    || !lengths(edge_feather)
                {
                    return Err(invalid());
                }
            }
            Self::Glow {
                threshold,
                radius,
                intensity,
            } => {
                if !nonnegative(*threshold) || !nonnegative(*intensity) || !lengths(radius) {
                    return Err(invalid());
                }
            }
            Self::Sharpen { amount, radius } => {
                if !nonnegative(*amount) || !lengths(radius) {
                    return Err(invalid());
                }
            }
            Self::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => {
                if !unit(*amount) || !unit(*midpoint) || !unit(*roundness) || !nonnegative(*feather)
                {
                    return Err(invalid());
                }
            }
            Self::CornerPin { pins, .. } => {
                if pins
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Err(invalid());
                }
                // Pin quad validity is independent of the (possibly empty)
                // source bounds: always check convexity and nondegeneracy.
                corner_pin_inverse(
                    PixelBounds {
                        min: [0.0; 2],
                        max: [1.0; 2],
                    },
                    *pins,
                )?;
            }
            Self::Stabilize {
                frame, unmap, size, ..
            } => {
                let finite2x3 = |m: &[[f32; 3]; 2]| m.iter().flatten().all(|v| v.is_finite());
                if !finite2x3(frame)
                    || !finite2x3(unmap)
                    || size.iter().any(|v| !v.is_finite() || *v <= 0.0)
                    || size.iter().any(|v| *v > 1_000_000.0)
                {
                    return Err(invalid());
                }
            }
            _ => unreachable!("not a standard effect"),
        }
        // Kernel budgets: erosions cap at the Gaussian radius; feathers and
        // glow/sharpen radii share the separable Gaussian budget.
        for sigma in self.kernel_sigmas() {
            for s in sigma {
                gaussian_kernel(s)?;
            }
        }
        let shrink = match self {
            Self::ChromaKey { edge_shrink, .. } | Self::LumaKey { edge_shrink, .. } => {
                Some(*edge_shrink)
            }
            _ => None,
        };
        if let Some(shrink) = shrink
            && shrink.iter().any(|v| v.ceil() > STANDARD_KERNEL_MAX_RADIUS)
        {
            return Err(RenderError::UnsupportedFeature(
                "effect kernel radius exceeds 1024 output pixels".into(),
            ));
        }
        Ok(())
    }
    /// FX-008 parameter contract mirrors the model-side checks plus the
    /// shared output-pixel kernel budget (ADR-0137).
    fn validate_fx008(&self) -> Result<(), RenderError> {
        let invalid = || RenderError::InvalidInput("invalid fx008 effect parameters".into());
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        match self {
            Self::Grain { amount, size, .. } => {
                if !unit(*amount)
                    || !size.is_finite()
                    || *size <= 0.0
                    || *size > FX008_KERNEL_MAX_RADIUS
                {
                    return Err(invalid());
                }
            }
            Self::Mosaic { block_size, .. } => {
                if !block_size.is_finite()
                    || *block_size <= 0.0
                    || *block_size > FX008_KERNEL_MAX_RADIUS
                {
                    return Err(invalid());
                }
            }
            Self::Invert { .. } => {}
            Self::ChannelMixer { matrix } => {
                if !matrix
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() <= 1_024.0)
                {
                    return Err(invalid());
                }
            }
            Self::Tint { amount, .. } => {
                if !unit(*amount) {
                    return Err(invalid());
                }
            }
            Self::DirectionalBlur {
                direction, length, ..
            } => {
                if !direction.iter().all(|v| v.is_finite())
                    || !length.is_finite()
                    || *length < 0.0
                    || *length > FX008_KERNEL_MAX_RADIUS * 2.0
                {
                    return Err(invalid());
                }
                // A nonzero-length blur carries a unit direction; a zero
                // length collapses to identity and accepts any stored vector.
                let norm = direction[0].hypot(direction[1]);
                if *length > 0.0 && (norm - 1.0).abs() > 1e-3 {
                    return Err(invalid());
                }
            }
            Self::RadialBlur {
                mode,
                amount,
                center,
            } => {
                let bounded = match mode {
                    kronello_model::RadialBlurMode::Spin => amount.abs() <= 1_000_000.0,
                    kronello_model::RadialBlurMode::Zoom => unit(*amount),
                };
                if !amount.is_finite()
                    || !bounded
                    || !center
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                {
                    return Err(invalid());
                }
            }
            Self::Displace { displacement, .. } => {
                if !displacement
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                {
                    return Err(invalid());
                }
            }
            Self::Generate {
                point_a,
                point_b,
                cell_size,
                line_width,
                ..
            } => {
                let point =
                    |p: &[f32; 2]| p.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.0);
                if !point(point_a)
                    || !point(point_b)
                    || !cell_size.is_finite()
                    || *cell_size <= 0.0
                    || *cell_size > 1_000_000.0
                    || !line_width.is_finite()
                    || *line_width < 0.0
                    || *line_width > 1_000_000.0
                {
                    return Err(invalid());
                }
            }
            _ => unreachable!("not an fx008 effect"),
        }
        Ok(())
    }
    /// COLOR-002 parameter contract mirrors the model-side checks.
    fn validate_color(&self) -> Result<(), RenderError> {
        let invalid = || RenderError::InvalidInput("invalid color effect parameters".into());
        match self {
            Self::ColorExposure { exposure, offset } => {
                if !exposure.is_finite() || !offset.is_finite() {
                    return Err(invalid());
                }
            }
            Self::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                if ![in_black, in_white, gamma, out_black, out_white]
                    .into_iter()
                    .all(|v| v.is_finite())
                    || in_white <= in_black
                    || *gamma <= 0.0
                {
                    return Err(invalid());
                }
            }
            Self::ColorCurves { points } => {
                if !(2..=kronello_model::CURVES_MAX_POINTS).contains(&points.len())
                    || !points
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    || !points.windows(2).all(|w| w[0][0] < w[1][0])
                {
                    return Err(invalid());
                }
            }
            Self::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => {
                if ![hue_shift, saturation, lightness]
                    .into_iter()
                    .all(|v| v.is_finite())
                {
                    return Err(invalid());
                }
            }
            Self::ColorLut { lut, intensity } => {
                if !intensity.is_finite() || !(0.0..=1.0).contains(intensity) {
                    return Err(invalid());
                }
                lut.validate()
                    .and_then(|()| lut.validate_document_size())
                    .map_err(RenderError::from)?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    /// Bilinear translation requires floor/ceil source taps in addition to
    /// blur. Keying/glow/sharpen expand by their kernel halo; corner pin
    /// conservatively requests the whole incoming source quad since any
    /// destination pixel may sample it (ADR-0115).
    pub fn required_input(&self, output: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. }
            | Self::AffineGaussianBlur { .. }
            | Self::ChromaKey { .. }
            | Self::LumaKey { .. }
            | Self::Glow { .. }
            | Self::Sharpen { .. } => output.expand(halo),
            Self::CornerPin { source, .. } => source.unwrap_or(output),
            // TRACK-002: the kernel reads the input raster at `unmap(s')`
            // where s' is the border-resolved source-extent position, so the
            // conservative request is the `unmap` hull of the source frame.
            Self::Stabilize { unmap, size, .. } => {
                let r = |c: [f64; 2]| {
                    [
                        f64::from(unmap[0][0]) * c[0]
                            + f64::from(unmap[0][1]) * c[1]
                            + f64::from(unmap[0][2]),
                        f64::from(unmap[1][0]) * c[0]
                            + f64::from(unmap[1][1]) * c[1]
                            + f64::from(unmap[1][2]),
                    ]
                };
                let (w, h) = (f64::from(size[0]), f64::from(size[1]));
                let corners = [r([0.0, 0.0]), r([w, 0.0]), r([w, h]), r([0.0, h])];
                PixelBounds {
                    min: [0, 1].map(|i| {
                        corners
                            .iter()
                            .map(|c| c[i])
                            .fold(f64::INFINITY, f64::min)
                            .floor()
                    }),
                    max: [0, 1].map(|i| {
                        corners
                            .iter()
                            .map(|c| c[i])
                            .fold(f64::NEG_INFINITY, f64::max)
                            .ceil()
                    }),
                }
            }
            Self::DropShadow { offset, .. } | Self::AffineDropShadow { offset, .. } => {
                let shifted = output.translate(offset.map(|v| -f64::from(v)));
                let shifted = PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                };
                output.union(shifted.expand(halo))
            }
            // FX-008: mosaic/directional/displace read their exact extents;
            // the displacement *map* surface itself is sampled at the output
            // position, so the EffectMap DAG node requests `output` for it.
            Self::Mosaic { .. } | Self::DirectionalBlur { .. } | Self::Displace { .. } => {
                output.expand(halo)
            }
            // FX-008 radial blur: every sampled position sits on the segment
            // between `center` and the output pixel (zoom) or on the same
            // radius (spin), so the conservative request is the axis-aligned
            // square covering all output-corner radii around `center`.
            Self::RadialBlur { center, .. } => {
                let radius = [0, 1].map(|i| {
                    (output.min[i] - f64::from(center[i]))
                        .abs()
                        .max((output.max[i] - f64::from(center[i])).abs())
                });
                PixelBounds {
                    min: [0, 1].map(|i| f64::from(center[i]) - radius[i] - 1.0),
                    max: [0, 1].map(|i| f64::from(center[i]) + radius[i] + 1.0),
                }
            }
            _ => output,
        }
    }
    /// FX-005 keying and vignette keep the input extent: the matte can only
    /// shrink coverage. Glow/sharpen expand by their kernel; corner pin
    /// reports the destination quad hull (ADR-0115).
    pub fn output_bounds(&self, input: PixelBounds) -> PixelBounds {
        let halo = self.halo();
        match self {
            Self::GaussianBlur { .. }
            | Self::AffineGaussianBlur { .. }
            | Self::Glow { .. }
            | Self::Sharpen { .. } => input.expand(halo),
            Self::ChromaKey { .. } | Self::LumaKey { .. } | Self::Vignette { .. } => input,
            Self::CornerPin { pins, .. } => corner_pin_hull(*pins),
            // TRACK-002: edge-extension borders keep coverage over the whole
            // input raster; a transparent fill only covers output pixels whose
            // corrected source position lands inside the frame, i.e. the
            // inverse-`frame` hull of the source rectangle.
            Self::Stabilize {
                frame,
                size,
                border,
                fill,
                ..
            } => {
                if *border == kronello_model::StabilizeBorder::Fill
                    && fill.components().alpha.get() == 0.0
                {
                    let (w, h) = (f64::from(size[0]), f64::from(size[1]));
                    let Some(inv) = stabilize_inverse(frame) else {
                        return input;
                    };
                    let r = |c: [f64; 2]| {
                        [
                            inv[0][0] * c[0] + inv[0][1] * c[1] + inv[0][2],
                            inv[1][0] * c[0] + inv[1][1] * c[1] + inv[1][2],
                        ]
                    };
                    let corners = [r([0.0, 0.0]), r([w, 0.0]), r([w, h]), r([0.0, h])];
                    PixelBounds {
                        min: [0, 1].map(|i| {
                            corners
                                .iter()
                                .map(|c| c[i])
                                .fold(f64::INFINITY, f64::min)
                                .floor()
                        }),
                        max: [0, 1].map(|i| {
                            corners
                                .iter()
                                .map(|c| c[i])
                                .fold(f64::NEG_INFINITY, f64::max)
                                .ceil()
                        }),
                    }
                } else {
                    input
                }
            }
            Self::DropShadow { offset, .. } | Self::AffineDropShadow { offset, .. } => {
                let shifted = input.expand(halo).translate(offset.map(f64::from));
                input.union(PixelBounds {
                    min: shifted.min.map(f64::floor),
                    max: shifted.max.map(f64::ceil),
                })
            }
            Self::DirectionalBlur { .. } | Self::Displace { .. } => input.expand(halo),
            // FX-008 radial blur: spin keeps ink on its radius around
            // `center`; zoom projects ink outward by up to 1/(1-amount).
            // The conservative hull is the axis-aligned square covering the
            // farthest input corner radius, scaled for zoom.
            Self::RadialBlur {
                mode,
                amount,
                center,
            } => {
                let radius = [0, 1].map(|i| {
                    (input.min[i] - f64::from(center[i]))
                        .abs()
                        .max((input.max[i] - f64::from(center[i])).abs())
                });
                let factor = match mode {
                    kronello_model::RadialBlurMode::Spin => 1.0,
                    kronello_model::RadialBlurMode::Zoom => {
                        if f64::from(*amount) >= 1.0 {
                            16_777_216.0
                        } else {
                            1.0 / (1.0 - f64::from(*amount))
                        }
                    }
                };
                PixelBounds {
                    min: [0, 1].map(|i| f64::from(center[i]) - radius[i] * factor - 1.0),
                    max: [0, 1].map(|i| f64::from(center[i]) + radius[i] * factor + 1.0),
                }
            }
            _ => input,
        }
    }
}
/// Conservative coverage hull of the corner-pin destination quad on the
/// output pixel lattice.
pub fn corner_pin_hull(pins: [[f32; 2]; 4]) -> PixelBounds {
    PixelBounds {
        min: [0, 1].map(|i| {
            pins.iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::INFINITY, f64::min)
                .floor()
        }),
        max: [0, 1].map(|i| {
            pins.iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
        }),
    }
}
/// Inverse homography from destination output-pixel edge coordinates to
/// source edge coordinates over `source`, as a row-major 3x3 matrix. Quad
/// corners map TL→pins[0], TR→pins[1], BR→pins[2], BL→pins[3]. The quad must
/// be strictly convex; a degenerate or self-intersecting quad is a typed
/// error.
pub fn corner_pin_inverse(
    source: PixelBounds,
    pins: [[f32; 2]; 4],
) -> Result<[[f32; 3]; 3], RenderError> {
    let degenerate =
        || RenderError::InvalidInput("corner pin requires a nondegenerate convex quad".into());
    if !source.min.iter().chain(&source.max).all(|v| v.is_finite())
        || source.max[0] <= source.min[0]
        || source.max[1] <= source.min[1]
    {
        return Err(degenerate());
    }
    let d = pins.map(|p| p.map(f64::from));
    let e = [0, 1, 2, 3].map(|i| {
        let (a, b) = (d[i], d[(i + 1) % 4]);
        [b[0] - a[0], b[1] - a[1]]
    });
    // Strict convexity: all four cross products share one nonzero sign.
    let crosses = [0, 1, 2, 3].map(|i| e[i][0] * e[(i + 1) % 4][1] - e[i][1] * e[(i + 1) % 4][0]);
    let scale = e
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        .max(f64::EPSILON);
    if crosses
        .iter()
        .any(|c| !c.is_finite() || c.abs() <= 1e-6 * scale * scale)
    {
        return Err(degenerate());
    }
    if !(crosses.iter().all(|c| *c > 0.0) || crosses.iter().all(|c| *c < 0.0)) {
        return Err(degenerate());
    }
    // Unit-square to quad homography (Heckbert): the source rectangle is
    // normalized to the unit square so sampling bounds stay [0,1].
    let sx = d[1][0] - d[0][0] - d[2][0] + d[3][0];
    let sy = d[1][1] - d[0][1] - d[2][1] + d[3][1];
    let dx1 = d[1][0] - d[2][0];
    let dx2 = d[3][0] - d[2][0];
    let dy1 = d[1][1] - d[2][1];
    let dy2 = d[3][1] - d[2][1];
    let denominator = dx1 * dy2 - dx2 * dy1;
    if !denominator.is_finite() || denominator.abs() <= 1e-6 * scale * scale {
        return Err(degenerate());
    }
    let g = (sx * dy2 - dx2 * sy) / denominator;
    let h = (dx1 * sy - sx * dy1) / denominator;
    // Forward map: u,v in the unit square to output-pixel x,y.
    let forward = [
        [
            d[1][0] - d[0][0] + g * d[1][0],
            d[3][0] - d[0][0] + h * d[3][0],
            d[0][0],
        ],
        [
            d[1][1] - d[0][1] + g * d[1][1],
            d[3][1] - d[0][1] + h * d[3][1],
            d[0][1],
        ],
        [g, h, 1.0],
    ];
    let det = forward[0][0] * (forward[1][1] * forward[2][2] - forward[1][2] * forward[2][1])
        - forward[0][1] * (forward[1][0] * forward[2][2] - forward[1][2] * forward[2][0])
        + forward[0][2] * (forward[1][0] * forward[2][1] - forward[1][1] * forward[2][0]);
    let norm = forward
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0, f64::max);
    if !det.is_finite() || det.abs() <= 1e-12 * norm * norm * norm {
        return Err(degenerate());
    }
    let m = |r: usize, c: usize| forward[r][c];
    let cofactor = [
        [
            m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1),
            m(0, 2) * m(2, 1) - m(0, 1) * m(2, 2),
            m(0, 1) * m(1, 2) - m(0, 2) * m(1, 1),
        ],
        [
            m(1, 2) * m(2, 0) - m(1, 0) * m(2, 2),
            m(0, 0) * m(2, 2) - m(0, 2) * m(2, 0),
            m(0, 2) * m(1, 0) - m(0, 0) * m(1, 2),
        ],
        [
            m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0),
            m(0, 1) * m(2, 0) - m(0, 0) * m(2, 1),
            m(0, 0) * m(1, 1) - m(0, 1) * m(1, 0),
        ],
    ];
    let inverse = cofactor.map(|row| row.map(|v| v / det));
    if inverse.iter().flatten().any(|v| !v.is_finite()) {
        return Err(degenerate());
    }
    // Compose with the unit-square to source-rectangle map so the result maps
    // destination edge coordinates directly to source edge coordinates.
    let w = source.max[0] - source.min[0];
    let h = source.max[1] - source.min[1];
    let total = [
        [
            w * inverse[0][0],
            w * inverse[0][1],
            w * inverse[0][2] + source.min[0],
        ],
        [
            h * inverse[1][0],
            h * inverse[1][1],
            h * inverse[1][2] + source.min[1],
        ],
        inverse[2],
    ];
    if total.iter().flatten().any(|v| !v.is_finite()) {
        return Err(degenerate());
    }
    Ok(total.map(|row| row.map(|v| v as f32)))
}
/// Symmetric discrete Gaussian, normalized over [-ceil(3*sigma), +ceil(3*sigma)].
/// CPU and GPU consume these exact float32 weights; sigma zero is identity.
pub fn gaussian_kernel(sigma: f32) -> Result<Vec<f32>, RenderError> {
    if !sigma.is_finite() || sigma < 0.0 {
        return Err(RenderError::InvalidInput("invalid gaussian sigma".into()));
    }
    let radius = (3.0 * sigma).ceil();
    if radius > 1024.0 {
        return Err(RenderError::UnsupportedFeature(
            "effect kernel radius exceeds 1024 output pixels".into(),
        ));
    }
    if sigma == 0.0 {
        return Ok(vec![1.0]);
    }
    let radius = radius as i32;
    let mut weights: Vec<_> = (-radius..=radius)
        .map(|i| {
            let x = f64::from(i) / f64::from(sigma);
            (-0.5 * x * x).exp()
        })
        .collect();
    let sum: f64 = weights.iter().sum();
    for w in &mut weights {
        *w /= sum;
    }
    Ok(weights.into_iter().map(|w| w as f32).collect())
}

/// Finite nonsingular affine range. The normalized determinant cutoff is part
/// of v2 semantics, including sigma zero; it is never repaired by clamping.
pub fn validate_affine_linear(linear: [[f64; 2]; 2]) -> Result<(), RenderError> {
    let m = linear
        .into_iter()
        .flatten()
        .map(f64::abs)
        .fold(0.0, f64::max);
    if !linear.into_iter().flatten().all(f64::is_finite) || m == 0.0 {
        return Err(RenderError::UnsupportedFeature(
            "degenerate affine effect transform".into(),
        ));
    }
    let n = linear.map(|r| r.map(|v| v / m));
    if (n[0][0] * n[1][1] - n[0][1] * n[1][0]).abs() <= 1e-6 {
        return Err(RenderError::UnsupportedFeature(
            "degenerate affine effect transform".into(),
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaussianTap {
    pub offset: [i32; 2],
    pub weight: f32,
}
/// v2 samples exp(-q/2) at integer output offsets, retaining q<=9. The
/// transformed circular 3-sigma support is elliptical, not an axis-aligned
/// separable approximation. CPU/GPU consume identical row-major f32 weights.
pub fn affine_gaussian_kernel(c: [f64; 3]) -> Result<Vec<GaussianTap>, RenderError> {
    let unsupported =
        || RenderError::UnsupportedFeature("affine Gaussian covariance or kernel budget".into());
    if c == [0.0; 3] {
        return Ok(vec![GaussianTap {
            offset: [0; 2],
            weight: 1.0,
        }]);
    }
    if !c.into_iter().all(f64::is_finite) || c[0] <= 0.0 || c[2] <= 0.0 {
        return Err(unsupported());
    }
    let m = c[0].max(c[2]);
    let n = c.map(|v| v / m);
    let det = n[0] * n[2] - n[1] * n[1];
    if !det.is_finite() || det <= 1e-12 {
        return Err(unsupported());
    }
    let determinant = c[0] * c[2] - c[1] * c[1];
    if !determinant.is_normal() || determinant <= 0.0 {
        return Err(unsupported());
    }
    let radius = [c[0], c[2]].map(|v| (3.0 * v.sqrt()).ceil());
    if radius.iter().any(|r| *r > 1024.0) {
        return Err(unsupported());
    }
    let [rx, ry] = radius.map(|v| v as i32);
    if (2 * rx + 1) as usize * (2 * ry + 1) as usize > AFFINE_KERNEL_MAX_CANDIDATES {
        return Err(unsupported());
    }
    let mut taps = Vec::new();
    let mut sum = 0.0;
    for y in -ry..=ry {
        for x in -rx..=rx {
            let (x0, y0) = (f64::from(x), f64::from(y));
            // The central tap is exact without a determinant division.
            let q = if x == 0 && y == 0 {
                0.0
            } else {
                (c[2] * x0 * x0 - 2.0 * c[1] * x0 * y0 + c[0] * y0 * y0) / determinant
            };
            if q <= 9.0 {
                let weight = (-0.5 * q).exp();
                sum += weight;
                taps.push((
                    GaussianTap {
                        offset: [x, y],
                        weight: 0.0,
                    },
                    weight,
                ));
            }
        }
    }
    Ok(taps
        .into_iter()
        .map(|(mut tap, w)| {
            tap.weight = (w / sum) as f32;
            tap
        })
        .collect())
}

#[cfg(test)]
mod affine_tests {
    use super::*;
    #[test]
    fn fx002_covariance_kernel_and_fractional_shadow_halo() {
        let design = ResolvedEffect::AffineDropShadow {
            sigma: 1.0,
            linear: [[2.0, 1.0], [0.0, 1.0]],
            offset: [3.0, -0.5],
            color: Color::from_srgb8([0; 3], None),
            opacity: 0.5,
        };
        let effect = PixelEffect::from_design(&design, [0.5, 2.0]).unwrap();
        assert_eq!(effect.covariance(), Some([1.25, 1.0, 4.0]));
        assert_eq!(effect.shadow().unwrap().0, [1.5, -1.0]);
        assert_eq!(effect.halo(), [4.0, 6.0]);
        let roi = PixelBounds {
            min: [10.0; 2],
            max: [20.0; 2],
        };
        assert_eq!(
            effect.required_input(roi),
            PixelBounds {
                min: [4.0, 5.0],
                max: [23.0, 27.0]
            }
        );
        assert_eq!(
            effect.output_bounds(roi),
            PixelBounds {
                min: [7.0, 3.0],
                max: [26.0, 25.0]
            }
        );
        let taps = affine_gaussian_kernel([5.0, 1.0, 1.0]).unwrap();
        let center = taps.iter().find(|t| t.offset == [0, 0]).unwrap().weight;
        // q(1,1)=1, q(1,-1)=2: the cross term cannot be discarded.
        for (offset, q) in [([1, 1], 1.0_f64), ([1, -1], 2.0)] {
            let w = taps.iter().find(|t| t.offset == offset).unwrap().weight;
            assert!((f64::from(w / center) - (-0.5 * q).exp()).abs() < 1e-7);
        }
        assert!((taps.iter().map(|t| t.weight).sum::<f32>() - 1.0).abs() < 1e-6);
        for tap in &taps {
            let opposite = taps
                .iter()
                .find(|t| t.offset == tap.offset.map(|v| -v))
                .unwrap();
            assert_eq!(tap.weight, opposite.weight);
        }
    }
    #[test]
    fn fx002_degenerate_transforms_and_finite_budgets_are_typed() {
        for linear in [
            [[0.0; 2]; 2],
            [[1.0, 1.0], [1.0, 1.0]],
            [[1.0, 0.0], [0.0, 1e-7]],
            [[f64::INFINITY, 0.0], [0.0, 1.0]],
        ] {
            assert_eq!(
                validate_affine_linear(linear).unwrap_err().code(),
                "UNSUPPORTED_FEATURE"
            );
        }
        for covariance in [
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1e-13],
            [1e6, 0.0, 1.0],
            [43.0 * 43.0, 0.0, 43.0 * 43.0],
            [f64::INFINITY, 0.0, 1.0],
            [1e-160, 0.0, 1e-160],
        ] {
            assert_eq!(
                affine_gaussian_kernel(covariance).unwrap_err().code(),
                "UNSUPPORTED_FEATURE"
            );
        }
        // Square candidate budget: 253*253 candidates is accepted; 259*259 is not.
        assert!(affine_gaussian_kernel([42.0 * 42.0, 0.0, 42.0 * 42.0]).is_ok());
        assert_eq!(
            affine_gaussian_kernel([0.0; 3]).unwrap(),
            vec![GaussianTap {
                offset: [0; 2],
                weight: 1.0
            }]
        );
    }
}
