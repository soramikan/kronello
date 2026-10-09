use crate::{
    Color, CoordinateSpace, DescriptorDefinition, DescriptorId, FiniteF64, NumericBound,
    NumericRange, PropertyDescriptor, SchemaKey, SchemaRegistry, Unit, Value, ValueRange,
};
use uuid::Uuid;

/// Fixed UUID v4 identity for `kronello.transform.position`.
pub const TRANSFORM_POSITION_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x60b3f16c_3677_4c83_8b90_f175f965ee98));
/// Fixed UUID v4 identity for `kronello.transform.anchor`.
pub const TRANSFORM_ANCHOR_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x83e2ea66_c48b_4cc9_9f70_2b9fa2e0584f));
/// Fixed UUID v4 identity for `kronello.transform.scale`.
pub const TRANSFORM_SCALE_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xfaec5922_1cdb_44da_a24c_4f3d8fc03739));
/// Fixed UUID v4 identity for `kronello.transform.rotation`.
pub const TRANSFORM_ROTATION_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x24c59f64_041a_45a4_9cbe_dc709cc0cf98));
/// Fixed UUID v4 identity for `kronello.transform.skew`.
pub const TRANSFORM_SKEW_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x7d31c350_09e9_4380_aa73_f886297e8364));
/// Fixed UUID v4 identity for `kronello.opacity`.
pub const OPACITY_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xa3e91b5c_c1d2_4950_8f03_156cfa7d3b9b));
/// Fixed UUID v4 identity for `kronello.fill_color`.
pub const FILL_COLOR_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xe35b24d6_139d_4eea_b7dc_4a289770a9c5));
/// Fixed UUID v4 identity for `kronello.stroke_width`.
pub const STROKE_WIDTH_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x98c01340_51f5_4b72_a039_722ba5b3a3b0));

/// Fixed identity for nonnegative linear audio volume.
pub const AUDIO_VOLUME_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xe9cf4a80_2b64_4b8e_9e29_dfe6bc119a63));

/// Fixed identity for stereo audio balance (`kronello.audio.pan`, [-1, 1]).
pub const AUDIO_PAN_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x4c1d7f3e_9a28_4b62_8e51_7f0c2d9a6b34));

/// Fixed identity for authored layer blending.
pub const BLEND_MODE_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x7ba7fae2_3e1d_4bd9_9b84_2e6a6a9fe101));

/// Fixed identity for `kronello.media.crop_origin` (source-space window min).
pub const MEDIA_CROP_ORIGIN_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x4d2a8c1e_6f07_4b93_a1e5_3d79c2f408aa));
/// Fixed identity for `kronello.media.crop_size` (source-space window extent;
/// `[0, 0]` leaves the frame uncropped).
pub const MEDIA_CROP_SIZE_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x9b8e1f42_c365_4d0a_b6f2_7a41e9d530c7));

pub(crate) fn registry() -> SchemaRegistry {
    let zero = FiniteF64::new(0.0).expect("finite built-in zero");
    let one = FiniteF64::new(1.0).expect("finite built-in one");
    let definition = |id, key, name, unit, default: Value| {
        DescriptorDefinition::new(
            id,
            SchemaKey::new(key).expect("valid built-in key"),
            name,
            default.value_type(),
            unit,
            default,
        )
    };
    let mut position = definition(
        TRANSFORM_POSITION_ID,
        "kronello.transform.position",
        "Position",
        Unit::DesignPx,
        Value::Vec2([zero; 2]),
    );
    position.coordinate_space = Some(CoordinateSpace::ParentDesign);
    let anchor = definition(
        TRANSFORM_ANCHOR_ID,
        "kronello.transform.anchor",
        "Anchor",
        Unit::DesignPx,
        Value::Vec2([zero; 2]),
    );
    let scale = definition(
        TRANSFORM_SCALE_ID,
        "kronello.transform.scale",
        "Scale",
        Unit::Dimensionless,
        Value::Vec2([one; 2]),
    );
    let rotation = definition(
        TRANSFORM_ROTATION_ID,
        "kronello.transform.rotation",
        "Rotation",
        Unit::Degrees,
        Value::Angle(zero),
    );
    let skew = definition(
        TRANSFORM_SKEW_ID,
        "kronello.transform.skew",
        "Skew",
        Unit::Degrees,
        Value::Angle(zero),
    );
    let mut opacity = definition(
        OPACITY_ID,
        "kronello.opacity",
        "Opacity",
        Unit::Dimensionless,
        Value::Scalar(one),
    );
    opacity.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(0.0, 1.0).expect("valid built-in opacity range"),
    ));
    let fill_color = definition(
        FILL_COLOR_ID,
        "kronello.fill_color",
        "Fill color",
        Unit::Dimensionless,
        Value::Color(Color::from_srgb8([0; 3], None)),
    );
    let mut stroke_width = definition(
        STROKE_WIDTH_ID,
        "kronello.stroke_width",
        "Stroke width",
        Unit::DesignPx,
        Value::Scalar(zero),
    );
    stroke_width.range = Some(ValueRange::Scalar(NumericRange {
        min: Some(NumericBound {
            value: zero,
            inclusive: true,
        }),
        max: None,
    }));

    let mut volume = definition(
        AUDIO_VOLUME_ID,
        "kronello.audio.volume",
        "Volume",
        Unit::Dimensionless,
        Value::Scalar(one),
    );
    volume.range = Some(ValueRange::Scalar(NumericRange {
        min: Some(NumericBound {
            value: zero,
            inclusive: true,
        }),
        max: Some(NumericBound {
            value: FiniteF64::new(f64::from(f32::MAX)).expect("finite gain limit"),
            inclusive: true,
        }),
    }));
    volume.capabilities.expressions = false;
    volume.capabilities.modifiers = false;
    // GUI-012 (ADR-0138): constant stereo balance on a clip. -1 is hard left,
    // +1 hard right; the mixer writes one shared value per audio track. A
    // constant-only contract keeps the mix deterministic per rendered block.
    let mut pan = definition(
        AUDIO_PAN_ID,
        "kronello.audio.pan",
        "Pan",
        Unit::Dimensionless,
        Value::Scalar(zero),
    );
    pan.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(-1.0, 1.0).expect("valid built-in pan range"),
    ));
    pan.capabilities.curves = false;
    pan.capabilities.expressions = false;
    pan.capabilities.modifiers = false;
    let mut blend = definition(
        BLEND_MODE_ID,
        crate::BLEND_KEY,
        "Blend mode",
        Unit::Dimensionless,
        Value::Enum("normal".into()),
    );
    blend.animatable = false;
    blend.interpolation_modes.clear();
    blend.capabilities.curves = false;
    blend.capabilities.expressions = false;
    blend.capabilities.modifiers = false;
    // AI-003 (ADR-0126): source-pixel crop window on visual media. A zero
    // size disables the crop; smart-reframe rules write these as layout
    // inputs through the shared dependency path.
    let crop_origin = definition(
        MEDIA_CROP_ORIGIN_ID,
        "kronello.media.crop_origin",
        "Media crop origin",
        Unit::DesignPx,
        Value::Vec2([zero; 2]),
    );
    let crop_size = definition(
        MEDIA_CROP_SIZE_ID,
        "kronello.media.crop_size",
        "Media crop size",
        Unit::DesignPx,
        Value::Vec2([zero; 2]),
    );
    let mut registry = SchemaRegistry::new();
    for definition in [
        blend,
        volume,
        pan,
        position,
        anchor,
        scale,
        rotation,
        skew,
        opacity,
        fill_color,
        stroke_width,
        crop_origin,
        crop_size,
    ] {
        let descriptor = PropertyDescriptor::new(definition).expect("valid built-in descriptor");
        registry
            .register(descriptor)
            .expect("unique built-in identity");
    }
    // FX-004 clip mask parameters are clip-owned builtins like the rest.
    for descriptor in crate::mask_descriptors() {
        registry
            .register(descriptor)
            .expect("unique built-in mask identity");
    }
    registry
}
