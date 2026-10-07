//! Subtitle sidecar interchange (SUB-002): strict SRT, WebVTT and ITT parsing
//! and serialization against the first-class caption model (ADR-0107).
//!
//! Import builds ordinary `CaptionSet`/`ClipPlace` edit plans, so imported cues
//! pass through revision validation, transactions, idempotency and selective
//! undo like every other edit. Subtitle payload text is data: markup is decoded
//! into span attributes or rejected, never retained as executable content.
use std::collections::BTreeMap;
use std::path::PathBuf;

use kronello_model::*;
use kronello_store::Event;
use kronello_time::{Rational, Time, TimeMap, TimeRange};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{EditCommand, ServiceError};

/// Caller-allocated stable identities for one imported cue (planned/apply
/// commands never allocate IDs during mutation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionCueIds {
    pub caption: CaptionId,
    pub clip: ClipId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionsImportPlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub sequence: SequenceId,
    /// Existing caption track, or the ID of the caption track the plan creates.
    pub track: TrackId,
    pub format: CaptionFormat,
    /// UTF-8 sidecar document text.
    pub content: String,
    /// Base cue style; per-cue format styling overrides individual fields.
    pub style: CaptionStyle,
    /// Exactly one identity pair per parsed cue, in cue order.
    pub cue_ids: Vec<CaptionCueIds>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionsImportRequest {
    pub plan: CaptionsImportPlanRequest,
    pub session_id: Uuid,
    pub idempotency_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionsExportRequest {
    pub project: PathBuf,
    pub sequence: SequenceId,
    pub format: CaptionFormat,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionsExportResult {
    pub format: CaptionFormat,
    /// Deterministic serialized sidecar text (UTF-8).
    pub content: String,
}

fn invalid(message: impl Into<String>) -> ServiceError {
    ServiceError::new("INVALID_CAPTION_FORMAT", message.into())
}
fn unsupported(message: impl Into<String>) -> ServiceError {
    ServiceError::new("UNSUPPORTED_FEATURE", message.into())
}

/// One decoded cue before model objects exist.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ParsedCue {
    pub start: Time,
    pub end: Time,
    /// `\n`-separated payload text with markup removed and entities decoded.
    pub text: String,
    pub spans: Vec<CaptionSpan>,
    /// Horizontal anchor column from cue alignment hints.
    pub anchor_column: Option<CaptionAnchorColumn>,
    pub fill: Option<Color>,
    pub size: Option<f64>,
    pub font_family: Option<String>,
    pub background: Option<Color>,
}
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionAnchorColumn {
    Start,
    Center,
    End,
}
impl CaptionAnchorColumn {
    fn apply(self, anchor: CaptionAnchor) -> CaptionAnchor {
        use CaptionAnchor::*;
        match (self, anchor) {
            (Self::Start, TopCenter | TopRight) => TopLeft,
            (Self::Start, Center | CenterRight) => CenterLeft,
            (Self::Start, BottomCenter | BottomRight) => BottomLeft,
            (Self::Center, TopLeft | TopRight) => TopCenter,
            (Self::Center, CenterLeft | CenterRight) => Center,
            (Self::Center, BottomLeft | BottomRight) => BottomCenter,
            (Self::End, TopLeft | TopCenter) => TopRight,
            (Self::End, CenterLeft | Center) => CenterRight,
            (Self::End, BottomLeft | BottomCenter) => BottomRight,
            (_, same) => same,
        }
    }
}

/// Exact timestamp `HH:MM:SS` or `MM:SS` with exactly three fractional digits
/// at the format's decimal separator (SRT `,`, VTT `.`).
fn parse_timestamp_ms(input: &str, separator: char) -> Result<Time, ServiceError> {
    let (clock, millis) = input.split_once(separator).ok_or_else(|| {
        invalid(format!(
            "timestamp requires {separator} milliseconds: {input}"
        ))
    })?;
    if input.matches(separator).count() != 1
        || millis.len() != 3
        || !millis.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid(format!("malformed timestamp: {input}")));
    }
    let fields: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds) = match fields.as_slice() {
        [h, m, s] => (parse_u64(h)?, parse_u64(m)?, parse_u64(s)?),
        [m, s] => (0, parse_u64(m)?, parse_u64(s)?),
        _ => return Err(invalid(format!("malformed timestamp: {input}"))),
    };
    if minutes >= 60 || seconds >= 60 {
        return Err(invalid(format!("timestamp field out of range: {input}")));
    }
    let ms = hours
        .checked_mul(3_600_000)
        .and_then(|v| v.checked_add(minutes * 60_000 + seconds * 1000))
        .and_then(|v| v.checked_add(millis.parse().ok()?))
        .ok_or_else(|| invalid(format!("timestamp overflow: {input}")))?;
    Time::new(
        i64::try_from(ms).map_err(|_| invalid("timestamp overflow"))?,
        1000,
    )
    .map_err(|e| invalid(e.to_string()))
}
fn parse_u64(s: &str) -> Result<u64, ServiceError> {
    if s.is_empty() || s.len() > 12 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(format!("malformed numeric field: {s}")));
    }
    s.parse().map_err(|_| invalid("numeric field overflow"))
}
/// Serialize an exact-rational time, rejecting times that are not whole
/// milliseconds rather than silently rounding authored timing.
fn format_timestamp_ms(time: Time) -> Result<String, ServiceError> {
    if time < Time::ZERO {
        return Err(invalid("negative cue time"));
    }
    let ms = time
        .checked_mul(Time::from_integer(1000))
        .map_err(|e| invalid(e.to_string()))?;
    let whole = ms.floor();
    if Time::from_integer(whole) != ms {
        return Err(ServiceError::new(
            "TIMING_PRECISION",
            "cue timing must be a whole millisecond for this format",
        ));
    }
    let whole = u64::try_from(whole).map_err(|_| invalid("cue time overflow"))?;
    Ok(format!(
        "{:02}:{:02}:{:02}.{:03}",
        whole / 3_600_000,
        whole / 60_000 % 60,
        whole / 1000 % 60,
        whole % 1000
    ))
}

/// Decode the entities that subtitle formats allow; anything else is data
/// loss, so it is a typed rejection instead of a silent passthrough.
fn decode_entities(input: &str) -> Result<String, ServiceError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest
            .find(';')
            .ok_or_else(|| invalid("unterminated entity"))?;
        let entity = &rest[..end];
        rest = &rest[end + 1..];
        let decoded = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => char::from_u32(
                u32::from_str_radix(&entity[2..], 16)
                    .map_err(|_| invalid("malformed numeric entity"))?,
            )
            .ok_or_else(|| invalid("invalid numeric entity"))?,
            _ if entity.starts_with('#') => char::from_u32(
                entity[1..]
                    .parse::<u32>()
                    .map_err(|_| invalid("malformed numeric entity"))?,
            )
            .ok_or_else(|| invalid("invalid numeric entity"))?,
            _ => return Err(unsupported(format!("unsupported entity &{entity};"))),
        };
        out.push(decoded);
    }
    out.push_str(rest);
    Ok(out)
}
fn escape_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Payload-text builder for SRT/VTT markup: records `\n`-separated plain text
/// plus `CaptionSpan` ranges while validating tag balance.
#[derive(Default)]
struct MarkupBuilder {
    text: String,
    spans: Vec<CaptionSpan>,
    /// (normalized tag, byte offset where the open tag's content starts).
    open: Vec<(String, usize)>,
}
impl MarkupBuilder {
    fn text(&mut self, raw: &str) -> Result<(), ServiceError> {
        self.text.push_str(&decode_entities(raw)?);
        Ok(())
    }
    /// XML character data arrives already entity-decoded by the reader; a
    /// second decode pass would reject literal `&` as an unterminated entity.
    fn xml_text(&mut self, decoded: &str) {
        self.text.push_str(decoded);
    }
    fn newline(&mut self) {
        self.text.push('\n');
    }
    fn tag(&mut self, name: &str) -> Result<(), ServiceError> {
        // Validate against the closed tag set immediately.
        tag_kind(name)?;
        self.open.push((name.to_string(), self.text.len()));
        Ok(())
    }
    fn close(&mut self, name: &str) -> Result<(), ServiceError> {
        let name = name.trim().to_ascii_lowercase();
        let Some((open_name, start)) = self.open.pop() else {
            return Err(invalid(format!("unmatched closing tag {name}")));
        };
        let open_tag = open_name.to_ascii_lowercase();
        // `<font ...>` closes with `</font>`.
        let open_head = open_tag.split_whitespace().next().unwrap_or("");
        if open_head != name {
            return Err(invalid(format!("mismatched markup tag {open_name}/{name}")));
        }
        if self.text.len() > start {
            let tag = open_name;
            self.span(&tag, start, self.text.len())?;
        }
        Ok(())
    }
    fn span(&mut self, tag: &str, start: usize, end: usize) -> Result<(), ServiceError> {
        let Some(existing) = self
            .spans
            .iter_mut()
            .find(|s| s.range.start == start && s.range.end == end)
        else {
            let mut span = CaptionSpan {
                range: TextRange { start, end },
                bold: None,
                italic: None,
                color: None,
                font: None,
            };
            apply_tag(&mut span, tag)?;
            self.spans.push(span);
            return Ok(());
        };
        apply_tag(existing, tag)
    }
    fn finish(self) -> Result<(String, Vec<CaptionSpan>), ServiceError> {
        if let Some((name, _)) = self.open.last() {
            return Err(invalid(format!("unclosed markup tag {name}")));
        }
        let mut spans = self.spans;
        spans.retain(|s| s.bold.is_some() || s.italic.is_some() || s.color.is_some());
        spans.sort_by_key(|s| (s.range.start, s.range.end));
        Ok((self.text, spans))
    }
}
fn tag_kind(tag: &str) -> Result<(), ServiceError> {
    let lower = tag.trim().to_ascii_lowercase();
    let head = lower.split_whitespace().next().unwrap_or("");
    match head {
        "b" | "i" => Ok(()),
        "font" => Ok(()),
        _ => Err(unsupported(format!("unsupported caption markup <{tag}>"))),
    }
}
/// `b`/`i`/`font color` style tags shared by SRT and VTT cue text.
fn apply_tag(span: &mut CaptionSpan, tag: &str) -> Result<(), ServiceError> {
    let lower = tag.trim().to_ascii_lowercase();
    match lower.split_whitespace().next().unwrap_or("") {
        "b" => span.bold = Some(true),
        "i" => span.italic = Some(true),
        "font" => {
            for attr in lower.split_whitespace().skip(1) {
                if let Some(value) = attr
                    .strip_prefix("color=")
                    .map(|v| v.trim_matches('"').trim_matches('\''))
                {
                    span.color = Some(parse_color(value)?);
                }
            }
        }
        _ => return Err(unsupported(format!("unsupported caption markup <{tag}>"))),
    }
    Ok(())
}

/// `#RRGGBB` / `#RRGGBBAA`, plus the TTML/CSS1 named colors.
fn parse_color(input: &str) -> Result<Color, ServiceError> {
    let hex = |s: &str| -> Result<u8, ServiceError> {
        u8::from_str_radix(s, 16).map_err(|_| invalid(format!("invalid color {input}")))
    };
    if let Some(digits) = input.strip_prefix('#') {
        return match digits.len() {
            6 => Ok(Color::from_srgb8(
                [
                    hex(&digits[0..2])?,
                    hex(&digits[2..4])?,
                    hex(&digits[4..6])?,
                ],
                None,
            )),
            8 => Ok(Color::from_srgb8(
                [
                    hex(&digits[0..2])?,
                    hex(&digits[2..4])?,
                    hex(&digits[4..6])?,
                ],
                Some(hex(&digits[6..8])?),
            )),
            _ => Err(invalid(format!("invalid color {input}"))),
        };
    }
    let named: [u8; 3] = match input {
        "white" => [255, 255, 255],
        "black" => [0, 0, 0],
        "red" => [255, 0, 0],
        "lime" => [0, 255, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "navy" => [0, 0, 128],
        "transparent" => return Ok(Color::from_srgb8([0; 3], Some(0))),
        _ => return Err(unsupported(format!("unsupported color name {input}"))),
    };
    Ok(Color::from_srgb8(named, None))
}
/// Sidecar colors are sRGB8. Stored colors are explicitly tagged: sRGB-tagged
/// components serialize directly, linear Rec.709 passes through the sRGB
/// transfer function, and other gamuts are typed rejections rather than a
/// silent matrix.
fn format_color(color: Color) -> Result<String, ServiceError> {
    let components = color.components();
    let to_srgb = |v: f64| -> u8 {
        let v = v.clamp(0.0, 1.0);
        let srgb = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (srgb * 255.0).round() as i64 as u8
    };
    let srgb = match color.space() {
        ColorSpace::Srgb => |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as i64 as u8,
        ColorSpace::LinearRec709 => to_srgb,
        ColorSpace::LinearRec2020 => {
            return Err(unsupported(
                "caption color requires gamut conversion for sidecar output",
            ));
        }
    };
    let alpha = (components.alpha.get().clamp(0.0, 1.0) * 255.0).round() as i64 as u8;
    Ok(format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        srgb(components.r.get()),
        srgb(components.g.get()),
        srgb(components.b.get()),
        alpha
    ))
}

/// Normalize input line endings to `\n`, rejecting content that cannot be
/// represented canonically.
fn normalize_lines(content: &str) -> Result<String, ServiceError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    if content.contains('\0') {
        return Err(invalid("NUL byte in caption document"));
    }
    Ok(content.replace("\r\n", "\n").replace('\r', "\n"))
}

// ---------------------------------------------------------------- SRT ------

fn parse_srt(content: &str) -> Result<Vec<ParsedCue>, ServiceError> {
    let content = normalize_lines(content)?;
    let mut cues = vec![];
    for block in content.split("\n\n") {
        let block = block.trim_matches('\n');
        if block.is_empty() {
            continue;
        }
        let mut lines = block.split('\n');
        let mut timing = lines.next().expect("nonempty block").trim();
        // The numeric index line is optional metadata, never part of the text.
        if !timing.contains("-->") {
            if timing.bytes().all(|b| b.is_ascii_digit()) && !timing.is_empty() {
                timing = lines
                    .next()
                    .ok_or_else(|| invalid("SRT block missing timing"))?
                    .trim();
            } else {
                return Err(invalid(format!("SRT block missing timing: {timing}")));
            }
        }
        if !timing.contains("-->") {
            return Err(invalid(format!("SRT block missing timing: {timing}")));
        }
        let (start_s, rest) = timing.split_once("-->").expect("checked above");
        let end_s = rest.trim();
        if end_s.split_whitespace().count() != 1 {
            return Err(unsupported("SRT coordinate/positioning suffix"));
        }
        let start = parse_timestamp_ms(start_s.trim(), ',')?;
        let end = parse_timestamp_ms(end_s, ',')?;
        if end <= start {
            return Err(invalid("SRT cue end must follow start"));
        }
        let mut builder = MarkupBuilder::default();
        let mut first = true;
        for line in lines {
            if !first {
                builder.newline();
            }
            first = false;
            parse_inline_markup(&mut builder, line)?;
        }
        let (text, spans) = builder.finish()?;
        cues.push(ParsedCue {
            start,
            end,
            text,
            spans,
            anchor_column: None,
            fill: None,
            size: None,
            font_family: None,
            background: None,
        });
    }
    if cues.is_empty() {
        return Err(invalid("SRT document contains no cues"));
    }
    Ok(cues)
}

/// Inline `<b>/<i>/<font color>` markup inside one SRT/VTT text line.
fn parse_inline_markup(builder: &mut MarkupBuilder, line: &str) -> Result<(), ServiceError> {
    let mut rest = line;
    while let Some(i) = rest.find('<') {
        builder.text(&rest[..i])?;
        rest = &rest[i + 1..];
        let end = rest
            .find('>')
            .ok_or_else(|| invalid("unterminated markup tag"))?;
        let tag = rest[..end].to_string();
        rest = &rest[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            builder.close(name)?;
        } else if tag.ends_with('/') {
            return Err(unsupported(format!("unsupported empty tag <{tag}>")));
        } else {
            builder.tag(&tag)?;
        }
    }
    builder.text(rest)
}

fn write_srt(cues: &[ExportCue]) -> Result<String, ServiceError> {
    // CRLF is the conventional SRT record separator.
    let mut out = String::new();
    for (index, cue) in cues.iter().enumerate() {
        out.push_str(&format!("{}\r\n", index + 1));
        out.push_str(&format!(
            "{} --> {}\r\n",
            format_timestamp_ms(cue.start)?.replace('.', ","),
            format_timestamp_ms(cue.end)?.replace('.', ",")
        ));
        out.push_str(&cue.text.replace('\n', "\r\n"));
        out.push_str("\r\n");
        if index + 1 < cues.len() {
            out.push_str("\r\n");
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- WebVTT ------

fn parse_vtt(content: &str) -> Result<Vec<ParsedCue>, ServiceError> {
    let content = normalize_lines(content)?;
    let mut blocks = content.split("\n\n");
    let header = blocks
        .next()
        .ok_or_else(|| invalid("empty WebVTT document"))?;
    let rest = header
        .strip_prefix("WEBVTT")
        .ok_or_else(|| invalid("missing WEBVTT signature"))?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) && !rest.starts_with('\n') {
        return Err(invalid("malformed WEBVTT signature"));
    }
    let mut cues = vec![];
    for block in blocks {
        let block = block.trim_matches('\n');
        if block.is_empty() {
            continue;
        }
        if block.starts_with("NOTE") {
            continue;
        }
        if block.starts_with("STYLE") || block.starts_with("REGION") {
            return Err(unsupported("WebVTT STYLE/REGION blocks"));
        }
        let mut lines = block.split('\n');
        let first = lines.next().expect("nonempty block");
        let (timing, text_lines): (&str, Vec<&str>) = if first.contains("-->") {
            (first.trim(), lines.collect())
        } else {
            // Cue identifier line; its value is not persisted in the model.
            (
                lines
                    .next()
                    .ok_or_else(|| invalid("VTT cue missing timing"))?
                    .trim(),
                lines.collect(),
            )
        };
        let (start_s, rest) = timing
            .split_once("-->")
            .ok_or_else(|| invalid("VTT missing -->"))?;
        let start = parse_timestamp_ms(start_s.trim(), '.')?;
        let mut parts = rest.split_whitespace();
        let end_s = parts.next().ok_or_else(|| invalid("VTT missing cue end"))?;
        let end = parse_timestamp_ms(end_s, '.')?;
        if end <= start {
            return Err(invalid("VTT cue end must follow start"));
        }
        let mut anchor_column = None;
        for setting in parts {
            let Some((name, value)) = setting.split_once(':') else {
                return Err(invalid(format!("malformed VTT cue setting {setting}")));
            };
            match name {
                "align" => {
                    anchor_column = Some(match value {
                        "left" | "start" => CaptionAnchorColumn::Start,
                        "center" | "middle" => CaptionAnchorColumn::Center,
                        "right" | "end" => CaptionAnchorColumn::End,
                        _ => return Err(invalid(format!("unknown VTT align {value}"))),
                    });
                }
                "line" | "position" | "size" | "vertical" => {
                    return Err(unsupported(format!("VTT cue setting {name}")));
                }
                _ => return Err(invalid(format!("unknown VTT cue setting {name}"))),
            }
        }
        let mut builder = MarkupBuilder::default();
        let mut first_line = true;
        for line in text_lines {
            if !first_line {
                builder.newline();
            }
            first_line = false;
            parse_inline_markup(&mut builder, line)?;
        }
        let (text, spans) = builder.finish()?;
        cues.push(ParsedCue {
            start,
            end,
            text,
            spans,
            anchor_column,
            fill: None,
            size: None,
            font_family: None,
            background: None,
        });
    }
    if cues.is_empty() {
        return Err(invalid("WebVTT document contains no cues"));
    }
    Ok(cues)
}

fn write_vtt(cues: &[ExportCue]) -> Result<String, ServiceError> {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        out.push_str(&format!(
            "{} --> {}\n",
            format_timestamp_ms(cue.start)?,
            format_timestamp_ms(cue.end)?
        ));
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }
    Ok(out)
}

// ------------------------------------------------------------------ ITT ----

const TTML_NS: &str = "http://www.w3.org/ns/ttml";
const TTML_STYLING_NS: &str = "http://www.w3.org/ns/ttml#styling";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Style attributes declared on one ITT element, resolved with `style`
/// references (element attributes win; first reference wins on conflicts).
#[derive(Default, Clone)]
struct IttStyle {
    bold: Option<bool>,
    italic: Option<bool>,
    color: Option<Color>,
    font_family: Option<String>,
    font_size: Option<f64>,
    text_align: Option<CaptionAnchorColumn>,
    background: Option<Color>,
}
impl IttStyle {
    /// Fill fields this element did not declare from an enclosing style.
    fn inherit_from(&mut self, base: &Self) {
        let Self {
            bold,
            italic,
            color,
            font_family,
            font_size,
            text_align,
            background,
        } = base;
        self.bold = self.bold.or(*bold);
        self.italic = self.italic.or(*italic);
        self.color = self.color.or(*color);
        self.font_family = self.font_family.take().or_else(|| font_family.clone());
        self.font_size = self.font_size.or(*font_size);
        self.text_align = self.text_align.or(*text_align);
        self.background = self.background.or(*background);
    }
    fn empty(&self) -> bool {
        self.bold.is_none()
            && self.italic.is_none()
            && self.color.is_none()
            && self.font_family.is_none()
            && self.font_size.is_none()
            && self.text_align.is_none()
            && self.background.is_none()
    }
}

fn parse_itt_time(input: &str) -> Result<Time, ServiceError> {
    if let Some(offset) = input.strip_suffix("ms") {
        let ms: u64 = offset
            .parse()
            .map_err(|_| invalid(format!("malformed offset time {input}")))?;
        return Time::new(
            i64::try_from(ms).map_err(|_| invalid("time overflow"))?,
            1000,
        )
        .map_err(|e| invalid(e.to_string()));
    }
    if let Some(offset) = input.strip_suffix('s') {
        // Offset times keep exact fractions as rationals.
        let (whole, frac) = offset.split_once('.').unwrap_or((offset, ""));
        let seconds: u64 = whole
            .parse()
            .map_err(|_| invalid(format!("malformed offset time {input}")))?;
        let mut numerator = seconds;
        let mut denominator = 1_u64;
        for digit in frac.bytes() {
            if !digit.is_ascii_digit() {
                return Err(invalid(format!("malformed offset time {input}")));
            }
            numerator = numerator
                .checked_mul(10)
                .and_then(|v| v.checked_add(u64::from(digit - b'0')))
                .ok_or_else(|| invalid("offset time overflow"))?;
            denominator = denominator
                .checked_mul(10)
                .ok_or_else(|| invalid("offset time overflow"))?;
        }
        return Time::new(
            i64::try_from(numerator).map_err(|_| invalid("time overflow"))?,
            i64::try_from(denominator).map_err(|_| invalid("time overflow"))?,
        )
        .map_err(|e| invalid(e.to_string()));
    }
    if input.contains(':') {
        // Clock time keeps exact fractional seconds as a rational.
        let fields: Vec<&str> = input.split(':').collect();
        let (h, m, s) = match fields.as_slice() {
            [h, m, s] => (parse_u64(h)?, parse_u64(m)?, *s),
            [_, _, _, _] => {
                return Err(unsupported(format!(
                    "frame/tick timing {input} requires an unsupported time base"
                )));
            }
            _ => return Err(invalid(format!("malformed clock time {input}"))),
        };
        if m >= 60 {
            return Err(invalid(format!("clock time out of range {input}")));
        }
        let (secs, frac) = s.split_once('.').unwrap_or((s, ""));
        let seconds: u64 = secs
            .parse()
            .map_err(|_| invalid(format!("malformed clock time {input}")))?;
        if seconds >= 60 || (frac.is_empty() && input.contains('.')) {
            return Err(invalid(format!("clock time out of range {input}")));
        }
        let mut numerator = 0_u64;
        let mut denominator = 1_u64;
        for digit in frac.bytes() {
            if !digit.is_ascii_digit() {
                return Err(invalid(format!("malformed clock time {input}")));
            }
            numerator = numerator
                .checked_mul(10)
                .and_then(|v| v.checked_add(u64::from(digit - b'0')))
                .ok_or_else(|| invalid("clock fraction overflow"))?;
            denominator = denominator
                .checked_mul(10)
                .ok_or_else(|| invalid("clock fraction overflow"))?;
        }
        let whole = h
            .checked_mul(3600)
            .and_then(|v| v.checked_add(m * 60 + seconds))
            .ok_or_else(|| invalid("clock time overflow"))?;
        let total = whole
            .checked_mul(denominator)
            .and_then(|v| v.checked_add(numerator))
            .ok_or_else(|| invalid("clock time overflow"))?;
        return Time::new(
            i64::try_from(total).map_err(|_| invalid("time overflow"))?,
            i64::try_from(denominator).map_err(|_| invalid("time overflow"))?,
        )
        .map_err(|e| invalid(e.to_string()));
    }
    Err(unsupported(format!(
        "unsupported ITT time expression {input}"
    )))
}

fn itt_attrs(
    attrs: &[xml::attribute::OwnedAttribute],
    styles: &BTreeMap<String, IttStyle>,
) -> Result<IttStyle, ServiceError> {
    let mut style = IttStyle::default();
    let mut referenced = String::new();
    for attr in attrs {
        let local = attr.name.local_name.as_str();
        match (attr.name.namespace.as_deref(), local) {
            (Some(ns), _) if ns == TTML_STYLING_NS => match local {
                "fontWeight" => {
                    style.bold = Some(match attr.value.as_str() {
                        "bold" => true,
                        "normal" => false,
                        _ => return Err(unsupported("tts:fontWeight value")),
                    });
                }
                "fontStyle" => {
                    style.italic = Some(match attr.value.as_str() {
                        "italic" | "oblique" => true,
                        "normal" => false,
                        _ => return Err(unsupported("tts:fontStyle value")),
                    });
                }
                "color" => style.color = Some(parse_color(attr.value.trim())?),
                "fontFamily" => style.font_family = Some(attr.value.trim().to_string()),
                "fontSize" => {
                    let px = attr
                        .value
                        .trim()
                        .strip_suffix("px")
                        .ok_or_else(|| unsupported("tts:fontSize requires px"))?;
                    style.font_size = Some(
                        px.trim()
                            .parse::<f64>()
                            .map_err(|_| invalid("malformed tts:fontSize"))?,
                    );
                }
                "textAlign" => {
                    style.text_align = Some(match attr.value.as_str() {
                        "left" | "start" => CaptionAnchorColumn::Start,
                        "center" => CaptionAnchorColumn::Center,
                        "right" | "end" => CaptionAnchorColumn::End,
                        _ => return Err(unsupported("tts:textAlign value")),
                    });
                }
                "backgroundColor" => {
                    style.background = Some(parse_color(attr.value.trim())?);
                }
                other => return Err(unsupported(format!("tts:{other}"))),
            },
            (None, "style") => referenced = attr.value.clone(),
            _ => (),
        }
    }
    // Referenced styles fill undeclared fields, in document order.
    for reference in referenced.split_whitespace() {
        let Some(base) = styles.get(reference) else {
            return Err(invalid(format!("missing ITT style {reference}")));
        };
        style.inherit_from(base);
    }
    Ok(style)
}

fn parse_itt(content: &str) -> Result<Vec<ParsedCue>, ServiceError> {
    let content = normalize_lines(content)?;
    use xml::reader::{EventReader, XmlEvent};
    let reader = EventReader::new(content.as_bytes());
    let mut styles: BTreeMap<String, IttStyle> = BTreeMap::new();
    let mut cues = vec![];
    let mut cue: Option<ParsedCue> = None;
    let mut cue_style = IttStyle::default();
    let mut builder = MarkupBuilder::default();
    // Enclosing styled elements: (effective style, byte offset content began).
    let mut span_stack: Vec<(IttStyle, usize)> = vec![];
    let mut saw_tt = false;
    let mut depth = 0_u32;
    for event in reader {
        match event.map_err(|e| invalid(format!("malformed XML: {e}")))? {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                depth += 1;
                if depth > 64 {
                    return Err(unsupported("ITT nesting depth"));
                }
                let local = name.local_name.as_str();
                if !saw_tt {
                    if local != "tt" || name.namespace.as_deref() != Some(TTML_NS) {
                        return Err(invalid("ITT document must be a ttml:tt element"));
                    }
                    saw_tt = true;
                    continue;
                }
                if cue.is_some() {
                    match local {
                        "br" => builder.newline(),
                        "span" => {
                            let mut style = itt_attrs(&attributes, &styles)?;
                            style.inherit_from(&cue_style);
                            span_stack.push((style, builder.text.len()));
                        }
                        _ => {
                            return Err(unsupported(format!(
                                "unsupported ITT cue element <{local}>"
                            )));
                        }
                    }
                } else {
                    match local {
                        "style" => {
                            let id = attributes
                                .iter()
                                .find(|a| {
                                    a.name.local_name == "id"
                                        && a.name.namespace.as_deref() == Some(XML_NS)
                                })
                                .map(|a| a.value.clone())
                                .ok_or_else(|| invalid("ITT style requires xml:id"))?;
                            styles.insert(id, itt_attrs(&attributes, &styles)?);
                        }
                        "region" => return Err(unsupported("ITT regions")),
                        "head" | "body" | "div" | "layout" | "styling" | "metadata" => (),
                        "p" => {
                            cue_style = itt_attrs(&attributes, &styles)?;
                            let mut begin = None;
                            let mut end = None;
                            let mut dur = None;
                            for a in &attributes {
                                match (a.name.namespace.as_deref(), a.name.local_name.as_str()) {
                                    (None, "begin") => begin = Some(parse_itt_time(&a.value)?),
                                    (None, "end") => end = Some(parse_itt_time(&a.value)?),
                                    (None, "dur") => dur = Some(parse_itt_time(&a.value)?),
                                    _ => (),
                                }
                            }
                            let begin = begin.ok_or_else(|| invalid("ITT cue requires begin"))?;
                            let end = match (end, dur) {
                                (Some(end), None) => end,
                                (None, Some(dur)) => {
                                    begin.checked_add(dur).map_err(|e| invalid(e.to_string()))?
                                }
                                (None, None) => {
                                    return Err(invalid("ITT cue requires end or dur"));
                                }
                                (Some(_), Some(_)) => {
                                    return Err(invalid("ITT cue sets both end and dur"));
                                }
                            };
                            cue = Some(ParsedCue {
                                start: begin,
                                end,
                                text: String::new(),
                                spans: vec![],
                                anchor_column: cue_style.text_align,
                                fill: cue_style.color,
                                size: cue_style.font_size,
                                font_family: cue_style.font_family.clone(),
                                background: cue_style.background,
                            });
                            builder = MarkupBuilder::default();
                            span_stack.clear();
                        }
                        _ => return Err(unsupported(format!("ITT element <{local}>"))),
                    }
                }
            }
            XmlEvent::EndElement { name } => {
                depth = depth.saturating_sub(1);
                match name.local_name.as_str() {
                    "p" => {
                        let mut parsed = cue.take().ok_or_else(|| invalid("unbalanced </p>"))?;
                        let (text, spans) = std::mem::take(&mut builder).finish()?;
                        parsed.text = text;
                        parsed.spans = spans;
                        for (style, start) in span_stack.drain(..) {
                            if start >= parsed.text.len() {
                                continue;
                            }
                            let span = CaptionSpan {
                                range: TextRange {
                                    start,
                                    end: parsed.text.len(),
                                },
                                bold: style.bold,
                                italic: style.italic,
                                color: style.color,
                                font: None,
                            };
                            merge_span(&mut parsed.spans, span);
                        }
                        if !cue_style.empty() && !parsed.text.is_empty() {
                            if cue_style.font_family.is_some() {
                                // Kept on the cue for the caller's family
                                // comparison; it is not a span font.
                            }
                            if cue_style.bold.is_some()
                                || cue_style.italic.is_some()
                                || cue_style.color.is_some()
                            {
                                merge_span(
                                    &mut parsed.spans,
                                    CaptionSpan {
                                        range: TextRange {
                                            start: 0,
                                            end: parsed.text.len(),
                                        },
                                        bold: cue_style.bold,
                                        italic: cue_style.italic,
                                        color: None,
                                        font: None,
                                    },
                                );
                            }
                        }
                        if parsed.end <= parsed.start {
                            return Err(invalid("ITT cue end must follow start"));
                        }
                        parsed.spans.sort_by_key(|s| (s.range.start, s.range.end));
                        cues.push(parsed);
                        cue_style = IttStyle::default();
                    }
                    "span" => {
                        let Some((style, start)) = span_stack.pop() else {
                            return Err(invalid("unbalanced </span>"));
                        };
                        if style.font_family.is_some() {
                            return Err(unsupported(
                                "span tts:fontFamily requires an explicit locked font",
                            ));
                        }
                        if style.background.is_some() {
                            return Err(unsupported("span tts:backgroundColor"));
                        }
                        let end = builder.text.len();
                        if end > start {
                            merge_span(
                                &mut builder.spans,
                                CaptionSpan {
                                    range: TextRange { start, end },
                                    bold: style.bold,
                                    italic: style.italic,
                                    color: style.color,
                                    font: None,
                                },
                            );
                        }
                    }
                    _ => (),
                }
            }
            XmlEvent::Characters(text) | XmlEvent::CData(text) if cue.is_some() => {
                builder.xml_text(&text);
            }
            _ => (),
        }
    }
    if cue.is_some() {
        return Err(invalid("unclosed ITT cue"));
    }
    if cues.is_empty() {
        return Err(invalid("ITT document contains no cues"));
    }
    Ok(cues)
}

/// Merge span attributes into an existing same-range span or append; explicit
/// element attributes win over inherited ones.
fn merge_span(spans: &mut Vec<CaptionSpan>, span: CaptionSpan) {
    if let Some(existing) = spans.iter_mut().find(|s| s.range == span.range) {
        existing.bold = span.bold.or(existing.bold);
        existing.italic = span.italic.or(existing.italic);
        existing.color = span.color.or(existing.color);
    } else if span.bold.is_some() || span.italic.is_some() || span.color.is_some() {
        spans.push(span);
    }
}

fn write_itt(cues: &[ExportCue]) -> Result<String, ServiceError> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\">\n <body>\n  <div>\n",
    );
    for cue in cues {
        let doc = cue.document;
        out.push_str(&format!(
            "   <p begin=\"{}\" end=\"{}\" tts:color=\"{}\"",
            format_timestamp_ms(cue.start)?,
            format_timestamp_ms(cue.end)?,
            format_color(doc.style.fill)?,
        ));
        let align = match doc.placement.anchor {
            CaptionAnchor::TopLeft | CaptionAnchor::CenterLeft | CaptionAnchor::BottomLeft => {
                "left"
            }
            CaptionAnchor::TopRight | CaptionAnchor::CenterRight | CaptionAnchor::BottomRight => {
                "right"
            }
            _ => "center",
        };
        out.push_str(&format!(" tts:textAlign=\"{align}\""));
        if let Some(background) = doc.style.background {
            out.push_str(&format!(
                " tts:backgroundColor=\"{}\"",
                format_color(background)?
            ));
        }
        out.push('>');
        // Markup over the cue text; spans are ordered and nonoverlapping.
        let mut inner = String::new();
        let mut cursor = 0;
        for span in &doc.spans {
            if span.range.start > cursor {
                inner.push_str(&escape_text(&doc.text[cursor..span.range.start]));
            }
            inner.push_str("<span");
            if span.bold == Some(true) {
                inner.push_str(" tts:fontWeight=\"bold\"");
            }
            if span.italic == Some(true) {
                inner.push_str(" tts:fontStyle=\"italic\"");
            }
            if let Some(color) = span.color {
                inner.push_str(&format!(" tts:color=\"{}\"", format_color(color)?));
            }
            inner.push('>');
            inner.push_str(&escape_text(&doc.text[span.range.start..span.range.end]));
            inner.push_str("</span>");
            cursor = span.range.end;
        }
        inner.push_str(&escape_text(&doc.text[cursor..]));
        out.push_str(&inner.replace('\n', "<br/>"));
        out.push_str("</p>\n");
    }
    out.push_str("  </div>\n </body>\n</tt>\n");
    Ok(out)
}

// --------------------------------------------------------------- driver ----

#[doc(hidden)]
pub struct ExportCue<'a> {
    pub start: Time,
    pub end: Time,
    /// Serialized text with span markup applied (empty for ITT, which writes
    /// markup directly from the document).
    pub text: String,
    pub document: &'a CaptionDocument,
}

/// Markup serialization for SRT/VTT: spans become `<b>`, `<i>` and
/// `<font color>` tags around the entity-escaped cue text.
fn markup_text(document: &CaptionDocument) -> Result<String, ServiceError> {
    let mut out = String::new();
    let mut cursor = 0;
    for span in &document.spans {
        out.push_str(&escape_text(&document.text[cursor..span.range.start]));
        if span.bold == Some(true) {
            out.push_str("<b>");
        }
        if span.italic == Some(true) {
            out.push_str("<i>");
        }
        if let Some(color) = span.color {
            out.push_str(&format!("<font color=\"{}\">", format_color(color)?));
        }
        out.push_str(&escape_text(
            &document.text[span.range.start..span.range.end],
        ));
        if span.color.is_some() {
            out.push_str("</font>");
        }
        if span.italic == Some(true) {
            out.push_str("</i>");
        }
        if span.bold == Some(true) {
            out.push_str("</b>");
        }
        cursor = span.range.end;
    }
    out.push_str(&escape_text(&document.text[cursor..]));
    Ok(out)
}

/// Collect a sequence's caption cues in deterministic display order
/// (start time, then clip ID) across every caption track.
fn export_cues<'a>(
    project: &'a Project,
    sequence: &'a Sequence,
    format: CaptionFormat,
) -> Result<Vec<ExportCue<'a>>, ServiceError> {
    let mut clips: Vec<_> = sequence
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Caption)
        .flat_map(|t| t.clips.iter())
        .filter(|c| matches!(c.source_ref, SourceRef::Caption { .. }))
        .collect();
    clips.sort_by_key(|c| (c.timeline_range.start(), c.id));
    let mut cues = vec![];
    for clip in clips {
        let SourceRef::Caption { caption } = &clip.source_ref else {
            unreachable!()
        };
        if project
            .captions
            .iter()
            .any(|c| matches!(c, DocumentObject::Opaque(o) if o.id == caption.as_uuid()))
        {
            return Err(ServiceError::new(
                "UNSUPPORTED_CAPTION",
                "opaque caption cannot be exported",
            ));
        }
        let document = project
            .captions
            .iter()
            .find_map(|c| match c {
                DocumentObject::Known(c) if c.id == *caption => Some(c),
                _ => None,
            })
            .ok_or_else(|| invalid("caption document missing"))?;
        document
            .validate()
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        cues.push(ExportCue {
            start: clip.timeline_range.start(),
            end: clip.timeline_range.end(),
            text: if format == CaptionFormat::Itt {
                String::new()
            } else {
                markup_text(document)?
            },
            document,
        });
    }
    Ok(cues)
}

/// Parse one sidecar document into validated cues with canonical `\n` text.
#[doc(hidden)]
pub fn parse(content: &str, format: CaptionFormat) -> Result<Vec<ParsedCue>, ServiceError> {
    let mut cues = match format {
        CaptionFormat::Srt => parse_srt(content)?,
        CaptionFormat::Vtt => parse_vtt(content)?,
        CaptionFormat::Itt => parse_itt(content)?,
    };
    cues.sort_by_key(|c| c.start);
    for pair in cues.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(ServiceError::new(
                "CLIP_OVERLAP",
                "overlapping cues cannot be imported onto a caption track",
            ));
        }
    }
    Ok(cues)
}

/// Build the exact command list an import plan contains, so plan hashes agree.
fn plan_commands(request: &CaptionsImportPlanRequest) -> Result<Vec<EditCommand>, ServiceError> {
    let cues = parse(&request.content, request.format)?;
    if cues.len() != request.cue_ids.len() {
        return Err(ServiceError::invalid(format!(
            "cue_ids must contain exactly {} entries (one per parsed cue)",
            cues.len()
        )));
    }
    let store = crate::open_existing(&request.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let sequence = snapshot
        .document
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == request.sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))?;
    let mut commands = vec![];
    if let Some(track) = sequence.tracks.iter().find(|t| t.id == request.track) {
        if track.kind != TrackKind::Caption {
            return Err(ServiceError::new(
                "INVALID_CLIP",
                "captions import requires a caption track",
            ));
        }
    } else {
        commands.push(EditCommand::Timeline(Box::new(
            crate::TimelineCommand::TrackAppend {
                sequence: request.sequence,
                track: Track {
                    state: None,
                    id: request.track,
                    kind: TrackKind::Caption,
                    clips: vec![],
                },
            },
        )));
    }
    let registry = crate::edit::registry();
    for (cue, ids) in cues.into_iter().zip(&request.cue_ids) {
        if cue
            .font_family
            .as_ref()
            .is_some_and(|family| *family != request.style.font.family)
        {
            return Err(unsupported("cue fontFamily does not match the import font"));
        }
        let document = CaptionDocument {
            id: ids.caption,
            version: CAPTION_VERSION,
            text: cue.text,
            style: {
                let mut style = request.style.clone();
                if let Some(fill) = cue.fill {
                    style.fill = fill;
                }
                if let Some(size) = cue.size {
                    style.size = FiniteF64::new(size).map_err(|_| invalid("cue font size"))?;
                    if style.size.get() <= 0.0 {
                        return Err(invalid("cue font size must be positive"));
                    }
                }
                style
            },
            spans: cue.spans,
            placement: {
                let mut placement = CaptionPlacement::default();
                if let Some(column) = cue.anchor_column {
                    placement.anchor = column.apply(placement.anchor);
                }
                placement
            },
            format: Some(request.format),
        };
        document
            .validate()
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        // Clip-level caption overrides resolve against the service registry
        // before the plan is offered.
        document
            .resolve(&[], &registry)
            .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
        let clip = Clip {
            id: ids.clip,
            source_ref: SourceRef::Caption {
                caption: ids.caption,
            },
            timeline_range: TimeRange::new(cue.start, cue.end)
                .map_err(|e| invalid(e.to_string()))?,
            source_in: Time::ZERO,
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE)
                .map_err(|e| invalid(e.to_string()))?,
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
            volume: None,
            links: vec![],
            effects: vec![],
            masks: vec![],
            markers: vec![],
            properties: vec![],
        };
        commands.push(EditCommand::CaptionSet { caption: document });
        commands.push(EditCommand::Timeline(Box::new(
            crate::TimelineCommand::ClipPlace {
                sequence: request.sequence,
                track: request.track,
                clip: Box::new(clip),
            },
        )));
    }
    Ok(commands)
}

pub(crate) fn import_plan(
    request: CaptionsImportPlanRequest,
) -> Result<crate::EditPlan, ServiceError> {
    let commands = plan_commands(&request)?;
    crate::edit::plan(crate::PlanRequest {
        project: request.project,
        base_revision: request.base_revision,
        commands,
    })
}

pub(crate) fn import(request: CaptionsImportRequest) -> Result<Event, ServiceError> {
    let commands = plan_commands(&request.plan)?;
    let plan = crate::edit::plan(crate::PlanRequest {
        project: request.plan.project.clone(),
        base_revision: request.plan.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(crate::EditApplyRequest {
        project: request.plan.project,
        base_revision: request.plan.base_revision,
        session_id: request.session_id,
        idempotency_key: request.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}

pub(crate) fn export(request: CaptionsExportRequest) -> Result<CaptionsExportResult, ServiceError> {
    let store = crate::open_existing(&request.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let sequence = snapshot
        .document
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == request.sequence => Some(s),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("SOURCE_MISSING", "sequence missing"))?;
    let cues = export_cues(&snapshot.document, sequence, request.format)?;
    let content = match request.format {
        CaptionFormat::Srt => write_srt(&cues)?,
        CaptionFormat::Vtt => write_vtt(&cues)?,
        CaptionFormat::Itt => write_itt(&cues)?,
    };
    Ok(CaptionsExportResult {
        format: request.format,
        content,
    })
}

/// Serialize already-collected cues — used by job sidecar writing without
/// reopening the project store.
pub(crate) fn serialize(
    project: &Project,
    sequence: &Sequence,
    format: CaptionFormat,
) -> Result<String, ServiceError> {
    let cues = export_cues(project, sequence, format)?;
    match format {
        CaptionFormat::Srt => write_srt(&cues),
        CaptionFormat::Vtt => write_vtt(&cues),
        CaptionFormat::Itt => write_itt(&cues),
    }
}
