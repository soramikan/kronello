//! Deterministic horizontal layout from evaluated text and explicitly supplied,
//! locked font bytes. No filesystem, font discovery, fallback, or persistent cache.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use kronello_model::{
    Color, FiniteF64, FontRef, Path, PathSegment, ResolvedText, TEXT_LAYOUT_VERSION, TextAlignment,
    TextDirection, TextError, TextRange,
};
use rustybuzz::{BufferClusterLevel, Direction, UnicodeBuffer};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ttf_parser::{Face, GlyphId, OutlineBuilder};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

pub const LAYOUT_VERSION: u32 = TEXT_LAYOUT_VERSION;
const MAX_TEXT_BYTES: usize = 65_536;
const MAX_GLYPHS: usize = 131_072;
const MAX_OUTLINE_SEGMENTS: usize = 1_048_576;

#[derive(Debug, Clone, Copy)]
pub struct FontData<'a> {
    pub identity: &'a FontRef,
    pub bytes: &'a [u8],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingCluster {
    pub source: TextRange,
    pub text: String,
    pub font: FontRef,
}
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayoutError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("unsupported text layout version {version}")]
    UnsupportedVersion { version: u32 },
    #[error("unsupported text feature: {feature}")]
    UnsupportedFeature { feature: &'static str },
    #[error("missing locked font: {font:?}")]
    MissingFont { font: FontRef },
    #[error("font content hash mismatch: expected {expected}, actual {actual}")]
    FontHashMismatch { expected: String, actual: String },
    #[error("font metadata does not match the locked identity")]
    FontIdentityMismatch {
        expected: Box<FontRef>,
        actual: Box<FontRef>,
    },
    #[error("invalid font file or face index {face_index}")]
    InvalidFont { face_index: u32 },
    #[error("missing glyphs for source clusters: {clusters:?}")]
    MissingGlyphs { clusters: Vec<MissingCluster> },
    #[error("glyph {glyph_id} has no supported vector outline")]
    UnsupportedGlyphOutline { glyph_id: u16 },
    #[error("text exceeds the conservative layout work budget")]
    BudgetExceeded,
    #[error("derived text geometry is non-finite")]
    NonFiniteGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub struct GraphemeCluster {
    pub source: TextRange,
    /// Index range into LayoutResult.shaping_clusters. Several graphemes can
    /// reference the same shaping cluster (e.g. a ligature).
    pub shaping_clusters: Range<usize>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShapingCluster {
    pub source: TextRange,
    pub graphemes: Range<usize>,
    pub glyphs: Range<usize>,
    pub style_index: usize,
    pub advance: f64,
    pub unsafe_to_break_before: bool,
    /// Explicit separators retain source correspondence with no glyphs.
    pub mandatory_break: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationUnit {
    pub source: TextRange,
    pub graphemes: Range<usize>,
    pub shaping_clusters: Range<usize>,
    pub glyphs: Range<usize>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub glyph_id: u16,
    /// Identifies the font and fill in the evaluated input's styles.
    pub style_index: usize,
    pub source: TextRange,
    pub graphemes: Range<usize>,
    pub shaping_cluster: usize,
    /// Baseline origin plus shaping offset, in text-local design_px (+Y down).
    pub position: [f64; 2],
    pub advance: f64,
    pub fill: Color,
    /// Positioned text-local vector path, suitable for coverage rasterization.
    pub outline: Path,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLine {
    /// Includes an explicit separator when present.
    pub source: TextRange,
    pub shaping_clusters: Range<usize>,
    pub glyphs: Range<usize>,
    pub baseline: f64,
    pub advance: f64,
    pub bounds: Bounds,
    pub hard_break: bool,
    /// An indivisible segment can exceed wrap width; no silent clipping occurs.
    pub overflow: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    pub layout_version: u32,
    pub lines: Vec<LayoutLine>,
    pub glyphs: Vec<PositionedGlyph>,
    pub graphemes: Vec<GraphemeCluster>,
    pub shaping_clusters: Vec<ShapingCluster>,
    /// Conservative units also join boundaries marked unsafe by the shaper.
    /// These indices are derived, never stable document identifiers.
    pub animation_units: Vec<AnimationUnit>,
    pub layout_bounds: Bounds,
    pub ink_bounds: Option<Bounds>,
}

/// Creates a lock identity from actual bytes at an explicit import boundary.
/// Layout verifies this identity again and never treats it as a lookup hint.
pub fn pin_font(bytes: &[u8], face_index: u32) -> Result<FontRef, LayoutError> {
    let face =
        Face::parse(bytes, face_index).map_err(|_| LayoutError::InvalidFont { face_index })?;
    let name = |id| {
        face.names()
            .into_iter()
            .filter(|n| n.name_id == id)
            .find_map(|n| n.to_string())
    };
    Ok(FontRef {
        family: name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| name(ttf_parser::name_id::FAMILY))
            .ok_or(LayoutError::InvalidFont { face_index })?,
        postscript_name: name(ttf_parser::name_id::POST_SCRIPT_NAME)
            .ok_or(LayoutError::InvalidFont { face_index })?,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        face_index,
    })
}
fn verified_faces<'a>(
    text: &ResolvedText,
    fonts: &[FontData<'a>],
) -> Result<Vec<Face<'a>>, LayoutError> {
    let mut verified = BTreeMap::new();
    for style in &text.styles {
        if verified.contains_key(&style.font) {
            continue;
        }
        let mut candidates = fonts.iter().filter(|f| f.identity == &style.font);
        let source = candidates.next().ok_or_else(|| LayoutError::MissingFont {
            font: style.font.clone(),
        })?;
        // Ambiguous duplicate sources are not selected by input order.
        if candidates.next().is_some() {
            return Err(LayoutError::UnsupportedFeature {
                feature: "duplicate font source",
            });
        }
        let hash = format!("{:x}", Sha256::digest(source.bytes));
        if hash != style.font.sha256 {
            return Err(LayoutError::FontHashMismatch {
                expected: style.font.sha256.clone(),
                actual: hash,
            });
        }
        let actual = pin_font(source.bytes, style.font.face_index)?;
        if actual != style.font {
            return Err(LayoutError::FontIdentityMismatch {
                expected: Box::new(style.font.clone()),
                actual: Box::new(actual),
            });
        }
        let face = Face::parse(source.bytes, style.font.face_index).map_err(|_| {
            LayoutError::InvalidFont {
                face_index: style.font.face_index,
            }
        })?;
        if face.is_variable() {
            return Err(LayoutError::UnsupportedFeature {
                feature: "variable font axes",
            });
        }
        verified.insert(style.font.clone(), face);
    }
    Ok(text
        .styles
        .iter()
        .map(|s| verified[&s.font].clone())
        .collect())
}
/// Verifies supplied locked bytes even when a caller reuses derived layout.
pub fn validate_fonts(text: &ResolvedText, fonts: &[FontData<'_>]) -> Result<(), LayoutError> {
    text.validate()?;
    verified_faces(text, fonts)?;
    Ok(())
}
fn separator(text: &str) -> bool {
    matches!(text, "\n" | "\r" | "\r\n" | "\u{2028}" | "\u{2029}")
}
fn variation(c: char) -> bool {
    matches!(c, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}
fn missing_variation(text: &str, face: &Face<'_>) -> bool {
    let mut previous = None;
    for c in text.chars() {
        if variation(c) && previous.is_none_or(|base| face.glyph_variation_index(base, c).is_none())
        {
            return true;
        }
        previous = Some(c);
    }
    false
}
struct RawGlyph {
    id: u16,
    offset: [f64; 2],
    advance: f64,
}
struct RawCluster {
    source: TextRange,
    style: usize,
    glyphs: Vec<RawGlyph>,
    advance: f64,
    unsafe_before: bool,
    separator: bool,
}

fn shape_run(
    text: &ResolvedText,
    faces: &[Face<'_>],
    range: TextRange,
    style: usize,
    script: Script,
) -> Result<Vec<RawCluster>, LayoutError> {
    let face = rustybuzz::Face::from_slice(
        faces[style].raw_face().data,
        text.styles[style].font.face_index,
    )
    .ok_or(LayoutError::InvalidFont {
        face_index: text.styles[style].font.face_index,
    })?;
    let mut buffer = UnicodeBuffer::new();
    // Assign the same source cluster to every scalar of one extended grapheme.
    // The shaper's Unicode tables need not have the same grapheme version.
    for (offset, grapheme) in text.text[range.start..range.end].grapheme_indices(true) {
        for c in grapheme.chars() {
            buffer.add(c, offset as u32);
        }
    }
    buffer.set_cluster_level(BufferClusterLevel::MonotoneGraphemes);
    buffer.set_script(script.short_name().parse().expect("Unicode script ISO tag"));
    buffer.set_language("ja".parse().expect("fixed BCP47 language"));
    buffer.guess_segment_properties();
    if buffer.direction() != Direction::LeftToRight {
        return Err(LayoutError::UnsupportedFeature {
            feature: "bidirectional/RTL layout",
        });
    }
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let scale = text.styles[style].size.get() / f64::from(faces[style].units_per_em());
    let mut clusters: Vec<RawCluster> = Vec::new();
    for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
        let start = range.start + info.cluster as usize;
        if clusters.last().is_none_or(|c| c.source.start != start) {
            if let Some(previous) = clusters.last_mut() {
                previous.source.end = start;
            }
            clusters.push(RawCluster {
                source: TextRange {
                    start,
                    end: range.end,
                },
                style,
                glyphs: vec![],
                advance: 0.0,
                unsafe_before: false,
                separator: false,
            });
        }
        let cluster = clusters.last_mut().expect("just created shaping cluster");
        cluster.unsafe_before |= info.unsafe_to_break();
        let advance = f64::from(position.x_advance) * scale;
        cluster.advance += advance;
        cluster.glyphs.push(RawGlyph {
            id: info.glyph_id as u16,
            offset: [
                f64::from(position.x_offset) * scale,
                -f64::from(position.y_offset) * scale,
            ],
            advance,
        });
    }
    Ok(clusters)
}
fn shape_text(text: &ResolvedText, faces: &[Face<'_>]) -> Result<Vec<RawCluster>, LayoutError> {
    let mut clusters = Vec::new();
    for (style, span) in text.styles.iter().enumerate() {
        let slice = &text.text[span.range.start..span.range.end];
        let mut run_start = span.range.start;
        let mut run_script = Script::Common;
        for (offset, grapheme) in slice.grapheme_indices(true) {
            let start = span.range.start + offset;
            let script = grapheme
                .chars()
                .map(|c| c.script())
                .find(|s| !matches!(s, Script::Common | Script::Inherited))
                .unwrap_or(run_script);
            let hard = separator(grapheme);
            if hard
                || (script != run_script
                    && start > run_start
                    && !matches!(run_script, Script::Common | Script::Inherited))
            {
                if start > run_start {
                    clusters.extend(shape_run(
                        text,
                        faces,
                        TextRange {
                            start: run_start,
                            end: start,
                        },
                        style,
                        run_script,
                    )?);
                }
                run_start = start;
            }
            if hard {
                clusters.push(RawCluster {
                    source: TextRange {
                        start,
                        end: start + grapheme.len(),
                    },
                    style,
                    glyphs: vec![],
                    advance: 0.0,
                    unsafe_before: false,
                    separator: true,
                });
                run_start = start + grapheme.len();
                run_script = Script::Common;
            } else {
                run_script = script;
            }
        }
        if run_start < span.range.end {
            clusters.extend(shape_run(
                text,
                faces,
                TextRange {
                    start: run_start,
                    end: span.range.end,
                },
                style,
                run_script,
            )?);
        }
    }
    let missing: Vec<_> = clusters
        .iter()
        .filter(|c| {
            !c.separator
                && (c.glyphs.iter().any(|g| g.id == 0)
                    || missing_variation(&text.text[c.source.start..c.source.end], &faces[c.style]))
        })
        .map(|c| MissingCluster {
            source: c.source,
            text: text.text[c.source.start..c.source.end].to_owned(),
            font: text.styles[c.style].font.clone(),
        })
        .collect();
    if !missing.is_empty() {
        return Err(LayoutError::MissingGlyphs { clusters: missing });
    }
    if clusters.iter().map(|c| c.glyphs.len()).sum::<usize>() > MAX_GLYPHS {
        return Err(LayoutError::BudgetExceeded);
    }
    Ok(clusters)
}

/// Basic Japanese kinsoku set, versioned with LAYOUT_VERSION. Mandatory source
/// separators take precedence. No hanging punctuation or automatic compression.
pub fn prohibited_line_start(c: char) -> bool {
    "、。，．・：；？！‼⁇⁈⁉ー〜～…‥ヽヾゝゞ々〻」』）］｝〕〉》】〙〗〟’”»)]},.!?:;ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ".contains(c)
}
pub fn prohibited_line_end(c: char) -> bool {
    "「『（［｛〔〈《【〘〖〝‘“«([{ ".trim_end().contains(c)
}
fn legal_break(text: &str, position: usize) -> bool {
    !text[..position]
        .chars()
        .next_back()
        .is_some_and(prohibited_line_end)
        && !text[position..]
            .chars()
            .next()
            .is_some_and(prohibited_line_start)
}
fn line_ranges(text: &ResolvedText, clusters: &[RawCluster]) -> Vec<Range<usize>> {
    let opportunities: BTreeSet<_> = unicode_linebreak::linebreaks(&text.text)
        .map(|(i, _)| i)
        .collect();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < clusters.len() {
        let mut width = 0.0;
        let mut fitting = None;
        let mut selected = clusters.len();
        for end in start + 1..=clusters.len() {
            let current = &clusters[end - 1];
            width += current.advance;
            let hard = current.separator || end == clusters.len();
            let legal = hard
                || (!clusters[end].unsafe_before
                    && opportunities.contains(&current.source.end)
                    && legal_break(&text.text, current.source.end));
            if !legal {
                continue;
            }
            if width <= text.wrap_width.get() {
                fitting = Some(end);
            }
            if width > text.wrap_width.get() {
                selected = fitting.unwrap_or(end);
                break;
            }
            if hard {
                selected = end;
                break;
            }
        }
        lines.push(start..selected);
        start = selected;
    }
    // Empty text and terminal newlines each have an empty final line box.
    if clusters.is_empty() || clusters.last().is_some_and(|c| c.separator) {
        lines.push(clusters.len()..clusters.len());
    }
    lines
}

/// All lengths/positions/bounds use design_px. Output pixel scale is absent.
/// Preserves shaping across line breaks by permitting only safe shaper boundaries.
pub fn layout(text: &ResolvedText, fonts: &[FontData<'_>]) -> Result<LayoutResult, LayoutError> {
    if text.text.len() > MAX_TEXT_BYTES || text.styles.len() > 4096 {
        return Err(LayoutError::BudgetExceeded);
    }
    text.validate()?;
    if text.layout_version != LAYOUT_VERSION {
        return Err(LayoutError::UnsupportedVersion {
            version: text.layout_version,
        });
    }
    if text.direction != TextDirection::Horizontal {
        return Err(LayoutError::UnsupportedFeature {
            feature: "vertical text",
        });
    }
    if !text.ruby.is_empty() {
        return Err(LayoutError::UnsupportedFeature { feature: "ruby" });
    }
    if text.text.chars().any(|c| {
        (c.is_control() && !matches!(c, '\n' | '\r'))
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        return Err(LayoutError::UnsupportedFeature {
            feature: "tabs/control/bidi formatting",
        });
    }
    let faces = verified_faces(text, fonts)?;
    let raw = shape_text(text, &faces)?;
    let mut result = LayoutResult {
        layout_version: LAYOUT_VERSION,
        lines: vec![],
        glyphs: vec![],
        shaping_clusters: vec![],
        animation_units: vec![],
        graphemes: text
            .text
            .grapheme_indices(true)
            .map(|(start, g)| GraphemeCluster {
                source: TextRange {
                    start,
                    end: start + g.len(),
                },
                shaping_clusters: 0..0,
            })
            .collect(),
        layout_bounds: Bounds {
            min: [0.0; 2],
            max: [text.wrap_width.get(), 0.0],
        },
        ink_bounds: None,
    };
    let ascent = text
        .styles
        .iter()
        .zip(&faces)
        .map(|(style, face)| {
            f64::from(face.ascender()) * style.size.get() / f64::from(face.units_per_em())
        })
        .fold(0.0, f64::max);
    let mut outline_commands = 0;
    for (line_index, cluster_range) in line_ranges(text, &raw).into_iter().enumerate() {
        let advance: f64 = raw[cluster_range.clone()].iter().map(|c| c.advance).sum();
        let x_offset = match text.alignment {
            TextAlignment::Start => 0.0,
            TextAlignment::Center => (text.wrap_width.get() - advance) / 2.0,
            TextAlignment::End => text.wrap_width.get() - advance,
        };
        let top = line_index as f64 * text.line_height.get();
        let baseline = top + ascent;
        let glyph_start = result.glyphs.len();
        let mut x = x_offset;
        for cluster_index in cluster_range.clone() {
            let c = &raw[cluster_index];
            let grapheme_start = result
                .graphemes
                .partition_point(|g| g.source.end <= c.source.start);
            let grapheme_end = result
                .graphemes
                .partition_point(|g| g.source.start < c.source.end);
            let graphemes = grapheme_start..grapheme_end;
            let start = result.glyphs.len();
            for glyph in &c.glyphs {
                let position = [x + glyph.offset[0], baseline + glyph.offset[1]];
                let scale =
                    text.styles[c.style].size.get() / f64::from(faces[c.style].units_per_em());
                let (outline, bounds) = glyph_outline(&faces[c.style], glyph.id, scale, position)?;
                outline_commands += outline.segments.len();
                if outline_commands > MAX_OUTLINE_SEGMENTS {
                    return Err(LayoutError::BudgetExceeded);
                }
                if let Some(bounds) = bounds {
                    result.ink_bounds = Some(union(result.ink_bounds, bounds));
                }
                result.glyphs.push(PositionedGlyph {
                    glyph_id: glyph.id,
                    style_index: c.style,
                    source: c.source,
                    graphemes: graphemes.clone(),
                    shaping_cluster: cluster_index,
                    position,
                    advance: glyph.advance,
                    fill: text.styles[c.style].fill,
                    outline,
                });
                x += glyph.advance;
            }
            for g in &mut result.graphemes[graphemes.clone()] {
                g.shaping_clusters = cluster_index..cluster_index + 1;
            }
            result.shaping_clusters.push(ShapingCluster {
                source: c.source,
                graphemes,
                glyphs: start..result.glyphs.len(),
                style_index: c.style,
                advance: c.advance,
                unsafe_to_break_before: c.unsafe_before,
                mandatory_break: c.separator,
            });
        }
        let source = TextRange {
            start: raw
                .get(cluster_range.start)
                .map_or(text.text.len(), |c| c.source.start),
            end: cluster_range
                .end
                .checked_sub(1)
                .and_then(|i| raw.get(i))
                .filter(|_| !cluster_range.is_empty())
                .map_or(text.text.len(), |c| c.source.end),
        };
        result.lines.push(LayoutLine {
            source,
            shaping_clusters: cluster_range.clone(),
            glyphs: glyph_start..result.glyphs.len(),
            baseline,
            advance,
            bounds: Bounds {
                min: [x_offset, top],
                max: [x_offset + advance, top + text.line_height.get()],
            },
            hard_break: !cluster_range.is_empty() && raw[cluster_range.end - 1].separator,
            overflow: advance > text.wrap_width.get(),
        });
    }
    for (index, cluster) in result.shaping_clusters.iter().enumerate() {
        if cluster.unsafe_to_break_before
            && !cluster.mandatory_break
            && let Some(previous) = result.animation_units.last_mut()
        {
            previous.source.end = cluster.source.end;
            previous.graphemes.end = cluster.graphemes.end;
            previous.shaping_clusters.end = index + 1;
            previous.glyphs.end = cluster.glyphs.end;
        } else {
            result.animation_units.push(AnimationUnit {
                source: cluster.source,
                graphemes: cluster.graphemes.clone(),
                shaping_clusters: index..index + 1,
                glyphs: cluster.glyphs.clone(),
            });
        }
    }
    result.layout_bounds.max[1] = result.lines.len() as f64 * text.line_height.get();
    if !result.layout_bounds.max.iter().all(|v| v.is_finite())
        || result.lines.iter().any(|l| {
            !l.baseline.is_finite()
                || !l
                    .bounds
                    .min
                    .into_iter()
                    .chain(l.bounds.max)
                    .all(f64::is_finite)
        })
    {
        return Err(LayoutError::NonFiniteGeometry);
    }
    Ok(result)
}
fn union(previous: Option<Bounds>, next: Bounds) -> Bounds {
    previous.map_or(next, |p| Bounds {
        min: [p.min[0].min(next.min[0]), p.min[1].min(next.min[1])],
        max: [p.max[0].max(next.max[0]), p.max[1].max(next.max[1])],
    })
}
struct Outline {
    segments: Vec<PathSegment>,
    scale: f64,
    origin: [f64; 2],
    invalid: bool,
}
impl Outline {
    fn point(&mut self, x: f32, y: f32) -> [FiniteF64; 2] {
        let p = [
            self.origin[0] + f64::from(x) * self.scale,
            self.origin[1] - f64::from(y) * self.scale,
        ];
        if p.iter().any(|v| !v.is_finite()) {
            self.invalid = true;
        }
        p.map(|v| FiniteF64::new(v).unwrap_or_else(|_| FiniteF64::new(0.0).expect("finite zero")))
    }
}
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.segments.push(PathSegment::MoveTo(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.segments.push(PathSegment::LineTo(p));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let control = self.point(x1, y1);
        let end = self.point(x, y);
        self.segments.push(PathSegment::QuadTo { control, end });
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let control1 = self.point(x1, y1);
        let control2 = self.point(x2, y2);
        let end = self.point(x, y);
        self.segments.push(PathSegment::CubicTo {
            control1,
            control2,
            end,
        });
    }
    fn close(&mut self) {
        self.segments.push(PathSegment::Close);
    }
}
fn glyph_outline(
    face: &Face<'_>,
    glyph_id: u16,
    scale: f64,
    position: [f64; 2],
) -> Result<(Path, Option<Bounds>), LayoutError> {
    if !position.iter().all(|v| v.is_finite()) || !scale.is_finite() {
        return Err(LayoutError::NonFiniteGeometry);
    }
    if face.is_color_glyph(GlyphId(glyph_id))
        || face
            .glyph_raster_image(GlyphId(glyph_id), u16::MAX)
            .is_some()
        || face.glyph_svg_image(GlyphId(glyph_id)).is_some()
    {
        return Err(LayoutError::UnsupportedGlyphOutline { glyph_id });
    }
    let mut builder = Outline {
        segments: vec![],
        scale,
        origin: position,
        invalid: false,
    };
    let rect = face.outline_glyph(GlyphId(glyph_id), &mut builder);
    if builder.invalid {
        return Err(LayoutError::NonFiniteGeometry);
    }
    if rect.is_none() && !builder.segments.is_empty() {
        return Err(LayoutError::UnsupportedGlyphOutline { glyph_id });
    }
    let bounds = rect.map(|r| Bounds {
        min: [
            position[0] + f64::from(r.x_min) * scale,
            position[1] - f64::from(r.y_max) * scale,
        ],
        max: [
            position[0] + f64::from(r.x_max) * scale,
            position[1] - f64::from(r.y_min) * scale,
        ],
    });
    if bounds.is_some_and(|b| !b.min.into_iter().chain(b.max).all(f64::is_finite)) {
        return Err(LayoutError::NonFiniteGeometry);
    }
    Ok((
        Path {
            segments: builder.segments,
        },
        bounds,
    ))
}
