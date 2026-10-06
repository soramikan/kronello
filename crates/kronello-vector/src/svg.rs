//! Bounded SVG interchange. Only explicit local path geometry and solid paint
//! are imported; references, XML entities and executable content never resolve.
use kronello_model::{Color, FillRule, FiniteF64, Path, PathSegment};
use kurbo::{BezPath, PathEl};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const SVG_MAX_BYTES: usize = 1_048_576;
pub const SVG_MAX_PATHS: usize = 4096;
pub const SVG_MAX_SEGMENTS: usize = 65_536;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgPath {
    pub path: Path,
    pub fill: Option<Color>,
    pub fill_rule: FillRule,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgReport {
    pub paths: Vec<SvgPath>,
    pub unsupported: Vec<String>,
    pub external_references: Vec<String>,
}
impl SvgReport {
    pub fn ensure_supported(&self) -> Result<(), SvgError> {
        if !self.unsupported.is_empty() || !self.external_references.is_empty() {
            Err(SvgError::Unsupported)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Error)]
pub enum SvgError {
    #[error("SVG input or geometry exceeds the bounded interchange limit")]
    BudgetExceeded,
    #[error("invalid SVG XML, path geometry or solid paint")]
    InvalidDocument,
    #[error("SVG has unsupported features or external references; inspect the support report")]
    Unsupported,
}

/// A deliberately strict XML subset: no entities, DTD, processing instructions,
/// namespace aliases, CSS, groups or implicit style inheritance.
pub fn inspect_svg(input: &str) -> Result<SvgReport, SvgError> {
    if input.len() > SVG_MAX_BYTES {
        return Err(SvgError::BudgetExceeded);
    }
    let mut report = SvgReport {
        paths: vec![],
        unsupported: vec![],
        external_references: vec![],
    };
    let mut cursor = 0;
    let mut stack: Vec<String> = vec![];
    let mut root = false;
    let mut segments = 0;
    while cursor < input.len() {
        let tail = &input[cursor..];
        if !tail.starts_with('<') {
            let end = tail.find('<').unwrap_or(tail.len());
            if !tail[..end].trim().is_empty() {
                report.unsupported.push("text content".into());
            }
            cursor += end;
            continue;
        }
        if tail.starts_with("<!--") {
            let end = tail.find("-->").ok_or(SvgError::InvalidDocument)?;
            cursor += end + 3;
            continue;
        }
        if tail.starts_with("<!") || tail.starts_with("<?") {
            report
                .unsupported
                .push("XML declaration, DTD or processing instruction".into());
            return Ok(report);
        }
        let mut quote = None;
        let mut end = None;
        for (i, ch) in tail.char_indices().skip(1) {
            match (quote, ch) {
                (Some(q), c) if q == c => quote = None,
                (None, '\'' | '"') => quote = Some(ch),
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => (),
            }
        }
        let end = end.ok_or(SvgError::InvalidDocument)?;
        let body = tail[1..end].trim();
        cursor += end + 1;
        if let Some(name) = body.strip_prefix('/') {
            if stack.pop().as_deref() != Some(name.trim()) {
                return Err(SvgError::InvalidDocument);
            }
            continue;
        }
        let self_close = body.ends_with('/');
        let body = body.trim_end_matches('/').trim();
        let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let name = &body[..name_end];
        let attrs = attributes(&body[name_end..])?;
        if !root {
            if name != "svg" {
                return Err(SvgError::InvalidDocument);
            }
            root = true;
        } else if stack.is_empty() {
            return Err(SvgError::InvalidDocument);
        }
        for (key, value) in &attrs {
            if key == "href" || key.ends_with(":href") || value.contains("url(") {
                report.external_references.push(value.clone());
            }
            if key.starts_with("on") {
                report.unsupported.push(format!("event attribute {key}"));
            }
            if value.contains('&') {
                report.unsupported.push("XML entities".into());
            }
        }
        if name == "svg" && stack.is_empty() {
            for (key, value) in &attrs {
                match key.as_str() {
                    "xmlns" if value == "http://www.w3.org/2000/svg" => (),
                    "width" | "height" => {
                        if value
                            .parse::<f64>()
                            .ok()
                            .is_none_or(|n| !n.is_finite() || n <= 0.0)
                        {
                            report.unsupported.push(format!("root unit {key}"));
                        }
                    }
                    _ => report.unsupported.push(format!("svg attribute {key}")),
                }
            }
        } else if name == "path" && stack.as_slice() == ["svg"] {
            for key in attrs.keys() {
                if !matches!(key.as_str(), "d" | "fill" | "fill-rule" | "id") {
                    report.unsupported.push(format!("path attribute {key}"));
                }
            }
            let d = attrs.get("d").ok_or(SvgError::InvalidDocument)?;
            // Entities cannot be silently interpreted as coordinates.
            if !d.contains('&') {
                let path = parse_path(d)?;
                segments += path.segments.len();
                if segments > SVG_MAX_SEGMENTS || report.paths.len() >= SVG_MAX_PATHS {
                    return Err(SvgError::BudgetExceeded);
                }
                let fill = match attrs.get("fill").map(String::as_str).unwrap_or("black") {
                    "none" => None,
                    "black" => Some(Color::from_srgb8([0; 3], None)),
                    value => match parse_color(value) {
                        Ok(color) => Some(color),
                        Err(_) => {
                            report.unsupported.push(format!("fill paint {value}"));
                            None
                        }
                    },
                };
                let fill_rule = match attrs
                    .get("fill-rule")
                    .map(String::as_str)
                    .unwrap_or("nonzero")
                {
                    "nonzero" => FillRule::Nonzero,
                    "evenodd" => FillRule::Evenodd,
                    _ => return Err(SvgError::InvalidDocument),
                };
                report.paths.push(SvgPath {
                    path,
                    fill,
                    fill_rule,
                });
            }
        } else {
            report.unsupported.push(format!("element {name}"));
        }
        if !self_close {
            stack.push(name.to_owned());
            if stack.len() > 64 {
                return Err(SvgError::BudgetExceeded);
            }
        }
    }
    if !root || !stack.is_empty() {
        return Err(SvgError::InvalidDocument);
    }
    report.unsupported.sort();
    report.unsupported.dedup();
    report.external_references.sort();
    report.external_references.dedup();
    Ok(report)
}
fn attributes(mut input: &str) -> Result<BTreeMap<String, String>, SvgError> {
    let mut attrs = BTreeMap::new();
    while !input.trim_start().is_empty() {
        input = input.trim_start();
        let end = input
            .find(|c: char| c == '=' || c.is_whitespace())
            .ok_or(SvgError::InvalidDocument)?;
        let key = &input[..end];
        if key.is_empty() {
            return Err(SvgError::InvalidDocument);
        }
        input = input[end..]
            .trim_start()
            .strip_prefix('=')
            .ok_or(SvgError::InvalidDocument)?
            .trim_start();
        let quote = input
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'))
            .ok_or(SvgError::InvalidDocument)?;
        input = &input[1..];
        let end = input.find(quote).ok_or(SvgError::InvalidDocument)?;
        if input[..end].contains('<') {
            return Err(SvgError::InvalidDocument);
        }
        if attrs
            .insert(key.to_owned(), input[..end].to_owned())
            .is_some()
        {
            return Err(SvgError::InvalidDocument);
        }
        input = &input[end + 1..];
        if attrs.len() > 256 {
            return Err(SvgError::BudgetExceeded);
        }
    }
    Ok(attrs)
}
fn parse_color(value: &str) -> Result<Color, SvgError> {
    let hex = value.strip_prefix('#').ok_or(SvgError::InvalidDocument)?;
    let hex = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if hex.len() != 6 || !hex.is_ascii() {
        return Err(SvgError::InvalidDocument);
    }
    let byte = |i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| SvgError::InvalidDocument);
    Ok(Color::from_srgb8([byte(0)?, byte(2)?, byte(4)?], None))
}
fn parse_path(d: &str) -> Result<Path, SvgError> {
    // Bound command count before the SVG parser allocates; arcs expand to cubics.
    if d.bytes().filter(u8::is_ascii_alphabetic).count() > SVG_MAX_SEGMENTS / 4
        || d.len() > SVG_MAX_BYTES / 2
    {
        return Err(SvgError::BudgetExceeded);
    }
    let bez = BezPath::from_svg(d).map_err(|_| SvgError::InvalidDocument)?;
    if bez.elements().len() > SVG_MAX_SEGMENTS {
        return Err(SvgError::BudgetExceeded);
    }
    let p = |p: kurbo::Point| -> Result<[FiniteF64; 2], SvgError> {
        Ok([
            FiniteF64::new(p.x).map_err(|_| SvgError::InvalidDocument)?,
            FiniteF64::new(p.y).map_err(|_| SvgError::InvalidDocument)?,
        ])
    };
    let mut segments = Vec::new();
    for el in bez.elements() {
        segments.push(match *el {
            PathEl::MoveTo(v) => PathSegment::MoveTo(p(v)?),
            PathEl::LineTo(v) => PathSegment::LineTo(p(v)?),
            PathEl::QuadTo(a, b) => PathSegment::QuadTo {
                control: p(a)?,
                end: p(b)?,
            },
            PathEl::CurveTo(a, b, c) => PathSegment::CubicTo {
                control1: p(a)?,
                control2: p(b)?,
                end: p(c)?,
            },
            PathEl::ClosePath => PathSegment::Close,
        });
    }
    let path = Path { segments };
    kronello_model::validate_path(&path).map_err(|_| SvgError::InvalidDocument)?;
    Ok(path)
}
/// Export path geometry without flattening controls or silently converting paint.
pub fn export_svg(paths: &[SvgPath]) -> Result<String, SvgError> {
    if paths.len() > SVG_MAX_PATHS
        || paths.iter().map(|p| p.path.segments.len()).sum::<usize>() > SVG_MAX_SEGMENTS
    {
        return Err(SvgError::BudgetExceeded);
    }
    let mut output = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for path in paths {
        kronello_model::validate_path(&path.path).map_err(|_| SvgError::InvalidDocument)?;
        let mut d = String::new();
        for el in &path.path.segments {
            let point = |p: [FiniteF64; 2]| format!("{} {}", p[0].get(), p[1].get());
            d.push_str(&match *el {
                PathSegment::MoveTo(p) => format!("M{} ", point(p)),
                PathSegment::LineTo(p) => format!("L{} ", point(p)),
                PathSegment::QuadTo { control, end } => {
                    format!("Q{} {} ", point(control), point(end))
                }
                PathSegment::CubicTo {
                    control1,
                    control2,
                    end,
                } => format!("C{} {} {} ", point(control1), point(control2), point(end)),
                PathSegment::Close => "Z ".into(),
            });
        }
        let fill = match path.fill {
            None => "none".into(),
            Some(color) => {
                // The SVG subset exports tagged sRGB8 only: HDR/linear paint must be explicit.
                if color.space() != kronello_model::ColorSpace::Srgb {
                    return Err(SvgError::Unsupported);
                }
                let components = color.components();
                if components.alpha.get() != 1.0 {
                    return Err(SvgError::Unsupported);
                }
                let rgb = [components.r.get(), components.g.get(), components.b.get()];
                if rgb
                    .iter()
                    .any(|v| ((*v * 255.0).round() / 255.0 - *v).abs() > 1e-12)
                {
                    return Err(SvgError::Unsupported);
                }
                format!(
                    "#{:02x}{:02x}{:02x}",
                    (rgb[0] * 255.0).round() as u8,
                    (rgb[1] * 255.0).round() as u8,
                    (rgb[2] * 255.0).round() as u8
                )
            }
        };
        let rule = match path.fill_rule {
            FillRule::Nonzero => "nonzero",
            FillRule::Evenodd => "evenodd",
        };
        output.push_str(&format!(
            "<path d=\"{d}\" fill=\"{fill}\" fill-rule=\"{rule}\"/>"
        ));
        if output.len() > SVG_MAX_BYTES {
            return Err(SvgError::BudgetExceeded);
        }
    }
    output.push_str("</svg>");
    Ok(output)
}
