//! Stateless opaque continuation, not an authorization credential.
use crate::ServiceError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    version: u32,
    operation: String,
    project: Uuid,
    pub revision: u64,
    binding: Value,
    pub after: Value,
    pub floor: Option<String>,
}
pub(crate) fn expired() -> ServiceError {
    ServiceError::new(
        "CURSOR_EXPIRED",
        "cursor snapshot or history is no longer retained",
    )
}
fn invalid() -> ServiceError {
    ServiceError::new("INVALID_CURSOR", "malformed or unsupported cursor")
}
impl Cursor {
    pub fn new(
        operation: &str,
        project: Uuid,
        revision: u64,
        binding: Value,
        after: Value,
        floor: Option<String>,
    ) -> Self {
        Self {
            version: 1,
            operation: operation.into(),
            project,
            revision,
            binding,
            after,
            floor,
        }
    }
    pub fn validate(
        &self,
        operation: &str,
        project: Uuid,
        binding: &Value,
    ) -> Result<(), ServiceError> {
        if self.operation != operation || self.project != project || self.binding != *binding {
            return Err(ServiceError::new(
                "CURSOR_MISMATCH",
                "cursor project, operation or query parameters differ",
            ));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<String, ServiceError> {
        let bytes = serde_json::to_vec(self)?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let token = format!("k1.{hex}.{:x}", Sha256::digest(&bytes));
        if token.len() > 65536 {
            return Err(ServiceError::invalid(
                "query parameters exceed cursor size limit",
            ));
        }
        Ok(token)
    }
    pub fn decode(token: &str) -> Result<Self, ServiceError> {
        if token.len() > 65536 {
            return Err(invalid());
        }
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 3 || parts[0] != "k1" || !parts[1].len().is_multiple_of(2) {
            return Err(invalid());
        }
        let bytes = parts[1]
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let value = std::str::from_utf8(pair).map_err(|_| invalid())?;
                u8::from_str_radix(value, 16).map_err(|_| invalid())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if format!("{:x}", Sha256::digest(&bytes)) != parts[2] {
            return Err(invalid());
        }
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if cursor.version != 1 {
            return Err(invalid());
        }
        Ok(cursor)
    }
}
