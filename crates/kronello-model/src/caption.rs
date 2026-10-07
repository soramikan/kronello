//! First-class caption/cue documents (ADR-0107). Versioned, strictly decoded
//! objects; subtitle text is data with `\n` separators, never markup or code.
use std::collections::{BTreeMap, BTreeSet};

use crate::*;
use kronello_time::Rational;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

pub const CAPTION_VERSION: u32 = 1;

/// Nine-direction anchor inside the caption safe area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}
impl CaptionAnchor {
    /// Horizontal and vertical anchor ratios inside the safe area.
    pub fn ratios(self) -> [f64; 2] {
        use CaptionAnchor::*;
        match self {
            TopLeft | CenterLeft | BottomLeft => [0.0, self.vertical()],
            TopCenter | Center | BottomCenter => [0.5, self.vertical()],
            TopRight | CenterRight | BottomRight => [1.0, self.vertical()],
        }
    }
    fn vertical(self) -> f64 {
        use CaptionAnchor::*;
        match self {
            TopLeft | TopCenter | TopRight => 0.0,
            CenterLeft | Center | CenterRight => 0.5,
            BottomLeft | BottomCenter | BottomRight => 1.0,
        }
    }
    /// Text alignment implied by the anchor column.
    pub fn alignment(self) -> TextAlignment {
        use CaptionAnchor::*;
        match self {
            TopLeft | CenterLeft | BottomLeft => TextAlignment::Start,
            TopCenter | Center | BottomCenter => TextAlignment::Center,
            TopRight | CenterRight | BottomRight => TextAlignment::End,
        }
    }
    pub fn name(self) -> &'static str {
        use CaptionAnchor::*;
        match self {
            TopLeft => "top_left",
            TopCenter => "top_center",
            TopRight => "top_right",
            CenterLeft => "center_left",
            Center => "center",
            CenterRight => "center_right",
            BottomLeft => "bottom_left",
            BottomCenter => "bottom_center",
            BottomRight => "bottom_right",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        use CaptionAnchor::*;
        Some(match name {
            "top_left" => TopLeft,
            "top_center" => TopCenter,
            "top_right" => TopRight,
            "center_left" => CenterLeft,
            "center" => Center,
            "center_right" => CenterRight,
            "bottom_left" => BottomLeft,
            "bottom_center" => BottomCenter,
            "bottom_right" => BottomRight,
            _ => return None,
        })
    }
}

/// Nine-direction caption placement relative to the sequence safe area.
/// Ratios are stored exactly as rationals, never as floating point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionPlacement {
    pub anchor: CaptionAnchor,
    /// Inset applied to both sides of the sequence extent per axis, as a ratio
    /// of that extent. Each component must be in `0..=1/2`.
    pub safe_area_inset: [Rational; 2],
    /// Additional offset from the anchor point, as a ratio of the sequence
    /// extent. +x moves right, +y moves down.
    pub offset: [Rational; 2],
}
impl Default for CaptionPlacement {
    fn default() -> Self {
        Self {
            anchor: CaptionAnchor::BottomCenter,
            safe_area_inset: [Rational::new(1, 20).expect("valid inset"); 2],
            offset: [Rational::ZERO; 2],
        }
    }
}
impl CaptionPlacement {
    /// Exact ratios resolved to f64 only at the evaluation boundary.
    pub fn resolve(&self) -> Result<ResolvedCaptionPlacement, CaptionError> {
        let ratio = |r: Rational| {
            if r.denominator() == 0 {
                return Err(CaptionError::Invalid("placement denominator".into()));
            }
            Ok(r.numerator() as f64 / r.denominator() as f64)
        };
        Ok(ResolvedCaptionPlacement {
            anchor: self.anchor,
            safe_area_inset: [
                ratio(self.safe_area_inset[0])?,
                ratio(self.safe_area_inset[1])?,
            ],
            offset: [ratio(self.offset[0])?, ratio(self.offset[1])?],
        })
    }
    fn validate(&self) -> Result<(), CaptionError> {
        let half = Rational::new(1, 2).expect("constant half");
        for inset in self.safe_area_inset {
            if inset < Rational::ZERO || inset > half {
                return Err(CaptionError::Invalid(
                    "safe_area_inset must be a ratio in 0..=1/2".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Evaluated placement in dimensionless ratios of the sequence extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedCaptionPlacement {
    pub anchor: CaptionAnchor,
    pub safe_area_inset: [f64; 2],
    pub offset: [f64; 2],
}
impl ResolvedCaptionPlacement {
    /// Text-block origin in design_px for a block of the given size.
    /// The block width equals the safe-area width, so the horizontal anchor
    /// selects the line alignment inside that box rather than moving the box.
    pub fn origin(&self, extent: DesignExtent, block: [f64; 2]) -> [f64; 2] {
        let extent = [extent.width(), extent.height()];
        let safe_min = [0, 1].map(|i| self.safe_area_inset[i] * extent[i]);
        let safe_max = [0, 1].map(|i| extent[i] * (1.0 - self.safe_area_inset[i]));
        let anchor = self.anchor.ratios();
        [0, 1].map(|i| {
            safe_min[i] + anchor[i] * (safe_max[i] - safe_min[i]) + self.offset[i] * extent[i]
                - anchor[i] * block[i]
        })
    }
    /// Safe-area width in design_px; the cue block wraps inside this width.
    pub fn wrap_width(&self, extent: DesignExtent) -> f64 {
        extent.width() * (1.0 - 2.0 * self.safe_area_inset[0])
    }
}

/// Stroke drawn outside the glyph fill, in the sequence working color space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionOutline {
    pub color: Color,
    /// Visible outline ring width in design_px; zero is allowed but draws
    /// nothing. Rendering strokes the glyph path centered at twice this width.
    pub width: FiniteF64,
}

/// Cue-wide base style. Span attributes override these values inside their
/// text range only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionStyle {
    /// Hash-locked face; rendering never substitutes family/system fonts.
    pub font: FontRef,
    /// Positive glyph size in design_px.
    pub size: FiniteF64,
    /// Straight tagged color evaluated in the sequence working space.
    pub fill: Color,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<CaptionOutline>,
    /// Optional rectangle behind the laid-out cue block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
}
impl CaptionStyle {
    fn validate(&self) -> Result<(), CaptionError> {
        self.font.validate()?;
        if self.size.get() <= 0.0 {
            return Err(CaptionError::Invalid(
                "caption size must be positive".into(),
            ));
        }
        if let Some(outline) = &self.outline
            && outline.width.get() < 0.0
        {
            return Err(CaptionError::Invalid(
                "outline width must be nonnegative".into(),
            ));
        }
        Ok(())
    }
}

/// Span-level style override. Unknown attributes are rejected by strict
/// decoding instead of being silently dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionSpan {
    pub range: TextRange,
    /// Drawn as a synthesized stroke when no replacement span font is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Drawn as a synthesized oblique shear when no replacement span font is
    /// given. Both flags are draw-time attributes of the span, not markup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Explicit replacement face; its bytes remain hash-locked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontRef>,
}

/// Draw-time synthesis flags attached to one resolved style entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptionSpanFlags {
    pub bold: bool,
    pub italic: bool,
}

/// Import provenance of a cue document. Export always reserializes the stored
/// document; this metadata never drives rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionFormat {
    Srt,
    Vtt,
    Itt,
}
impl CaptionFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Itt => "itt",
        }
    }
}

/// Versioned cue document. One `CaptionDocument` describes exactly one cue's
/// text, style and placement; its timeline interval lives on the caption clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionDocument {
    pub id: CaptionId,
    /// Supported document version; this implementation accepts only 1.
    pub version: u32,
    /// Cue payload text. `\n` is the only line separator and the content is
    /// data: import decoders strip markup, and nothing here is executed.
    pub text: String,
    pub style: CaptionStyle,
    /// Ordered, nonoverlapping span overrides at grapheme boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<CaptionSpan>,
    pub placement: CaptionPlacement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<CaptionFormat>,
}

#[derive(Debug, Error)]
pub enum CaptionError {
    #[error("unsupported caption document version: {0}")]
    UnsupportedVersion(u32),
    #[error("invalid caption: {0}")]
    Invalid(String),
    #[error("missing caption content {id}")]
    MissingContent { id: CaptionId },
    #[error("duplicate caption content {id}")]
    DuplicateContent { id: CaptionId },
    #[error("opaque caption content is unsupported")]
    UnsupportedContent,
    #[error("unsupported caption feature: {0}")]
    UnsupportedFeature(String),
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Model(#[from] ModelError),
}
impl CaptionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedVersion(_) => "UNSUPPORTED_CAPTION_VERSION",
            Self::Invalid(_) => "INVALID_CAPTION",
            Self::MissingContent { .. } => "CAPTION_MISSING",
            Self::DuplicateContent { .. } => "CAPTION_DUPLICATE",
            Self::UnsupportedContent => "UNSUPPORTED_CAPTION",
            Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            Self::Text(_) => "INVALID_CAPTION_TEXT",
            Self::Model(_) => "INVALID_CAPTION",
        }
    }
}

impl CaptionDocument {
    pub fn ensure_supported_version(&self) -> Result<(), CaptionError> {
        if self.version != CAPTION_VERSION {
            return Err(CaptionError::UnsupportedVersion(self.version));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), CaptionError> {
        self.ensure_supported_version()?;
        self.style.validate()?;
        // Canonical storage separates lines with '\n' only; other Unicode or
        // carriage-return separators are normalized at import boundaries.
        if self
            .text
            .chars()
            .any(|c| matches!(c, '\r' | '\u{2028}' | '\u{2029}'))
        {
            return Err(CaptionError::Invalid(
                "caption text must use \\n line separators only".into(),
            ));
        }
        let boundaries: BTreeSet<_> = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([self.text.len()])
            .collect();
        let mut end = 0;
        for span in &self.spans {
            let range = span.range;
            if range.start < end
                || range.start >= range.end
                || range.end > self.text.len()
                || !boundaries.contains(&range.start)
                || !boundaries.contains(&range.end)
            {
                return Err(CaptionError::Invalid(
                    "span ranges must be ordered, nonoverlapping and on grapheme boundaries".into(),
                ));
            }
            if let Some(font) = &span.font {
                font.validate()?;
            }
            end = range.end;
        }
        self.placement.validate()?;
        Ok(())
    }

    /// Hash-locked fonts referenced by the base style and every span.
    pub fn fonts(&self) -> Vec<FontRef> {
        let mut fonts = vec![self.style.font.clone()];
        fonts.extend(self.spans.iter().filter_map(|s| s.font.clone()));
        fonts
    }

    /// Apply clip-level `kronello.caption.*` properties over the document
    /// style. Other clip properties (transform, opacity, blend, effects) stay
    /// on the normal evaluation path. Caption descriptors are constant-only,
    /// so a non-constant source fails descriptor validation rather than being
    /// evaluated.
    pub fn resolve(
        &self,
        overrides: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<ResolvedCaption, CaptionError> {
        self.validate()?;
        let mut font = self.style.font.clone();
        let mut size = self.style.size;
        let mut fill = self.style.fill;
        let mut outline = self.style.outline.clone();
        let mut background = self.style.background;
        let mut placement = self.placement.clone();
        let mut offset_override: Option<[f64; 2]> = None;
        let mut inset_override: Option<[f64; 2]> = None;
        let mut seen = BTreeSet::new();
        for property in overrides {
            let key = property.descriptor().key.as_str();
            if !key.starts_with("kronello.caption.") {
                continue;
            }
            if !seen.insert(key.to_string()) {
                return Err(CaptionError::Invalid(format!(
                    "duplicate caption override {key}"
                )));
            }
            property.validate(registry)?;
            let PropertySource::Constant(value) = property.source() else {
                return Err(CaptionError::UnsupportedFeature(format!(
                    "nonconstant caption override {key}"
                )));
            };
            match (key, value) {
                ("kronello.caption.font", Value::String(encoded)) => {
                    font = serde_json::from_str::<FontRef>(encoded)
                        .map_err(|e| CaptionError::Invalid(e.to_string()))?;
                    font.validate()?;
                }
                ("kronello.caption.font_size", Value::Scalar(v)) => size = *v,
                ("kronello.caption.color", Value::Color(v)) => fill = *v,
                ("kronello.caption.outline_width", Value::Scalar(v)) => {
                    outline = Some(CaptionOutline {
                        width: *v,
                        color: outline
                            .as_ref()
                            .map(|o| o.color)
                            .unwrap_or_else(|| Color::from_srgb8([0; 3], None)),
                    });
                }
                ("kronello.caption.outline_color", Value::Color(v)) => {
                    outline = Some(CaptionOutline {
                        width: outline
                            .as_ref()
                            .map(|o| o.width)
                            .unwrap_or_else(|| FiniteF64::new(0.0).expect("finite zero")),
                        color: *v,
                    });
                }
                ("kronello.caption.background", Value::Color(v)) => background = Some(*v),
                ("kronello.caption.anchor", Value::Enum(v)) => {
                    placement.anchor = CaptionAnchor::from_name(v).ok_or_else(|| {
                        CaptionError::Invalid(format!("unknown caption anchor {v}"))
                    })?;
                }
                ("kronello.caption.offset", Value::Vec2(v)) => {
                    offset_override = Some([v[0].get(), v[1].get()]);
                }
                ("kronello.caption.safe_area_inset", Value::Vec2(v)) => {
                    inset_override = Some([v[0].get(), v[1].get()]);
                }
                _ => {
                    return Err(CaptionError::Invalid(format!(
                        "caption override {key} has an incompatible value"
                    )));
                }
            }
        }
        let mut resolved_placement = placement.resolve()?;
        if let Some(offset) = offset_override {
            resolved_placement.offset = offset;
        }
        if let Some(inset) = inset_override {
            resolved_placement.safe_area_inset = inset;
        }
        for inset in resolved_placement.safe_area_inset {
            if !(0.0..=0.5).contains(&inset) {
                return Err(CaptionError::Invalid(
                    "safe_area_inset override must be a ratio in 0..=0.5".into(),
                ));
            }
        }
        let mut styles = Vec::new();
        let mut span_flags = Vec::new();
        let mut start = 0;
        for span in &self.spans {
            if span.range.start > start {
                push_style(
                    &mut styles,
                    &mut span_flags,
                    TextRange {
                        start,
                        end: span.range.start,
                    },
                    font.clone(),
                    size,
                    fill,
                    CaptionSpanFlags::default(),
                );
            }
            push_style(
                &mut styles,
                &mut span_flags,
                span.range,
                span.font.clone().unwrap_or_else(|| font.clone()),
                size,
                span.color.unwrap_or(fill),
                CaptionSpanFlags {
                    bold: span.bold.unwrap_or(false),
                    italic: span.italic.unwrap_or(false),
                },
            );
            start = span.range.end;
        }
        if start < self.text.len() {
            push_style(
                &mut styles,
                &mut span_flags,
                TextRange {
                    start,
                    end: self.text.len(),
                },
                font,
                size,
                fill,
                CaptionSpanFlags::default(),
            );
        }
        Ok(ResolvedCaption {
            text: self.text.clone(),
            styles,
            span_flags,
            font_size: size.get(),
            outline,
            background,
            placement: resolved_placement,
        })
    }
}

/// Fixed caption line-height ratio for v1, in em units relative to the
/// effective glyph size. Inter-line spacing follows the subtitle convention of
/// 6/5 of the glyph size; it is not part of the stored style.
pub const CAPTION_LINE_HEIGHT_RATIO: f64 = 6.0 / 5.0;

/// Synthesized bold: outer ring width added around the glyph outline, as a
/// ratio of the effective glyph size. The rendered stroke is centered at twice
/// this ring width so the authored value is the visible weight.
pub const CAPTION_BOLD_WIDTH_RATIO: f64 = 0.04;

/// Synthesized italic: oblique shear factor applied in text-local space.
/// Roughly 11 degrees, matching common caption oblique conventions.
pub const CAPTION_ITALIC_SHEAR: f64 = 0.2;

fn push_style(
    styles: &mut Vec<ResolvedTextStyle>,
    flags: &mut Vec<CaptionSpanFlags>,
    range: TextRange,
    font: FontRef,
    size: FiniteF64,
    fill: Color,
    span: CaptionSpanFlags,
) {
    styles.push(ResolvedTextStyle {
        gradient: None,
        range,
        font,
        size,
        fill,
    });
    flags.push(span);
}

/// Fully evaluated caption style for one clip: document values with clip
/// overrides applied, before extent-dependent layout metrics are attached.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCaption {
    /// Cue payload text, `\n` separated.
    pub text: String,
    /// Complete ordered coverage at grapheme boundaries, one entry per
    /// contiguous style run.
    pub styles: Vec<ResolvedTextStyle>,
    /// Draw-time synthesis flags parallel to `styles`.
    pub span_flags: Vec<CaptionSpanFlags>,
    /// Effective glyph size in design_px; also derives the line height.
    pub font_size: f64,
    pub outline: Option<CaptionOutline>,
    pub background: Option<Color>,
    pub placement: ResolvedCaptionPlacement,
}
impl ResolvedCaption {
    /// Attach extent-dependent layout metrics and build the layout input.
    /// `wrap_width` is the safe-area width in design_px.
    pub fn resolved_text(&self, wrap_width: f64) -> Result<ResolvedText, CaptionError> {
        let text = ResolvedText {
            layout_version: TEXT_LAYOUT_VERSION,
            text: self.text.clone(),
            styles: self.styles.clone(),
            direction: TextDirection::Horizontal,
            ruby: vec![],
            character_animations: vec![],
            wrap_width: FiniteF64::new(wrap_width)?,
            line_height: FiniteF64::new(self.font_size * CAPTION_LINE_HEIGHT_RATIO)?,
            alignment: self.placement.anchor.alignment(),
        };
        text.validate()?;
        Ok(text)
    }
    /// Hash-locked fonts required to lay out the resolved cue.
    pub fn fonts(&self) -> Vec<FontRef> {
        self.styles.iter().map(|s| s.font.clone()).collect()
    }
}

/// Fixed UUID identities and constant-only descriptors for clip-level caption
/// overrides. v1 style values are never animated: curves, expressions and
/// modifiers are all disabled.
pub const CAPTION_FONT_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x30a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_FONT_SIZE_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x31a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_COLOR_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x32a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_OUTLINE_WIDTH_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x33a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_OUTLINE_COLOR_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x34a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_BACKGROUND_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x35a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_ANCHOR_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x36a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_OFFSET_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x37a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));
pub const CAPTION_SAFE_AREA_INSET_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0x38a0c1d2_e4f5_4a6b_9c8d_7e6f5a4b3c2d));

/// Explicitly register these with the service/render schema registries, like
/// `text_descriptors`. `kronello.caption.font` encodes a canonical JSON
/// `FontRef` in a String value; placement values are dimensionless ratios.
pub fn caption_descriptors() -> Vec<PropertyDescriptor> {
    let zero = FiniteF64::new(0.0).expect("finite caption default");
    let size = FiniteF64::new(32.0).expect("finite caption default");
    let entries: [(DescriptorId, &str, &str, Value); 9] = [
        (
            CAPTION_FONT_ID,
            "font",
            "Font",
            Value::String(String::new()),
        ),
        (
            CAPTION_FONT_SIZE_ID,
            "font_size",
            "Font size",
            Value::Scalar(size),
        ),
        (
            CAPTION_COLOR_ID,
            "color",
            "Color",
            Value::Color(Color::from_srgb8([255; 3], None)),
        ),
        (
            CAPTION_OUTLINE_WIDTH_ID,
            "outline_width",
            "Outline width",
            Value::Scalar(zero),
        ),
        (
            CAPTION_OUTLINE_COLOR_ID,
            "outline_color",
            "Outline color",
            Value::Color(Color::from_srgb8([0; 3], None)),
        ),
        (
            CAPTION_BACKGROUND_ID,
            "background",
            "Background",
            Value::Color(Color::from_srgb8([0; 3], Some(0))),
        ),
        (
            CAPTION_ANCHOR_ID,
            "anchor",
            "Anchor",
            Value::Enum("bottom_center".into()),
        ),
        (
            CAPTION_OFFSET_ID,
            "offset",
            "Offset",
            Value::Vec2([zero; 2]),
        ),
        (
            CAPTION_SAFE_AREA_INSET_ID,
            "safe_area_inset",
            "Safe area inset",
            Value::Vec2([zero; 2]),
        ),
    ];
    entries
        .into_iter()
        .map(|(id, name, label, default)| {
            let value_type = default.value_type();
            let mut definition = DescriptorDefinition::new(
                id,
                SchemaKey::new(format!("kronello.caption.{name}")).expect("valid caption key"),
                label,
                value_type,
                if matches!(name, "font_size" | "outline_width") {
                    Unit::DesignPx
                } else {
                    Unit::Dimensionless
                },
                default,
            );
            if name == "font_size" || name == "outline_width" {
                definition.range = Some(ValueRange::Scalar(NumericRange {
                    min: Some(NumericBound {
                        value: zero,
                        inclusive: name == "outline_width",
                    }),
                    max: None,
                }));
            }
            if matches!(name, "offset" | "safe_area_inset") {
                definition.range =
                    Some(ValueRange::Vec2([0, 1].map(|_| {
                        NumericRange::inclusive(-1.0, 1.0).expect("valid caption range")
                    })));
            }
            definition.animatable = false;
            definition.interpolation_modes.clear();
            definition.capabilities.curves = false;
            definition.capabilities.expressions = false;
            definition.capabilities.modifiers = false;
            PropertyDescriptor::new(definition).expect("valid caption descriptor")
        })
        .collect()
}

/// Full validation of the project's caption pool: unique identities, every
/// known document passing v1 validation, and every clip-referenced cue present
/// and decodable. `Project::validate_storage` performs the looser preservation
/// checks; call this at edit/render boundaries.
pub fn validate_caption_contents(project: &Project) -> Result<(), CaptionError> {
    let mut contents = BTreeMap::new();
    for object in &project.captions {
        let id = match object {
            DocumentObject::Known(caption) => {
                caption.validate()?;
                caption.id
            }
            DocumentObject::Opaque(value) => CaptionId::from_uuid(value.id),
        };
        if contents.insert(id, object).is_some() {
            return Err(CaptionError::DuplicateContent { id });
        }
    }
    for object in &project.sequences {
        let DocumentObject::Known(sequence) = object else {
            continue;
        };
        for clip in sequence.tracks.iter().flat_map(|track| track.clips.iter()) {
            let SourceRef::Caption { caption } = &clip.source_ref else {
                continue;
            };
            match contents.get(caption) {
                Some(DocumentObject::Known(_)) => (),
                Some(DocumentObject::Opaque(_)) => {
                    return Err(CaptionError::UnsupportedContent);
                }
                None => return Err(CaptionError::MissingContent { id: *caption }),
            }
        }
    }
    Ok(())
}
