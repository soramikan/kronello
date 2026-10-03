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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleSpan {
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
    pub wrap_width: PropertyId,
    /// Absolute baseline spacing in design_px, not a font-size multiplier.
    pub line_height: PropertyId,
    /// Enum Property: "start", "center", or "end".
    pub alignment: PropertyId,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTextStyle {
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
    pub wrap_width: FiniteF64,
    pub line_height: FiniteF64,
    pub alignment: TextAlignment,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TextError {
    #[error("invalid locked font reference")]
    InvalidFontRef,
    #[error("style spans must cover the text exactly at grapheme boundaries")]
    InvalidSpans,
    #[error("invalid ruby source range")]
    InvalidRubyRange,
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
    for association in ruby {
        let range = association.base;
        if range.start >= range.end
            || !boundaries.contains(&range.start)
            || !boundaries.contains(&range.end)
        {
            return Err(TextError::InvalidRubyRange);
        }
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
        parameters
    }
    pub fn property_ids(&self) -> Vec<PropertyId> {
        self.parameters()
            .into_iter()
            .map(|(id, _, _)| id)
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
                validate_parameter(id, ty, value)?;
            }
        }
        Ok(())
    }
    pub fn resolve(&self, values: &BTreeMap<PropertyId, Value>) -> Result<ResolvedText, TextError> {
        for (id, ty, _) in self.parameters() {
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
                .map(|s| ResolvedTextStyle {
                    range: s.range,
                    font: s.font.clone(),
                    size: scalar(s.size),
                    fill: match values[&s.fill] {
                        Value::Color(v) => v,
                        _ => unreachable!(),
                    },
                })
                .collect(),
            direction: self.direction,
            ruby: self.ruby.clone(),
            wrap_width: scalar(self.wrap_width),
            line_height: scalar(self.line_height),
            alignment: match &values[&self.alignment] {
                Value::Enum(v) if v == "start" => TextAlignment::Start,
                Value::Enum(v) if v == "center" => TextAlignment::Center,
                _ => TextAlignment::End,
            },
        };
        resolved.validate()?;
        Ok(resolved)
    }
}
fn validate_parameter(id: PropertyId, ty: ValueType, value: &Value) -> Result<(), TextError> {
    let valid = match (ty, value) {
        (ValueType::Scalar, Value::Scalar(v)) => v.get() > 0.0,
        (ValueType::Color, Value::Color(_)) => true,
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
pub fn text_descriptors() -> Vec<PropertyDescriptor> {
    let n = |v| FiniteF64::new(v).expect("finite text default");
    [
        (
            0xa132814b_f79c_4c5f_a59d_b9f0dd9a66dd,
            "font_size",
            Value::Scalar(n(32.0)),
        ),
        (
            0x57b6f88f_51c8_4d76_b052_04e4235281f3,
            "wrap_width",
            Value::Scalar(n(640.0)),
        ),
        (
            0x5d72aedd_02fe_4191_879f_f69c55eeecdf,
            "line_height",
            Value::Scalar(n(48.0)),
        ),
        (
            0x5f72d26a_cc83_434e_9599_a35dad348a96,
            "alignment",
            Value::Enum("start".into()),
        ),
    ]
    .into_iter()
    .map(|(id, name, default)| {
        let scalar = default.value_type() == ValueType::Scalar;
        let mut definition = DescriptorDefinition::new(
            DescriptorId::from_uuid(Uuid::from_u128(id)),
            SchemaKey::new(format!("kronello.text.{name}")).expect("valid text key"),
            name,
            default.value_type(),
            if scalar {
                Unit::DesignPx
            } else {
                Unit::Dimensionless
            },
            default,
        );
        if scalar {
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
