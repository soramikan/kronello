//! Ordered, versioned effects. Parameters reference the owning node's Properties.
use crate::{
    AssetId, Color, DescriptorDefinition, DescriptorId, FiniteF64, NumericRange, Property,
    PropertyDescriptor, PropertyId, SchemaKey, SchemaRegistry, Unit, Value, ValueRange, ValueType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GAUSSIAN_BLUR_ID: &str = "kronello.gaussian_blur";
pub const DROP_SHADOW_ID: &str = "kronello.drop_shadow";
pub const EFFECT_VERSION: u32 = 1;
pub const AUDIO_GAIN_ID: &str = "kronello.audio.gain";
/// AUDIO-007 (ADR-0117): parametric EQ driven by a band data table.
pub const AUDIO_EQ_ID: &str = "kronello.audio.eq";
/// AUDIO-007: first-to-fourth order Butterworth high-pass filter.
pub const AUDIO_HPF_ID: &str = "kronello.audio.hpf";
/// AUDIO-007: first-to-fourth order Butterworth low-pass filter.
pub const AUDIO_LPF_ID: &str = "kronello.audio.lpf";
/// AUDIO-008: stereo-linked feed-forward peak compressor.
pub const AUDIO_COMPRESSOR_ID: &str = "kronello.audio.compressor";
/// AUDIO-008: peak ceiling limiter with instant attack and release.
pub const AUDIO_LIMITER_ID: &str = "kronello.audio.limiter";
/// AUDIO-007: maximum parametric EQ band count.
pub const AUDIO_EQ_MAX_BANDS: usize = 8;
/// AUDIO-007: supported Butterworth HPF/LPF orders.
pub const AUDIO_FILTER_MAX_ORDER: u32 = 4;
/// AUDIO-007: frequency parameters stay strictly below 48 kHz Nyquist.
pub const AUDIO_FILTER_MAX_FREQ_HZ: f64 = 24_000.0;
/// AUDIO-007: peak/shelf Q bound keeps the biquads well conditioned.
pub const AUDIO_EQ_MAX_Q: f64 = 100.0;
/// AUDIO-007/008: bound for decibel-valued effect parameters.
pub const AUDIO_MAX_DB: f64 = 120.0;
/// AUDIO-008: compressor ratio upper bound.
pub const AUDIO_MAX_RATIO: f64 = 100.0;
/// AUDIO-008: envelope time constant range in milliseconds.
pub const AUDIO_MIN_TIME_MS: f64 = 0.01;
/// AUDIO-008: envelope time constant range in milliseconds.
pub const AUDIO_MAX_TIME_MS: f64 = 10_000.0;
pub const AFFINE_EFFECT_VERSION: u32 = 2;
/// COLOR-002 pointwise color correction effect ids (ADR-0108).
pub const COLOR_EXPOSURE_ID: &str = "kronello.color.exposure";
pub const COLOR_LEVELS_ID: &str = "kronello.color.levels";
pub const COLOR_CURVES_ID: &str = "kronello.color.curves";
pub const COLOR_HSL_ID: &str = "kronello.color.hsl";
/// COLOR-002 supported version is EFFECT_VERSION (1).
pub const COLOR_EFFECT_VERSION: u32 = EFFECT_VERSION;
/// COLOR-003 pointwise `.cube` LUT application effect id (ADR-0113).
pub const COLOR_LUT_ID: &str = "kronello.color.lut";
/// COLOR-003 supported version; shares the COLOR-002 semantic version family.
pub const COLOR_LUT_VERSION: u32 = COLOR_EFFECT_VERSION;
/// Maximum accepted COLOR-002 curves control-point count.
pub const CURVES_MAX_POINTS: usize = 64;
/// FX-005 keying effect ids (ADR-0115). Both are matte-producing effects:
/// they rewrite alpha and are the only effects allowed to create or destroy
/// coverage inside the input bounds.
pub const KEYING_CHROMA_ID: &str = "kronello.keying.chroma";
pub const KEYING_LUMA_ID: &str = "kronello.keying.luma";
/// FX-006 standard effect ids (ADR-0115).
pub const GLOW_ID: &str = "kronello.glow";
pub const SHARPEN_ID: &str = "kronello.sharpen";
pub const VIGNETTE_ID: &str = "kronello.vignette";
pub const CORNER_PIN_ID: &str = "kronello.corner_pin";
/// FX-005/FX-006 supported version is EFFECT_VERSION (1).
pub const STANDARD_EFFECT_VERSION: u32 = EFFECT_VERSION;

/// Unknown ids, parameters, fields and variants are retained verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Effect {
    Known(EffectDefinition),
    Opaque(serde_json::Value),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectDefinition {
    pub effect_id: String,
    pub version: u32,
    pub parameters: EffectParameters,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectParameters {
    GaussianBlur {
        sigma: PropertyId,
    },
    DropShadow {
        sigma: PropertyId,
        offset: PropertyId,
        color: PropertyId,
        opacity: PropertyId,
    },
    AudioGain {
        gain: PropertyId,
    },
    /// AUDIO-007 parametric EQ. `bands` references a data table property
    /// with `kind` (peak | low_shelf | high_shelf), `freq_hz`, `gain_db`
    /// and `q` columns and 1..=AUDIO_EQ_MAX_BANDS rows.
    AudioEq {
        bands: PropertyId,
    },
    /// AUDIO-007 Butterworth high-pass; `order` is 1..=AUDIO_FILTER_MAX_ORDER.
    AudioHpf {
        cutoff_hz: PropertyId,
        order: PropertyId,
    },
    /// AUDIO-007 Butterworth low-pass; `order` is 1..=AUDIO_FILTER_MAX_ORDER.
    AudioLpf {
        cutoff_hz: PropertyId,
        order: PropertyId,
    },
    /// AUDIO-008 stereo-linked feed-forward peak compressor.
    AudioCompressor {
        threshold_db: PropertyId,
        ratio: PropertyId,
        attack_ms: PropertyId,
        release_ms: PropertyId,
        makeup_db: PropertyId,
    },
    /// AUDIO-008 ceiling limiter with instant attack and release.
    AudioLimiter {
        ceiling_db: PropertyId,
        release_ms: PropertyId,
    },
    /// COLOR-002 exposure: premultiplied working RGB is scaled by
    /// 2^exposure and shifted by offset; alpha is preserved (ADR-0108).
    ColorExposure {
        /// EV stops; finite scalar.
        exposure: PropertyId,
        /// Additive premultiplied-space offset; finite scalar.
        offset: PropertyId,
    },
    /// COLOR-002 levels with gamma, applied per premultiplied RGB channel.
    ColorLevels {
        in_black: PropertyId,
        in_white: PropertyId,
        /// Positive finite scalar; 1.0 is linear.
        gamma: PropertyId,
        out_black: PropertyId,
        out_white: PropertyId,
    },
    /// COLOR-002 monotone-cubic RGB channel curve. The property is a data
    /// table with scalar `x` and `y` columns, `x` strictly increasing, both
    /// in [0,1], and at most CURVES_MAX_POINTS rows.
    ColorCurves {
        curve: PropertyId,
    },
    /// COLOR-002 hue/saturation/lightness in working HSL; alpha preserved.
    ColorHsl {
        /// Shift in degrees; finite angle.
        hue_shift: PropertyId,
        /// Multiplier where 1.0 leaves saturation unchanged; finite scalar.
        saturation: PropertyId,
        /// Additive lightness term; finite scalar.
        lightness: PropertyId,
    },
    /// FX-005 chroma key (ADR-0115): alpha = keyed Cb/Cr matte, then edge
    /// shrink/feather adjust the matte and `spill` suppresses the key hue.
    ChromaKey {
        /// Screen color being removed.
        key_color: PropertyId,
        /// Cb/Cr distance that maps to full transparency; 0..=1.
        similarity: PropertyId,
        /// Matte erosion radius in design_px.
        edge_shrink: PropertyId,
        /// Matte Gaussian feather sigma in design_px.
        edge_feather: PropertyId,
        /// Spill suppression amount; 0..=1.
        spill: PropertyId,
    },
    /// FX-005 luma key (ADR-0115): working-space luminance distance drives
    /// the alpha matte, then edge shrink/feather adjust it.
    LumaKey {
        /// Straight-color luminance that becomes fully transparent; 0..=1.
        key_luma: PropertyId,
        /// Luminance distance that maps to full opacity; 0..=1.
        tolerance: PropertyId,
        /// Matte erosion radius in design_px.
        edge_shrink: PropertyId,
        /// Matte Gaussian feather sigma in design_px.
        edge_feather: PropertyId,
    },
    /// FX-006 glow (ADR-0115): straight luminance above `threshold` is
    /// extracted, blurred by a Gaussian of sigma `radius`, and added back.
    Glow {
        /// Straight luminance cutoff; nonnegative scalar.
        threshold: PropertyId,
        /// Gaussian sigma in design_px.
        radius: PropertyId,
        /// Additive contribution of the blurred bloom; nonnegative scalar.
        intensity: PropertyId,
    },
    /// FX-006 unsharp mask (ADR-0115): out = source + amount * (source -
    /// blur(source)); the alpha channel participates and stays in [0,1].
    Sharpen {
        /// Unsharp strength; nonnegative scalar.
        amount: PropertyId,
        /// Gaussian sigma in design_px.
        radius: PropertyId,
    },
    /// FX-006 vignette (ADR-0115): darkens RGB toward the image corners;
    /// alpha is preserved.
    Vignette {
        /// Maximum darkening factor; 0..=1.
        amount: PropertyId,
        /// Normalized distance where darkening starts; 0..=1.
        midpoint: PropertyId,
        /// Smoothstep width of the falloff; nonnegative scalar.
        feather: PropertyId,
        /// Rectangle-to-ellipse shape blend; 0..=1.
        roundness: PropertyId,
    },
    /// FX-006 corner pin (ADR-0115): the four corners of the incoming
    /// surface's bounds are moved to these absolute Composition design_px
    /// positions, in order top-left, top-right, bottom-right, bottom-left.
    CornerPin {
        top_left: PropertyId,
        top_right: PropertyId,
        bottom_right: PropertyId,
        bottom_left: PropertyId,
    },
    /// COLOR-003 3D `.cube` LUT (ADR-0113). `lut` references an
    /// `AssetKind::Data` asset whose content bytes parse as a normalized
    /// `CubeLut`; the document stores the reference, never expanded bytes.
    /// `intensity` blends identity into the sampled output on 0..=1. Alpha is
    /// preserved and the authored stack order is honored.
    ColorLut {
        lut: PropertyId,
        intensity: PropertyId,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ResolvedEffect {
    AffineGaussianBlur {
        sigma: f64,
        linear: [[f64; 2]; 2],
    },
    AffineDropShadow {
        sigma: f64,
        linear: [[f64; 2]; 2],
        offset: [f64; 2],
        color: Color,
        opacity: f64,
    },
    GaussianBlur {
        sigma: f64,
    },
    DropShadow {
        sigma: f64,
        offset: [f64; 2],
        color: Color,
        opacity: f64,
    },
    /// COLOR-002 resolved pointwise operations keep f64 authoring precision.
    ColorExposure {
        exposure: f64,
        offset: f64,
    },
    ColorLevels {
        in_black: f64,
        in_white: f64,
        gamma: f64,
        out_black: f64,
        out_white: f64,
    },
    /// Monotonically increasing `(x, y)` control points, already validated.
    ColorCurves {
        curve: Vec<[f64; 2]>,
    },
    ColorHsl {
        hue_shift: f64,
        saturation: f64,
        lightness: f64,
    },
    /// FX-005 resolved forms; lengths stay in design_px until DAG lowering.
    ChromaKey {
        key_color: Color,
        similarity: f64,
        edge_shrink: f64,
        edge_feather: f64,
        spill: f64,
    },
    LumaKey {
        key_luma: f64,
        tolerance: f64,
        edge_shrink: f64,
        edge_feather: f64,
    },
    /// FX-006 resolved forms. `radius` is the Gaussian sigma in design_px;
    /// corner-pin positions are absolute Composition design_px points in
    /// top-left, top-right, bottom-right, bottom-left order.
    Glow {
        threshold: f64,
        radius: f64,
        intensity: f64,
    },
    Sharpen {
        amount: f64,
        radius: f64,
    },
    Vignette {
        amount: f64,
        midpoint: f64,
        feather: f64,
        roundness: f64,
    },
    CornerPin {
        corners: [[f64; 2]; 4],
    },
    /// COLOR-003 resolved effect. The asset id is content-verified against
    /// the snapshot `luts` input before a `PixelEffect` is built; the lattice
    /// itself never enters the resolved value so snapshots stay hash-stable.
    ColorLut {
        lut: AssetId,
        intensity: f64,
    },
}
/// AUDIO-007/008: one validated parametric EQ band (ADR-0117).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEqBand {
    pub kind: AudioEqBandKind,
    pub freq_hz: f64,
    pub gain_db: f64,
    pub q: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEqBandKind {
    Peak,
    LowShelf,
    HighShelf,
}
/// AUDIO-007/008: constant, range-validated audio effect parameters. The
/// audio evaluator turns this into the deterministic DSP chain shared by
/// realtime playback and export (ADR-0117).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedAudioEffect {
    Eq {
        bands: Vec<AudioEqBand>,
    },
    Hpf {
        cutoff_hz: f64,
        order: u32,
    },
    Lpf {
        cutoff_hz: f64,
        order: u32,
    },
    Compressor {
        threshold_db: f64,
        ratio: f64,
        attack_ms: f64,
        release_ms: f64,
        makeup_db: f64,
    },
    Limiter {
        ceiling_db: f64,
        release_ms: f64,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("UNSUPPORTED_FEATURE: unknown effect or semantic version")]
    UnsupportedFeature,
    #[error("invalid effect parameter {0}")]
    InvalidParameter(PropertyId),
    #[error("effect stack exceeds 16 entries")]
    StackBudget,
}
impl EffectDefinition {
    pub fn ensure_supported(&self) -> Result<(), EffectError> {
        let (id, latest) = match self.parameters {
            EffectParameters::AudioGain { .. } => (AUDIO_GAIN_ID, EFFECT_VERSION),
            EffectParameters::AudioEq { .. } => (AUDIO_EQ_ID, EFFECT_VERSION),
            EffectParameters::AudioHpf { .. } => (AUDIO_HPF_ID, EFFECT_VERSION),
            EffectParameters::AudioLpf { .. } => (AUDIO_LPF_ID, EFFECT_VERSION),
            EffectParameters::AudioCompressor { .. } => (AUDIO_COMPRESSOR_ID, EFFECT_VERSION),
            EffectParameters::AudioLimiter { .. } => (AUDIO_LIMITER_ID, EFFECT_VERSION),
            EffectParameters::GaussianBlur { .. } => (GAUSSIAN_BLUR_ID, AFFINE_EFFECT_VERSION),
            EffectParameters::DropShadow { .. } => (DROP_SHADOW_ID, AFFINE_EFFECT_VERSION),
            EffectParameters::ColorExposure { .. } => (COLOR_EXPOSURE_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorLevels { .. } => (COLOR_LEVELS_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorCurves { .. } => (COLOR_CURVES_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ColorHsl { .. } => (COLOR_HSL_ID, COLOR_EFFECT_VERSION),
            EffectParameters::ChromaKey { .. } => (KEYING_CHROMA_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::LumaKey { .. } => (KEYING_LUMA_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Glow { .. } => (GLOW_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Sharpen { .. } => (SHARPEN_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::Vignette { .. } => (VIGNETTE_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::CornerPin { .. } => (CORNER_PIN_ID, STANDARD_EFFECT_VERSION),
            EffectParameters::ColorLut { .. } => (COLOR_LUT_ID, COLOR_LUT_VERSION),
        };
        if self.effect_id != id || !(EFFECT_VERSION..=latest).contains(&self.version) {
            return Err(EffectError::UnsupportedFeature);
        }
        Ok(())
    }
    fn references(&self) -> Vec<(PropertyId, ValueType, Unit)> {
        match self.parameters {
            EffectParameters::AudioGain { gain } => {
                vec![(gain, ValueType::Scalar, Unit::Dimensionless)]
            }
            EffectParameters::AudioEq { bands } => {
                vec![(bands, ValueType::DataTable, Unit::Dimensionless)]
            }
            EffectParameters::AudioHpf { cutoff_hz, order }
            | EffectParameters::AudioLpf { cutoff_hz, order } => vec![
                (cutoff_hz, ValueType::Scalar, Unit::Dimensionless),
                (order, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::AudioCompressor {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_db,
            } => [threshold_db, ratio, attack_ms, release_ms, makeup_db]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::AudioLimiter {
                ceiling_db,
                release_ms,
            } => [ceiling_db, release_ms]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::GaussianBlur { sigma } => {
                vec![(sigma, ValueType::Scalar, Unit::DesignPx)]
            }
            EffectParameters::DropShadow {
                sigma,
                offset,
                color,
                opacity,
            } => vec![
                (sigma, ValueType::Scalar, Unit::DesignPx),
                (offset, ValueType::Vec2, Unit::DesignPx),
                (color, ValueType::Color, Unit::Dimensionless),
                (opacity, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ColorExposure { exposure, offset } => vec![
                (exposure, ValueType::Scalar, Unit::Dimensionless),
                (offset, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => [in_black, in_white, gamma, out_black, out_white]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::ColorCurves { curve } => {
                vec![(curve, ValueType::DataTable, Unit::Dimensionless)]
            }
            EffectParameters::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => vec![
                (hue_shift, ValueType::Angle, Unit::Degrees),
                (saturation, ValueType::Scalar, Unit::Dimensionless),
                (lightness, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => vec![
                (key_color, ValueType::Color, Unit::Dimensionless),
                (similarity, ValueType::Scalar, Unit::Dimensionless),
                (edge_shrink, ValueType::Scalar, Unit::DesignPx),
                (edge_feather, ValueType::Scalar, Unit::DesignPx),
                (spill, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => vec![
                (key_luma, ValueType::Scalar, Unit::Dimensionless),
                (tolerance, ValueType::Scalar, Unit::Dimensionless),
                (edge_shrink, ValueType::Scalar, Unit::DesignPx),
                (edge_feather, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::Glow {
                threshold,
                radius,
                intensity,
            } => vec![
                (threshold, ValueType::Scalar, Unit::Dimensionless),
                (radius, ValueType::Scalar, Unit::DesignPx),
                (intensity, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::Sharpen { amount, radius } => vec![
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (radius, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => vec![
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (midpoint, ValueType::Scalar, Unit::Dimensionless),
                (feather, ValueType::Scalar, Unit::Dimensionless),
                (roundness, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::CornerPin {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => [top_left, top_right, bottom_right, bottom_left]
                .into_iter()
                .map(|id| (id, ValueType::Vec2, Unit::DesignPx))
                .collect(),
            EffectParameters::ColorLut { lut, intensity } => vec![
                (lut, ValueType::AssetRef, Unit::Dimensionless),
                (intensity, ValueType::Scalar, Unit::Dimensionless),
            ],
        }
    }
    pub fn validate(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), EffectError> {
        self.ensure_supported()?;
        for (id, ty, unit) in self.references() {
            let p = properties
                .iter()
                .find(|p| p.id() == id)
                .ok_or(EffectError::InvalidParameter(id))?;
            let d = registry
                .lookup(&p.descriptor().key)
                .map_err(|_| EffectError::InvalidParameter(id))?;
            if d.definition().value_type != ty || d.definition().unit != unit {
                return Err(EffectError::InvalidParameter(id));
            }
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        self.ensure_supported()?;
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        // COLOR-002 pointwise corrections validate parameter magnitudes and
        // table shape at resolution; alpha/HDR handling is a renderer contract.
        if matches!(
            self.parameters,
            EffectParameters::ColorExposure { .. }
                | EffectParameters::ColorLevels { .. }
                | EffectParameters::ColorCurves { .. }
                | EffectParameters::ColorHsl { .. }
                | EffectParameters::ColorLut { .. }
        ) {
            return self.resolve_color(values);
        }
        // FX-005/FX-006 keying and standard effects (ADR-0115).
        if matches!(
            self.parameters,
            EffectParameters::ChromaKey { .. }
                | EffectParameters::LumaKey { .. }
                | EffectParameters::Glow { .. }
                | EffectParameters::Sharpen { .. }
                | EffectParameters::Vignette { .. }
                | EffectParameters::CornerPin { .. }
        ) {
            return self.resolve_standard(values);
        }
        // AUDIO-007/008: filters and dynamics are executed only by the audio
        // evaluator. The generic resolve still validates parameters so that
        // failures surface as typed errors before the domain rejection.
        if matches!(
            self.parameters,
            EffectParameters::AudioEq { .. }
                | EffectParameters::AudioHpf { .. }
                | EffectParameters::AudioLpf { .. }
                | EffectParameters::AudioCompressor { .. }
                | EffectParameters::AudioLimiter { .. }
        ) {
            self.resolve_audio(values)?;
            return Err(EffectError::UnsupportedFeature);
        }
        let sigma_id = match self.parameters {
            // Audio effects are executed only by the audio evaluator.
            EffectParameters::AudioGain { .. } => return Err(EffectError::UnsupportedFeature),
            EffectParameters::GaussianBlur { sigma }
            | EffectParameters::DropShadow { sigma, .. } => sigma,
            EffectParameters::AudioEq { .. }
            | EffectParameters::AudioHpf { .. }
            | EffectParameters::AudioLpf { .. }
            | EffectParameters::AudioCompressor { .. }
            | EffectParameters::AudioLimiter { .. }
            | EffectParameters::ColorExposure { .. }
            | EffectParameters::ColorLevels { .. }
            | EffectParameters::ColorCurves { .. }
            | EffectParameters::ColorHsl { .. }
            | EffectParameters::ChromaKey { .. }
            | EffectParameters::LumaKey { .. }
            | EffectParameters::Glow { .. }
            | EffectParameters::Sharpen { .. }
            | EffectParameters::Vignette { .. }
            | EffectParameters::CornerPin { .. }
            | EffectParameters::ColorLut { .. } => unreachable!("handled above"),
        };
        let sigma = scalar(sigma_id)?;
        if !(0.0..=1_000_000.0).contains(&sigma) {
            return Err(EffectError::InvalidParameter(sigma_id));
        }
        Ok(match self.parameters {
            EffectParameters::AudioGain { .. } => return Err(EffectError::UnsupportedFeature),
            EffectParameters::AudioEq { .. }
            | EffectParameters::AudioHpf { .. }
            | EffectParameters::AudioLpf { .. }
            | EffectParameters::AudioCompressor { .. }
            | EffectParameters::AudioLimiter { .. } => unreachable!("handled above"),
            EffectParameters::GaussianBlur { .. } => {
                if self.version == AFFINE_EFFECT_VERSION {
                    ResolvedEffect::AffineGaussianBlur {
                        sigma,
                        linear: [[1.0, 0.0], [0.0, 1.0]],
                    }
                } else {
                    ResolvedEffect::GaussianBlur { sigma }
                }
            }
            EffectParameters::DropShadow {
                offset,
                color,
                opacity,
                ..
            } => {
                let offset_value = match values.get(&offset) {
                    Some(Value::Vec2(v)) => v.map(FiniteF64::get),
                    _ => return Err(EffectError::InvalidParameter(offset)),
                };
                if offset_value.iter().any(|v| v.abs() > 1_000_000.0) {
                    return Err(EffectError::InvalidParameter(offset));
                }
                let color_value = match values.get(&color) {
                    Some(Value::Color(v)) => *v,
                    _ => return Err(EffectError::InvalidParameter(color)),
                };
                let opacity_value = scalar(opacity)?;
                if !(0.0..=1.0).contains(&opacity_value) {
                    return Err(EffectError::InvalidParameter(opacity));
                }
                if self.version == AFFINE_EFFECT_VERSION {
                    ResolvedEffect::AffineDropShadow {
                        sigma,
                        linear: [[1.0, 0.0], [0.0, 1.0]],
                        offset: offset_value,
                        color: color_value,
                        opacity: opacity_value,
                    }
                } else {
                    ResolvedEffect::DropShadow {
                        sigma,
                        offset: offset_value,
                        color: color_value,
                        opacity: opacity_value,
                    }
                }
            }
            _ => unreachable!("handled above"),
        })
    }
    /// FX-005/FX-006 parameter validation (ADR-0115). Probability-like
    /// parameters are range-checked; lengths and strengths share the 1e6
    /// scalar budget; corner pins are absolute Composition design_px points.
    fn resolve_standard(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let unit_interval = |id| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (0.0..=1.0).contains(&value) {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let nonnegative = |id| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (0.0..=1_000_000.0).contains(&value) {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let point = |id| -> Result<[f64; 2], EffectError> {
            match values.get(&id) {
                Some(Value::Vec2(v)) => {
                    let v = v.map(FiniteF64::get);
                    if v.iter().all(|c| c.abs() <= 1_000_000.0) {
                        Ok(v)
                    } else {
                        Err(EffectError::InvalidParameter(id))
                    }
                }
                _ => Err(EffectError::InvalidParameter(id)),
            }
        };
        match self.parameters {
            EffectParameters::ChromaKey {
                key_color,
                similarity,
                edge_shrink,
                edge_feather,
                spill,
            } => {
                let key_color = match values.get(&key_color) {
                    Some(Value::Color(v)) => *v,
                    _ => return Err(EffectError::InvalidParameter(key_color)),
                };
                Ok(ResolvedEffect::ChromaKey {
                    key_color,
                    similarity: unit_interval(similarity)?,
                    edge_shrink: nonnegative(edge_shrink)?,
                    edge_feather: nonnegative(edge_feather)?,
                    spill: unit_interval(spill)?,
                })
            }
            EffectParameters::LumaKey {
                key_luma,
                tolerance,
                edge_shrink,
                edge_feather,
            } => Ok(ResolvedEffect::LumaKey {
                key_luma: unit_interval(key_luma)?,
                tolerance: unit_interval(tolerance)?,
                edge_shrink: nonnegative(edge_shrink)?,
                edge_feather: nonnegative(edge_feather)?,
            }),
            EffectParameters::Glow {
                threshold,
                radius,
                intensity,
            } => Ok(ResolvedEffect::Glow {
                threshold: nonnegative(threshold)?,
                radius: nonnegative(radius)?,
                intensity: nonnegative(intensity)?,
            }),
            EffectParameters::Sharpen { amount, radius } => Ok(ResolvedEffect::Sharpen {
                amount: nonnegative(amount)?,
                radius: nonnegative(radius)?,
            }),
            EffectParameters::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => Ok(ResolvedEffect::Vignette {
                amount: unit_interval(amount)?,
                midpoint: unit_interval(midpoint)?,
                feather: nonnegative(feather)?,
                roundness: unit_interval(roundness)?,
            }),
            EffectParameters::CornerPin {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => Ok(ResolvedEffect::CornerPin {
                corners: [
                    point(top_left)?,
                    point(top_right)?,
                    point(bottom_right)?,
                    point(bottom_left)?,
                ],
            }),
            _ => unreachable!("standard resolution is only invoked for FX-005/006 variants"),
        }
    }
    /// COLOR-002 parameter validation happens here because ranges are
    /// cross-parameter (levels) or structural (curve table). Magnitude bounds
    /// match the existing 1e6 scalar budget.
    fn resolve_color(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let bounded = |id, limit: f64| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if value.abs() <= limit {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        match self.parameters {
            EffectParameters::ColorExposure { exposure, offset } => {
                Ok(ResolvedEffect::ColorExposure {
                    exposure: bounded(exposure, 1_024.0)?,
                    offset: bounded(offset, 1_000_000.0)?,
                })
            }
            EffectParameters::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                let in_black_v = bounded(in_black, 1_000_000.0)?;
                let in_white_v = bounded(in_white, 1_000_000.0)?;
                if in_white_v <= in_black_v {
                    return Err(EffectError::InvalidParameter(in_white));
                }
                let gamma_v = scalar(gamma)?;
                if !(gamma_v > 0.0 && gamma_v <= 1_000_000.0) {
                    return Err(EffectError::InvalidParameter(gamma));
                }
                Ok(ResolvedEffect::ColorLevels {
                    in_black: in_black_v,
                    in_white: in_white_v,
                    gamma: gamma_v,
                    out_black: bounded(out_black, 1_000_000.0)?,
                    out_white: bounded(out_white, 1_000_000.0)?,
                })
            }
            EffectParameters::ColorCurves { curve } => {
                let table = match values.get(&curve) {
                    Some(Value::DataTable(t)) => t,
                    _ => return Err(EffectError::InvalidParameter(curve)),
                };
                let points = curve_points(table).ok_or(EffectError::InvalidParameter(curve))?;
                Ok(ResolvedEffect::ColorCurves { curve: points })
            }
            EffectParameters::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => {
                let hue = match values.get(&hue_shift) {
                    Some(Value::Angle(v)) => v.get(),
                    _ => return Err(EffectError::InvalidParameter(hue_shift)),
                };
                if hue.abs() > 1_000_000.0 {
                    return Err(EffectError::InvalidParameter(hue_shift));
                }
                Ok(ResolvedEffect::ColorHsl {
                    hue_shift: hue,
                    saturation: bounded(saturation, 1_000_000.0)?,
                    lightness: bounded(lightness, 1_000_000.0)?,
                })
            }
            EffectParameters::ColorLut { lut, intensity } => {
                // The asset id resolves only the authored reference; lattice
                // bytes come from the snapshot luts input (ADR-0113).
                let asset = match values.get(&lut) {
                    Some(Value::AssetRef(id)) => *id,
                    _ => return Err(EffectError::InvalidParameter(lut)),
                };
                let intensity_value = scalar(intensity)?;
                if !(0.0..=1.0).contains(&intensity_value) {
                    return Err(EffectError::InvalidParameter(intensity));
                }
                Ok(ResolvedEffect::ColorLut {
                    lut: asset,
                    intensity: intensity_value,
                })
            }
            _ => unreachable!("color resolution is only invoked for color variants"),
        }
    }
    /// AUDIO-007/008: resolve constant audio filter/dynamics parameters into
    /// the deterministic DSP specification shared by playback and export
    /// (ADR-0117). Every failure is a typed parameter error.
    pub fn resolve_audio(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedAudioEffect, EffectError> {
        self.ensure_supported()?;
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let bounded = |id, min: f64, max: f64| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (min..=max).contains(&value) {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let frequency = |id| {
            let value = scalar(id)?;
            if value > 0.0 && value < AUDIO_FILTER_MAX_FREQ_HZ {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let milliseconds = |id| bounded(id, AUDIO_MIN_TIME_MS, AUDIO_MAX_TIME_MS);
        let decibels = |id| bounded(id, -AUDIO_MAX_DB, AUDIO_MAX_DB);
        let order = |id| {
            let value = scalar(id)?;
            if value.fract() == 0.0
                && (1..=i64::from(AUDIO_FILTER_MAX_ORDER)).contains(&(value as i64))
            {
                Ok(value as u32)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        match self.parameters {
            EffectParameters::AudioEq { bands } => {
                let table = match values.get(&bands) {
                    Some(Value::DataTable(t)) => t,
                    _ => return Err(EffectError::InvalidParameter(bands)),
                };
                let rows = eq_bands(table).ok_or(EffectError::InvalidParameter(bands))?;
                Ok(ResolvedAudioEffect::Eq { bands: rows })
            }
            EffectParameters::AudioHpf {
                cutoff_hz,
                order: o,
            } => Ok(ResolvedAudioEffect::Hpf {
                cutoff_hz: frequency(cutoff_hz)?,
                order: order(o)?,
            }),
            EffectParameters::AudioLpf {
                cutoff_hz,
                order: o,
            } => Ok(ResolvedAudioEffect::Lpf {
                cutoff_hz: frequency(cutoff_hz)?,
                order: order(o)?,
            }),
            EffectParameters::AudioCompressor {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_db,
            } => Ok(ResolvedAudioEffect::Compressor {
                threshold_db: decibels(threshold_db)?,
                ratio: bounded(ratio, 1.0, AUDIO_MAX_RATIO)?,
                attack_ms: milliseconds(attack_ms)?,
                release_ms: milliseconds(release_ms)?,
                makeup_db: decibels(makeup_db)?,
            }),
            EffectParameters::AudioLimiter {
                ceiling_db,
                release_ms,
            } => Ok(ResolvedAudioEffect::Limiter {
                ceiling_db: bounded(ceiling_db, -AUDIO_MAX_DB, 0.0)?,
                release_ms: milliseconds(release_ms)?,
            }),
            _ => Err(EffectError::UnsupportedFeature),
        }
    }
}
/// Validate and extract the AUDIO-007 EQ band table: exactly four columns
/// `kind` (enum: peak | low_shelf | high_shelf), `freq_hz`, `gain_db` and `q`
/// (scalars), 1..=AUDIO_EQ_MAX_BANDS rows (ADR-0117).
fn eq_bands(table: &crate::DataTable) -> Option<Vec<AudioEqBand>> {
    if table.columns.len() != 4
        || table.columns.get("kind") != Some(&ValueType::Enum)
        || table.columns.get("freq_hz") != Some(&ValueType::Scalar)
        || table.columns.get("gain_db") != Some(&ValueType::Scalar)
        || table.columns.get("q") != Some(&ValueType::Scalar)
        || table.rows.is_empty()
        || table.rows.len() > AUDIO_EQ_MAX_BANDS
    {
        return None;
    }
    let mut bands = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        if row.len() != 4 {
            return None;
        }
        let kind = match row.get("kind") {
            Some(Value::Enum(v)) if v == "peak" => AudioEqBandKind::Peak,
            Some(Value::Enum(v)) if v == "low_shelf" => AudioEqBandKind::LowShelf,
            Some(Value::Enum(v)) if v == "high_shelf" => AudioEqBandKind::HighShelf,
            _ => return None,
        };
        let (Some(Value::Scalar(freq)), Some(Value::Scalar(gain)), Some(Value::Scalar(q))) =
            (row.get("freq_hz"), row.get("gain_db"), row.get("q"))
        else {
            return None;
        };
        let (freq_hz, gain_db, q) = (freq.get(), gain.get(), q.get());
        if !(freq_hz > 0.0
            && freq_hz < AUDIO_FILTER_MAX_FREQ_HZ
            && gain_db.abs() <= AUDIO_MAX_DB
            && q > 0.0
            && q <= AUDIO_EQ_MAX_Q)
        {
            return None;
        }
        bands.push(AudioEqBand {
            kind,
            freq_hz,
            gain_db,
            q,
        });
    }
    Some(bands)
}
/// Validate and extract the COLOR-002 curve table: exactly two scalar columns
/// named `x` and `y`, 2..=CURVES_MAX_POINTS rows, strictly increasing `x`,
/// both coordinates in [0,1].
fn curve_points(table: &crate::DataTable) -> Option<Vec<[f64; 2]>> {
    if table.columns.len() != 2
        || table.columns.get("x") != Some(&ValueType::Scalar)
        || table.columns.get("y") != Some(&ValueType::Scalar)
        || !(2..=CURVES_MAX_POINTS).contains(&table.rows.len())
    {
        return None;
    }
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        if row.len() != 2 {
            return None;
        }
        let (Some(Value::Scalar(x)), Some(Value::Scalar(y))) = (row.get("x"), row.get("y")) else {
            return None;
        };
        let (x, y) = (x.get(), y.get());
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return None;
        }
        if let Some(last) = points.last()
            && last[0] >= x
        {
            return None;
        }
        points.push([x, y]);
    }
    Some(points)
}
impl Effect {
    pub fn definition(&self) -> Result<&EffectDefinition, EffectError> {
        match self {
            Self::Known(d) => {
                d.ensure_supported()?;
                Ok(d)
            }
            Self::Opaque(_) => Err(EffectError::UnsupportedFeature),
        }
    }
}
/// Identity two-point curve table used as the COLOR-002 curves default.
fn curve_default() -> Value {
    let f = |v| FiniteF64::new(v).expect("finite curve default");
    let columns = BTreeMap::from([
        ("x".to_string(), ValueType::Scalar),
        ("y".to_string(), ValueType::Scalar),
    ]);
    let row = |x, y| {
        BTreeMap::from([
            ("x".to_string(), Value::Scalar(f(x))),
            ("y".to_string(), Value::Scalar(f(y))),
        ])
    };
    Value::DataTable(crate::DataTable {
        columns,
        rows: vec![row(0.0, 0.0), row(1.0, 1.0)],
    })
}
/// Identity single-band EQ table used as the AUDIO-007 `eq_bands` default.
fn eq_bands_default() -> Value {
    let f = |v| FiniteF64::new(v).expect("finite eq default");
    let columns = BTreeMap::from([
        ("kind".to_string(), ValueType::Enum),
        ("freq_hz".to_string(), ValueType::Scalar),
        ("gain_db".to_string(), ValueType::Scalar),
        ("q".to_string(), ValueType::Scalar),
    ]);
    Value::DataTable(crate::DataTable {
        columns,
        rows: vec![BTreeMap::from([
            ("kind".to_string(), Value::Enum("peak".to_string())),
            ("freq_hz".to_string(), Value::Scalar(f(1_000.0))),
            ("gain_db".to_string(), Value::Scalar(f(0.0))),
            ("q".to_string(), Value::Scalar(f(1.0))),
        ])],
    })
}
pub fn effect_descriptors() -> Vec<PropertyDescriptor> {
    let f = |v| FiniteF64::new(v).expect("finite effect default");
    [
        (
            0xf0000000_0010_4100_8000_000000000001,
            "sigma",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4100_8000_000000000002,
            "offset",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4100_8000_000000000003,
            "color",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000004,
            "opacity",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000005,
            "exposure",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000006,
            "exposure_offset",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000007,
            "in_black",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000008,
            "in_white",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_000000000009,
            "gamma",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000a,
            "out_black",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000b,
            "out_white",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000c,
            "curve",
            curve_default(),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000d,
            "hue_shift",
            Value::Angle(f(0.0)),
            Unit::Degrees,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000e,
            "saturation",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4100_8000_00000000000f,
            "lightness",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        // FX-005 keying descriptors (ADR-0115).
        (
            0xf0000000_0010_4300_8000_000000000001,
            "key_color",
            Value::Color(Color::from_srgb8([0, 177, 64], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000002,
            "key_luma",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000003,
            "similarity",
            Value::Scalar(f(0.4)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000004,
            "tolerance",
            Value::Scalar(f(0.1)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000005,
            "edge_shrink",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000006,
            "edge_feather",
            Value::Scalar(f(0.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000007,
            "spill",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        // FX-006 standard effect descriptors (ADR-0115).
        (
            0xf0000000_0010_4300_8000_000000000008,
            "threshold",
            Value::Scalar(f(0.8)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_000000000009,
            "radius",
            Value::Scalar(f(8.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000a,
            "intensity",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4200_8000_000000000001,
            "lut",
            Value::AssetRef(AssetId::from_uuid(uuid::Uuid::nil())),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000b,
            "amount",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000c,
            "midpoint",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000d,
            "feather",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000e,
            "roundness",
            Value::Scalar(f(0.5)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4300_8000_00000000000f,
            "top_left",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000010,
            "top_right",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000011,
            "bottom_right",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4300_8000_000000000012,
            "bottom_left",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        // AUDIO-007/008 parameter descriptors (ADR-0117).
        (
            0xf0000000_0010_4500_8000_000000000001,
            "eq_bands",
            eq_bands_default(),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000002,
            "cutoff_hz",
            Value::Scalar(f(1_000.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000003,
            "order",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000004,
            "threshold_db",
            Value::Scalar(f(-18.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000005,
            "ratio",
            Value::Scalar(f(4.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000006,
            "attack_ms",
            Value::Scalar(f(10.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000007,
            "release_ms",
            Value::Scalar(f(100.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000008,
            "makeup_db",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4500_8000_000000000009,
            "ceiling_db",
            Value::Scalar(f(-1.0)),
            Unit::Dimensionless,
        ),
    ]
    .into_iter()
    .map(|(id, name, value, unit)| {
        let mut d = DescriptorDefinition::new(
            DescriptorId::from_uuid(uuid::Uuid::from_u128(id)),
            SchemaKey::new(format!("kronello.effect.{name}")).expect("effect key"),
            name,
            value.value_type(),
            unit,
            value,
        );
        d.repeatable = true;
        if name == "offset" {
            d.coordinate_space = Some(crate::CoordinateSpace::LocalDesign);
        }
        // Corner pins are absolute Composition design_px positions.
        if matches!(
            name,
            "top_left" | "top_right" | "bottom_right" | "bottom_left"
        ) {
            d.coordinate_space = Some(crate::CoordinateSpace::CompositionDesign);
        }
        if name == "sigma" || name == "opacity" || name == "intensity" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, if name == "sigma" { 1_000_000.0 } else { 1.0 })
                    .expect("effect range"),
            ));
        }
        if matches!(
            name,
            "edge_shrink"
                | "edge_feather"
                | "radius"
                | "threshold"
                | "intensity"
                | "amount"
                | "feather"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, 1_000_000.0).expect("effect range"),
            ));
        }
        if matches!(
            name,
            "similarity" | "spill" | "key_luma" | "tolerance" | "midpoint" | "roundness"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, 1.0).expect("effect range"),
            ));
        }
        if name == "gamma" {
            let mut bound = NumericRange::inclusive(0.0, 1_000_000.0).expect("gamma range");
            bound.min.as_mut().expect("min").inclusive = false;
            d.range = Some(ValueRange::Scalar(bound));
        }
        PropertyDescriptor::new(d).expect("effect descriptor")
    })
    .collect()
}

// Decode JSON directly, avoiding serde's untagged Content buffer so future
// parameters keep arbitrary-precision numbers just like DocumentObject.
impl<'de> Deserialize<'de> for Effect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(deserializer)?;
        let definition = serde_json::from_str::<EffectDefinition>(&raw.to_string());
        Ok(match definition {
            Ok(d) => Self::Known(d),
            Err(_) => Self::Opaque(raw),
        })
    }
}
