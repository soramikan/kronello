//! Decode tagged JSON without serde Content buffering: arbitrary_precision
//! numbers must reach the concrete f64 fields through the JSON deserializer.
//! Raw payloads also preserve duplicate keys for strict request deserialization.
use std::collections::BTreeMap;

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error, MapAccess, Visitor},
};
use serde_json::value::RawValue;

use crate::{Request, Response, ResultData};

type Fields = BTreeMap<String, Box<RawValue>>;
fn fields<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Fields, D::Error> {
    struct Unique;
    impl<'de> Visitor<'de> for Unique {
        type Value = Fields;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a JSON object with unique field names")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Fields, M::Error> {
            let mut fields = Fields::new();
            while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                if fields.insert(key.clone(), value).is_some() {
                    return Err(M::Error::custom(format!("duplicate field: {key}")));
                }
            }
            Ok(fields)
        }
    }
    deserializer.deserialize_map(Unique)
}
fn take<T: DeserializeOwned, E: Error>(fields: &mut Fields, key: &'static str) -> Result<T, E> {
    let value = fields.remove(key).ok_or_else(|| E::missing_field(key))?;
    serde_json::from_str(value.get()).map_err(E::custom)
}
fn payload<T: DeserializeOwned, E: Error>(fields: &Fields) -> Result<T, E> {
    let json = serde_json::to_string(fields).map_err(E::custom)?;
    serde_json::from_str(&json).map_err(E::custom)
}
fn exhausted<E: Error>(fields: &Fields) -> Result<(), E> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(E::custom("unknown envelope field"))
    }
}
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "operation")?;
        match tag.as_str() {
            "project.create" => payload(&fields).map(Self::ProjectCreate),
            "project.import" => payload(&fields).map(Self::ProjectImport),
            "project.export" => payload(&fields).map(Self::ProjectExport),
            "project.info" => payload(&fields).map(Self::ProjectInfo),
            "render.frame" => payload(&fields).map(Self::RenderFrame),
            "render.sequence" => payload(&fields).map(Self::RenderSequence),
            "edit.plan" => payload(&fields).map(Self::EditPlan),
            "edit.apply" => payload(&fields).map(Self::EditApply),
            "edit.undo" => payload(&fields).map(Self::EditUndo),
            "history.list" => payload(&fields).map(Self::HistoryList),
            "scene.query" => payload(&fields).map(Self::SceneQuery),
            "property.sample" => payload(&fields).map(Self::PropertySample),
            "capabilities.get" => payload(&fields).map(Self::CapabilitiesGet),
            _ => Err(D::Error::custom("unknown operation")),
        }
    }
}
impl<'de> Deserialize<'de> for ResultData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "kind")?;
        let result = match tag.as_str() {
            "project" => Self::Project(take(&mut fields, "value")?),
            "export" => Self::Export(take(&mut fields, "value")?),
            "frame" => Self::Frame(take(&mut fields, "value")?),
            "sequence" => Self::Sequence(take(&mut fields, "value")?),
            "plan" => Self::Plan(take(&mut fields, "value")?),
            "edit" => Self::Edit(take(&mut fields, "value")?),
            "history" => Self::History(take(&mut fields, "value")?),
            "scene" => Self::Scene(take(&mut fields, "value")?),
            "samples" => Self::Samples(take(&mut fields, "value")?),
            "capabilities" => Self::Capabilities(take(&mut fields, "value")?),
            _ => return Err(D::Error::custom("unknown result kind")),
        };
        exhausted::<D::Error>(&fields)?;
        Ok(result)
    }
}
impl<'de> Deserialize<'de> for Response {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "status")?;
        let result = match tag.as_str() {
            "success" => Self::Success {
                result: take(&mut fields, "result")?,
            },
            "error" => Self::Error {
                error: take(&mut fields, "error")?,
            },
            _ => return Err(D::Error::custom("unknown response status")),
        };
        exhausted::<D::Error>(&fields)?;
        Ok(result)
    }
}
