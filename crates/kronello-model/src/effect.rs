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
/// AUDIO-011 (ADR-0131): hash-pinned third-party audio plugin binding.
/// Never executed by the in-process evaluator; processing happens only in
/// the detached plugin worker via `audio.plugin_process` / plugin jobs.
pub const AUDIO_PLUGIN_ID: &str = "kronello.audio.plugin";
/// AUDIO-011 supported semantic version.
pub const AUDIO_PLUGIN_VERSION: u32 = EFFECT_VERSION;
/// AUDIO-011: maximum rows in the `plugin_parameters` data table.
pub const AUDIO_PLUGIN_MAX_PARAMS: usize = 1_024;
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

/// TRACK-002 (ADR-0122): tracking-driven stabilization.
pub const STABILIZE_ID: &str = "kronello.stabilize";
pub const STABILIZE_VERSION: u32 = 1;

/// FX-008 (ADR-0137): remaining standard video effects. All nine share one
/// version family; every variant debuts at version 1.
pub const GRAIN_ID: &str = "kronello.grain";
pub const MOSAIC_ID: &str = "kronello.mosaic";
pub const INVERT_ID: &str = "kronello.invert";
pub const CHANNEL_MIXER_ID: &str = "kronello.channel_mixer";
pub const TINT_ID: &str = "kronello.tint";
pub const DIRECTIONAL_BLUR_ID: &str = "kronello.directional_blur";
pub const RADIAL_BLUR_ID: &str = "kronello.radial_blur";
pub const DISPLACE_ID: &str = "kronello.displace";
pub const GENERATE_ID: &str = "kronello.generate";
/// FX-008 supported version is 1 for every added video effect.
pub const FX008_EFFECT_VERSION: u32 = 1;
/// FX-008 audio effects, versioned like the earlier audio family.
pub const AUDIO_DELAY_ID: &str = "kronello.audio.delay";
pub const AUDIO_REVERB_ID: &str = "kronello.audio.reverb";
pub const AUDIO_PITCH_ID: &str = "kronello.audio.pitch";
pub const AUDIO_GATE_ID: &str = "kronello.audio.gate";
/// FX-008: all nine versioned video effect ids, for snapshot pinning loops.
pub const FX008_EFFECT_IDS: [&str; 9] = [
    GRAIN_ID,
    MOSAIC_ID,
    INVERT_ID,
    CHANNEL_MIXER_ID,
    TINT_ID,
    DIRECTIONAL_BLUR_ID,
    RADIAL_BLUR_ID,
    DISPLACE_ID,
    GENERATE_ID,
];
/// FX-008: all four versioned audio effect ids.
pub const FX008_AUDIO_IDS: [&str; 4] = [
    AUDIO_DELAY_ID,
    AUDIO_REVERB_ID,
    AUDIO_PITCH_ID,
    AUDIO_GATE_ID,
];
/// Grain temporal hashing folds the scene's rational time into the authored
/// seed; the value is a signed 31-bit integer.
pub const GRAIN_MAX_SEED: f64 = 2_147_483_647.0;
/// Pitch shift bound in semitones; ±48 spans four octaves each way.
pub const AUDIO_PITCH_MAX_SEMITONES: f64 = 48.0;
/// Gate hysteresis bound in dB.
pub const AUDIO_GATE_MAX_HYSTERESIS_DB: f64 = 120.0;
/// Channel-mixer coefficient magnitude bound.
pub const CHANNEL_MIXER_MAX_COEFFICIENT: f64 = 1_024.0;

/// FX-008: sampling basis for `kronello.mosaic`. `center` samples the block
/// center texel; `edge` samples the block's top-left texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MosaicBasis {
    Center,
    Edge,
}
/// FX-008: channel selection for `kronello.invert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvertChannel {
    Rgb,
    Red,
    Green,
    Blue,
    Alpha,
}
/// FX-008: displacement-map channel for `kronello.displace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplaceChannel {
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
}
/// FX-008: radial blur shape for `kronello.radial_blur`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RadialBlurMode {
    Spin,
    Zoom,
}
/// FX-008: synthesized layer kind for `kronello.generate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GenerateKind {
    GradientLinear,
    GradientRadial,
    Checkerboard,
    Grid,
}

/// TRACK-002: how the stabilized output covers regions the inverse warp maps
/// outside the source frame. `Fill` paints `fill_color`; `replicate` clamps
/// to edge texels; `reflect` mirrors the frame at its boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StabilizeBorder {
    Fill,
    Replicate,
    Reflect,
}

/// TRACK-002: source-frame resampling for the stabilized inverse warp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StabilizeSampling {
    Nearest,
    Bilinear,
}

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
    /// AUDIO-011 (ADR-0131): hash-pinned audio plugin binding. The document
    /// carries only identity + pin metadata — a bundle path string, format,
    /// component id, manifest hash and version — never plugin bytes. The
    /// binding is processed exclusively by the detached plugin worker; the
    /// audio evaluator rejects it with `UNSUPPORTED_FEATURE`.
    AudioPlugin {
        /// Bundle directory or module file path.
        bundle: PropertyId,
        /// `vst3` | `audio_unit`.
        format: PropertyId,
        /// VST3 32-hex class id or AU `type:subtype:manufacturer` triplet.
        component: PropertyId,
        /// Lowercase hex SHA-256 manifest pin (empty for built-in AUs).
        sha256: PropertyId,
        /// Recorded plugin version pin (AU hex version / VST3 class
        /// version string; empty = unpinned).
        plugin_version: PropertyId,
        /// DataTable `{ param: Scalar, value: Scalar }` — VST3 normalized
        /// parameter ids, AU native parameter values; ≤1024 rows.
        parameters: PropertyId,
    },
    /// TRACK-002 (ADR-0122): invert tracked camera motion. `tracking`
    /// references a `TrackingDataAsset`; the smoothed trajectory uses a
    /// symmetric window of `2*smoothing_radius+1` tracked samples;
    /// `max_displacement` (design px), `max_rotation` (degrees) and
    /// `max_crop` (unit interval of the source frame area) bound the applied
    /// correction; `border` selects the uncovered-region policy with
    /// `fill_color` for `fill`; `sampling` picks the resampler.
    Stabilize {
        tracking: PropertyId,
        smoothing_radius: PropertyId,
        max_displacement: PropertyId,
        max_rotation: PropertyId,
        max_crop: PropertyId,
        border: PropertyId,
        fill_color: PropertyId,
        sampling: PropertyId,
    },
    /// FX-008 (ADR-0137): deterministic film grain. `amount` scales the
    /// injected noise on 0..=1; `size` is the noise cell edge in design_px;
    /// `monochrome` shares one lattice across RGB; `seed` is a signed 31-bit
    /// integer folded with the scene time into the hash lattice.
    Grain {
        amount: PropertyId,
        size: PropertyId,
        monochrome: PropertyId,
        seed: PropertyId,
    },
    /// FX-008: pixelation. `block_size` is the block edge in design_px;
    /// `basis` selects the sampled texel inside each block.
    Mosaic {
        block_size: PropertyId,
        basis: PropertyId,
    },
    /// FX-008: straight-color inversion. `channel` selects which channels
    /// invert; alpha inversions keep straight RGB and recombine premultiplied.
    Invert {
        channel: PropertyId,
    },
    /// FX-008: 4x4 channel matrix on premultiplied RGBA. `matrix` references
    /// a data table with scalar columns `red`, `green`, `blue`, `alpha` and
    /// exactly four rows (row = output channel in r,g,b,a order).
    ChannelMixer {
        matrix: PropertyId,
    },
    /// FX-008: maps straight working luminance onto the black→white ramp and
    /// blends by `amount`; alpha is preserved.
    Tint {
        map_black: PropertyId,
        map_white: PropertyId,
        amount: PropertyId,
    },
    /// FX-008: uniform motion blur. `angle` is the direction in degrees,
    /// `length` the total blur span in design_px.
    DirectionalBlur {
        angle: PropertyId,
        length: PropertyId,
    },
    /// FX-008: spin/zoom radial blur around `center` (node-local design_px).
    /// `amount` is the total rotation in degrees for `spin` and the inward
    /// scale extent on 0..=1 for `zoom`.
    RadialBlur {
        mode: PropertyId,
        amount: PropertyId,
        center: PropertyId,
    },
    /// FX-008: displacement-map warp. The map layer is an explicitly bound
    /// secondary input (snapshot displacement binding); `channel_x` /
    /// `channel_y` pick the map channels and `scale_x` / `scale_y` are the
    /// signed maximum displacements in node-local design_px.
    Displace {
        channel_x: PropertyId,
        channel_y: PropertyId,
        scale_x: PropertyId,
        scale_y: PropertyId,
    },
    /// FX-008: source-independent layer synthesis replacing the incoming
    /// raster at its chain position. `kind` selects gradient/checkerboard/
    /// grid; `point_a`/`point_b` are node-local design_px anchors (gradient
    /// endpoints; radial uses `point_a` as center and `|b-a|` as radius);
    /// `cell_size` is the checker/grid cell edge and `line_width` the grid
    /// line width, both design_px.
    Generate {
        generator: PropertyId,
        color_a: PropertyId,
        color_b: PropertyId,
        point_a: PropertyId,
        point_b: PropertyId,
        cell_size: PropertyId,
        line_width: PropertyId,
    },
    /// FX-008: integer-sample echo. `delay_ms` rounds to whole 48 kHz
    /// samples; `feedback_db` (<=0) recirculates; `wet`/`dry` mix on 0..=1.
    AudioDelay {
        delay_ms: PropertyId,
        feedback_db: PropertyId,
        wet: PropertyId,
        dry: PropertyId,
    },
    /// FX-008 (ADR-0139): deterministic feedback-comb reverb realizing an
    /// algorithmically generated room response. `decay_ms` is the RT60
    /// target; `damping` is the in-loop lowpass amount on 0..=1.
    AudioReverb {
        decay_ms: PropertyId,
        damping: PropertyId,
        wet: PropertyId,
        dry: PropertyId,
    },
    /// FX-008: semitone pitch shift executed at the source stage through the
    /// deterministic WSOLA rate-resampled window (±AUDIO_PITCH_MAX_SEMITONES).
    AudioPitch {
        semitones: PropertyId,
    },
    /// FX-008: noise gate with hysteresis. `threshold_db` is the open level;
    /// the gate closes below `threshold_db - hysteresis_db`; `attack_ms` /
    /// `release_ms` smooth the open/close gain ramps.
    AudioGate {
        threshold_db: PropertyId,
        attack_ms: PropertyId,
        release_ms: PropertyId,
        hysteresis_db: PropertyId,
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
    /// TRACK-002 (ADR-0122) resolved effect. `tracking` is the authored
    /// `TrackingDataAsset` id; `inverse` is the output-frame→source-frame
    /// inverse correction (2x3 row-major affine, source pixel space) resolved
    /// per frame by the scene pass. `None` until that pass binds the tracking
    /// data to the resolved source time.
    Stabilize {
        tracking: AssetId,
        smoothing_radius: u32,
        max_displacement: f64,
        max_rotation: f64,
        max_crop: f64,
        border: StabilizeBorder,
        fill_color: Color,
        sampling: StabilizeSampling,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inverse: Option<[[f64; 3]; 2]>,
    },
    /// FX-008: `seed` is the resolved signed 31-bit integer; the DAG builder
    /// folds the scene time into it so the noise field is temporal.
    Grain {
        amount: f64,
        size: f64,
        monochrome: bool,
        seed: i64,
    },
    /// FX-008: `block_size` is the square block edge in design_px.
    Mosaic {
        block_size: f64,
        basis: MosaicBasis,
    },
    Invert {
        channel: InvertChannel,
    },
    /// FX-008: row-major premultiplied RGBA matrix (row = output channel).
    ChannelMixer {
        matrix: [[f64; 4]; 4],
    },
    Tint {
        map_black: Color,
        map_white: Color,
        amount: f64,
    },
    /// FX-008: `angle_degrees` in the node's local design space; the DAG
    /// mapper rotates it into the output lattice with the transform.
    DirectionalBlur {
        angle_degrees: f64,
        length: f64,
    },
    /// FX-008: `center` is node-local design_px; `amount` is degrees for
    /// `spin`, the 0..=1 inward extent for `zoom`.
    RadialBlur {
        mode: RadialBlurMode,
        amount: f64,
        center: [f64; 2],
    },
    /// FX-008: `displacement` is the design_px offset contributed per unit
    /// signed channel value; column 0 maps `channel_x`, column 1 maps
    /// `channel_y`. Resolution stores `diag(scale_x, scale_y)`; the DAG
    /// mapper composes the node transform behind it so rotations shear the
    /// displacement axes exactly like drop-shadow offsets.
    Displace {
        channel_x: DisplaceChannel,
        channel_y: DisplaceChannel,
        displacement: [[f64; 2]; 2],
    },
    /// FX-008: all geometry in node-local design_px; see `EffectParameters`.
    Generate {
        generator: GenerateKind,
        color_a: Color,
        color_b: Color,
        point_a: [f64; 2],
        point_b: [f64; 2],
        cell_size: f64,
        line_width: f64,
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
    /// FX-008: `delay_samples` is the integer 48 kHz delay resolved from
    /// `delay_ms`; `feedback_db` is the recirculation gain in dB (<=0).
    Delay {
        delay_samples: u32,
        feedback_db: f64,
        wet: f64,
        dry: f64,
    },
    /// FX-008 (ADR-0139): deterministic feedback-comb reverb. `decay_s` is
    /// the RT60 target in seconds; `damping` is 0..=1.
    Reverb {
        decay_s: f64,
        damping: f64,
        wet: f64,
        dry: f64,
    },
    /// FX-008: semitone pitch shift; executed at the source stage, never as
    /// an in-chain processor.
    Pitch {
        semitones: f64,
    },
    Gate {
        threshold_db: f64,
        attack_ms: f64,
        release_ms: f64,
        hysteresis_db: f64,
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
            EffectParameters::AudioPlugin { .. } => (AUDIO_PLUGIN_ID, AUDIO_PLUGIN_VERSION),
            EffectParameters::Stabilize { .. } => (STABILIZE_ID, STABILIZE_VERSION),
            EffectParameters::Grain { .. } => (GRAIN_ID, FX008_EFFECT_VERSION),
            EffectParameters::Mosaic { .. } => (MOSAIC_ID, FX008_EFFECT_VERSION),
            EffectParameters::Invert { .. } => (INVERT_ID, FX008_EFFECT_VERSION),
            EffectParameters::ChannelMixer { .. } => (CHANNEL_MIXER_ID, FX008_EFFECT_VERSION),
            EffectParameters::Tint { .. } => (TINT_ID, FX008_EFFECT_VERSION),
            EffectParameters::DirectionalBlur { .. } => (DIRECTIONAL_BLUR_ID, FX008_EFFECT_VERSION),
            EffectParameters::RadialBlur { .. } => (RADIAL_BLUR_ID, FX008_EFFECT_VERSION),
            EffectParameters::Displace { .. } => (DISPLACE_ID, FX008_EFFECT_VERSION),
            EffectParameters::Generate { .. } => (GENERATE_ID, FX008_EFFECT_VERSION),
            EffectParameters::AudioDelay { .. } => (AUDIO_DELAY_ID, EFFECT_VERSION),
            EffectParameters::AudioReverb { .. } => (AUDIO_REVERB_ID, EFFECT_VERSION),
            EffectParameters::AudioPitch { .. } => (AUDIO_PITCH_ID, EFFECT_VERSION),
            EffectParameters::AudioGate { .. } => (AUDIO_GATE_ID, EFFECT_VERSION),
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
            EffectParameters::AudioPlugin {
                bundle,
                format,
                component,
                sha256,
                plugin_version,
                parameters,
            } => vec![
                (bundle, ValueType::String, Unit::Dimensionless),
                (format, ValueType::Enum, Unit::Dimensionless),
                (component, ValueType::String, Unit::Dimensionless),
                (sha256, ValueType::String, Unit::Dimensionless),
                (plugin_version, ValueType::String, Unit::Dimensionless),
                (parameters, ValueType::DataTable, Unit::Dimensionless),
            ],
            EffectParameters::Stabilize {
                tracking,
                smoothing_radius,
                max_displacement,
                max_rotation,
                max_crop,
                border,
                fill_color,
                sampling,
            } => vec![
                (tracking, ValueType::AssetRef, Unit::Dimensionless),
                (smoothing_radius, ValueType::Scalar, Unit::Dimensionless),
                (max_displacement, ValueType::Scalar, Unit::DesignPx),
                (max_rotation, ValueType::Angle, Unit::Degrees),
                (max_crop, ValueType::Scalar, Unit::Dimensionless),
                (border, ValueType::Enum, Unit::Dimensionless),
                (fill_color, ValueType::Color, Unit::Dimensionless),
                (sampling, ValueType::Enum, Unit::Dimensionless),
            ],
            EffectParameters::Grain {
                amount,
                size,
                monochrome,
                seed,
            } => vec![
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (size, ValueType::Scalar, Unit::DesignPx),
                (monochrome, ValueType::Bool, Unit::Dimensionless),
                (seed, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::Mosaic { block_size, basis } => vec![
                (block_size, ValueType::Scalar, Unit::DesignPx),
                (basis, ValueType::Enum, Unit::Dimensionless),
            ],
            EffectParameters::Invert { channel } => {
                vec![(channel, ValueType::Enum, Unit::Dimensionless)]
            }
            EffectParameters::ChannelMixer { matrix } => {
                vec![(matrix, ValueType::DataTable, Unit::Dimensionless)]
            }
            EffectParameters::Tint {
                map_black,
                map_white,
                amount,
            } => vec![
                (map_black, ValueType::Color, Unit::Dimensionless),
                (map_white, ValueType::Color, Unit::Dimensionless),
                (amount, ValueType::Scalar, Unit::Dimensionless),
            ],
            EffectParameters::DirectionalBlur { angle, length } => vec![
                (angle, ValueType::Angle, Unit::Degrees),
                (length, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::RadialBlur {
                mode,
                amount,
                center,
            } => vec![
                (mode, ValueType::Enum, Unit::Dimensionless),
                (amount, ValueType::Scalar, Unit::Dimensionless),
                (center, ValueType::Vec2, Unit::DesignPx),
            ],
            EffectParameters::Displace {
                channel_x,
                channel_y,
                scale_x,
                scale_y,
            } => vec![
                (channel_x, ValueType::Enum, Unit::Dimensionless),
                (channel_y, ValueType::Enum, Unit::Dimensionless),
                (scale_x, ValueType::Scalar, Unit::DesignPx),
                (scale_y, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::Generate {
                generator: kind,
                color_a,
                color_b,
                point_a,
                point_b,
                cell_size,
                line_width,
            } => vec![
                (kind, ValueType::Enum, Unit::Dimensionless),
                (color_a, ValueType::Color, Unit::Dimensionless),
                (color_b, ValueType::Color, Unit::Dimensionless),
                (point_a, ValueType::Vec2, Unit::DesignPx),
                (point_b, ValueType::Vec2, Unit::DesignPx),
                (cell_size, ValueType::Scalar, Unit::DesignPx),
                (line_width, ValueType::Scalar, Unit::DesignPx),
            ],
            EffectParameters::AudioDelay {
                delay_ms,
                feedback_db,
                wet,
                dry,
            } => [delay_ms, feedback_db, wet, dry]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::AudioReverb {
                decay_ms,
                damping,
                wet,
                dry,
            } => [decay_ms, damping, wet, dry]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
            EffectParameters::AudioPitch { semitones } => {
                vec![(semitones, ValueType::Scalar, Unit::Dimensionless)]
            }
            EffectParameters::AudioGate {
                threshold_db,
                attack_ms,
                release_ms,
                hysteresis_db,
            } => [threshold_db, attack_ms, release_ms, hysteresis_db]
                .into_iter()
                .map(|id| (id, ValueType::Scalar, Unit::Dimensionless))
                .collect(),
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
        // TRACK-002 (ADR-0122): stabilize carries the authored tracking
        // reference and evaluated parameters; the per-frame inverse transform
        // is bound later by the scene pass.
        if matches!(self.parameters, EffectParameters::Stabilize { .. }) {
            return self.resolve_stabilize(values);
        }
        // FX-008 (ADR-0137): remaining standard video effects.
        if matches!(
            self.parameters,
            EffectParameters::Grain { .. }
                | EffectParameters::Mosaic { .. }
                | EffectParameters::Invert { .. }
                | EffectParameters::ChannelMixer { .. }
                | EffectParameters::Tint { .. }
                | EffectParameters::DirectionalBlur { .. }
                | EffectParameters::RadialBlur { .. }
                | EffectParameters::Displace { .. }
                | EffectParameters::Generate { .. }
        ) {
            return self.resolve_fx008(values);
        }
        // AUDIO-007/008/FX-008: filters and dynamics are executed only by the
        // audio evaluator. The generic resolve still validates parameters so
        // that failures surface as typed errors before the domain rejection.
        if matches!(
            self.parameters,
            EffectParameters::AudioEq { .. }
                | EffectParameters::AudioHpf { .. }
                | EffectParameters::AudioLpf { .. }
                | EffectParameters::AudioCompressor { .. }
                | EffectParameters::AudioLimiter { .. }
                | EffectParameters::AudioPlugin { .. }
                | EffectParameters::AudioDelay { .. }
                | EffectParameters::AudioReverb { .. }
                | EffectParameters::AudioPitch { .. }
                | EffectParameters::AudioGate { .. }
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
            | EffectParameters::AudioPlugin { .. }
            | EffectParameters::AudioDelay { .. }
            | EffectParameters::AudioReverb { .. }
            | EffectParameters::AudioPitch { .. }
            | EffectParameters::AudioGate { .. }
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
            | EffectParameters::ColorLut { .. }
            | EffectParameters::Stabilize { .. }
            | EffectParameters::Grain { .. }
            | EffectParameters::Mosaic { .. }
            | EffectParameters::Invert { .. }
            | EffectParameters::ChannelMixer { .. }
            | EffectParameters::Tint { .. }
            | EffectParameters::DirectionalBlur { .. }
            | EffectParameters::RadialBlur { .. }
            | EffectParameters::Displace { .. }
            | EffectParameters::Generate { .. } => unreachable!("handled above"),
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
            | EffectParameters::AudioLimiter { .. }
            | EffectParameters::AudioPlugin { .. } => unreachable!("handled above"),
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
    /// TRACK-002 (ADR-0122) parameter validation. Enum strings parse into the
    /// versioned policy enums; numeric parameters share the effect budgets;
    /// `smoothing_radius` accepts integral scalars only. The inverse warp is
    /// scene-resolved, so `inverse` starts unset.
    fn resolve_stabilize(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedEffect, EffectError> {
        let scalar = |id| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(EffectError::InvalidParameter(id)),
        };
        let EffectParameters::Stabilize {
            tracking,
            smoothing_radius,
            max_displacement,
            max_rotation,
            max_crop,
            border,
            fill_color,
            sampling,
        } = self.parameters
        else {
            unreachable!("stabilize resolution is only invoked for the stabilize variant")
        };
        let tracking = match values.get(&tracking) {
            Some(Value::AssetRef(id)) => *id,
            _ => return Err(EffectError::InvalidParameter(tracking)),
        };
        let radius = scalar(smoothing_radius)?;
        if !(radius.fract() == 0.0 && (0.0..=4096.0).contains(&radius)) {
            return Err(EffectError::InvalidParameter(smoothing_radius));
        }
        let displacement = scalar(max_displacement)?;
        if !(0.0..=1_000_000.0).contains(&displacement) {
            return Err(EffectError::InvalidParameter(max_displacement));
        }
        let rotation = match values.get(&max_rotation) {
            Some(Value::Angle(v)) => v.get(),
            _ => return Err(EffectError::InvalidParameter(max_rotation)),
        };
        if !(0.0..=1_000_000.0).contains(&rotation) {
            return Err(EffectError::InvalidParameter(max_rotation));
        }
        let crop = scalar(max_crop)?;
        if !(0.0..=1.0).contains(&crop) {
            return Err(EffectError::InvalidParameter(max_crop));
        }
        let border = match values.get(&border) {
            Some(Value::Enum(v)) => match v.as_str() {
                "fill" => StabilizeBorder::Fill,
                "replicate" => StabilizeBorder::Replicate,
                "reflect" => StabilizeBorder::Reflect,
                _ => return Err(EffectError::InvalidParameter(border)),
            },
            _ => return Err(EffectError::InvalidParameter(border)),
        };
        let fill_color = match values.get(&fill_color) {
            Some(Value::Color(v)) => *v,
            _ => return Err(EffectError::InvalidParameter(fill_color)),
        };
        let sampling = match values.get(&sampling) {
            Some(Value::Enum(v)) => match v.as_str() {
                "nearest" => StabilizeSampling::Nearest,
                "bilinear" => StabilizeSampling::Bilinear,
                _ => return Err(EffectError::InvalidParameter(sampling)),
            },
            _ => return Err(EffectError::InvalidParameter(sampling)),
        };
        Ok(ResolvedEffect::Stabilize {
            tracking,
            smoothing_radius: radius as u32,
            max_displacement: displacement,
            max_rotation: rotation,
            max_crop: crop,
            border,
            fill_color,
            sampling,
            inverse: None,
        })
    }
    /// FX-008 parameter validation (ADR-0137). Unit-interval parameters are
    /// range-checked; lengths, positions and angles share the existing 1e6
    /// budget; enum parameters are validated against their descriptor values.
    fn resolve_fx008(
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
        let positive = |id| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if value > 0.0 && value <= 1_000_000.0 {
                Ok(value)
            } else {
                Err(EffectError::InvalidParameter(id))
            }
        };
        let bounded = |id, min: f64, max: f64| -> Result<f64, EffectError> {
            let value = scalar(id)?;
            if (min..=max).contains(&value) {
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
        let color = |id| -> Result<Color, EffectError> {
            match values.get(&id) {
                Some(Value::Color(v)) => Ok(*v),
                _ => Err(EffectError::InvalidParameter(id)),
            }
        };
        let enumeration = |id, allowed: &[&str]| -> Result<&str, EffectError> {
            match values.get(&id) {
                Some(Value::Enum(v)) if allowed.contains(&v.as_str()) => Ok(v.as_str()),
                _ => Err(EffectError::InvalidParameter(id)),
            }
        };
        match self.parameters {
            EffectParameters::Grain {
                amount,
                size,
                monochrome,
                seed,
            } => {
                let monochrome = match values.get(&monochrome) {
                    Some(Value::Bool(v)) => *v,
                    _ => return Err(EffectError::InvalidParameter(monochrome)),
                };
                let seed_value = bounded(seed, -GRAIN_MAX_SEED, GRAIN_MAX_SEED)?;
                if seed_value.fract() != 0.0 {
                    return Err(EffectError::InvalidParameter(seed));
                }
                Ok(ResolvedEffect::Grain {
                    amount: unit_interval(amount)?,
                    size: positive(size)?,
                    monochrome,
                    seed: seed_value as i64,
                })
            }
            EffectParameters::Mosaic { block_size, basis } => {
                let basis = match enumeration(basis, &["center", "edge"])? {
                    "center" => MosaicBasis::Center,
                    _ => MosaicBasis::Edge,
                };
                Ok(ResolvedEffect::Mosaic {
                    block_size: positive(block_size)?,
                    basis,
                })
            }
            EffectParameters::Invert { channel } => {
                let channel = match enumeration(channel, &["rgb", "red", "green", "blue", "alpha"])?
                {
                    "red" => InvertChannel::Red,
                    "green" => InvertChannel::Green,
                    "blue" => InvertChannel::Blue,
                    "alpha" => InvertChannel::Alpha,
                    _ => InvertChannel::Rgb,
                };
                Ok(ResolvedEffect::Invert { channel })
            }
            EffectParameters::ChannelMixer { matrix } => {
                let table = match values.get(&matrix) {
                    Some(Value::DataTable(t)) => t,
                    _ => return Err(EffectError::InvalidParameter(matrix)),
                };
                Ok(ResolvedEffect::ChannelMixer {
                    matrix: channel_mixer(table).ok_or(EffectError::InvalidParameter(matrix))?,
                })
            }
            EffectParameters::Tint {
                map_black,
                map_white,
                amount,
            } => Ok(ResolvedEffect::Tint {
                map_black: color(map_black)?,
                map_white: color(map_white)?,
                amount: unit_interval(amount)?,
            }),
            EffectParameters::DirectionalBlur { angle, length } => {
                let angle_degrees = match values.get(&angle) {
                    Some(Value::Angle(v)) => v.get(),
                    _ => return Err(EffectError::InvalidParameter(angle)),
                };
                if angle_degrees.abs() > 1_000_000.0 {
                    return Err(EffectError::InvalidParameter(angle));
                }
                Ok(ResolvedEffect::DirectionalBlur {
                    angle_degrees,
                    length: nonnegative(length)?,
                })
            }
            EffectParameters::RadialBlur {
                mode,
                amount,
                center,
            } => {
                let mode = match enumeration(mode, &["spin", "zoom"])? {
                    "spin" => RadialBlurMode::Spin,
                    _ => RadialBlurMode::Zoom,
                };
                let amount = match mode {
                    // Spin amount is a signed total rotation in degrees.
                    RadialBlurMode::Spin => bounded(amount, -1_000_000.0, 1_000_000.0)?,
                    RadialBlurMode::Zoom => unit_interval(amount)?,
                };
                Ok(ResolvedEffect::RadialBlur {
                    mode,
                    amount,
                    center: point(center)?,
                })
            }
            EffectParameters::Displace {
                channel_x,
                channel_y,
                scale_x,
                scale_y,
            } => {
                let channel = |id| -> Result<DisplaceChannel, EffectError> {
                    match enumeration(id, &["red", "green", "blue", "alpha", "luminance"])? {
                        "red" => Ok(DisplaceChannel::Red),
                        "green" => Ok(DisplaceChannel::Green),
                        "blue" => Ok(DisplaceChannel::Blue),
                        "alpha" => Ok(DisplaceChannel::Alpha),
                        _ => Ok(DisplaceChannel::Luminance),
                    }
                };
                Ok(ResolvedEffect::Displace {
                    channel_x: channel(channel_x)?,
                    channel_y: channel(channel_y)?,
                    displacement: [
                        [bounded(scale_x, -1_000_000.0, 1_000_000.0)?, 0.0],
                        [0.0, bounded(scale_y, -1_000_000.0, 1_000_000.0)?],
                    ],
                })
            }
            EffectParameters::Generate {
                generator: kind,
                color_a,
                color_b,
                point_a,
                point_b,
                cell_size,
                line_width,
            } => {
                let kind = match enumeration(
                    kind,
                    &["gradient_linear", "gradient_radial", "checkerboard", "grid"],
                )? {
                    "gradient_radial" => GenerateKind::GradientRadial,
                    "checkerboard" => GenerateKind::Checkerboard,
                    "grid" => GenerateKind::Grid,
                    _ => GenerateKind::GradientLinear,
                };
                Ok(ResolvedEffect::Generate {
                    generator: kind,
                    color_a: color(color_a)?,
                    color_b: color(color_b)?,
                    point_a: point(point_a)?,
                    point_b: point(point_b)?,
                    cell_size: positive(cell_size)?,
                    line_width: nonnegative(line_width)?,
                })
            }
            _ => unreachable!("fx008 resolution is only invoked for fx008 variants"),
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
            EffectParameters::AudioPlugin {
                bundle,
                format,
                component,
                sha256,
                plugin_version,
                parameters,
            } => {
                // AUDIO-011 (ADR-0131): validate the authored pin fields so
                // malformed bindings surface as typed parameter errors before
                // the domain rejection below. Execution belongs to the
                // detached plugin worker, never this evaluator.
                plugin_binding_values(
                    values,
                    bundle,
                    format,
                    component,
                    sha256,
                    plugin_version,
                    parameters,
                )?;
                Err(EffectError::UnsupportedFeature)
            }
            EffectParameters::AudioDelay {
                delay_ms,
                feedback_db,
                wet,
                dry,
            } => {
                // ADR-0137: the delay is an integer number of 48 kHz samples.
                // Milliseconds quantize deterministically by rounding.
                let ms = milliseconds(delay_ms)?;
                let delay_samples = ms * 48.0;
                if !(1.0..=1_000_000.0).contains(&delay_samples) {
                    return Err(EffectError::InvalidParameter(delay_ms));
                }
                Ok(ResolvedAudioEffect::Delay {
                    delay_samples: delay_samples.round() as u32,
                    feedback_db: bounded(feedback_db, -AUDIO_MAX_DB, 0.0)?,
                    wet: bounded(wet, 0.0, 1.0)?,
                    dry: bounded(dry, 0.0, 1.0)?,
                })
            }
            EffectParameters::AudioReverb {
                decay_ms,
                damping,
                wet,
                dry,
            } => Ok(ResolvedAudioEffect::Reverb {
                decay_s: milliseconds(decay_ms)? / 1_000.0,
                damping: bounded(damping, 0.0, 1.0)?,
                wet: bounded(wet, 0.0, 1.0)?,
                dry: bounded(dry, 0.0, 1.0)?,
            }),
            EffectParameters::AudioPitch { semitones } => Ok(ResolvedAudioEffect::Pitch {
                semitones: bounded(
                    semitones,
                    -AUDIO_PITCH_MAX_SEMITONES,
                    AUDIO_PITCH_MAX_SEMITONES,
                )?,
            }),
            EffectParameters::AudioGate {
                threshold_db,
                attack_ms,
                release_ms,
                hysteresis_db,
            } => Ok(ResolvedAudioEffect::Gate {
                threshold_db: bounded(threshold_db, -AUDIO_MAX_DB, 0.0)?,
                attack_ms: milliseconds(attack_ms)?,
                release_ms: milliseconds(release_ms)?,
                hysteresis_db: bounded(hysteresis_db, 0.0, AUDIO_GATE_MAX_HYSTERESIS_DB)?,
            }),
            _ => Err(EffectError::UnsupportedFeature),
        }
    }
}
/// AUDIO-011: validate the authored plugin binding values (`bundle`,
/// `format`, `component`, `sha256`, `plugin_version`, `parameters`). Mirrors
/// the `kronello-plugin` `PluginSpec::validate` contract without depending on
/// it — the model layer stays free of process/host types.
fn plugin_binding_values(
    values: &BTreeMap<PropertyId, Value>,
    bundle: PropertyId,
    format: PropertyId,
    component: PropertyId,
    sha256: PropertyId,
    plugin_version: PropertyId,
    parameters: PropertyId,
) -> Result<(), EffectError> {
    let text = |id: PropertyId| match values.get(&id) {
        Some(Value::String(value)) => Ok(value.as_str()),
        _ => Err(EffectError::InvalidParameter(id)),
    };
    let bundle_v = text(bundle)?;
    let format_v = match values.get(&format) {
        Some(Value::Enum(value)) => value.as_str(),
        _ => return Err(EffectError::InvalidParameter(format)),
    };
    let component_v = text(component)?;
    let sha256_v = text(sha256)?;
    let _ = text(plugin_version)?;
    let vst3 = match format_v {
        "vst3" => true,
        "audio_unit" => false,
        _ => return Err(EffectError::InvalidParameter(format)),
    };
    if vst3 && bundle_v.is_empty() {
        return Err(EffectError::InvalidParameter(bundle));
    }
    if bundle_v.is_empty() && !sha256_v.is_empty() {
        // A file-less built-in component carries no bytes to pin.
        return Err(EffectError::InvalidParameter(sha256));
    }
    let sha256_ok = sha256_v.is_empty()
        || (sha256_v.len() == 64
            && sha256_v
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    if !sha256_ok {
        return Err(EffectError::InvalidParameter(sha256));
    }
    let component_ok = if vst3 {
        component_v.len() == 32 && component_v.bytes().all(|b| b.is_ascii_hexdigit())
    } else {
        let parts: Vec<&str> = component_v.split(':').collect();
        parts.len() == 3 && parts.iter().all(|p| p.len() == 4 && p.is_ascii())
    };
    if !component_ok {
        return Err(EffectError::InvalidParameter(component));
    }
    let table = match values.get(&parameters) {
        Some(Value::DataTable(table)) => table,
        _ => return Err(EffectError::InvalidParameter(parameters)),
    };
    if table.columns.len() != 2
        || table.columns.get("param") != Some(&ValueType::Scalar)
        || table.columns.get("value") != Some(&ValueType::Scalar)
        || table.rows.len() > AUDIO_PLUGIN_MAX_PARAMS
    {
        return Err(EffectError::InvalidParameter(parameters));
    }
    for row in &table.rows {
        if row.len() != 2 {
            return Err(EffectError::InvalidParameter(parameters));
        }
        let (Some(Value::Scalar(param)), Some(Value::Scalar(value))) =
            (row.get("param"), row.get("value"))
        else {
            return Err(EffectError::InvalidParameter(parameters));
        };
        let (param, value) = (param.get(), value.get());
        // `param` carries the u32 parameter id. VST3 values are the
        // normalized 0..=1 domain; AudioUnit values are native and finite.
        if !(0.0..=u32::MAX as f64).contains(&param)
            || param.fract() != 0.0
            || (vst3 && !(0.0..=1.0).contains(&value))
        {
            return Err(EffectError::InvalidParameter(parameters));
        }
    }
    Ok(())
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
/// FX-008: validate and extract the `kronello.channel_mixer` 4x4 coefficient
/// table. Exactly four scalar columns `red`, `green`, `blue`, `alpha` and
/// exactly four rows; row order maps to output channels r, g, b, a.
fn channel_mixer(table: &crate::DataTable) -> Option<[[f64; 4]; 4]> {
    const COLUMNS: [&str; 4] = ["red", "green", "blue", "alpha"];
    if table.columns.len() != 4
        || COLUMNS
            .iter()
            .any(|name| table.columns.get(*name) != Some(&ValueType::Scalar))
        || table.rows.len() != 4
    {
        return None;
    }
    let mut matrix = [[0.0; 4]; 4];
    for (row, output) in table.rows.iter().zip(matrix.iter_mut()) {
        if row.len() != 4 {
            return None;
        }
        for (name, cell) in COLUMNS.iter().zip(output.iter_mut()) {
            match row.get(*name) {
                Some(Value::Scalar(v)) if v.get().abs() <= CHANNEL_MIXER_MAX_COEFFICIENT => {
                    *cell = v.get();
                }
                _ => return None,
            }
        }
    }
    Some(matrix)
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
/// Empty `{ param, value }` scalar table used as the AUDIO-011
/// `plugin_parameters` default (ADR-0131).
fn plugin_params_default() -> Value {
    Value::DataTable(crate::DataTable {
        columns: BTreeMap::from([
            ("param".to_string(), ValueType::Scalar),
            ("value".to_string(), ValueType::Scalar),
        ]),
        rows: Vec::new(),
    })
}
/// Identity 4x4 `{ red, green, blue, alpha }` coefficient table used as the
/// FX-008 `matrix` default (ADR-0137).
fn mixer_default() -> Value {
    let f = |v| FiniteF64::new(v).expect("finite mixer default");
    let columns = BTreeMap::from([
        ("red".to_string(), ValueType::Scalar),
        ("green".to_string(), ValueType::Scalar),
        ("blue".to_string(), ValueType::Scalar),
        ("alpha".to_string(), ValueType::Scalar),
    ]);
    let row = |r, g, b, a| {
        BTreeMap::from([
            ("red".to_string(), Value::Scalar(f(r))),
            ("green".to_string(), Value::Scalar(f(g))),
            ("blue".to_string(), Value::Scalar(f(b))),
            ("alpha".to_string(), Value::Scalar(f(a))),
        ])
    };
    Value::DataTable(crate::DataTable {
        columns,
        rows: vec![
            row(1.0, 0.0, 0.0, 0.0),
            row(0.0, 1.0, 0.0, 0.0),
            row(0.0, 0.0, 1.0, 0.0),
            row(0.0, 0.0, 0.0, 1.0),
        ],
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
        // AUDIO-011 plugin binding descriptors (ADR-0131). Reserved 49xx.
        (
            0xf0000000_0010_4900_8000_000000000001,
            "plugin_bundle",
            Value::String(String::new()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4900_8000_000000000002,
            "plugin_format",
            Value::Enum("vst3".to_string()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4900_8000_000000000003,
            "plugin_component",
            Value::String(String::new()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4900_8000_000000000004,
            "plugin_sha256",
            Value::String(String::new()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4900_8000_000000000005,
            "plugin_version",
            Value::String(String::new()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4900_8000_000000000006,
            "plugin_parameters",
            plugin_params_default(),
            Unit::Dimensionless,
        ),
        // TRACK-002 stabilize descriptors (ADR-0122).
        (
            0xf0000000_0010_4700_8000_000000000001,
            "tracking",
            Value::AssetRef(AssetId::from_uuid(uuid::Uuid::nil())),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4700_8000_000000000002,
            "smoothing_radius",
            Value::Scalar(f(16.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4700_8000_000000000003,
            "max_displacement",
            Value::Scalar(f(64.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_4700_8000_000000000004,
            "max_rotation",
            Value::Angle(f(5.0)),
            Unit::Degrees,
        ),
        (
            0xf0000000_0010_4700_8000_000000000005,
            "max_crop",
            Value::Scalar(f(0.25)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4700_8000_000000000006,
            "border",
            Value::Enum("replicate".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4700_8000_000000000007,
            "fill_color",
            Value::Color(Color::from_srgb8([0; 3], Some(0))),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_4700_8000_000000000008,
            "sampling",
            Value::Enum("bilinear".into()),
            Unit::Dimensionless,
        ),
        // FX-008 descriptors (ADR-0137). Reserved 55xx.
        (
            0xf0000000_0010_5500_8000_000000000001,
            "grain_amount",
            Value::Scalar(f(0.25)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000002,
            "grain_size",
            Value::Scalar(f(1.5)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000003,
            "monochrome",
            Value::Bool(true),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000004,
            "seed",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000005,
            "block_size",
            Value::Scalar(f(8.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000006,
            "mosaic_basis",
            Value::Enum("center".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000007,
            "invert_channel",
            Value::Enum("rgb".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000008,
            "matrix",
            mixer_default(),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000009,
            "map_black",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000a,
            "map_white",
            Value::Color(Color::from_srgb8([255; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000b,
            "tint_amount",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000c,
            "angle",
            Value::Angle(f(45.0)),
            Unit::Degrees,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000d,
            "length",
            Value::Scalar(f(16.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000e,
            "radial_mode",
            Value::Enum("spin".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000000f,
            "radial_amount",
            Value::Scalar(f(10.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000010,
            "radial_center",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000011,
            "displace_channel_x",
            Value::Enum("luminance".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000012,
            "displace_channel_y",
            Value::Enum("luminance".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000013,
            "displace_scale_x",
            Value::Scalar(f(16.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000014,
            "displace_scale_y",
            Value::Scalar(f(16.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000015,
            "generate_kind",
            Value::Enum("gradient_linear".into()),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000016,
            "generate_color_a",
            Value::Color(Color::from_srgb8([255; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000017,
            "generate_color_b",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000018,
            "generate_point_a",
            Value::Vec2([f(0.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_000000000019,
            "generate_point_b",
            Value::Vec2([f(64.0); 2]),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001a,
            "generate_cell_size",
            Value::Scalar(f(16.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001b,
            "generate_line_width",
            Value::Scalar(f(1.0)),
            Unit::DesignPx,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001c,
            "delay_ms",
            Value::Scalar(f(250.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001d,
            "feedback_db",
            Value::Scalar(f(-12.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001e,
            "wet",
            Value::Scalar(f(0.3)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_00000000001f,
            "dry",
            Value::Scalar(f(1.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000020,
            "decay_ms",
            Value::Scalar(f(800.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000021,
            "damping",
            Value::Scalar(f(0.3)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000022,
            "semitones",
            Value::Scalar(f(0.0)),
            Unit::Dimensionless,
        ),
        (
            0xf0000000_0010_5500_8000_000000000023,
            "hysteresis_db",
            Value::Scalar(f(6.0)),
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
        // FX-008 node-local design_px anchors.
        if matches!(
            name,
            "radial_center" | "generate_point_a" | "generate_point_b"
        ) {
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
                | "length"
                | "generate_line_width"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, 1_000_000.0).expect("effect range"),
            ));
        }
        if matches!(
            name,
            "similarity"
                | "spill"
                | "key_luma"
                | "tolerance"
                | "midpoint"
                | "roundness"
                | "grain_amount"
                | "tint_amount"
                | "wet"
                | "dry"
                | "damping"
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
        // FX-008 strictly positive lengths.
        if matches!(name, "grain_size" | "block_size" | "generate_cell_size") {
            let mut bound = NumericRange::inclusive(0.0, 1_000_000.0).expect("length range");
            bound.min.as_mut().expect("min").inclusive = false;
            d.range = Some(ValueRange::Scalar(bound));
        }
        // FX-008 signed extents.
        if matches!(
            name,
            "displace_scale_x" | "displace_scale_y" | "radial_amount"
        ) {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(-1_000_000.0, 1_000_000.0).expect("effect range"),
            ));
        }
        if name == "angle" {
            d.range = Some(ValueRange::Angle(
                NumericRange::inclusive(-1_000_000.0, 1_000_000.0).expect("angle range"),
            ));
        }
        if name == "seed" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(-GRAIN_MAX_SEED, GRAIN_MAX_SEED).expect("seed range"),
            ));
        }
        if matches!(name, "delay_ms" | "decay_ms") {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(AUDIO_MIN_TIME_MS, AUDIO_MAX_TIME_MS)
                    .expect("audio time range"),
            ));
        }
        if name == "feedback_db" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(-AUDIO_MAX_DB, 0.0).expect("feedback range"),
            ));
        }
        if name == "semitones" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(-AUDIO_PITCH_MAX_SEMITONES, AUDIO_PITCH_MAX_SEMITONES)
                    .expect("semitone range"),
            ));
        }
        if name == "hysteresis_db" {
            d.range = Some(ValueRange::Scalar(
                NumericRange::inclusive(0.0, AUDIO_GATE_MAX_HYSTERESIS_DB)
                    .expect("hysteresis range"),
            ));
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
