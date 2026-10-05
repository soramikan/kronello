//! Keep tagged numeric payloads on the JSON decoder. Serde's Content buffer
//! represents arbitrary-precision numbers as private maps, not f64 values.
//! RawValue also preserves nested duplicate fields for the concrete decoder.
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::value::RawValue;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Adjacent {
    pub kind: String,
    #[serde(default, deserialize_with = "present_value")]
    pub value: Option<Box<RawValue>>,
}

// An explicit null is a payload (e.g. Constant(None)), not an absent field.
fn present_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(d).map(Some)
}

impl Adjacent {
    pub fn value<T: DeserializeOwned, E: serde::de::Error>(&self) -> Result<T, E> {
        let value = self
            .value
            .as_ref()
            .ok_or_else(|| E::missing_field("value"))?;
        serde_json::from_str(value.get()).map_err(E::custom)
    }

    pub fn unit<T, E: serde::de::Error>(&self, variant: T) -> Result<T, E> {
        if self.value.is_some() {
            self.value::<(), E>()?;
        }
        Ok(variant)
    }

    pub fn unknown<E: serde::de::Error>(&self, variants: &'static [&'static str]) -> E {
        E::unknown_variant(&self.kind, variants)
    }
}
