//! Content-addressed inline tables. Evaluation never resolves external locators.
use crate::{
    AssetId, DataTable, DocumentObject, Project, ProjectError, Value, expression_value_bytes,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EXPRESSION_DATA_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionDataAsset {
    pub id: AssetId,
    pub version: u32,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
    pub table: DataTable,
}
impl ExpressionDataAsset {
    pub fn new(id: AssetId, table: DataTable) -> Result<Self, ProjectError> {
        let mut data = Self {
            id,
            version: EXPRESSION_DATA_VERSION,
            content_hash: String::new(),
            table,
        };
        data.validate_table()?;
        data.content_hash = data.computed_hash()?;
        Ok(data)
    }
    fn validate_table(&self) -> Result<(), ProjectError> {
        let bad = || ProjectError::InvalidDocument("invalid expression DataAsset table".into());
        if self.version != EXPRESSION_DATA_VERSION {
            return Err(ProjectError::UnsupportedMeaning);
        }
        if self.table.columns.is_empty()
            || self.table.columns.len() > 64
            || self.table.rows.len() > 65536
        {
            return Err(bad());
        }
        let mut bytes = 0usize;
        for name in self.table.columns.keys() {
            if name.is_empty() || name.len() > 1024 {
                return Err(bad());
            }
            bytes = bytes.saturating_add(name.len() + 64);
        }
        for row in &self.table.rows {
            if row.len() != self.table.columns.len() {
                return Err(bad());
            }
            for (column, ty) in &self.table.columns {
                let value = row.get(column).ok_or_else(bad)?;
                if value.value_type() != *ty || matches!(value, Value::DataTable(_)) {
                    return Err(bad());
                }
                bytes = bytes.saturating_add(column.len() + expression_value_bytes(value) + 64);
                if bytes > 1_048_576 {
                    return Err(bad());
                }
            }
        }
        Ok(())
    }
    pub fn computed_hash(&self) -> Result<String, ProjectError> {
        self.validate_table()?;
        let bytes = serde_json::to_vec(&(self.version, &self.table))
            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.content_hash != self.computed_hash()? {
            return Err(ProjectError::InvalidDocument(
                "expression DataAsset hash mismatch".into(),
            ));
        }
        Ok(())
    }
}
impl Project {
    pub fn expression_data_inputs(&self) -> Result<Vec<ExpressionDataAsset>, ProjectError> {
        self.expression_data_assets
            .iter()
            .map(|d| match d {
                DocumentObject::Known(d) => {
                    d.validate()?;
                    Ok(d.clone())
                }
                DocumentObject::Opaque(_) => Err(ProjectError::UnsupportedMeaning),
            })
            .collect()
    }
}
