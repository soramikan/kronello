use crate::{AssetId, ModelError};
use serde::{Deserialize, Serialize};

/// No non-finite value can be constructed or deserialized through the public API.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct FiniteF64(f64);

impl FiniteF64 {
    pub fn new(value: f64) -> Result<Self, ModelError> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(ModelError::NonFinite { value })
        }
    }
    pub const fn get(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for FiniteF64 {
    type Error = ModelError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<FiniteF64> for f64 {
    fn from(value: FiniteF64) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Scalar,
    Vec2,
    Vec3,
    Angle,
    Color,
    Bool,
    Enum,
    String,
    AssetRef,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    Srgb,
    LinearRec709,
    LinearRec2020,
}

/// Straight RGB with independent alpha. Alpha defaults only at input; saved
/// colors always emit all four components and an explicit color space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorComponents {
    pub r: FiniteF64,
    pub g: FiniteF64,
    pub b: FiniteF64,
    #[serde(default = "opaque_alpha")]
    pub alpha: FiniteF64,
}
fn opaque_alpha() -> FiniteF64 {
    FiniteF64(1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ColorWire", into = "ColorWire")]
pub struct Color {
    space: ColorSpace,
    components: ColorComponents,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorWire {
    space: ColorSpace,
    components: ColorComponents,
}

impl Color {
    pub fn new(space: ColorSpace, rgb: [f64; 3], alpha: f64) -> Result<Self, ModelError> {
        let components = ColorComponents {
            r: FiniteF64::new(rgb[0])?,
            g: FiniteF64::new(rgb[1])?,
            b: FiniteF64::new(rgb[2])?,
            alpha: FiniteF64::new(alpha)?,
        };
        if !(0.0..=1.0).contains(&alpha) {
            return Err(ModelError::OutOfRange {
                component: 3,
                value: alpha,
            });
        }
        if space == ColorSpace::Srgb {
            for (component, value) in rgb.into_iter().enumerate() {
                if !(0.0..=1.0).contains(&value) {
                    return Err(ModelError::OutOfRange { component, value });
                }
            }
        }
        Ok(Self { space, components })
    }

    /// Untagged 8-bit inputs are normalized into explicitly tagged sRGB. No
    /// transfer function is applied to RGB or alpha at this storage boundary.
    pub fn from_srgb8(rgb: [u8; 3], alpha: Option<u8>) -> Self {
        Self {
            space: ColorSpace::Srgb,
            components: ColorComponents {
                r: FiniteF64(f64::from(rgb[0]) / 255.0),
                g: FiniteF64(f64::from(rgb[1]) / 255.0),
                b: FiniteF64(f64::from(rgb[2]) / 255.0),
                alpha: FiniteF64(f64::from(alpha.unwrap_or(255)) / 255.0),
            },
        }
    }

    /// Input adapter for `#RRGGBB` or `#RRGGBBAA`; not a saved Color encoding.
    pub fn from_srgb_hex(input: &str) -> Result<Self, ModelError> {
        let hex = input.strip_prefix('#').ok_or(ModelError::InvalidSrgbHex)?;
        if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(ModelError::InvalidSrgbHex);
        }
        let mut bytes = [255; 4];
        for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
            let digit = |c: u8| {
                if c.is_ascii_digit() {
                    c - b'0'
                } else {
                    c.to_ascii_lowercase() - b'a' + 10
                }
            };
            bytes[index] = digit(pair[0]) * 16 + digit(pair[1]);
        }
        Ok(Self::from_srgb8(
            [bytes[0], bytes[1], bytes[2]],
            Some(bytes[3]),
        ))
    }
    pub const fn space(self) -> ColorSpace {
        self.space
    }
    pub const fn components(self) -> ColorComponents {
        self.components
    }
}
impl TryFrom<ColorWire> for Color {
    type Error = ModelError;
    fn try_from(wire: ColorWire) -> Result<Self, Self::Error> {
        let c = wire.components;
        Self::new(wire.space, [c.r.get(), c.g.get(), c.b.get()], c.alpha.get())
    }
}
impl From<Color> for ColorWire {
    fn from(color: Color) -> Self {
        Self {
            space: color.space,
            components: color.components,
        }
    }
}

/// A document geometry container only. Topology checking/morph evaluation is
/// deferred to the vector/animation layer; Path currently supports Hold only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    pub segments: Vec<PathSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PathSegment {
    MoveTo([FiniteF64; 2]),
    LineTo([FiniteF64; 2]),
    CubicTo {
        control1: [FiniteF64; 2],
        control2: [FiniteF64; 2],
        end: [FiniteF64; 2],
    },
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Scalar(FiniteF64),
    Vec2([FiniteF64; 2]),
    Vec3([FiniteF64; 3]),
    /// Continuous degrees: never reduced modulo 360.
    Angle(FiniteF64),
    Color(Color),
    Bool(bool),
    Enum(String),
    String(String),
    AssetRef(AssetId),
    Path(Path),
}
impl Value {
    pub const fn value_type(&self) -> ValueType {
        match self {
            Self::Scalar(_) => ValueType::Scalar,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Angle(_) => ValueType::Angle,
            Self::Color(_) => ValueType::Color,
            Self::Bool(_) => ValueType::Bool,
            Self::Enum(_) => ValueType::Enum,
            Self::String(_) => ValueType::String,
            Self::AssetRef(_) => ValueType::AssetRef,
            Self::Path(_) => ValueType::Path,
        }
    }
}
