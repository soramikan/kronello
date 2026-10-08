//! Document media organization (ADR-0129). A `Bin` is authored project data
//! grouping asset ids; membership lives on the bin so one asset may belong to
//! several bins, and asset records stay free of organizational back-links.
use crate::{AssetId, BinId, ProjectError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bin {
    pub id: BinId,
    pub name: String,
    /// Insertion-ordered membership. The list behaves like an ordered set:
    /// membership edits assign each asset at most once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetId>,
}
impl Bin {
    pub fn new(id: BinId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            assets: Vec::new(),
        }
    }
    /// Field-level checks that do not need sibling document contents.
    /// Membership against `Project.assets` is enforced by
    /// [`crate::Project::validate_storage`].
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.name.trim().is_empty() {
            return Err(ProjectError::InvalidDocument(
                "bin name must not be empty".into(),
            ));
        }
        let mut members = std::collections::BTreeSet::new();
        for asset in &self.assets {
            if !members.insert(*asset) {
                return Err(ProjectError::InvalidDocument(
                    "bin membership must not contain duplicates".into(),
                ));
            }
        }
        Ok(())
    }
}
