//! Human-readable surface syntax for the canonical expression AST (ADR-0105).
//!
//! The text form is only a display and editing representation. The canonical
//! postorder [`Expression`] AST remains the stored truth and is the sole source
//! of evaluation semantics (ADR-0040, ADR-0058). [`parse_expression`] produces
//! the same AST a tool would build directly, and [`format_expression`] is its
//! inverse for every valid AST that fits the syntax limits, so
//! `parse_expression(&format_expression(e)?, &metadata) == e` for in-syntax
//! ASTs. Expression metadata (id / semantic version / value_type / budget)
//! always arrives through the editing envelope and is never re-derived from
//! text.
//!
//! Grammar summary: a single expression, insignificant whitespace, JSON string
//! quoting, signed decimal finite numeric literals, `+ - * /` with the usual
//! precedence and left associativity, parentheses, a fixed function table, and
//! `time_offset` rationals only as fixed arguments of `curve`, `audio_feature`
//! and `audio_band`. No statements, assignments, loops or JavaScript
//! compatibility exist.

use crate::{
    AssetId, AudioFeature, CurveId, Expression, ExpressionBudget, ExpressionError, ExpressionId,
    ExpressionNode, NodeId, PropertyId, Value, ValueType,
};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;
use uuid::Uuid;

/// Maximum UTF-8 input size in bytes (64 KiB).
pub const EXPRESSION_TEXT_MAX_BYTES: usize = 64 * 1024;
/// Maximum token count after lexing.
pub const EXPRESSION_TEXT_MAX_TOKENS: usize = 8_192;
/// Maximum syntactic nesting level of parentheses and call arguments.
pub const EXPRESSION_TEXT_MAX_DEPTH: usize = 64;

const FUNCTION_NAMES: &[&str] = &[
    "angle",
    "audio_band",
    "audio_feature",
    "clamp",
    "continuous_noise",
    "curve",
    "data_cell",
    "lerp",
    "literal",
    "noise",
    "property",
    "property_sample",
    "sin",
    "time",
    "vec2",
    "vec3",
];
const AUDIO_FEATURE_NAMES: &[&str] = &["rms", "onset", "beat"];

/// One typed syntax diagnostic: a UTF-8 byte range plus a 1-based line and
/// character column and the tokens that would have been accepted. API and GUI
/// share the same payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionDiagnostic {
    pub byte_start: usize,
    pub byte_end: usize,
    /// 1-based line of `byte_start`.
    pub line: usize,
    /// 1-based character column of `byte_start`.
    pub column: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<String>,
    pub message: String,
}

/// Rejected expression text. Parsing stops at the first failure and keeps the
/// ordered diagnostics; the previously stored expression is never replaced by
/// uncommitted or invalid input.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionSyntaxError {
    pub diagnostics: Vec<ExpressionDiagnostic>,
}
impl std::fmt::Display for ExpressionSyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, d) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{}:{}: {}", d.line, d.column, d.message)?;
        }
        Ok(())
    }
}
impl std::error::Error for ExpressionSyntaxError {}
impl ExpressionSyntaxError {
    pub const CODE: &'static str = "EXPRESSION_SYNTAX";
    pub const fn code(&self) -> &'static str {
        Self::CODE
    }
}

/// A text-applied expression failure: either a syntax diagnostic or the
/// existing typed AST validation of the parsed result.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ExpressionTextError {
    #[error(transparent)]
    Syntax(#[from] ExpressionSyntaxError),
    #[error(transparent)]
    Invalid(#[from] ExpressionError),
}
impl ExpressionTextError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax(e) => e.code(),
            Self::Invalid(e) => e.code(),
        }
    }
    /// Diagnostics shared with the GUI, present for syntax failures.
    pub fn diagnostics(&self) -> Option<&[ExpressionDiagnostic]> {
        match self {
            Self::Syntax(e) => Some(&e.diagnostics),
            Self::Invalid(_) => None,
        }
    }
}

/// An AST that cannot be emitted within the text syntax surface. The stored
/// AST is preserved; nothing is rewritten silently.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ExpressionFormatError {
    /// Not a canonical AST (version, shape, type or budget), so it cannot have
    /// a faithful text representation.
    #[error("expression is not a valid canonical AST: {0}")]
    Invalid(#[from] ExpressionError),
    /// The formatted output would exceed a syntax input limit.
    #[error("formatted expression exceeds the {0} syntax limit")]
    OutputLimit(&'static str),
}
impl ExpressionFormatError {
    pub const CODE: &'static str = "EXPRESSION_FORMAT";
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(e) => e.code(),
            Self::OutputLimit(_) => Self::CODE,
        }
    }
}

/// Editing envelope for text-applied expressions. Identity, semantic version,
/// declared result type and budget are never re-derived from text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionMetadata {
    pub id: ExpressionId,
    pub version: u32,
    pub value_type: ValueType,
    #[serde(default)]
    pub budget: ExpressionBudget,
}
impl From<&Expression> for ExpressionMetadata {
    fn from(expression: &Expression) -> Self {
        Self {
            id: expression.id,
            version: expression.version,
            value_type: expression.value_type,
            budget: expression.budget,
        }
    }
}
impl ExpressionMetadata {
    fn expression(self, nodes: Vec<ExpressionNode>) -> Expression {
        Expression {
            id: self.id,
            version: self.version,
            value_type: self.value_type,
            budget: self.budget,
            nodes,
        }
    }
}

fn line_column(text: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for &b in text.as_bytes().iter().take(byte) {
        if b == b'\n' {
            line += 1;
            column = 1;
        } else if b & 0xC0 != 0x80 {
            column += 1;
        }
    }
    (line, column)
}
fn diagnostic(
    text: &str,
    byte_start: usize,
    byte_end: usize,
    message: impl Into<String>,
    expected: &[&str],
) -> ExpressionDiagnostic {
    let (line, column) = line_column(text, byte_start.min(text.len()));
    ExpressionDiagnostic {
        byte_start: byte_start.min(text.len()),
        byte_end: byte_end.min(text.len()),
        line,
        column,
        expected: expected.iter().map(|s| (*s).to_string()).collect(),
        message: message.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Number,
    Str,
    Ident,
}
#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}
impl Token {
    fn span(&self) -> (usize, usize) {
        (self.start, self.end)
    }
}

fn lex(text: &str) -> Result<Vec<Token>, ExpressionSyntaxError> {
    let fail = |d: ExpressionDiagnostic| ExpressionSyntaxError {
        diagnostics: vec![d],
    };
    if text.len() > EXPRESSION_TEXT_MAX_BYTES {
        return Err(fail(diagnostic(
            text,
            EXPRESSION_TEXT_MAX_BYTES,
            text.len(),
            format!(
                "expression text exceeds {} bytes",
                EXPRESSION_TEXT_MAX_BYTES
            ),
            &[],
        )));
    }
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let push = |tokens: &mut Vec<Token>, kind, start, end| -> Result<(), ExpressionSyntaxError> {
        if tokens.len() >= EXPRESSION_TEXT_MAX_TOKENS {
            return Err(ExpressionSyntaxError {
                diagnostics: vec![diagnostic(
                    text,
                    start,
                    end,
                    format!("expression exceeds {} tokens", EXPRESSION_TEXT_MAX_TOKENS),
                    &[],
                )],
            });
        }
        tokens.push(Token { kind, start, end });
        Ok(())
    };
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        i += 1;
        match c {
            b'(' => push(&mut tokens, TokenKind::LParen, start, i)?,
            b')' => push(&mut tokens, TokenKind::RParen, start, i)?,
            b',' => push(&mut tokens, TokenKind::Comma, start, i)?,
            b'+' => push(&mut tokens, TokenKind::Plus, start, i)?,
            b'-' => push(&mut tokens, TokenKind::Minus, start, i)?,
            b'*' => push(&mut tokens, TokenKind::Star, start, i)?,
            b'/' => push(&mut tokens, TokenKind::Slash, start, i)?,
            b'0'..=b'9' => {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                    let mut j = i + 1;
                    if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
                        j += 1;
                    }
                    if j >= bytes.len() || !bytes[j].is_ascii_digit() {
                        return Err(fail(diagnostic(
                            text,
                            i,
                            j.min(bytes.len()),
                            "invalid numeric literal: expected exponent digits",
                            &["exponent digits"],
                        )));
                    }
                    i = j;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < bytes.len()
                    && (matches!(bytes[i], b'.' | b'"')
                        || bytes[i].is_ascii_alphabetic()
                        || bytes[i] == b'_')
                {
                    return Err(fail(diagnostic(
                        text,
                        i,
                        i + 1,
                        "invalid numeric literal",
                        &["number"],
                    )));
                }
                push(&mut tokens, TokenKind::Number, start, i)?;
            }
            b'"' => {
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'"' => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        b'\\' => {
                            i += 1;
                            if i < bytes.len() {
                                // Skip the escaped unit so \" does not close.
                                let width = utf8_width(bytes[i]);
                                i += width;
                            }
                        }
                        _ => i += utf8_width(bytes[i]),
                    }
                }
                if !closed {
                    return Err(fail(diagnostic(
                        text,
                        start,
                        bytes.len(),
                        "unterminated string literal",
                        &["closing '\"'"],
                    )));
                }
                push(&mut tokens, TokenKind::Str, start, i)?;
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                push(&mut tokens, TokenKind::Ident, start, i)?;
            }
            // Whitespace is insignificant and limited to ASCII spaces.
            b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C => {}
            _ => {
                let width = utf8_width(c);
                let end = start + width;
                return Err(fail(diagnostic(
                    text,
                    start,
                    end,
                    format!("unexpected character '{}'", &text[start..end]),
                    &["expression"],
                )));
            }
        }
    }
    Ok(tokens)
}
fn utf8_width(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    nodes: Vec<ExpressionNode>,
}
type PResult<T> = Result<T, ExpressionDiagnostic>;

impl<'a> Parser<'a> {
    fn fail<T>(
        &self,
        start: usize,
        end: usize,
        message: impl Into<String>,
        expected: &[&str],
    ) -> PResult<T> {
        Err(diagnostic(self.text, start, end, message, expected))
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }
    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += usize::from(token.is_some());
        token
    }
    /// Byte span of the current lookahead, or end of input.
    fn lookahead_span(&self) -> (usize, usize) {
        self.peek()
            .map_or_else(|| (self.text.len(), self.text.len()), |t| (t.start, t.end))
    }
    fn expect(&mut self, kind: TokenKind, expected: &str) -> PResult<Token> {
        if self.peek_kind() == Some(kind) {
            return Ok(self.bump().unwrap());
        }
        let (start, end) = self.lookahead_span();
        let found = self.peek().map_or("end of input", |t| self.slice(t));
        self.fail(
            start,
            end,
            format!("expected {expected}, found '{found}'"),
            &[expected],
        )
    }
    fn push(&mut self, node: ExpressionNode) -> u32 {
        let index = self.nodes.len() as u32;
        self.nodes.push(node);
        index
    }
    fn slice(&self, token: &Token) -> &'a str {
        &self.text[token.start..token.end]
    }
    fn string_content(&self, token: &Token) -> PResult<String> {
        serde_json::from_str::<String>(self.slice(token)).map_err(|e| {
            let offset = e.column().saturating_sub(1).min(token.end - token.start);
            diagnostic(
                self.text,
                token.start + offset,
                token.end,
                format!("invalid string literal: {e}"),
                &[],
            )
        })
    }
    /// One unsigned decimal integer fixed argument: digits only, no sign,
    /// fraction or exponent (seed / element / band).
    fn u32_arg(&mut self, name: &str) -> PResult<u32> {
        let expected = "unsigned decimal integer";
        match self.peek() {
            Some(token) if token.kind == TokenKind::Number => {
                let text = self.slice(token);
                if !text.bytes().all(|b| b.is_ascii_digit()) {
                    let (start, end) = token.span();
                    return self.fail(
                        start,
                        end,
                        format!("{name} must be an unsigned decimal integer literal"),
                        &[expected],
                    );
                }
                let text = text.to_string();
                let span = self.bump().unwrap().span();
                text.parse::<u32>().map_err(|_| {
                    diagnostic(
                        self.text,
                        span.0,
                        span.1,
                        format!("{name} is outside the u32 range 0..=4294967295"),
                        &[expected],
                    )
                })
            }
            _ => {
                let (start, end) = self.lookahead_span();
                self.fail(
                    start,
                    end,
                    format!("expected {expected} for {name}"),
                    &[expected],
                )
            }
        }
    }
    fn string_arg(&mut self, name: &str) -> PResult<(String, (usize, usize))> {
        let expected = "string literal";
        match self.peek() {
            Some(token) if token.kind == TokenKind::Str => {
                let token = self.bump().unwrap();
                Ok((self.string_content(&token)?, token.span()))
            }
            _ => {
                let (start, end) = self.lookahead_span();
                self.fail(
                    start,
                    end,
                    format!("expected {expected} for {name}"),
                    &[expected],
                )
            }
        }
    }
    fn uuid_arg(&mut self, name: &str) -> PResult<Uuid> {
        let (span, text) = {
            let token = match self.peek() {
                Some(token) if token.kind == TokenKind::Str => token.clone(),
                _ => {
                    let (start, end) = self.lookahead_span();
                    return self.fail(
                        start,
                        end,
                        format!("expected stable UUID string for {name}"),
                        &["UUID string"],
                    );
                }
            };
            (token.span(), self.string_content(&token)?)
        };
        self.pos += 1;
        Uuid::parse_str(&text).map_err(|_| {
            diagnostic(
                self.text,
                span.0,
                span.1,
                format!("{name} is not a UUID string"),
                &["UUID string"],
            )
        })
    }
    /// `null` (composition input) or a stable node UUID string.
    fn node_arg(&mut self) -> PResult<Option<NodeId>> {
        match self.peek() {
            Some(token) if token.kind == TokenKind::Ident && self.slice(token) == "null" => {
                self.pos += 1;
                Ok(None)
            }
            Some(token) if token.kind == TokenKind::Str => {
                Ok(Some(NodeId::from_uuid(self.uuid_arg("node reference")?)))
            }
            _ => {
                let (start, end) = self.lookahead_span();
                self.fail(
                    start,
                    end,
                    "expected null or a node UUID string",
                    &["null", "node UUID string"],
                )
            }
        }
    }
    fn value_type_arg(&mut self) -> PResult<ValueType> {
        let (span, text) = {
            let token = match self.peek() {
                Some(token) if token.kind == TokenKind::Str => token.clone(),
                _ => {
                    let (start, end) = self.lookahead_span();
                    return self.fail(
                        start,
                        end,
                        "expected value type name string",
                        &["value type name"],
                    );
                }
            };
            (token.span(), self.string_content(&token)?)
        };
        self.pos += 1;
        value_type_from_name(&text).ok_or_else(|| {
            diagnostic(
                self.text,
                span.0,
                span.1,
                format!("unknown value type '{text}'"),
                VALUE_TYPE_NAMES,
            )
        })
    }
    fn i64_decimal(&self, token: &Token, component: &str) -> PResult<i64> {
        let text = self.string_content(token)?;
        let digits = text.strip_prefix('-').unwrap_or(&text);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return self.fail(
                token.start,
                token.end,
                format!("{component} must be a signed decimal integer string"),
                &["decimal integer string"],
            );
        }
        text.parse::<i64>().map_err(|_| {
            diagnostic(
                self.text,
                token.start,
                token.end,
                format!("{component} is outside the i64 range"),
                &["decimal integer string"],
            )
        })
    }
    /// `time_offset("num", "den")` is only a fixed argument of the listed
    /// functions, never a standalone node.
    fn time_offset_arg(&mut self) -> PResult<Time> {
        let (start, _) = self.lookahead_span();
        match self.peek() {
            Some(token) if token.kind == TokenKind::Ident && self.slice(token) == "time_offset" => {
                self.pos += 1;
            }
            _ => {
                return self.fail(
                    start,
                    self.lookahead_span().1,
                    "expected time_offset(\"num\", \"den\")",
                    &["time_offset(\"num\", \"den\")"],
                );
            }
        }
        self.expect(TokenKind::LParen, "'('")?;
        let numerator = self.expect(TokenKind::Str, "numerator string")?;
        let numerator = self.i64_decimal(&numerator, "time_offset numerator")?;
        self.expect(TokenKind::Comma, "','")?;
        let denominator = self.expect(TokenKind::Str, "denominator string")?;
        let denominator = self.i64_decimal(&denominator, "time_offset denominator")?;
        self.expect(TokenKind::RParen, "')'")?;
        Time::new(numerator, denominator).map_err(|e| {
            diagnostic(
                self.text,
                start,
                self.lookahead_span().1,
                format!("invalid time_offset rational: {e}"),
                &[],
            )
        })
    }
    fn comma(&mut self) -> PResult<()> {
        self.expect(TokenKind::Comma, "','").map(|_| ())
    }
    fn close(&mut self) -> PResult<()> {
        self.expect(TokenKind::RParen, "')'").map(|_| ())
    }

    fn expression(&mut self, depth: usize) -> PResult<u32> {
        if depth > EXPRESSION_TEXT_MAX_DEPTH {
            let (start, end) = self.lookahead_span();
            return self.fail(
                start,
                end,
                format!("expression nesting exceeds {}", EXPRESSION_TEXT_MAX_DEPTH),
                &[],
            );
        }
        // additive := multiplicative (('+'|'-') multiplicative)*, left-assoc
        let mut left = self.multiplicative(depth)?;
        loop {
            let subtract = match self.peek_kind() {
                Some(TokenKind::Plus) => false,
                Some(TokenKind::Minus) => true,
                _ => break,
            };
            self.pos += 1;
            let right = self.multiplicative(depth)?;
            left = self.push(if subtract {
                ExpressionNode::Subtract { left, right }
            } else {
                ExpressionNode::Add { left, right }
            });
        }
        Ok(left)
    }
    fn multiplicative(&mut self, depth: usize) -> PResult<u32> {
        // multiplicative := primary (('*'|'/') primary)*, left-assoc
        let mut left = self.primary(depth)?;
        loop {
            let divide = match self.peek_kind() {
                Some(TokenKind::Star) => false,
                Some(TokenKind::Slash) => true,
                _ => break,
            };
            self.pos += 1;
            let right = self.primary(depth)?;
            left = self.push(if divide {
                ExpressionNode::Divide { left, right }
            } else {
                ExpressionNode::Multiply { left, right }
            });
        }
        Ok(left)
    }
    fn number_literal(&mut self, negative: bool) -> PResult<u32> {
        let token = self.bump().unwrap();
        let text = self.slice(&token);
        let parsed: f64 = text.parse().map_err(|_| {
            diagnostic(
                self.text,
                token.start,
                token.end,
                "invalid numeric literal",
                &[],
            )
        })?;
        // IEEE negation is exact and keeps -0 representable.
        let parsed = if negative { -parsed } else { parsed };
        let value = crate::FiniteF64::new(parsed).map_err(|_| {
            diagnostic(
                self.text,
                token.start,
                token.end,
                "numeric literal must be finite",
                &[],
            )
        })?;
        Ok(self.push(ExpressionNode::Literal(Value::Scalar(value))))
    }
    fn primary(&mut self, depth: usize) -> PResult<u32> {
        const EXPECTED: &[&str] = &["number", "string", "true", "false", "'('", "function call"];
        let Some(token) = self.peek().cloned() else {
            let end = self.text.len();
            return self.fail(
                end,
                end,
                "expected expression, found end of input",
                EXPECTED,
            );
        };
        match token.kind {
            TokenKind::Number => self.number_literal(false),
            TokenKind::Str => {
                self.pos += 1;
                let content = self.string_content(&token)?;
                Ok(self.push(ExpressionNode::Literal(Value::String(content))))
            }
            TokenKind::Plus | TokenKind::Minus => {
                let negative = token.kind == TokenKind::Minus;
                self.pos += 1;
                match self.peek_kind() {
                    Some(TokenKind::Number) => self.number_literal(negative),
                    _ => {
                        let (start, end) = self.lookahead_span();
                        self.fail(
                            start,
                            end,
                            "a sign applies only to a numeric literal; write 0 - x",
                            &["number"],
                        )
                    }
                }
            }
            TokenKind::LParen => {
                self.pos += 1;
                let inner = self.expression(depth + 1)?;
                self.expect(TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            TokenKind::Ident => self.ident_or_call(&token, depth),
            _ => self.fail(
                token.start,
                token.end,
                format!("expected expression, found '{}'", self.slice(&token)),
                EXPECTED,
            ),
        }
    }
    fn ident_or_call(&mut self, token: &Token, depth: usize) -> PResult<u32> {
        let name = self.slice(token);
        match name {
            "true" | "false" => {
                self.pos += 1;
                Ok(self.push(ExpressionNode::Literal(Value::Bool(name == "true"))))
            }
            "null" => self.fail(
                token.start,
                token.end,
                "null is only allowed as the node argument of property / property_sample",
                &[],
            ),
            name if FUNCTION_NAMES.contains(&name) => self.call(name, depth),
            "time_offset" => self.fail(
                token.start,
                token.end,
                "time_offset is only allowed as the fixed offset argument of \
                 curve / audio_feature / audio_band",
                &[],
            ),
            _ => self.fail(
                token.start,
                token.end,
                format!("unknown function or literal '{name}'"),
                FUNCTION_NAMES,
            ),
        }
    }
    /// Parse one function call. `name` is the identifier; the cursor is still
    /// on it. Arity is fixed per function.
    fn call(&mut self, name: &str, depth: usize) -> PResult<u32> {
        self.pos += 1;
        self.expect(TokenKind::LParen, "'('")?;
        let node = match name {
            "time" => {
                self.close()?;
                ExpressionNode::Time
            }
            "sin" => {
                let input = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Sin { input }
            }
            "angle" => {
                let degrees = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Angle { degrees }
            }
            "clamp" => {
                let value = self.expression(depth + 1)?;
                self.comma()?;
                let min = self.expression(depth + 1)?;
                self.comma()?;
                let max = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Clamp { value, min, max }
            }
            "lerp" => {
                let from = self.expression(depth + 1)?;
                self.comma()?;
                let to = self.expression(depth + 1)?;
                self.comma()?;
                let amount = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Lerp { from, to, amount }
            }
            "vec2" => {
                let x = self.expression(depth + 1)?;
                self.comma()?;
                let y = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Vec2 { x, y }
            }
            "vec3" => {
                let x = self.expression(depth + 1)?;
                self.comma()?;
                let y = self.expression(depth + 1)?;
                self.comma()?;
                let z = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::Vec3 { x, y, z }
            }
            "noise" | "continuous_noise" => {
                let seed = self.u32_arg("seed")?;
                self.comma()?;
                let element = self.u32_arg("element")?;
                self.comma()?;
                let input = self.expression(depth + 1)?;
                self.close()?;
                if name == "noise" {
                    ExpressionNode::Noise {
                        seed,
                        element,
                        input,
                    }
                } else {
                    ExpressionNode::ContinuousNoise {
                        seed,
                        element,
                        input,
                    }
                }
            }
            "property" => {
                let node = self.node_arg()?;
                self.comma()?;
                let property = PropertyId::from_uuid(self.uuid_arg("property reference")?);
                self.comma()?;
                let value_type = self.value_type_arg()?;
                self.close()?;
                ExpressionNode::Property {
                    node,
                    property,
                    value_type,
                }
            }
            "property_sample" => {
                let node = self.node_arg()?;
                self.comma()?;
                let property = PropertyId::from_uuid(self.uuid_arg("property reference")?);
                self.comma()?;
                let value_type = self.value_type_arg()?;
                self.comma()?;
                let lookback = self.expression(depth + 1)?;
                self.close()?;
                ExpressionNode::PropertySample {
                    node,
                    property,
                    value_type,
                    lookback,
                }
            }
            "curve" => {
                let curve = CurveId::from_uuid(self.uuid_arg("curve reference")?);
                self.comma()?;
                let offset = self.time_offset_arg()?;
                self.comma()?;
                let value_type = self.value_type_arg()?;
                self.close()?;
                ExpressionNode::CurveSample {
                    curve,
                    offset,
                    value_type,
                }
            }
            "data_cell" => {
                let asset = AssetId::from_uuid(self.uuid_arg("data asset reference")?);
                self.comma()?;
                let (column, _) = self.string_arg("column")?;
                self.comma()?;
                let row = self.expression(depth + 1)?;
                self.comma()?;
                let value_type = self.value_type_arg()?;
                self.close()?;
                ExpressionNode::DataAssetCell {
                    asset,
                    column,
                    row,
                    value_type,
                }
            }
            "audio_feature" => {
                let asset = AssetId::from_uuid(self.uuid_arg("audio analysis reference")?);
                self.comma()?;
                let (span, feature_name) = {
                    let token = match self.peek() {
                        Some(token) if token.kind == TokenKind::Str => token.clone(),
                        _ => {
                            let (start, end) = self.lookahead_span();
                            return self.fail(
                                start,
                                end,
                                "expected audio feature name string",
                                AUDIO_FEATURE_NAMES,
                            );
                        }
                    };
                    (token.span(), self.string_content(&token)?)
                };
                self.pos += 1;
                let feature = match feature_name.as_str() {
                    "rms" => AudioFeature::Rms,
                    "onset" => AudioFeature::Onset,
                    "beat" => AudioFeature::Beat,
                    _ => {
                        return self.fail(
                            span.0,
                            span.1,
                            format!("unknown audio feature '{feature_name}'"),
                            AUDIO_FEATURE_NAMES,
                        );
                    }
                };
                self.comma()?;
                let offset = self.time_offset_arg()?;
                self.close()?;
                ExpressionNode::AudioFeature {
                    asset,
                    feature,
                    offset,
                }
            }
            "audio_band" => {
                let asset = AssetId::from_uuid(self.uuid_arg("audio analysis reference")?);
                self.comma()?;
                let band = self.u32_arg("band")?;
                self.comma()?;
                let offset = self.time_offset_arg()?;
                self.close()?;
                ExpressionNode::AudioFeature {
                    asset,
                    feature: AudioFeature::BandEnergy { band },
                    offset,
                }
            }
            "literal" => {
                let (json, span) = self.string_arg("literal value JSON")?;
                self.close()?;
                let value = serde_json::from_str::<Value>(&json).map_err(|e| {
                    diagnostic(
                        self.text,
                        span.0,
                        span.1,
                        format!("literal() is not a valid Value JSON: {e}"),
                        &[],
                    )
                })?;
                ExpressionNode::Literal(value)
            }
            _ => unreachable!("call dispatch covers FUNCTION_NAMES"),
        };
        Ok(self.push(node))
    }
}

fn parse_nodes(text: &str) -> Result<Vec<ExpressionNode>, ExpressionSyntaxError> {
    let tokens = lex(text)?;
    let mut parser = Parser {
        text,
        tokens,
        pos: 0,
        nodes: Vec::new(),
    };
    parser.expression(0).map_err(|d| ExpressionSyntaxError {
        diagnostics: vec![d],
    })?;
    if parser.pos != parser.tokens.len() {
        let (start, end) = parser.lookahead_span();
        let found = &parser.text[start..end];
        return Err(ExpressionSyntaxError {
            diagnostics: vec![diagnostic(
                text,
                start,
                end,
                format!("unexpected trailing input '{found}'"),
                &["end of input"],
            )],
        });
    }
    Ok(parser.nodes)
}

/// Parse one expression text into the canonical AST and validate it with the
/// existing AST checks (shape, types, budget, supported semantic version).
/// `metadata` is the editing envelope: the stored id, semantic version,
/// value_type and budget, never re-derived from text.
pub fn parse_expression(
    text: &str,
    metadata: &ExpressionMetadata,
) -> Result<Expression, ExpressionTextError> {
    let nodes = parse_nodes(text)?;
    let expression = metadata.clone().expression(nodes);
    expression
        .validate()
        .map_err(ExpressionTextError::Invalid)?;
    Ok(expression)
}

const VALUE_TYPE_NAMES: &[&str] = &[
    "scalar",
    "vec2",
    "vec3",
    "angle",
    "color",
    "bool",
    "enum",
    "string",
    "asset_ref",
    "data_table",
    "path",
];
fn value_type_name(value_type: ValueType) -> &'static str {
    match value_type {
        ValueType::Scalar => "scalar",
        ValueType::Vec2 => "vec2",
        ValueType::Vec3 => "vec3",
        ValueType::Angle => "angle",
        ValueType::Color => "color",
        ValueType::Bool => "bool",
        ValueType::Enum => "enum",
        ValueType::String => "string",
        ValueType::AssetRef => "asset_ref",
        ValueType::DataTable => "data_table",
        ValueType::Path => "path",
    }
}
fn value_type_from_name(name: &str) -> Option<ValueType> {
    Some(match name {
        "scalar" => ValueType::Scalar,
        "vec2" => ValueType::Vec2,
        "vec3" => ValueType::Vec3,
        "angle" => ValueType::Angle,
        "color" => ValueType::Color,
        "bool" => ValueType::Bool,
        "enum" => ValueType::Enum,
        "string" => ValueType::String,
        "asset_ref" => ValueType::AssetRef,
        "data_table" => ValueType::DataTable,
        "path" => ValueType::Path,
        _ => return None,
    })
}

const ADD_SUB: u8 = 1;
const MUL_DIV: u8 = 2;

fn infix_precedence(node: &ExpressionNode) -> Option<u8> {
    match node {
        ExpressionNode::Add { .. } | ExpressionNode::Subtract { .. } => Some(ADD_SUB),
        ExpressionNode::Multiply { .. } | ExpressionNode::Divide { .. } => Some(MUL_DIV),
        _ => None,
    }
}
fn emit_u32(out: &mut String, value: u32) {
    let _ = write!(out, "{value}");
}
fn emit_uuid(out: &mut String, id: Uuid) {
    let _ = write!(out, "\"{id}\"");
}
fn emit_string(out: &mut String, value: &str) {
    // JSON string quoting is the surface string form.
    out.push_str(&serde_json::to_string(value).expect("string serializes"));
}
fn emit_offset(out: &mut String, offset: Time) {
    let _ = write!(
        out,
        "time_offset(\"{}\", \"{}\")",
        offset.numerator(),
        offset.denominator()
    );
}
fn emit_node_ref(out: &mut String, node: Option<NodeId>) {
    match node {
        Some(node) => emit_uuid(out, node.as_uuid()),
        None => out.push_str("null"),
    }
}
fn emit(
    expression: &Expression,
    index: u32,
    parent_precedence: u8,
    right_operand: bool,
    out: &mut String,
) {
    let node = &expression.nodes[index as usize];
    // Only infix children can need parentheses; the canonical text never
    // reorders or re-associates the stored AST (e.g. `a - (b - c)` keeps its
    // parentheses and is never flattened to `a - b - c`).
    let parens = infix_precedence(node)
        .is_some_and(|p| p < parent_precedence || (right_operand && p == parent_precedence));
    if parens {
        out.push('(');
    }
    match node {
        ExpressionNode::Literal(value) => match value {
            Value::Scalar(v) => {
                let _ = write!(out, "{}", v.get());
            }
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::String(s) => emit_string(out, s),
            _ => {
                out.push_str("literal(");
                let inner = serde_json::to_string(value).expect("Value serializes");
                emit_string(out, &inner);
                out.push(')');
            }
        },
        ExpressionNode::Time => out.push_str("time()"),
        ExpressionNode::Add { left, right } => {
            emit_infix(expression, *left, *right, "+", ADD_SUB, out);
        }
        ExpressionNode::Subtract { left, right } => {
            emit_infix(expression, *left, *right, "-", ADD_SUB, out);
        }
        ExpressionNode::Multiply { left, right } => {
            emit_infix(expression, *left, *right, "*", MUL_DIV, out);
        }
        ExpressionNode::Divide { left, right } => {
            emit_infix(expression, *left, *right, "/", MUL_DIV, out);
        }
        ExpressionNode::Clamp { value, min, max } => {
            out.push_str("clamp(");
            emit(expression, *value, 0, false, out);
            out.push_str(", ");
            emit(expression, *min, 0, false, out);
            out.push_str(", ");
            emit(expression, *max, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Lerp { from, to, amount } => {
            out.push_str("lerp(");
            emit(expression, *from, 0, false, out);
            out.push_str(", ");
            emit(expression, *to, 0, false, out);
            out.push_str(", ");
            emit(expression, *amount, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Sin { input } => {
            out.push_str("sin(");
            emit(expression, *input, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Vec2 { x, y } => {
            out.push_str("vec2(");
            emit(expression, *x, 0, false, out);
            out.push_str(", ");
            emit(expression, *y, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Vec3 { x, y, z } => {
            out.push_str("vec3(");
            emit(expression, *x, 0, false, out);
            out.push_str(", ");
            emit(expression, *y, 0, false, out);
            out.push_str(", ");
            emit(expression, *z, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Angle { degrees } => {
            out.push_str("angle(");
            emit(expression, *degrees, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Noise {
            seed,
            element,
            input,
        }
        | ExpressionNode::ContinuousNoise {
            seed,
            element,
            input,
        } => {
            out.push_str(if matches!(node, ExpressionNode::Noise { .. }) {
                "noise("
            } else {
                "continuous_noise("
            });
            emit_u32(out, *seed);
            out.push_str(", ");
            emit_u32(out, *element);
            out.push_str(", ");
            emit(expression, *input, 0, false, out);
            out.push(')');
        }
        ExpressionNode::Property {
            node,
            property,
            value_type,
        } => {
            out.push_str("property(");
            emit_node_ref(out, *node);
            out.push_str(", ");
            emit_uuid(out, property.as_uuid());
            out.push_str(", ");
            emit_string(out, value_type_name(*value_type));
            out.push(')');
        }
        ExpressionNode::PropertySample {
            node,
            property,
            value_type,
            lookback,
        } => {
            out.push_str("property_sample(");
            emit_node_ref(out, *node);
            out.push_str(", ");
            emit_uuid(out, property.as_uuid());
            out.push_str(", ");
            emit_string(out, value_type_name(*value_type));
            out.push_str(", ");
            emit(expression, *lookback, 0, false, out);
            out.push(')');
        }
        ExpressionNode::CurveSample {
            curve,
            offset,
            value_type,
        } => {
            out.push_str("curve(");
            emit_uuid(out, curve.as_uuid());
            out.push_str(", ");
            emit_offset(out, *offset);
            out.push_str(", ");
            emit_string(out, value_type_name(*value_type));
            out.push(')');
        }
        ExpressionNode::DataAssetCell {
            asset,
            column,
            row,
            value_type,
        } => {
            out.push_str("data_cell(");
            emit_uuid(out, asset.as_uuid());
            out.push_str(", ");
            emit_string(out, column);
            out.push_str(", ");
            emit(expression, *row, 0, false, out);
            out.push_str(", ");
            emit_string(out, value_type_name(*value_type));
            out.push(')');
        }
        ExpressionNode::AudioFeature {
            asset,
            feature,
            offset,
        } => match feature {
            AudioFeature::Rms | AudioFeature::Onset | AudioFeature::Beat => {
                out.push_str("audio_feature(");
                emit_uuid(out, asset.as_uuid());
                out.push_str(", ");
                emit_string(
                    out,
                    match feature {
                        AudioFeature::Rms => "rms",
                        AudioFeature::Onset => "onset",
                        AudioFeature::Beat => "beat",
                        AudioFeature::BandEnergy { .. } => unreachable!(),
                    },
                );
                out.push_str(", ");
                emit_offset(out, *offset);
                out.push(')');
            }
            AudioFeature::BandEnergy { band } => {
                out.push_str("audio_band(");
                emit_uuid(out, asset.as_uuid());
                out.push_str(", ");
                emit_u32(out, *band);
                out.push_str(", ");
                emit_offset(out, *offset);
                out.push(')');
            }
        },
    }
    if parens {
        out.push(')');
    }
}
fn emit_infix(
    expression: &Expression,
    left: u32,
    right: u32,
    operator: &str,
    precedence: u8,
    out: &mut String,
) {
    emit(expression, left, precedence, false, out);
    out.push(' ');
    out.push_str(operator);
    out.push(' ');
    emit(expression, right, precedence, true, out);
}

/// Format a canonical AST into the surface text. The AST's left/right
/// subtrees, postorder and literal types are preserved exactly: no operator
/// reordering, constant folding or constructor/literal replacement happens.
/// ASTs outside the syntax surface — invalid canonical shape or output that
/// would exceed the input limits — return a typed diagnostic instead of text.
pub fn format_expression(expression: &Expression) -> Result<String, ExpressionFormatError> {
    expression
        .validate()
        .map_err(ExpressionFormatError::Invalid)?;
    // Tree depth is bounded independently of the emitted text: a left-spine
    // tree formats flat, yet emitter recursion still follows the AST depth.
    let mut depths = vec![0usize; expression.nodes.len()];
    for (i, node) in expression.nodes.iter().enumerate() {
        depths[i] = node
            .operands()
            .iter()
            .map(|child| depths[*child as usize])
            .max()
            .unwrap_or(0)
            + 1;
    }
    if depths.last().copied().unwrap_or(0) > EXPRESSION_TEXT_MAX_DEPTH {
        return Err(ExpressionFormatError::OutputLimit("64 nesting depth"));
    }
    let mut out = String::new();
    emit(
        expression,
        expression.nodes.len() as u32 - 1,
        0,
        false,
        &mut out,
    );
    if out.len() > EXPRESSION_TEXT_MAX_BYTES {
        return Err(ExpressionFormatError::OutputLimit("64KiB input size"));
    }
    let tokens = lex(&out).expect("formatter output always lexes");
    if tokens.len() > EXPRESSION_TEXT_MAX_TOKENS {
        return Err(ExpressionFormatError::OutputLimit("8192 token"));
    }
    let mut depth = 0usize;
    let mut maximum = 0usize;
    for token in &tokens {
        match token.kind {
            TokenKind::LParen => {
                depth += 1;
                maximum = maximum.max(depth);
            }
            TokenKind::RParen => depth -= 1,
            _ => {}
        }
    }
    if maximum > EXPRESSION_TEXT_MAX_DEPTH {
        return Err(ExpressionFormatError::OutputLimit("64 nesting depth"));
    }
    Ok(out)
}
