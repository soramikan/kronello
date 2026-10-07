//! UTF-8 text content and references to instance-evaluated design-space values.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::{
    Color, ContentId, DescriptorDefinition, DescriptorId, DocumentObject, FiniteF64, ModelError,
    NodeKind, Project, Property, PropertyDescriptor, PropertyId, PropertySource, SchemaKey,
    SchemaRegistry, Unit, Value, ValueType,
};

pub const TEXT_LAYOUT_VERSION: u32 = 1;
pub const TEXT_ADVANCED_LAYOUT_VERSION: u32 = 2;

/// Half-open UTF-8 byte range in the original, unnormalized text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

/// File hash plus face index fixes the actual bytes; names are verified too.
/// No family lookup or implicit system-font fallback is permitted.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct FontRef {
    pub family: String,
    pub postscript_name: String,
    pub sha256: String,
    pub face_index: u32,
}
impl FontRef {
    pub fn validate(&self) -> Result<(), TextError> {
        if self.family.is_empty()
            || self.postscript_name.is_empty()
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(TextError::InvalidFontRef);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Horizontal,
    VerticalRl,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlignment {
    Start,
    Center,
    End,
}

/// Per-animation-unit value distribution. `step` applies the evaluated value
/// unchanged, `ramp` blends linearly over the selected units in reading order,
/// `follow` keeps the first selected unit at full effect and trails later units
/// by `follow_smoothing`, and `random` draws a stable seeded factor per unit.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AnimatorMode {
    #[default]
    Step,
    Ramp,
    Follow,
    Random,
}
impl AnimatorMode {
    fn is_step(&self) -> bool {
        *self == Self::Step
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleSpan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Box<crate::Gradient>>,
    pub range: TextRange,
    pub font: FontRef,
    /// Positive Scalar Property in design_px.
    pub size: PropertyId,
    /// Tagged straight Color Property.
    pub fill: PropertyId,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RubyAssociation {
    pub base: TextRange,
    pub text: String,
}

/// Logical source selection. Ranges are never glyph or line indices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterAnimation {
    pub source: TextRange,
    /// Exact unnormalized source captured when the selector is authored.
    pub expected_text: String,
    pub offset: PropertyId,
    pub opacity: PropertyId,
    /// Vec2 per-unit scale factors, dimensionless; 1.0 is neutral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<PropertyId>,
    /// Angle per-unit rotation in degrees about the glyph anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<PropertyId>,
    /// Straight Color per-unit fill override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<PropertyId>,
    #[serde(default, skip_serializing_if = "AnimatorMode::is_step")]
    pub mode: AnimatorMode,
    /// Fixed seed for `random`; absent seeds still hash to stable factors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    /// Scalar in [0, 1], dimensionless; required when `mode` is `follow`.
    /// 0.0 delegates to `step`; larger values trail later units more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_smoothing: Option<PropertyId>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCharacterAnimation {
    pub source: TextRange,
    pub expected_text: String,
    pub offset: [FiniteF64; 2],
    pub opacity: FiniteF64,
    pub scale: Option<[FiniteF64; 2]>,
    pub rotation: Option<FiniteF64>,
    pub fill: Option<Color>,
    pub mode: AnimatorMode,
    pub seed: Option<u32>,
    pub follow_smoothing: Option<FiniteF64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextDocument {
    pub id: ContentId,
    pub layout_version: u32,
    pub text: String,
    /// Ordered, nonoverlapping, complete coverage at grapheme boundaries.
    pub styles: Vec<TextStyleSpan>,
    pub direction: TextDirection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ruby: Vec<RubyAssociation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub character_animations: Vec<CharacterAnimation>,
    pub wrap_width: PropertyId,
    /// Absolute baseline spacing in design_px, not a font-size multiplier.
    pub line_height: PropertyId,
    /// Enum Property: "start", "center", or "end".
    pub alignment: PropertyId,
    /// Optional `ValueType::Path` Property whose bezier guides glyph baselines.
    /// With a path, `alignment` anchors along path arc length instead of the
    /// wrap box; glyphs past the path end are dropped, not errors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PropertyId>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTextStyle {
    pub gradient: Option<Box<crate::ResolvedGradient>>,
    pub range: TextRange,
    pub font: FontRef,
    pub size: FiniteF64,
    pub fill: Color,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedText {
    pub layout_version: u32,
    pub text: String,
    pub styles: Vec<ResolvedTextStyle>,
    pub direction: TextDirection,
    pub ruby: Vec<RubyAssociation>,
    pub character_animations: Vec<ResolvedCharacterAnimation>,
    pub wrap_width: FiniteF64,
    pub line_height: FiniteF64,
    pub alignment: TextAlignment,
    /// Evaluated guide path in text-local design_px; layout flattens it.
    pub path: Option<crate::Path>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TextError {
    #[error(transparent)]
    Gradient(#[from] crate::ShapeError),
    #[error("invalid locked font reference")]
    InvalidFontRef,
    #[error("style spans must cover the text exactly at grapheme boundaries")]
    InvalidSpans,
    #[error("invalid ruby source range")]
    InvalidRubyRange,
    #[error("invalid character selector range or parameters")]
    InvalidCharacterAnimation,
    #[error("missing text property {id}")]
    MissingProperty { id: PropertyId },
    #[error("duplicate text property {id}")]
    DuplicateProperty { id: PropertyId },
    #[error("incompatible text property descriptor {id}")]
    InvalidDescriptor { id: PropertyId },
    #[error("invalid text parameter {id}")]
    InvalidParameter { id: PropertyId },
    #[error("nonpositive text layout dimension")]
    InvalidDimension,
    #[error("missing text content {id}")]
    MissingContent { id: ContentId },
    #[error("duplicate text content {id}")]
    DuplicateContent { id: ContentId },
    #[error("opaque text content is unsupported")]
    UnsupportedContent,
    #[error(transparent)]
    Model(#[from] ModelError),
}

fn validate_ranges<'a>(
    text: &str,
    styles: impl Iterator<Item = (TextRange, &'a FontRef)>,
    ruby: &[RubyAssociation],
) -> Result<(), TextError> {
    let boundaries: BTreeSet<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let mut end = 0;
    for (range, font) in styles {
        font.validate()?;
        if range.start != end || range.end <= range.start || !boundaries.contains(&range.end) {
            return Err(TextError::InvalidSpans);
        }
        end = range.end;
    }
    if end != text.len() {
        return Err(TextError::InvalidSpans);
    }
    let mut ruby_end = 0;
    for association in ruby {
        let range = association.base;
        if range.start < ruby_end
            || association.text.is_empty()
            || range.start >= range.end
            || !boundaries.contains(&range.start)
            || !boundaries.contains(&range.end)
        {
            return Err(TextError::InvalidRubyRange);
        }
        ruby_end = range.end;
    }
    Ok(())
}
impl ResolvedText {
    /// Revalidated by layout: public evaluated inputs cannot bypass invariants.
    pub fn validate(&self) -> Result<(), TextError> {
        validate_ranges(
            &self.text,
            self.styles.iter().map(|s| (s.range, &s.font)),
            &self.ruby,
        )?;
        validate_selectors(
            &self.text,
            self.character_animations.iter().map(|a| a.source),
        )?;
        if self
            .character_animations
            .iter()
            .any(|a| self.text[a.source.start..a.source.end] != a.expected_text)
        {
            return Err(TextError::InvalidCharacterAnimation);
        }
        if self
            .character_animations
            .iter()
            .any(|a| !(0.0..=1.0).contains(&a.opacity.get()))
        {
            return Err(TextError::InvalidCharacterAnimation);
        }
        if self.character_animations.iter().any(|a| {
            (a.mode == AnimatorMode::Follow && a.follow_smoothing.is_none())
                || a.follow_smoothing
                    .is_some_and(|v| !(0.0..=1.0).contains(&v.get()))
        }) {
            return Err(TextError::InvalidCharacterAnimation);
        }
        if let Some(path) = &self.path {
            crate::validate_path(path)?;
        }
        if self.wrap_width.get() <= 0.0
            || self.line_height.get() <= 0.0
            || self.styles.iter().any(|s| s.size.get() <= 0.0)
        {
            return Err(TextError::InvalidDimension);
        }
        Ok(())
    }
}
impl TextDocument {
    fn parameters(&self) -> Vec<(PropertyId, ValueType, Unit)> {
        let mut parameters = vec![
            (self.wrap_width, ValueType::Scalar, Unit::DesignPx),
            (self.line_height, ValueType::Scalar, Unit::DesignPx),
            (self.alignment, ValueType::Enum, Unit::Dimensionless),
        ];
        for span in &self.styles {
            parameters.extend([
                (span.size, ValueType::Scalar, Unit::DesignPx),
                (span.fill, ValueType::Color, Unit::Dimensionless),
            ]);
        }
        if let Some(path) = self.path {
            parameters.push((path, ValueType::Path, Unit::DesignPx));
        }
        for animation in &self.character_animations {
            parameters.extend([
                (animation.offset, ValueType::Vec2, Unit::DesignPx),
                (animation.opacity, ValueType::Scalar, Unit::Dimensionless),
            ]);
            if let Some(scale) = animation.scale {
                parameters.push((scale, ValueType::Vec2, Unit::Dimensionless));
            }
            if let Some(rotation) = animation.rotation {
                parameters.push((rotation, ValueType::Angle, Unit::Degrees));
            }
            if let Some(fill) = animation.fill {
                parameters.push((fill, ValueType::Color, Unit::Dimensionless));
            }
            if let Some(smoothing) = animation.follow_smoothing {
                parameters.push((smoothing, ValueType::Scalar, Unit::Dimensionless));
            }
        }
        parameters
    }
    /// Selector-owned unit-interval scalars (opacity, follow smoothing).
    fn unit_interval_parameter(&self, id: PropertyId) -> bool {
        self.character_animations
            .iter()
            .any(|a| a.opacity == id || a.follow_smoothing.is_some_and(|smoothing| smoothing == id))
    }
    pub fn property_ids(&self) -> Vec<PropertyId> {
        self.parameters()
            .into_iter()
            .map(|(id, _, _)| id)
            .chain(
                self.styles
                    .iter()
                    .filter_map(|s| s.gradient.as_ref())
                    .flat_map(|g| g.stops().iter().flat_map(|s| [s.color, s.offset])),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn validate(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), TextError> {
        validate_ranges(
            &self.text,
            self.styles.iter().map(|s| (s.range, &s.font)),
            &self.ruby,
        )?;
        validate_selectors(
            &self.text,
            self.character_animations.iter().map(|a| a.source),
        )?;
        if self
            .character_animations
            .iter()
            .any(|a| self.text[a.source.start..a.source.end] != a.expected_text)
        {
            return Err(TextError::InvalidCharacterAnimation);
        }
        if self
            .character_animations
            .iter()
            .any(|a| a.mode == AnimatorMode::Follow && a.follow_smoothing.is_none())
        {
            return Err(TextError::InvalidCharacterAnimation);
        }
        let mut by_id = BTreeMap::new();
        for property in properties {
            if by_id.insert(property.id(), property).is_some() {
                return Err(TextError::DuplicateProperty { id: property.id() });
            }
        }
        for (id, ty, unit) in self.parameters() {
            let property = by_id.get(&id).ok_or(TextError::MissingProperty { id })?;
            property.validate(registry)?;
            let descriptor = property.descriptor().resolve(registry)?.definition();
            if descriptor.value_type != ty || descriptor.unit != unit {
                return Err(TextError::InvalidDescriptor { id });
            }
            if !property.modifiers().iter().any(|m| m.enabled)
                && let PropertySource::Constant(value) = property.source()
            {
                if self.unit_interval_parameter(id) {
                    if !matches!(value, Value::Scalar(v) if (0.0..=1.0).contains(&v.get())) {
                        return Err(TextError::InvalidCharacterAnimation);
                    }
                } else {
                    validate_parameter(id, ty, value)?;
                }
            }
        }
        for gradient in self.styles.iter().filter_map(|s| s.gradient.as_ref()) {
            gradient.validate_properties(properties, registry)?;
        }
        Ok(())
    }
    pub fn resolve(&self, values: &BTreeMap<PropertyId, Value>) -> Result<ResolvedText, TextError> {
        for (id, ty, _) in self.parameters() {
            if self.unit_interval_parameter(id) {
                if !matches!(values.get(&id), Some(Value::Scalar(v)) if (0.0..=1.0).contains(&v.get()))
                {
                    return Err(TextError::InvalidCharacterAnimation);
                }
                continue;
            }
            validate_parameter(
                id,
                ty,
                values.get(&id).ok_or(TextError::MissingProperty { id })?,
            )?;
        }
        let scalar = |id| match values[&id] {
            Value::Scalar(v) => v,
            _ => unreachable!(),
        };
        let resolved = ResolvedText {
            layout_version: self.layout_version,
            text: self.text.clone(),
            styles: self
                .styles
                .iter()
                .map(|s| {
                    Ok(ResolvedTextStyle {
                        gradient: s
                            .gradient
                            .as_ref()
                            .map(|g| g.resolve(values).map(Box::new))
                            .transpose()?,
                        range: s.range,
                        font: s.font.clone(),
                        size: scalar(s.size),
                        fill: match values[&s.fill] {
                            Value::Color(v) => v,
                            _ => unreachable!(),
                        },
                    })
                })
                .collect::<Result<Vec<_>, TextError>>()?,
            direction: self.direction,
            ruby: self.ruby.clone(),
            character_animations: self
                .character_animations
                .iter()
                .map(|a| {
                    let offset = match values[&a.offset] {
                        Value::Vec2(v) => v,
                        _ => unreachable!(),
                    };
                    ResolvedCharacterAnimation {
                        source: a.source,
                        expected_text: a.expected_text.clone(),
                        offset,
                        opacity: scalar(a.opacity),
                        scale: a.scale.map(|id| match values[&id] {
                            Value::Vec2(v) => v,
                            _ => unreachable!(),
                        }),
                        rotation: a.rotation.map(|id| match values[&id] {
                            Value::Angle(v) => v,
                            _ => unreachable!(),
                        }),
                        fill: a.fill.map(|id| match values[&id] {
                            Value::Color(v) => v,
                            _ => unreachable!(),
                        }),
                        mode: a.mode,
                        seed: a.seed,
                        follow_smoothing: a.follow_smoothing.map(&scalar),
                    }
                })
                .collect(),
            wrap_width: scalar(self.wrap_width),
            line_height: scalar(self.line_height),
            alignment: match &values[&self.alignment] {
                Value::Enum(v) if v == "start" => TextAlignment::Start,
                Value::Enum(v) if v == "center" => TextAlignment::Center,
                _ => TextAlignment::End,
            },
            path: self.path.map(|id| match values[&id] {
                Value::Path(ref path) => path.clone(),
                _ => unreachable!(),
            }),
        };
        resolved.validate()?;
        Ok(resolved)
    }
}
fn validate_parameter(id: PropertyId, ty: ValueType, value: &Value) -> Result<(), TextError> {
    let valid = match (ty, value) {
        (ValueType::Scalar, Value::Scalar(v)) => v.get() > 0.0,
        (ValueType::Color, Value::Color(_)) => true,
        (ValueType::Vec2, Value::Vec2(_)) => true,
        (ValueType::Angle, Value::Angle(_)) => true,
        (ValueType::Path, Value::Path(path)) => crate::validate_path(path).is_ok(),
        (ValueType::Enum, Value::Enum(v)) => matches!(v.as_str(), "start" | "center" | "end"),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(TextError::InvalidParameter { id })
    }
}

pub fn validate_text_contents(
    project: &Project,
    registry: &SchemaRegistry,
) -> Result<(), TextError> {
    let mut contents = BTreeMap::new();
    for object in &project.texts {
        let id = match object {
            DocumentObject::Known(t) => t.id,
            DocumentObject::Opaque(o) => ContentId::from_uuid(o.id),
        };
        if contents.insert(id, object).is_some() {
            return Err(TextError::DuplicateContent { id });
        }
    }
    for object in &project.compositions {
        let DocumentObject::Known(composition) = object else {
            return Err(TextError::UnsupportedContent);
        };
        for node in &composition.nodes {
            if let NodeKind::Text { content_ref } = node.kind {
                match contents
                    .get(&content_ref)
                    .ok_or(TextError::MissingContent { id: content_ref })?
                {
                    DocumentObject::Known(text) => text.validate(&node.properties, registry)?,
                    DocumentObject::Opaque(_) => return Err(TextError::UnsupportedContent),
                }
            }
        }
    }
    Ok(())
}

/// Explicitly register these with SchemaRegistry::with_builtin(), like shapes.
/// `positive` attaches an open lower bound of zero to scalar descriptors.
pub fn text_descriptors() -> Vec<PropertyDescriptor> {
    let n = |v| FiniteF64::new(v).expect("finite text default");
    [
        (
            0x2c308a48_b4d3_4c52_a2b0_aa9dd167bbef,
            "style_color",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
            true,
            false,
        ),
        (
            0x3c308a48_b4d3_4c52_a2b0_aa9dd167bbef,
            "style_size",
            Value::Scalar(n(32.0)),
            Unit::DesignPx,
            true,
            true,
        ),
        (
            0x1c308a48_b4d3_4c52_a2b0_aa9dd167bbef,
            "character_offset",
            Value::Vec2([n(0.0), n(0.0)]),
            Unit::DesignPx,
            true,
            false,
        ),
        (
            0x0c308a48_b4d3_4c52_a2b0_aa9dd167bbef,
            "character_opacity",
            Value::Scalar(n(1.0)),
            Unit::Dimensionless,
            true,
            false,
        ),
        (
            0xf0000000_0010_4600_8000_000000000001,
            "path",
            Value::Path(crate::Path { segments: vec![] }),
            Unit::DesignPx,
            false,
            false,
        ),
        (
            0xf0000000_0010_4601_8000_000000000002,
            "character_scale",
            Value::Vec2([n(1.0), n(1.0)]),
            Unit::Dimensionless,
            true,
            false,
        ),
        (
            0xf0000000_0010_4602_8000_000000000003,
            "character_rotation",
            Value::Angle(n(0.0)),
            Unit::Degrees,
            true,
            false,
        ),
        (
            0xf0000000_0010_4603_8000_000000000004,
            "character_fill",
            Value::Color(Color::from_srgb8([0; 3], None)),
            Unit::Dimensionless,
            true,
            false,
        ),
        (
            0xf0000000_0010_4604_8000_000000000005,
            "character_follow_smoothing",
            Value::Scalar(n(0.0)),
            Unit::Dimensionless,
            true,
            false,
        ),
        (
            0xa132814b_f79c_4c5f_a59d_b9f0dd9a66dd,
            "font_size",
            Value::Scalar(n(32.0)),
            Unit::DesignPx,
            true,
            true,
        ),
        (
            0x57b6f88f_51c8_4d76_b052_04e4235281f3,
            "wrap_width",
            Value::Scalar(n(640.0)),
            Unit::DesignPx,
            true,
            true,
        ),
        (
            0x5d72aedd_02fe_4191_879f_f69c55eeecdf,
            "line_height",
            Value::Scalar(n(48.0)),
            Unit::DesignPx,
            true,
            true,
        ),
        (
            0x5f72d26a_cc83_434e_9599_a35dad348a96,
            "alignment",
            Value::Enum("start".into()),
            Unit::Dimensionless,
            false,
            false,
        ),
    ]
    .into_iter()
    .map(|(id, name, default, unit, repeatable, positive)| {
        let mut definition = DescriptorDefinition::new(
            DescriptorId::from_uuid(Uuid::from_u128(id)),
            SchemaKey::new(format!("kronello.text.{name}")).expect("valid text key"),
            name,
            default.value_type(),
            unit,
            default,
        );
        definition.repeatable = repeatable;
        if positive {
            definition.range = Some(crate::ValueRange::Scalar(crate::NumericRange {
                min: Some(crate::NumericBound {
                    value: n(0.0),
                    inclusive: false,
                }),
                max: None,
            }));
        }
        PropertyDescriptor::new(definition).expect("valid text descriptor")
    })
    .collect()
}

fn validate_selectors(
    text: &str,
    ranges: impl Iterator<Item = TextRange>,
) -> Result<(), TextError> {
    let boundaries: BTreeSet<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    for range in ranges {
        if range.start >= range.end
            || !boundaries.contains(&range.start)
            || !boundaries.contains(&range.end)
        {
            return Err(TextError::InvalidCharacterAnimation);
        }
    }
    Ok(())
}
