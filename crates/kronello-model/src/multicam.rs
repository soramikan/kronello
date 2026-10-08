//! NLE-007 multicam groups (ADR-0127). A `MulticamAsset` is a named set of
//! camera `angles`; every angle points at one visual stream of a document
//! `Asset` plus a rational `sync_offset` that maps multicam-local source time
//! to that angle's media time (`media_time = multicam_time + sync_offset`).
//! Clips reference one group and one active angle through
//! `SourceRef::Multicam`; switching angles updates that reference only.
use kronello_time::Time;
use serde::{Deserialize, Serialize};

use crate::{AngleId, AssetId, MulticamId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MulticamAngle {
    /// Stable UUID identity; never an array index or display name.
    pub id: AngleId,
    /// Camera media. Resolved against `Project.assets` at validation and
    /// render time; a missing asset is a typed `SOURCE_MISSING` failure.
    pub asset: AssetId,
    /// Visual stream inside `asset` that carries this angle's picture.
    pub stream_index: u32,
    /// Angle media time minus multicam-local source time, in rational seconds.
    /// `media_time = multicam_time + sync_offset`; the sign convention is fixed
    /// so `clip.angle_switch` changes only which stream is sampled.
    pub sync_offset: Time,
    /// Display label; never used as identity.
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MulticamAsset {
    /// Stable UUID identity; never an array index or display name.
    pub id: MulticamId,
    /// Display label; never used as identity.
    pub name: String,
    /// Angles in display order. Angle order never affects synchronization;
    /// `multicam.create` modes that need a reference take an explicit
    /// `AngleId` or use the first authored angle.
    pub angles: Vec<MulticamAngle>,
}

impl MulticamAsset {
    pub fn angle(&self, id: AngleId) -> Option<&MulticamAngle> {
        self.angles.iter().find(|angle| angle.id == id)
    }
    /// Structural invariants independent of the owning document. Angle asset
    /// and stream existence is checked by `Sequence::validate`, render
    /// lowering, and `multicam.create` because it needs document context.
    pub fn validate(&self) -> Result<(), crate::ProjectError> {
        if self.name.is_empty() || self.name.len() > 256 {
            return Err(crate::ProjectError::InvalidDocument(
                "multicam name must contain 1..256 UTF-8 bytes".into(),
            ));
        }
        if self.angles.is_empty() {
            return Err(crate::ProjectError::InvalidDocument(
                "multicam requires at least one angle".into(),
            ));
        }
        if self.angles.len() > 64 {
            return Err(crate::ProjectError::InvalidDocument(
                "multicam angle budget exceeded".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::from([self.id.as_uuid()]);
        for angle in &self.angles {
            if angle.name.len() > 256 {
                return Err(crate::ProjectError::InvalidDocument(
                    "multicam angle name must fit in 256 UTF-8 bytes".into(),
                ));
            }
            if !ids.insert(angle.id.as_uuid()) {
                return Err(crate::ProjectError::InvalidDocument(
                    "duplicate multicam angle id".into(),
                ));
            }
            // Angle assets are document AssetIds, not fresh identities, and
            // several angles may read different streams of one file. Only
            // aliasing a multicam/angle identity is ambiguous.
            if ids.contains(&angle.asset.as_uuid()) {
                return Err(crate::ProjectError::InvalidDocument(
                    "multicam angle asset aliases multicam identity".into(),
                ));
            }
        }
        Ok(())
    }
}
