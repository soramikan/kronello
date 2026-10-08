use crate::ModelError;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! stable_id {
    ($name:ident) => {
        /// Stable UUID identity, never derived from a display name or array index.
        /// UUID v4 allows independent editing processes to allocate IDs without a
        /// central counter. Allocate once during editing and persist it; ID
        /// generation is not part of deterministic property evaluation.
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

stable_id!(PropertyId);
stable_id!(CurveId);
stable_id!(ExpressionId);
stable_id!(DescriptorId);
stable_id!(AssetId);
stable_id!(ModifierId);
stable_id!(CompositionId);
stable_id!(NodeId);
stable_id!(CompositionInstanceId);
stable_id!(ContentId);
stable_id!(SequenceId);
stable_id!(TrackId);
stable_id!(ClipId);
stable_id!(MarkerId);
stable_id!(CaptionId);
stable_id!(MaskId);

/// Immutable namespaced schema identity, such as `kronello.transform.opacity`.
/// It is supplied by the schema author, independently of a localized label.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct SchemaKey(String);

impl SchemaKey {
    pub fn new(key: impl Into<String>) -> Result<Self, ModelError> {
        let key = key.into();
        if key.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        }) {
            return Err(ModelError::InvalidSchemaKey { key });
        }
        Ok(Self(key))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for SchemaKey {
    type Error = ModelError;
    fn try_from(key: String) -> Result<Self, Self::Error> {
        Self::new(key)
    }
}
impl From<SchemaKey> for String {
    fn from(key: SchemaKey) -> Self {
        key.0
    }
}
impl fmt::Display for SchemaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
