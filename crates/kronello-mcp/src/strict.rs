//! Validate duplicate object members before any lossy Value conversion is used.
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeSet;
use std::fmt;

struct Unique;
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}
struct UniqueVisitor;
impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Unique;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON with unique object members")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Unique, M::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!("duplicate field {key}")));
            }
            map.next_value::<Unique>()?;
        }
        Ok(Unique)
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Unique, S::Error> {
        while seq.next_element::<Unique>()?.is_some() {}
        Ok(Unique)
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Unique, E> {
        Ok(Unique)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
        Ok(Unique)
    }
}
pub fn validate(message: &str) -> Result<(), String> {
    serde_json::from_str::<Unique>(message)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
