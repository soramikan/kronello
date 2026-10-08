//! Versioned public document envelope; opaque objects retain future content.
use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AnimationCurve, Asset, AudioAnalysisDataAsset, CaptionDocument, Composition, Expression,
    Sequence, Shape, TemplateDefinition, TemplateInstance, TextDocument,
};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_SEMANTIC_VERSION: u32 = 1;

/// The entire object is retained when its known interpretation cannot be decoded.
/// This includes nested unknown fields and unknown enum variants.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocumentObject<T> {
    Known(T),
    Opaque(OpaqueObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct OpaqueObject {
    pub id: Uuid,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Project {
    pub id: Uuid,
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub semantic_version: u32,
    pub name: String,
    pub compositions: Vec<DocumentObject<Composition>>,
    pub curves: Vec<DocumentObject<AnimationCurve>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<DocumentObject<Expression>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<DocumentObject<Shape>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<DocumentObject<TextDocument>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captions: Vec<DocumentObject<CaptionDocument>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<DocumentObject<TemplateDefinition>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_instances: Vec<DocumentObject<TemplateInstance>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<DocumentObject<Asset>>,
    /// Original → preview-proxy linkage (ADR-0119); never affects authored
    /// media identity or export resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxies: Vec<crate::ProxyLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_analyses: Vec<DocumentObject<AudioAnalysisDataAsset>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracking_data_assets: Vec<DocumentObject<crate::TrackingDataAsset>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_data_assets: Vec<DocumentObject<crate::ExpressionDataAsset>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequences: Vec<DocumentObject<Sequence>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mattes: Vec<DocumentObject<crate::MatteRelation>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repeaters: Vec<DocumentObject<crate::Repeater>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub simulations: Vec<DocumentObject<crate::ParticleSimulation>>,
    /// FLOW-002 media organization (ADR-0129): unordered bin collection keyed
    /// by `Bin.id`. An asset may appear in several bins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<crate::Bin>,
    /// FLOW-003 shared export presets (ADR-0130): versioned, destination-free
    /// `render.submit` settings stored inside the document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_presets: Vec<crate::ExportPreset>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("unsupported public schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("unknown meaning or opaque content cannot be edited safely")]
    UnsupportedMeaning,
    #[error("invalid document: {0}")]
    InvalidDocument(String),
}

impl Default for Project {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            schema_version: PROJECT_SCHEMA_VERSION,
            semantic_version: PROJECT_SEMANTIC_VERSION,
            name: String::new(),
            compositions: Vec::new(),
            curves: Vec::new(),
            expressions: Vec::new(),
            shapes: Vec::new(),
            texts: Vec::new(),
            captions: Vec::new(),
            templates: Vec::new(),
            template_instances: Vec::new(),
            assets: Vec::new(),
            proxies: Vec::new(),
            audio_analyses: Vec::new(),
            tracking_data_assets: Vec::new(),
            expression_data_assets: Vec::new(),
            sequences: Vec::new(),
            mattes: Vec::new(),
            repeaters: Vec::new(),
            simulations: Vec::new(),
            bins: Vec::new(),
            export_presets: Vec::new(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

impl Project {
    /// Structural compatibility permits lossless storage of opaque contents.
    pub fn validate_storage(&self) -> Result<(), ProjectError> {
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.unknown_fields.keys().any(|key| {
            matches!(
                key.as_str(),
                "id" | "schema_version"
                    | "semantic_version"
                    | "name"
                    | "compositions"
                    | "curves"
                    | "expressions"
                    | "shapes"
                    | "texts"
                    | "captions"
                    | "templates"
                    | "template_instances"
                    | "assets"
                    | "proxies"
                    | "audio_analyses"
                    | "tracking_data_assets"
                    | "expression_data_assets"
                    | "sequences"
                    | "mattes"
                    | "repeaters"
                    | "simulations"
                    | "bins"
                    | "export_presets"
            )
        }) {
            return Err(ProjectError::InvalidDocument(
                "extension shadows a known field".into(),
            ));
        }
        if self
            .compositions
            .iter()
            .filter_map(|object| match object {
                DocumentObject::Opaque(value) => Some(value),
                _ => None,
            })
            .chain(self.curves.iter().filter_map(|object| match object {
                DocumentObject::Opaque(value) => Some(value),
                _ => None,
            }))
            .chain(self.shapes.iter().filter_map(|object| match object {
                DocumentObject::Opaque(value) => Some(value),
                _ => None,
            }))
            .chain(self.texts.iter().filter_map(|object| match object {
                DocumentObject::Opaque(value) => Some(value),
                _ => None,
            }))
            .chain(self.captions.iter().filter_map(|object| match object {
                DocumentObject::Opaque(value) => Some(value),
                _ => None,
            }))
            .any(|object| object.fields.contains_key("id"))
        {
            return Err(ProjectError::InvalidDocument(
                "opaque extension shadows id".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::from([self.id]);
        for object in &self.compositions {
            if let DocumentObject::Known(c) = object
                && c.nodes.iter().any(|n| !crate::valid_node_tags(&n.tags))
            {
                return Err(ProjectError::InvalidDocument("invalid node tags".into()));
            }
            let id = match object {
                DocumentObject::Known(value) => value.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.curves {
            let id = match object {
                DocumentObject::Known(value) => value.id().as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.expressions {
            let (id, fields) = match object {
                DocumentObject::Known(value) => (value.id.as_uuid(), None),
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed expression id".into(),
                ));
            }
        }
        for object in &self.simulations {
            let id = match object {
                DocumentObject::Known(s) => s.id.as_uuid(),
                DocumentObject::Opaque(s) => {
                    if s.fields.contains_key("id") {
                        return Err(ProjectError::InvalidDocument(
                            "shadowed simulation id".into(),
                        ));
                    }
                    s.id
                }
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate simulation id".into(),
                ));
            }
        }
        for object in &self.repeaters {
            let (id, fields) = match object {
                DocumentObject::Known(value) => (value.id.as_uuid(), None),
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed repeater id".into(),
                ));
            }
        }
        for object in &self.mattes {
            let id = match object {
                DocumentObject::Known(value) => value.id,
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate matte id".into()));
            }
        }
        for object in &self.shapes {
            let id = match object {
                DocumentObject::Known(value) => value.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.texts {
            let id = match object {
                DocumentObject::Known(value) => value.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.captions {
            let id = match object {
                DocumentObject::Known(caption) => {
                    if caption.version == crate::CAPTION_VERSION {
                        caption
                            .validate()
                            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
                    }
                    caption.id.as_uuid()
                }
                DocumentObject::Opaque(value) => {
                    if value.fields.contains_key("id") {
                        return Err(ProjectError::InvalidDocument(
                            "opaque extension shadows id".into(),
                        ));
                    }
                    value.id
                }
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed caption id".into(),
                ));
            }
        }
        for object in &self.templates {
            let (id, fields) = match object {
                DocumentObject::Known(value) => (value.id, None),
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed template id".into(),
                ));
            }
        }
        for object in &self.template_instances {
            let (id, fields) = match object {
                DocumentObject::Known(value) => (value.id.as_uuid(), None),
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed template instance id".into(),
                ));
            }
        }
        for object in &self.assets {
            let id = match object {
                DocumentObject::Known(asset) => {
                    asset.validate()?;
                    asset.id.as_uuid()
                }
                DocumentObject::Opaque(value) => {
                    if value.fields.contains_key("id") {
                        return Err(ProjectError::InvalidDocument(
                            "opaque extension shadows id".into(),
                        ));
                    }
                    value.id
                }
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.expression_data_assets {
            let (id, fields) = match object {
                DocumentObject::Known(value) => {
                    if value.version == crate::EXPRESSION_DATA_VERSION {
                        value.validate()?;
                    }
                    (value.id.as_uuid(), None)
                }
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed expression data asset id".into(),
                ));
            }
        }
        for object in &self.audio_analyses {
            let (id, fields) = match object {
                DocumentObject::Known(value) => {
                    if value.config.version == crate::AUDIO_ANALYSIS_VERSION {
                        value.validate()?;
                    }
                    (value.id.as_uuid(), None)
                }
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed audio analysis id".into(),
                ));
            }
        }
        for object in &self.tracking_data_assets {
            let (id, fields) = match object {
                DocumentObject::Known(value) => {
                    if value.version == crate::TRACKING_VERSION {
                        value.validate()?;
                    }
                    (value.id.as_uuid(), None)
                }
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed tracking data asset id".into(),
                ));
            }
        }
        {
            // One link per original and per proxy asset; the proxy asset itself
            // is a normal `assets` entry, so its id already sits in `ids`.
            // Link/content agreement (`proxy_link_state`) is runtime state —
            // stale links report via proxy.status and fall back at decode;
            // they never invalidate the stored document.
            let mut originals = std::collections::BTreeSet::new();
            let mut proxies = std::collections::BTreeSet::new();
            for link in &self.proxies {
                link.validate()?;
                if !originals.insert(link.original) || !proxies.insert(link.proxy) {
                    return Err(ProjectError::InvalidDocument(
                        "duplicate proxy link member".into(),
                    ));
                }
            }
        }
        for object in &self.sequences {
            let id = match object {
                DocumentObject::Known(sequence) => {
                    sequence
                        .validate(self)
                        .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
                    for track in &sequence.tracks {
                        if !ids.insert(track.id.as_uuid()) {
                            return Err(ProjectError::InvalidDocument("duplicate track id".into()));
                        }
                        for clip in &track.clips {
                            if !ids.insert(clip.id.as_uuid()) {
                                return Err(ProjectError::InvalidDocument(
                                    "duplicate clip id".into(),
                                ));
                            }
                        }
                    }
                    sequence.id.as_uuid()
                }
                DocumentObject::Opaque(value) => {
                    if value.fields.contains_key("id") {
                        return Err(ProjectError::InvalidDocument(
                            "opaque extension shadows id".into(),
                        ));
                    }
                    value.id
                }
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        // Preserve existing scene validation policy while preventing timeline
        // IDs from aliasing a scene placement or node.
        let mut timeline_ids = std::collections::BTreeSet::new();
        for object in &self.sequences {
            match object {
                DocumentObject::Known(sequence) => {
                    timeline_ids.insert(sequence.id.as_uuid());
                    for track in &sequence.tracks {
                        timeline_ids.insert(track.id.as_uuid());
                        timeline_ids.extend(track.clips.iter().map(|clip| clip.id.as_uuid()));
                    }
                }
                DocumentObject::Opaque(value) => {
                    timeline_ids.insert(value.id);
                }
            }
        }
        for object in &self.compositions {
            if let DocumentObject::Known(c) = object {
                for node in &c.nodes {
                    if timeline_ids.contains(&node.id.as_uuid())
                        || matches!(&node.kind, crate::NodeKind::CompositionInstance(i) if timeline_ids.contains(&i.id.as_uuid()))
                    {
                        return Err(ProjectError::InvalidDocument(
                            "timeline id aliases scene identity".into(),
                        ));
                    }
                }
            }
        }
        // FLOW-002/003 (ADR-0129/0130): bins and export presets are unordered
        // member collections like the other document collections. Known and
        // opaque objects both contribute ids, so references may point at opaque
        // records that this build cannot interpret but must preserve.
        let asset_ids: std::collections::BTreeSet<uuid::Uuid> = self
            .assets
            .iter()
            .map(|object| match object {
                DocumentObject::Known(asset) => asset.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            })
            .collect();
        let composition_ids: std::collections::BTreeSet<uuid::Uuid> = self
            .compositions
            .iter()
            .map(|object| match object {
                DocumentObject::Known(c) => c.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            })
            .collect();
        let sequence_ids: std::collections::BTreeSet<uuid::Uuid> = self
            .sequences
            .iter()
            .map(|object| match object {
                DocumentObject::Known(s) => s.id.as_uuid(),
                DocumentObject::Opaque(value) => value.id,
            })
            .collect();
        for bin in &self.bins {
            bin.validate()?;
            if !ids.insert(bin.id.as_uuid()) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
            if bin
                .assets
                .iter()
                .any(|asset| !asset_ids.contains(&asset.as_uuid()))
            {
                return Err(ProjectError::InvalidDocument(
                    "bin membership references an unknown asset".into(),
                ));
            }
        }
        for preset in &self.export_presets {
            preset.validate()?;
            if !ids.insert(preset.id.as_uuid()) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
            let target_ok = match (preset.composition, preset.target) {
                (Some(composition), None) => composition_ids.contains(&composition.as_uuid()),
                (None, Some(crate::ExportTarget::Composition { composition })) => {
                    composition_ids.contains(&composition.as_uuid())
                }
                (None, Some(crate::ExportTarget::Sequence { sequence })) => {
                    sequence_ids.contains(&sequence.as_uuid())
                }
                _ => false,
            };
            if !target_ok {
                return Err(ProjectError::InvalidDocument(
                    "export preset references an unknown target".into(),
                ));
            }
            if let crate::ExportOutput::CaptionSidecar { sequence, .. } = &preset.output
                && !sequence_ids.contains(&sequence.as_uuid())
            {
                return Err(ProjectError::InvalidDocument(
                    "export preset references an unknown sequence".into(),
                ));
            }
            if preset
                .output
                .audio_clips()
                .iter()
                .any(|clip| !asset_ids.contains(&clip.asset.as_uuid()))
            {
                return Err(ProjectError::InvalidDocument(
                    "export preset audio clip references an unknown asset".into(),
                ));
            }
        }
        Ok(())
    }

    /// Conservative until SERVICE-001 can prove edits independent of opaque data.
    pub fn ensure_editable(&self) -> Result<(), ProjectError> {
        self.validate_storage()?;
        if self.semantic_version != PROJECT_SEMANTIC_VERSION
            || !self.unknown_fields.is_empty()
            || self
                .compositions
                .iter()
                .any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.compositions.iter().any(|v| matches!(v, DocumentObject::Known(c) if c.nodes.iter().flat_map(|n| &n.effects).any(|e| e.definition().is_err())))
            || self
                .curves
                .iter()
                .any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.expressions.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(e) if !matches!(e.version, 1..=crate::EXPRESSION_SUPPORTED_VERSION)))
            || self
                .shapes
                .iter()
                .any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.sequences.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.sequences.iter().any(|v| matches!(v, DocumentObject::Known(s) if s.tracks.iter().flat_map(|t| &t.clips).flat_map(|c| &c.effects).any(|e| e.definition().is_err())))
            || self.assets.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.templates.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.template_instances.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.audio_analyses.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(a) if a.config.version != crate::AUDIO_ANALYSIS_VERSION))
            || self.tracking_data_assets.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(a) if a.version != crate::TRACKING_VERSION))
            || self.expression_data_assets.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(a) if a.version != crate::EXPRESSION_DATA_VERSION))
            || self.simulations.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(s) if s.version != crate::SIMULATION_VERSION))
            || self.repeaters.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(r) if r.version != crate::REPEATER_VERSION))
            || self.mattes.iter().any(|v| matches!(v, DocumentObject::Opaque(_)) || matches!(v, DocumentObject::Known(m) if m.version != crate::DOCUMENT_MATTE_VERSION))
            || self.texts.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.texts.iter().any(|v| matches!(v, DocumentObject::Known(t) if !matches!(t.layout_version, 1 | 2)))
            || self.captions.iter().any(|v| matches!(v, DocumentObject::Opaque(_)))
            || self.captions.iter().any(|v| matches!(v, DocumentObject::Known(c) if c.version != crate::CAPTION_VERSION))
            || self.curves.iter().any(
                |v| matches!(v, DocumentObject::Known(c) if c.ensure_supported_version().is_err()),
            )
        {
            return Err(ProjectError::UnsupportedMeaning);
        }
        Ok(())
    }
}

/// Generated from the same Rust types used by import/export. Opaque alternatives
/// describe preservation, not permission to execute unknown content.
pub fn project_json_schema() -> schemars::Schema {
    schemars::schema_for!(Project)
}

// Avoid serde's untagged/flatten Content buffer: it cannot losslessly represent
// arbitrary-precision JSON numbers. Capture JSON directly, then interpret known
// objects through the original strict model decoders.
impl<'de, T: DeserializeOwned> Deserialize<'de> for DocumentObject<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw =
            serde_json::to_value(unique_fields(deserializer)?).map_err(serde::de::Error::custom)?;
        let encoded = serde_json::to_string(&raw).map_err(serde::de::Error::custom)?;
        if let Ok(known) = serde_json::from_str::<T>(&encoded) {
            return Ok(Self::Known(known));
        }
        let opaque =
            serde_json::from_str::<OpaqueObject>(&encoded).map_err(serde::de::Error::custom)?;
        Ok(Self::Opaque(opaque))
    }
}
impl<'de> Deserialize<'de> for OpaqueObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = unique_fields(deserializer)?;
        let id = take_field::<Uuid, D::Error>(&mut fields, "id")?;
        Ok(Self { id, fields })
    }
}
impl<'de> Deserialize<'de> for Project {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = unique_fields(deserializer)?;
        Ok(Self {
            id: take_field::<_, D::Error>(&mut fields, "id")?,
            schema_version: take_field::<_, D::Error>(&mut fields, "schema_version")?,
            semantic_version: take_field::<_, D::Error>(&mut fields, "semantic_version")?,
            name: take_field::<_, D::Error>(&mut fields, "name")?,
            compositions: take_field::<_, D::Error>(&mut fields, "compositions")?,
            curves: take_field::<_, D::Error>(&mut fields, "curves")?,
            expressions: if fields.contains_key("expressions") {
                take_field::<_, D::Error>(&mut fields, "expressions")?
            } else {
                Vec::new()
            },
            shapes: if fields.contains_key("shapes") {
                take_field::<_, D::Error>(&mut fields, "shapes")?
            } else {
                Vec::new()
            },
            texts: if fields.contains_key("texts") {
                take_field::<_, D::Error>(&mut fields, "texts")?
            } else {
                Vec::new()
            },
            captions: if fields.contains_key("captions") {
                take_field::<_, D::Error>(&mut fields, "captions")?
            } else {
                Vec::new()
            },
            templates: if fields.contains_key("templates") {
                take_field::<_, D::Error>(&mut fields, "templates")?
            } else {
                Vec::new()
            },
            template_instances: if fields.contains_key("template_instances") {
                take_field::<_, D::Error>(&mut fields, "template_instances")?
            } else {
                Vec::new()
            },
            expression_data_assets: if fields.contains_key("expression_data_assets") {
                take_field::<_, D::Error>(&mut fields, "expression_data_assets")?
            } else {
                Vec::new()
            },
            audio_analyses: if fields.contains_key("audio_analyses") {
                take_field::<_, D::Error>(&mut fields, "audio_analyses")?
            } else {
                Vec::new()
            },
            tracking_data_assets: if fields.contains_key("tracking_data_assets") {
                take_field::<_, D::Error>(&mut fields, "tracking_data_assets")?
            } else {
                Vec::new()
            },
            proxies: if fields.contains_key("proxies") {
                take_field::<_, D::Error>(&mut fields, "proxies")?
            } else {
                Vec::new()
            },
            assets: if fields.contains_key("assets") {
                take_field::<_, D::Error>(&mut fields, "assets")?
            } else {
                Vec::new()
            },
            sequences: if fields.contains_key("sequences") {
                take_field::<_, D::Error>(&mut fields, "sequences")?
            } else {
                Vec::new()
            },
            simulations: if fields.contains_key("simulations") {
                take_field::<_, D::Error>(&mut fields, "simulations")?
            } else {
                Vec::new()
            },
            repeaters: if fields.contains_key("repeaters") {
                take_field::<_, D::Error>(&mut fields, "repeaters")?
            } else {
                Vec::new()
            },
            mattes: if fields.contains_key("mattes") {
                take_field::<_, D::Error>(&mut fields, "mattes")?
            } else {
                Vec::new()
            },
            bins: if fields.contains_key("bins") {
                take_field::<_, D::Error>(&mut fields, "bins")?
            } else {
                Vec::new()
            },
            export_presets: if fields.contains_key("export_presets") {
                take_field::<_, D::Error>(&mut fields, "export_presets")?
            } else {
                Vec::new()
            },
            unknown_fields: fields,
        })
    }
}
pub(crate) fn take_field<T: DeserializeOwned, E: serde::de::Error>(
    fields: &mut BTreeMap<String, Value>,
    name: &'static str,
) -> Result<T, E> {
    let value = fields.remove(name).ok_or_else(|| E::missing_field(name))?;
    // Re-decoding JSON, rather than a serde Content/Value deserializer, also
    // retains f64 interpretation in existing adjacent-tagged Value variants.
    serde_json::from_str(&value.to_string()).map_err(E::custom)
}

pub(crate) fn unique_fields<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Value>, D::Error> {
    struct FieldsVisitor;
    impl<'de> serde::de::Visitor<'de> for FieldsVisitor {
        type Value = BTreeMap<String, Value>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a JSON object with unique field names")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut fields = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, Value>()? {
                if fields.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate field: {key}")));
                }
            }
            Ok(fields)
        }
    }
    deserializer.deserialize_map(FieldsVisitor)
}
