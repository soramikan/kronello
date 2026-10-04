//! External immutable media references; all filesystem access belongs to backends.
use crate::AssetId;
use kronello_time::Rational;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Video,
    Audio,
    Image,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetLocator {
    pub relative: Option<String>,
    pub absolute: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamMetadata {
    pub index: u32,
    pub codec: String,
    pub time_base: Rational,
    pub duration: Option<Rational>,
    /// Absolute PTS origin. Missing legacy metadata means zero, never latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<Rational>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pixel_format: Option<String>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_matrix: Option<String>,
    pub color_range: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: AssetId,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
    pub kind: AssetKind,
    pub streams: Vec<StreamMetadata>,
    pub locator: AssetLocator,
}
impl Asset {
    pub fn validate(&self) -> Result<(), crate::ProjectError> {
        let valid_hash = self.content_hash.len() == 64
            && self
                .content_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let relative_valid = self.locator.relative.as_ref().is_none_or(|p| {
            !p.is_empty()
                && !p.starts_with('/')
                && !p.starts_with('\\')
                && !p.contains(':')
                && !p.split(['/', '\\']).any(|v| v == "..")
        });
        let absolute_valid = self.locator.absolute.as_ref().is_none_or(|p| {
            p.starts_with('/')
                || (p.len() > 2
                    && p.as_bytes()[1] == b':'
                    && matches!(p.as_bytes()[2], b'/' | b'\\'))
        });
        if !valid_hash
            || !relative_valid
            || !absolute_valid
            || (self.locator.relative.is_none() && self.locator.absolute.is_none())
            || self.streams.iter().any(|s| {
                s.time_base <= Rational::ZERO || s.duration.is_some_and(|d| d < Rational::ZERO)
            })
        {
            return Err(crate::ProjectError::InvalidDocument(
                "invalid asset hash, locator or time base".into(),
            ));
        }
        Ok(())
    }
}
