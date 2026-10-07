//! Deterministic horizontal layout from evaluated text and explicitly supplied,
//! locked font bytes. No filesystem, font discovery, fallback, or persistent cache.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use kronello_model::{
    AnimatorMode, Color, FiniteF64, FontRef, Path, PathSegment, ResolvedCharacterAnimation,
    ResolvedText, TEXT_ADVANCED_LAYOUT_VERSION, TEXT_LAYOUT_VERSION, TextAlignment, TextDirection,
    TextError, TextRange,
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
    /// Complete glyph membership including appended ruby glyphs.
    pub associated_glyphs: Vec<usize>,
    pub source: TextRange,
    pub graphemes: Range<usize>,
    pub shaping_clusters: Range<usize>,
    pub glyphs: Range<usize>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub gradient: Option<Box<kronello_model::ResolvedGradient>>,
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
    /// Source ranges whose glyphs fell outside a text path's measured extent.
    /// Dropping is a layout outcome, never an error; inspect reports these.
    pub dropped_on_path: Vec<TextRange>,
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
    if text.direction == TextDirection::VerticalRl {
        buffer.set_direction(Direction::TopToBottom);
    }
    if !matches!(
        buffer.direction(),
        Direction::LeftToRight | Direction::TopToBottom
    ) {
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
        let advance = if text.direction == TextDirection::VerticalRl {
            -f64::from(position.y_advance) * scale
        } else {
            f64::from(position.x_advance) * scale
        };
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
    if !matches!(text.layout_version, 1 | 2) {
        return Err(LayoutError::UnsupportedVersion {
            version: text.layout_version,
        });
    }
    if text.layout_version == 1 && text.direction != TextDirection::Horizontal {
        return Err(LayoutError::UnsupportedFeature {
            feature: "vertical text",
        });
    }
    if text.layout_version == 1 && !text.ruby.is_empty() {
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
    if text.layout_version == 1 && !text.character_animations.is_empty() {
        return Err(LayoutError::UnsupportedFeature {
            feature: "character animation requires layout version 2",
        });
    }
    if text.path.is_some() {
        if text.layout_version != TEXT_ADVANCED_LAYOUT_VERSION {
            return Err(LayoutError::UnsupportedFeature {
                feature: "text path requires layout version 2",
            });
        }
        if text.direction == TextDirection::VerticalRl {
            return Err(LayoutError::UnsupportedFeature {
                feature: "text path with vertical direction",
            });
        }
        if !text.ruby.is_empty() {
            return Err(LayoutError::UnsupportedFeature {
                feature: "text path with ruby",
            });
        }
    }
    let mut raw = shape_text(text, &faces)?;
    for ruby in &text.ruby {
        if text.text[ruby.base.start..ruby.base.end]
            .chars()
            .any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        {
            return Err(LayoutError::UnsupportedFeature {
                feature: "ruby across mandatory break",
            });
        }
        for c in &mut raw {
            if c.source.start > ruby.base.start && c.source.start < ruby.base.end {
                c.unsafe_before = true;
            }
        }
    }
    let mut result = LayoutResult {
        layout_version: text.layout_version,
        lines: vec![],
        glyphs: vec![],
        shaping_clusters: vec![],
        animation_units: vec![],
        dropped_on_path: vec![],
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
    if let Some(path) = &text.path {
        layout_path_line(
            text,
            &faces,
            &raw,
            path,
            ascent,
            &mut result,
            &mut outline_commands,
        )?;
    } else {
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
                    let position = if text.direction == TextDirection::VerticalRl {
                        [-top + glyph.offset[0], x + glyph.offset[1]]
                    } else {
                        [x + glyph.offset[0], baseline + glyph.offset[1]]
                    };
                    let scale =
                        text.styles[c.style].size.get() / f64::from(faces[c.style].units_per_em());
                    let (mut outline, bounds) =
                        glyph_outline(&faces[c.style], glyph.id, scale, position)?;
                    if text.direction == TextDirection::VerticalRl
                        && text.text[c.source.start..c.source.end]
                            .chars()
                            .any(|c| c.script() == Script::Latin)
                    {
                        map_path(&mut outline, |p| {
                            [
                                position[0] - (p[1] - position[1]),
                                position[1] + (p[0] - position[0]),
                            ]
                        })?;
                    }
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
                        gradient: text.styles[c.style].gradient.clone(),
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
                bounds: if text.direction == TextDirection::VerticalRl {
                    Bounds {
                        min: [-top - text.line_height.get(), x_offset],
                        max: [-top, x_offset + advance],
                    }
                } else {
                    Bounds {
                        min: [x_offset, top],
                        max: [x_offset + advance, top + text.line_height.get()],
                    }
                },
                hard_break: !cluster_range.is_empty() && raw[cluster_range.end - 1].separator,
                overflow: advance > text.wrap_width.get(),
            });
        }
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
            previous.associated_glyphs.extend(cluster.glyphs.clone());
        } else {
            result.animation_units.push(AnimationUnit {
                associated_glyphs: cluster.glyphs.clone().collect(),
                source: cluster.source,
                graphemes: cluster.graphemes.clone(),
                shaping_clusters: index..index + 1,
                glyphs: cluster.glyphs.clone(),
            });
        }
    }
    if text.path.is_none() {
        result.layout_bounds.max[1] = result.lines.len() as f64 * text.line_height.get();
        if text.direction == TextDirection::VerticalRl {
            result.layout_bounds = Bounds {
                min: [-(result.lines.len() as f64) * text.line_height.get(), 0.0],
                max: [0.0, text.wrap_width.get()],
            };
        }
    }
    if text.layout_version == 2 {
        append_ruby(text, fonts, &mut result)?;
    }
    apply_character_animations(text, &mut result)?;

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

/// Flattening tolerance for text guide paths in design_px. It bounds the
/// analytic error of arc-length sampling, not output pixels.
const PATH_LAYOUT_TOLERANCE: f64 = 0.02;

/// Arc-length table over a guide path's flattened subpaths, in document order.
/// A closed subpath's polyline returns to its start point, so its closing edge
/// carries real arc length and a ring can hold a full lap of text.
struct PathMeasure {
    spans: Vec<PathSpan>,
    length: f64,
}
struct PathSpan {
    /// Arc-length offset of this subpath inside the whole measure.
    offset: f64,
    /// Arc length at each vertex; `cumulative[0]` is 0 and the last element
    /// is the subpath's total length.
    cumulative: Vec<f64>,
    points: Vec<[f64; 2]>,
}
impl PathSpan {
    fn length(&self) -> f64 {
        self.cumulative.last().copied().unwrap_or(0.0)
    }
}
impl PathMeasure {
    fn build(path: &Path) -> Result<Self, LayoutError> {
        let flattened = kronello_vector::flatten_path(path, PATH_LAYOUT_TOLERANCE).map_err(
            |error| match error {
                kronello_vector::VectorError::GeometryBudgetExceeded => LayoutError::BudgetExceeded,
                kronello_vector::VectorError::NonFiniteGeometry => LayoutError::NonFiniteGeometry,
                _ => LayoutError::UnsupportedFeature {
                    feature: "text path geometry",
                },
            },
        )?;
        let mut spans = Vec::with_capacity(flattened.subpaths.len());
        let mut offset = 0.0;
        for polyline in flattened.subpaths {
            let mut cumulative = Vec::with_capacity(polyline.points.len());
            cumulative.push(0.0);
            for pair in polyline.points.windows(2) {
                let length = *cumulative.last().expect("seeded")
                    + (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                cumulative.push(length);
            }
            let length = *cumulative.last().expect("seeded");
            if !length.is_finite() {
                return Err(LayoutError::NonFiniteGeometry);
            }
            spans.push(PathSpan {
                offset,
                cumulative,
                points: polyline.points,
            });
            offset += length;
        }
        if !offset.is_finite() {
            return Err(LayoutError::NonFiniteGeometry);
        }
        Ok(Self {
            spans,
            length: offset,
        })
    }
    /// Path bounds across all subpaths; `None` when the path has no vertices.
    fn bounds(&self) -> Option<Bounds> {
        let mut bounds = None;
        for span in &self.spans {
            for point in &span.points {
                bounds = Some(union(
                    bounds,
                    Bounds {
                        min: *point,
                        max: *point,
                    },
                ));
            }
        }
        bounds
    }
    /// Point and unit tangent at arc length `s`, clamped into the measured
    /// extent. A zero-length subpath only answers at its own offset; spans are
    /// scanned in document order so a shared boundary prefers the earlier one.
    fn sample(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let Some(span) = self
            .spans
            .iter()
            .find(|span| s >= span.offset && s <= span.offset + span.length())
            .or_else(|| self.spans.first())
        else {
            return ([0.0; 2], [1.0, 0.0]);
        };
        if span.points.len() < 2 {
            return (span.points.first().copied().unwrap_or([0.0; 2]), [1.0, 0.0]);
        }
        let local = (s - span.offset).clamp(0.0, span.length());
        let index = span
            .cumulative
            .partition_point(|c| *c <= local)
            .saturating_sub(1)
            .min(span.cumulative.len() - 2);
        let a = span.points[index];
        let b = span.points[index + 1];
        let segment = span.cumulative[index + 1] - span.cumulative[index];
        let t = if segment > 0.0 {
            (local - span.cumulative[index]) / segment
        } else {
            0.0
        };
        let delta = [b[0] - a[0], b[1] - a[1]];
        let norm = delta[0].hypot(delta[1]);
        let tangent = if norm > 0.0 {
            [delta[0] / norm, delta[1] / norm]
        } else {
            [1.0, 0.0]
        };
        ([a[0] + delta[0] * t, a[1] + delta[1] * t], tangent)
    }
}

/// Lay the whole text run along the guide path's arc length in reading order.
/// `alignment` anchors the text advance at the path start/center/end; a glyph
/// whose baseline anchor lands outside `[0, length]` is dropped and its
/// cluster's source range is recorded for inspection, never treated as an
/// error. Explicit separators contribute no advance and keep no glyphs.
fn layout_path_line(
    text: &ResolvedText,
    faces: &[Face<'_>],
    raw: &[RawCluster],
    path: &Path,
    ascent: f64,
    result: &mut LayoutResult,
    outline_commands: &mut usize,
) -> Result<(), LayoutError> {
    let measure = PathMeasure::build(path)?;
    let advance: f64 = raw.iter().map(|c| c.advance).sum();
    let shift = match text.alignment {
        TextAlignment::Start => 0.0,
        TextAlignment::Center => (measure.length - advance) / 2.0,
        TextAlignment::End => measure.length - advance,
    };
    if !shift.is_finite() {
        return Err(LayoutError::NonFiniteGeometry);
    }
    let glyph_start = result.glyphs.len();
    let mut ink: Option<Bounds> = None;
    let mut x = 0.0;
    for (cluster_index, c) in raw.iter().enumerate() {
        let grapheme_start = result
            .graphemes
            .partition_point(|g| g.source.end <= c.source.start);
        let grapheme_end = result
            .graphemes
            .partition_point(|g| g.source.start < c.source.end);
        let graphemes = grapheme_start..grapheme_end;
        let start = result.glyphs.len();
        for glyph in &c.glyphs {
            // A glyph anchors at the midpoint of its advance on the baseline,
            // so mark widths straddle their own path station.
            let flat = [x + glyph.offset[0], ascent + glyph.offset[1]];
            x += glyph.advance;
            let station = flat[0] + glyph.advance / 2.0 + shift;
            if !(0.0..=measure.length).contains(&station) {
                if result.dropped_on_path.last() != Some(&c.source) {
                    result.dropped_on_path.push(c.source);
                }
                continue;
            }
            let (m, tangent) = measure.sample(station);
            let (sin, cos) = tangent[1].atan2(tangent[0]).sin_cos();
            let origin = [flat[0] + glyph.advance / 2.0, flat[1]];
            let transform = |p: [f64; 2]| {
                let d = [p[0] - origin[0], p[1] - origin[1]];
                [
                    m[0] + d[0] * cos - d[1] * sin,
                    m[1] + d[0] * sin + d[1] * cos,
                ]
            };
            let scale = text.styles[c.style].size.get() / f64::from(faces[c.style].units_per_em());
            let (mut outline, bounds) = glyph_outline(&faces[c.style], glyph.id, scale, flat)?;
            map_path(&mut outline, transform)?;
            *outline_commands += outline.segments.len();
            if *outline_commands > MAX_OUTLINE_SEGMENTS {
                return Err(LayoutError::BudgetExceeded);
            }
            if let Some(bounds) = bounds {
                ink = Some(union(ink, transform_bounds(bounds, transform)));
            }
            result.glyphs.push(PositionedGlyph {
                glyph_id: glyph.id,
                style_index: c.style,
                source: c.source,
                graphemes: graphemes.clone(),
                shaping_cluster: cluster_index,
                position: transform(flat),
                advance: glyph.advance,
                fill: text.styles[c.style].fill,
                gradient: text.styles[c.style].gradient.clone(),
                outline,
            });
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
    let bounds = match (measure.bounds(), ink) {
        (Some(path_bounds), Some(ink)) => union(Some(path_bounds), ink),
        (Some(path_bounds), None) => path_bounds,
        (None, Some(ink)) => ink,
        (None, None) => Bounds {
            min: [0.0; 2],
            max: [0.0; 2],
        },
    };
    result.layout_bounds = bounds;
    result.ink_bounds = ink;
    result.lines.push(LayoutLine {
        source: TextRange {
            start: raw.first().map_or(0, |c| c.source.start),
            end: raw.last().map_or(0, |c| c.source.end),
        },
        shaping_clusters: 0..raw.len(),
        glyphs: glyph_start..result.glyphs.len(),
        baseline: ascent,
        advance,
        bounds,
        hard_break: raw.last().is_some_and(|c| c.separator),
        overflow: false,
    });
    Ok(())
}

/// Axis-aligned bounds under an arbitrary rigid transform, evaluated at the
/// rect's four corners. Conservative for the affine maps path layout applies.
fn transform_bounds(bounds: Bounds, transform: impl Fn([f64; 2]) -> [f64; 2]) -> Bounds {
    let mut out = Bounds {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    for corner in [
        bounds.min,
        [bounds.max[0], bounds.min[1]],
        [bounds.min[0], bounds.max[1]],
        bounds.max,
    ] {
        let p = transform(corner);
        out.min = [out.min[0].min(p[0]), out.min[1].min(p[1])];
        out.max = [out.max[0].max(p[0]), out.max[1].max(p[1])];
    }
    out
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

fn map_path(path: &mut Path, transform: impl Fn([f64; 2]) -> [f64; 2]) -> Result<(), LayoutError> {
    let point = |p: &mut [FiniteF64; 2]| -> Result<(), LayoutError> {
        let v = transform(p.map(FiniteF64::get));
        *p = [
            FiniteF64::new(v[0]).map_err(|_| LayoutError::NonFiniteGeometry)?,
            FiniteF64::new(v[1]).map_err(|_| LayoutError::NonFiniteGeometry)?,
        ];
        Ok(())
    };
    for segment in &mut path.segments {
        match segment {
            PathSegment::MoveTo(p) | PathSegment::LineTo(p) => point(p)?,
            PathSegment::QuadTo { control, end } => {
                point(control)?;
                point(end)?;
            }
            PathSegment::CubicTo {
                control1,
                control2,
                end,
            } => {
                point(control1)?;
                point(control2)?;
                point(end)?;
            }
            PathSegment::Close => {}
        }
    }
    Ok(())
}
fn append_ruby(
    text: &ResolvedText,
    fonts: &[FontData<'_>],
    result: &mut LayoutResult,
) -> Result<(), LayoutError> {
    for association in &text.ruby {
        let parents: Vec<_> = result
            .glyphs
            .iter()
            .filter(|g| {
                g.source.start < association.base.end && g.source.end > association.base.start
            })
            .cloned()
            .collect();
        let Some(first) = parents.first() else {
            return Err(LayoutError::UnsupportedFeature {
                feature: "ruby without parent glyph",
            });
        };
        let style = &text.styles[first.style_index];
        if parents.iter().any(|g| g.style_index != first.style_index) {
            return Err(LayoutError::UnsupportedFeature {
                feature: "ruby over mixed styles",
            });
        }
        let mut ruby = text.clone();
        ruby.text = association.text.clone();
        ruby.ruby.clear();
        ruby.character_animations.clear();
        ruby.wrap_width = FiniteF64::new(1_000_000.0).expect("finite ruby width");
        ruby.alignment = TextAlignment::Start;
        ruby.styles = vec![kronello_model::ResolvedTextStyle {
            range: TextRange {
                start: 0,
                end: ruby.text.len(),
            },
            size: FiniteF64::new(style.size.get() * 0.5)
                .map_err(|_| LayoutError::NonFiniteGeometry)?,
            ..style.clone()
        }];
        let laid = layout(&ruby, fonts)?;
        if laid.lines.len() != 1 {
            return Err(LayoutError::UnsupportedFeature {
                feature: "ruby mandatory break",
            });
        }
        let advance: f64 = parents.iter().map(|g| g.advance).sum();
        let shift = if text.direction == TextDirection::VerticalRl {
            let parent_ink = glyph_ink_bounds(&parents);
            let ruby_ink = glyph_ink_bounds(&laid.glyphs);
            match (parent_ink, ruby_ink) {
                (Some(parent), Some(annotation)) => [
                    parent.max[0] + style.size.get() * 0.1 - annotation.min[0],
                    (parent.min[1] + parent.max[1] - annotation.min[1] - annotation.max[1]) * 0.5,
                ],
                _ => [
                    first.position[0] + style.size.get(),
                    first.position[1] + (advance - laid.lines[0].advance) * 0.5,
                ],
            }
        } else {
            [
                first.position[0] + (advance - laid.lines[0].advance) * 0.5,
                first.position[1] - style.size.get() * 0.9 - laid.lines[0].baseline,
            ]
        };
        for mut glyph in laid.glyphs {
            map_path(&mut glyph.outline, |p| [p[0] + shift[0], p[1] + shift[1]])?;
            glyph.position = [glyph.position[0] + shift[0], glyph.position[1] + shift[1]];
            glyph.source = association.base;
            glyph.graphemes = result
                .graphemes
                .partition_point(|g| g.source.end <= association.base.start)
                ..result
                    .graphemes
                    .partition_point(|g| g.source.start < association.base.end);
            glyph.shaping_cluster = first.shaping_cluster;
            glyph.style_index = first.style_index;
            let index = result.glyphs.len();
            for unit in &mut result.animation_units {
                if unit.source.start < association.base.end
                    && unit.source.end > association.base.start
                {
                    unit.associated_glyphs.push(index);
                }
            }
            result.glyphs.push(glyph);
            if result.glyphs.len() > MAX_GLYPHS {
                return Err(LayoutError::BudgetExceeded);
            }
        }
    }
    if result
        .glyphs
        .iter()
        .map(|g| g.outline.segments.len())
        .sum::<usize>()
        > MAX_OUTLINE_SEGMENTS
    {
        return Err(LayoutError::BudgetExceeded);
    }
    recompute_ink(result);
    Ok(())
}
/// Applies source selections to complete shaping units after layout. Ruby inherits
/// its parent's unit. The caller may reuse an unanimated cached layout.
/// All application is a pure function of the evaluated input: ordinals are
/// reading-order unit positions and `random` hashes the unit's source range.
pub fn apply_character_animations(
    text: &ResolvedText,
    result: &mut LayoutResult,
) -> Result<(), LayoutError> {
    let units = &result.animation_units;
    let glyphs = &mut result.glyphs;
    for animation in &text.character_animations {
        let selected: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| {
                unit.source.start < animation.source.end && unit.source.end > animation.source.start
            })
            .map(|(index, _)| index)
            .collect();
        for (ordinal, index) in selected.iter().enumerate() {
            let factor = animator_factor(animation, ordinal, selected.len(), units[*index].source);
            for &glyph_index in &units[*index].associated_glyphs {
                if let Some(glyph) = glyphs.get_mut(glyph_index) {
                    apply_glyph_animation(glyph, animation, factor)?;
                }
            }
        }
    }
    if !text.character_animations.is_empty() {
        recompute_ink(result);
    }
    Ok(())
}

/// Per-unit application factor in [0, 1] over the selected units in reading
/// order. `random` runs a splitmix64 finalizer over (seed, unit source range),
/// so identical inputs produce identical factors on every evaluation.
fn animator_factor(
    animation: &ResolvedCharacterAnimation,
    ordinal: usize,
    count: usize,
    unit_source: TextRange,
) -> f64 {
    match animation.mode {
        AnimatorMode::Step => 1.0,
        AnimatorMode::Ramp => {
            if count <= 1 {
                1.0
            } else {
                ordinal as f64 / (count - 1) as f64
            }
        }
        AnimatorMode::Follow => {
            if count <= 1 {
                1.0
            } else {
                let smoothing = animation
                    .follow_smoothing
                    .map(FiniteF64::get)
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                1.0 - smoothing * (ordinal as f64 / (count - 1) as f64)
            }
        }
        AnimatorMode::Random => {
            let mut z = (u64::from(animation.seed.unwrap_or(0)))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (unit_source.start as u64) << 20
                ^ (unit_source.end as u64) << 40;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
        }
    }
}

/// Scale and rotation pivot on the glyph's laid-out anchor position, so the
/// anchor itself is fixed and offset still applies in text-local axes. Path
/// anchors are path stations, so path glyphs spin around their station.
fn apply_glyph_animation(
    glyph: &mut PositionedGlyph,
    animation: &ResolvedCharacterAnimation,
    factor: f64,
) -> Result<(), LayoutError> {
    let scale = animation.scale.map(|s| {
        [
            1.0 + (s[0].get() - 1.0) * factor,
            1.0 + (s[1].get() - 1.0) * factor,
        ]
    });
    let radians =
        animation.rotation.map(FiniteF64::get).unwrap_or(0.0) * factor * std::f64::consts::PI
            / 180.0;
    if scale.is_some() || radians != 0.0 {
        let [sx, sy] = scale.unwrap_or([1.0, 1.0]);
        let (sin, cos) = radians.sin_cos();
        let origin = glyph.position;
        map_path(&mut glyph.outline, |p| {
            let d = [p[0] - origin[0], p[1] - origin[1]];
            [
                origin[0] + d[0] * sx * cos - d[1] * sy * sin,
                origin[1] + d[0] * sx * sin + d[1] * sy * cos,
            ]
        })?;
        // The pivot maps to itself; the anchor position is unchanged.
    }
    let offset = animation.offset.map(FiniteF64::get);
    let delta = [offset[0] * factor, offset[1] * factor];
    if delta != [0.0, 0.0] {
        map_path(&mut glyph.outline, |p| [p[0] + delta[0], p[1] + delta[1]])?;
        glyph.position = [glyph.position[0] + delta[0], glyph.position[1] + delta[1]];
    }
    let opacity = 1.0 - (1.0 - animation.opacity.get()) * factor;
    // A factor of zero reproduces the current paint, so gradient glyphs only
    // fail when an animator would actually change a painted result.
    let repaints = opacity != 1.0 || (animation.fill.is_some() && factor != 0.0);
    if repaints && glyph.gradient.is_some() {
        return Err(LayoutError::UnsupportedFeature {
            feature: "character paint with gradient",
        });
    }
    if let Some(target) = animation.fill.filter(|_| factor != 0.0) {
        let from = glyph.fill.components();
        let to = target.components();
        glyph.fill = Color::new(
            target.space(),
            [
                from.r.get() + (to.r.get() - from.r.get()) * factor,
                from.g.get() + (to.g.get() - from.g.get()) * factor,
                from.b.get() + (to.b.get() - from.b.get()) * factor,
            ],
            from.alpha.get() + (to.alpha.get() - from.alpha.get()) * factor,
        )
        .map_err(|_| LayoutError::NonFiniteGeometry)?;
    }
    let components = glyph.fill.components();
    if opacity != 1.0 {
        glyph.fill = Color::new(
            glyph.fill.space(),
            [components.r.get(), components.g.get(), components.b.get()],
            components.alpha.get() * opacity,
        )
        .map_err(|_| LayoutError::NonFiniteGeometry)?;
    }
    Ok(())
}
fn glyph_ink_bounds(glyphs: &[PositionedGlyph]) -> Option<Bounds> {
    let mut bounds = None;
    for glyph in glyphs {
        for segment in &glyph.outline.segments {
            let points: Vec<_> = match segment {
                PathSegment::MoveTo(p) | PathSegment::LineTo(p) => vec![*p],
                PathSegment::QuadTo { control, end } => vec![*control, *end],
                PathSegment::CubicTo {
                    control1,
                    control2,
                    end,
                } => vec![*control1, *control2, *end],
                PathSegment::Close => vec![],
            };
            for p in points {
                let p = p.map(FiniteF64::get);
                bounds = Some(union(bounds, Bounds { min: p, max: p }));
            }
        }
    }
    bounds
}

fn recompute_ink(result: &mut LayoutResult) {
    result.ink_bounds = glyph_ink_bounds(&result.glyphs);
}
