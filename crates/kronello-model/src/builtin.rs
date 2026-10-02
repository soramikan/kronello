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

    let mut registry = SchemaRegistry::new();
    for definition in [
        position,
        anchor,
        scale,
        rotation,
        skew,
        opacity,
        fill_color,
        stroke_width,
    ] {
        let descriptor = PropertyDescriptor::new(definition).expect("valid built-in descriptor");
        registry
            .register(descriptor)
            .expect("unique built-in identity");
    }
    registry
}
