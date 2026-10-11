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

/// Decode candidates retain the whole object on failure, including nested
/// unknown fields and enum variants. Storage validation rejects known-invalid
/// candidates; only unknown content may remain opaque in a stored project.
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
    /// AI-002 (ADR-0125): deterministic cut-detection results, locked to the
    /// analyzed source asset like tracking data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scene_boundary_assets: Vec<DocumentObject<crate::SceneBoundaryAsset>>,
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
    /// NLE-007 multicam groups (ADR-0127); plain entries, never opaque.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub multicams: Vec<crate::MulticamAsset>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("unsupported public schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("unknown meaning or opaque content cannot be edited safely")]
    UnsupportedMeaning,
    #[error("invalid document object: {0}")]
    InvalidObject(ObjectDiagnostic),
    #[error("unsupported document objects: {0:?}")]
    UnsupportedObjects(Vec<ObjectDiagnostic>),
    #[error("invalid document: {0}")]
    InvalidDocument(String),
}

/// A stable object address and its decoding or compatibility failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectDiagnostic {
    pub collection: String,
    pub id: Uuid,
    pub reason: String,
}
impl std::fmt::Display for ObjectDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}: {}", self.collection, self.id, self.reason)
    }
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
            scene_boundary_assets: Vec::new(),
            expression_data_assets: Vec::new(),
            sequences: Vec::new(),
            mattes: Vec::new(),
            repeaters: Vec::new(),
            simulations: Vec::new(),
            bins: Vec::new(),
            export_presets: Vec::new(),
            multicams: Vec::new(),
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
        validate_objects("compositions", &self.compositions)?;
        validate_objects("curves", &self.curves)?;
        validate_objects("expressions", &self.expressions)?;
        validate_objects("shapes", &self.shapes)?;
        validate_objects("texts", &self.texts)?;
        validate_objects("captions", &self.captions)?;
        validate_objects("templates", &self.templates)?;
        validate_objects("template_instances", &self.template_instances)?;
        validate_objects("assets", &self.assets)?;
        validate_objects("audio_analyses", &self.audio_analyses)?;
        validate_objects("tracking_data_assets", &self.tracking_data_assets)?;
        validate_objects("scene_boundary_assets", &self.scene_boundary_assets)?;
        validate_objects("expression_data_assets", &self.expression_data_assets)?;
        validate_objects("sequences", &self.sequences)?;
        validate_objects("mattes", &self.mattes)?;
        validate_objects("repeaters", &self.repeaters)?;
        validate_objects("simulations", &self.simulations)?;
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
                    | "scene_boundary_assets"
                    | "expression_data_assets"
                    | "sequences"
                    | "mattes"
                    | "repeaters"
                    | "simulations"
                    | "bins"
                    | "export_presets"
                    | "multicams"
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
                return Err(invalid_object(
                    "compositions",
                    c.id.as_uuid(),
                    "invalid node tags",
                ));
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
                DocumentObject::Known(value) => {
                    value.validate_structure().map_err(|e| {
                        ProjectError::InvalidObject(ObjectDiagnostic {
                            collection: "shapes".into(),
                            id: value.id.as_uuid(),
                            reason: e.to_string(),
                        })
                    })?;
                    value.id.as_uuid()
                }
                DocumentObject::Opaque(value) => value.id,
            };
            if !ids.insert(id) {
                return Err(ProjectError::InvalidDocument("duplicate object id".into()));
            }
        }
        for object in &self.texts {
            let id = match object {
                DocumentObject::Known(value) => {
                    value.validate_structure().map_err(|e| {
                        ProjectError::InvalidObject(ObjectDiagnostic {
                            collection: "texts".into(),
                            id: value.id.as_uuid(),
                            reason: e.to_string(),
                        })
                    })?;
                    value.id.as_uuid()
                }
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
                            .map_err(|e| invalid_object("captions", caption.id.as_uuid(), e))?;
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
                    asset
                        .validate()
                        .map_err(|e| invalid_object("assets", asset.id.as_uuid(), e))?;
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
                        value.validate().map_err(|e| {
                            invalid_object("expression_data_assets", value.id.as_uuid(), e)
                        })?;
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
                    if value.config.supported_version() {
                        value
                            .validate()
                            .map_err(|e| invalid_object("audio_analyses", value.id.as_uuid(), e))?;
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
                        value.validate().map_err(|e| {
                            invalid_object("tracking_data_assets", value.id.as_uuid(), e)
                        })?;
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
        for object in &self.scene_boundary_assets {
            let (id, fields) = match object {
                DocumentObject::Known(value) => {
                    if value.version == crate::SCENE_BOUNDARY_VERSION {
                        value.validate().map_err(|e| {
                            invalid_object("scene_boundary_assets", value.id.as_uuid(), e)
                        })?;
                    }
                    (value.id.as_uuid(), None)
                }
                DocumentObject::Opaque(value) => (value.id, Some(&value.fields)),
            };
            if !ids.insert(id) || fields.is_some_and(|f| f.contains_key("id")) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate or shadowed scene boundary asset id".into(),
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
        for multicam in &self.multicams {
            multicam.validate()?;
            if !ids.insert(multicam.id.as_uuid()) {
                return Err(ProjectError::InvalidDocument(
                    "duplicate multicam id".into(),
                ));
            }
            for angle in &multicam.angles {
                // Angle ids are stable UUID identity; they must not alias any
                // other document object id.
                if !ids.insert(angle.id.as_uuid()) {
                    return Err(ProjectError::InvalidDocument(
                        "duplicate multicam angle id".into(),
                    ));
                }
            }
        }
        for object in &self.sequences {
            let id = match object {
                DocumentObject::Known(sequence) => {
                    sequence
                        .validate(self)
                        .map_err(|e| invalid_object("sequences", sequence.id.as_uuid(), e))?;
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
        let mut diagnostics = Vec::new();
        if self.semantic_version != PROJECT_SEMANTIC_VERSION {
            diagnostics.push(ObjectDiagnostic {
                collection: "project".into(),
                id: self.id,
                reason: format!("unsupported semantic_version: {}", self.semantic_version),
            });
        }
        if !self.unknown_fields.is_empty() {
            diagnostics.push(ObjectDiagnostic {
                collection: "project".into(),
                id: self.id,
                reason: format!(
                    "unknown fields: {:?}",
                    self.unknown_fields.keys().collect::<Vec<_>>()
                ),
            });
        }
        collect_unsupported(
            "compositions",
            &self.compositions,
            |v| v.id.as_uuid(),
            |v| {
                v.nodes
                    .iter()
                    .flat_map(|n| &n.effects)
                    .find_map(|e| e.definition().err().map(|e| e.to_string()))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "curves",
            &self.curves,
            |v| v.id().as_uuid(),
            |v| v.ensure_supported_version().err().map(|e| e.to_string()),
            &mut diagnostics,
        );
        collect_unsupported(
            "expressions",
            &self.expressions,
            |v| v.id.as_uuid(),
            |v| {
                (!matches!(v.version, 1..=crate::EXPRESSION_SUPPORTED_VERSION))
                    .then(|| format!("unsupported expression version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "shapes",
            &self.shapes,
            |v| v.id.as_uuid(),
            |_| None,
            &mut diagnostics,
        );
        collect_unsupported(
            "texts",
            &self.texts,
            |v| v.id.as_uuid(),
            |v| {
                (!matches!(v.layout_version, 1 | 2))
                    .then(|| format!("unsupported layout_version: {}", v.layout_version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "captions",
            &self.captions,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::CAPTION_VERSION)
                    .then(|| format!("unsupported caption version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "templates",
            &self.templates,
            |v| v.id,
            |_| None,
            &mut diagnostics,
        );
        collect_unsupported(
            "template_instances",
            &self.template_instances,
            |v| v.id.as_uuid(),
            |_| None,
            &mut diagnostics,
        );
        collect_unsupported(
            "assets",
            &self.assets,
            |v| v.id.as_uuid(),
            |_| None,
            &mut diagnostics,
        );
        collect_unsupported(
            "audio_analyses",
            &self.audio_analyses,
            |v| v.id.as_uuid(),
            |v| {
                (!v.config.supported_version()).then(|| "unsupported audio analysis version".into())
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "tracking_data_assets",
            &self.tracking_data_assets,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::TRACKING_VERSION)
                    .then(|| format!("unsupported version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "scene_boundary_assets",
            &self.scene_boundary_assets,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::SCENE_BOUNDARY_VERSION)
                    .then(|| format!("unsupported version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "expression_data_assets",
            &self.expression_data_assets,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::EXPRESSION_DATA_VERSION)
                    .then(|| format!("unsupported version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "sequences",
            &self.sequences,
            |v| v.id.as_uuid(),
            |v| {
                v.tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .flat_map(|c| &c.effects)
                    .find_map(|e| e.definition().err().map(|e| e.to_string()))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "mattes",
            &self.mattes,
            |v| v.id,
            |v| {
                (v.version != crate::DOCUMENT_MATTE_VERSION)
                    .then(|| format!("unsupported matte version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "repeaters",
            &self.repeaters,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::REPEATER_VERSION)
                    .then(|| format!("unsupported version: {}", v.version))
            },
            &mut diagnostics,
        );
        collect_unsupported(
            "simulations",
            &self.simulations,
            |v| v.id.as_uuid(),
            |v| {
                (v.version != crate::SIMULATION_VERSION)
                    .then(|| format!("unsupported version: {}", v.version))
            },
            &mut diagnostics,
        );
        if !diagnostics.is_empty() {
            return Err(ProjectError::UnsupportedObjects(diagnostics));
        }
        Ok(())
    }
}

fn invalid_object(collection: &str, id: Uuid, reason: impl std::fmt::Display) -> ProjectError {
    ProjectError::InvalidObject(ObjectDiagnostic {
        collection: collection.into(),
        id,
        reason: reason.to_string(),
    })
}

// Classify extensions using the public type schema, not decoder error wording.
// This only detects unknown structure; missing fields, types and ranges remain
// the strict Rust decoder's responsibility.
fn has_unknown_content(raw: &Value, schema: &Value, root: &Value) -> bool {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p))
    {
        return has_unknown_content(raw, target, root);
    }
    for union in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(union).and_then(Value::as_array) {
            // Select tagged alternatives before inspecting their payload fields.
            let compatible: Vec<_> = branches
                .iter()
                .filter(|branch| {
                    if let Some(properties) = branch.get("properties").and_then(Value::as_object) {
                        return properties.iter().all(|(key, constraint)| {
                            raw.get(key).is_none_or(|value| {
                                constraint.get("const").is_none_or(|c| c == value)
                                    && constraint
                                        .get("enum")
                                        .and_then(Value::as_array)
                                        .is_none_or(|values| values.contains(value))
                            })
                        });
                    }
                    match branch.get("type").and_then(Value::as_str) {
                        Some("null") => raw.is_null(),
                        Some("object") => raw.is_object(),
                        Some("array") => raw.is_array(),
                        _ => true,
                    }
                })
                .collect();
            if compatible.is_empty() {
                return raw.as_object().is_some_and(|object| {
                    object.iter().any(|(key, value)| {
                        value.is_string()
                            && branches.iter().any(|branch| {
                                branch
                                    .get("properties")
                                    .and_then(|p| p.get(key))
                                    .is_some_and(|p| {
                                        p.get("const").is_some() || p.get("enum").is_some()
                                    })
                            })
                    })
                }); // Unknown string tag, not a malformed tag type.
            }
            return compatible.iter().all(|s| has_unknown_content(raw, s, root));
        }
    }
    if raw.is_string()
        && let Some(values) = schema.get("enum").and_then(Value::as_array)
        && !values.contains(raw)
    {
        return true;
    }
    if let Some(object) = raw.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, value) in object {
            if let Some(child) = properties.and_then(|p| p.get(key)) {
                if has_unknown_content(value, child, root) {
                    return true;
                }
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return true;
            } else if let Some(child) = schema.get("additionalProperties").filter(|s| s.is_object())
                && has_unknown_content(value, child, root)
            {
                return true;
            }
        }
    }
    if let Some(items) = raw.as_array()
        && let Some(item_schema) = schema.get("items")
    {
        return items
            .iter()
            .any(|item| has_unknown_content(item, item_schema, root));
    }
    false
}

fn unsupported_version(collection: &str, raw: &Value) -> Option<String> {
    let (field, supported): (&str, &[u64]) = match collection {
        "curves" => ("interpolation_version", &[1]),
        "texts" => ("layout_version", &[1, 2]),
        "expressions" => ("version", &[1, 2, 3]),
        "captions"
        | "tracking_data_assets"
        | "scene_boundary_assets"
        | "expression_data_assets"
        | "simulations"
        | "repeaters"
        | "mattes" => ("version", &[1]),
        "audio_analyses" => {
            return unsupported_version("audio_analysis_config", &raw["config"]);
        }
        "audio_analysis_config" => ("version", &[1, 2]),
        _ => return None,
    };
    raw.get(field)
        .and_then(Value::as_u64)
        .filter(|v| !supported.contains(v))
        .map(|v| format!("unsupported {field}: {v}"))
}

fn validate_objects<T: DeserializeOwned + JsonSchema>(
    collection: &str,
    objects: &[DocumentObject<T>],
) -> Result<(), ProjectError> {
    // Generate the schema only when the strict decoder failed.
    let mut schema = None;
    for object in objects {
        let DocumentObject::Opaque(opaque) = object else {
            continue;
        };
        let raw = serde_json::to_value(opaque)
            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
        if unsupported_version(collection, &raw).is_some() {
            continue;
        }
        let schema = schema.get_or_insert_with(|| {
            serde_json::to_value(schemars::schema_for!(T)).expect("serializable model schema")
        });
        if has_unknown_content(&raw, schema, schema) {
            continue;
        }
        let encoded = serde_json::to_string(&raw)
            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
        // Even explicitly constructed Opaque objects cannot bypass validation.
        if let Err(error) = serde_json::from_str::<T>(&encoded) {
            return Err(ProjectError::InvalidObject(ObjectDiagnostic {
                collection: collection.into(),
                id: opaque.id,
                reason: error.to_string(),
            }));
        }
    }
    Ok(())
}

fn collect_unsupported<T: Serialize + DeserializeOwned>(
    collection: &str,
    objects: &[DocumentObject<T>],
    known_id: impl Fn(&T) -> Uuid,
    known_reason: impl Fn(&T) -> Option<String>,
    diagnostics: &mut Vec<ObjectDiagnostic>,
) {
    for object in objects {
        let (id, reason) = match object {
            DocumentObject::Known(value) => (known_id(value), known_reason(value)),
            DocumentObject::Opaque(value) => {
                let raw = serde_json::to_value(value).expect("serializable opaque object");
                let reason = unsupported_version(collection, &raw).unwrap_or_else(|| {
                    let encoded = serde_json::to_string(&raw).expect("serializable opaque object");
                    serde_json::from_str::<T>(&encoded)
                        .err()
                        .map_or_else(|| "opaque object".into(), |e| e.to_string())
                });
                (value.id, Some(reason))
            }
        };
        if let Some(reason) = reason {
            diagnostics.push(ObjectDiagnostic {
                collection: collection.into(),
                id,
                reason,
            });
        }
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
            scene_boundary_assets: if fields.contains_key("scene_boundary_assets") {
                take_field::<_, D::Error>(&mut fields, "scene_boundary_assets")?
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
            multicams: if fields.contains_key("multicams") {
                take_field::<_, D::Error>(&mut fields, "multicams")?
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
