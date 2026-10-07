use std::collections::{BTreeMap, BTreeSet};

use kronello_eval::{DependencyGraph, EvaluationSnapshot, ReferenceBindings};
use kronello_model::*;
use kronello_text::{FontData, LayoutResult};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::RenderError;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const COLOR_VERSION: &str = "gpu002-color-v1";
pub const VECTOR_VERSION: &str = "render001-kurbo-flatten-v1";
pub const COVERAGE_VERSION: &str = "vec003-grid4-v2";
pub const STROKE_GEOMETRY_VERSION: &str = EXTENDED_STROKE_VERSION;
pub const GRADIENT_INTERPOLATION_VERSION: &str = "vec004-explicit-interpolation-v1";
pub const LAYOUT_BOUNDS_VERSION: u32 = 1;
pub const NODE_VISIBILITY_VERSION: u32 = 2;
fn legacy_visibility_version() -> u32 {
    1
}
pub const COMPOSITION_MEDIA_VERSION: u32 = 1;
pub const TEMPORAL_VERSION: u32 = 1;
pub const VIDEO_INPUT_VERSION: &str = "nle002-sdr-rgba8-nearest-v1";
fn initial_video_version() -> String {
    VIDEO_INPUT_VERSION.into()
}
fn initial_bounds_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticVersions {
    pub document: u32,
    #[serde(default = "legacy_visibility_version")]
    pub visibility: u32,
    #[serde(default = "expression_version")]
    pub expression: u32,
    pub interpolation: u32,
    pub time_map: u32,
    pub layout: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced_text: Option<u32>,
    /// Absent legacy snapshots use the initial bounds contract, never latest.
    #[serde(default = "initial_bounds_version")]
    pub bounds: u32,
    pub vector: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_operations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_matte: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeater: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_sampling: Option<u32>,
    pub color: String,
    pub coverage: String,
    pub stroke_geometry: String,
    pub gradient_interpolation: String,
    pub effects: BTreeMap<String, u32>,
    #[serde(default = "generator_versions")]
    pub generators: BTreeMap<String, u32>,
    #[serde(default = "initial_video_version")]
    pub video_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_media: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<u32>,
}
fn generator_versions() -> BTreeMap<String, u32> {
    BTreeMap::from([(SOLID_GENERATOR_ID.into(), 1)])
}
fn expression_version() -> u32 {
    EXPRESSION_VERSION
}
impl SemanticVersions {
    /// Pins explicitly at snapshot creation, never at execution or resume.
    pub fn current(document: u32) -> Self {
        Self {
            document,
            visibility: NODE_VISIBILITY_VERSION,
            expression: EXPRESSION_SUPPORTED_VERSION,
            interpolation: INTERPOLATION_VERSION,
            time_map: 1,
            layout: TEXT_LAYOUT_VERSION,
            advanced_text: Some(TEXT_ADVANCED_LAYOUT_VERSION),
            bounds: LAYOUT_BOUNDS_VERSION,
            vector: VECTOR_VERSION.into(),
            path_operations: Some(1),
            document_matte: Some(DOCUMENT_MATTE_VERSION),
            blend: Some(BLEND_VERSION),
            repeater: Some(kronello_model::REPEATER_VERSION),
            simulation: Some(kronello_model::SIMULATION_VERSION),
            reverse_sampling: Some(1),
            color: COLOR_VERSION.into(),
            coverage: COVERAGE_VERSION.into(),
            stroke_geometry: STROKE_GEOMETRY_VERSION.into(),
            gradient_interpolation: GRADIENT_INTERPOLATION_VERSION.into(),
            effects: BTreeMap::from([
                (GAUSSIAN_BLUR_ID.into(), AFFINE_EFFECT_VERSION),
                (DROP_SHADOW_ID.into(), AFFINE_EFFECT_VERSION),
                (COLOR_EXPOSURE_ID.into(), COLOR_EFFECT_VERSION),
                (COLOR_LEVELS_ID.into(), COLOR_EFFECT_VERSION),
                (COLOR_CURVES_ID.into(), COLOR_EFFECT_VERSION),
                (COLOR_HSL_ID.into(), COLOR_EFFECT_VERSION),
            ]),
            generators: generator_versions(),
            video_input: initial_video_version(),
            temporal: Some(TEMPORAL_VERSION),
            composition_media: Some(COMPOSITION_MEDIA_VERSION),
            hdr: Some(crate::HDR_VERSION),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderProfile {
    pub working_space: ColorSpace,
    pub flatten_tolerance_px: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<crate::TemporalSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<crate::HdrSettings>,
}
impl Default for RenderProfile {
    fn default() -> Self {
        Self {
            working_space: ColorSpace::LinearRec709,
            flatten_tolerance_px: 0.02,
            temporal: None,
            hdr: None,
        }
    }
}

/// No device handles, latest-document lookup, implicit fonts, or mutable state.
/// The complete Project (including independent opaque data) participates in the
/// identity even though only the selected dependency closure is executable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSnapshot {
    schema_version: u32,
    project: Project,
    composition: CompositionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sequence: Option<SequenceId>,
    revision: u64,
    semantic_versions: SemanticVersions,
    profile: RenderProfile,
    mattes: Vec<MatteBinding>,
    font_locks: Vec<FontRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatteKind {
    Alpha,
    Luminance,
    AlphaInverted,
    LuminanceInverted,
}

/// Explicit render inputs until a document-level matte model is implemented.
/// A consumed matte is removed from display roots/children, unless visible is
/// true. Keys use stable instance paths, never document-array positions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatteBinding {
    pub source: SceneKey,
    pub matte: SceneKey,
    pub kind: MatteKind,
    pub visible: bool,
}
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SceneKey {
    pub instance_path: InstancePath,
    pub node: NodeId,
}
impl From<&kronello_eval::NodeKey> for SceneKey {
    fn from(key: &kronello_eval::NodeKey) -> Self {
        Self {
            instance_path: key.instance_path.clone(),
            node: key.node,
        }
    }
}

impl RenderSnapshot {
    pub fn new(
        project: &Project,
        composition: CompositionId,
        revision: u64,
        profile: RenderProfile,
    ) -> Result<Self, RenderError> {
        Self::with_contract(
            project,
            composition,
            revision,
            profile,
            SemanticVersions::current(project.semantic_version),
            vec![],
        )
    }
    pub fn for_target(
        project: &Project,
        target: crate::RenderTarget,
        revision: u64,
        mut profile: RenderProfile,
    ) -> Result<Self, RenderError> {
        match target {
            crate::RenderTarget::Composition { composition } => {
                Self::new(project, composition, revision, profile)
            }
            crate::RenderTarget::Sequence { sequence } => {
                let root = crate::sequence::lower_sequence(project, sequence)?;
                let source = project
                    .sequences
                    .iter()
                    .find_map(|s| match s {
                        DocumentObject::Known(s) if s.id == sequence => Some(s),
                        _ => None,
                    })
                    .unwrap();
                profile.working_space = source.working_space;
                let mut value = Self {
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                    project: project.clone(),
                    composition: root.id,
                    sequence: Some(sequence),
                    revision,
                    semantic_versions: SemanticVersions::current(project.semantic_version),
                    profile,
                    mattes: vec![],
                    font_locks: vec![],
                };
                value.validate()?;
                let definitions = value.definitions()?;
                let mut locks = BTreeSet::new();
                for c in definitions {
                    for n in c.nodes {
                        if let NodeKind::Text { content_ref } = n.kind {
                            let text =
                                content(&project.texts, content_ref.as_uuid(), |t| t.id.as_uuid())?
                                    .ok_or(TextError::MissingContent { id: content_ref })?;
                            locks.extend(text.styles.iter().map(|s| s.font.clone()));
                        }
                    }
                }
                // Caption cue fonts, including clip-level overrides and span
                // faces, are locked by hash exactly like text node fonts.
                // Disabled cues render nothing, so they need no font locks
                // and a missing caption behind one must not fail the render.
                let registry = render_registry();
                for clip in source
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .filter(|c| c.enabled)
                {
                    let SourceRef::Caption { caption } = &clip.source_ref else {
                        continue;
                    };
                    let document =
                        content(&project.captions, caption.as_uuid(), |c| c.id.as_uuid())?
                            .ok_or(CaptionError::MissingContent { id: *caption })?;
                    locks.extend(document.resolve(&clip.properties, &registry)?.fonts());
                }
                value.font_locks = locks.into_iter().collect();
                Ok(value)
            }
        }
    }
    pub fn target(&self) -> crate::RenderTarget {
        match self.sequence {
            Some(sequence) => crate::RenderTarget::Sequence { sequence },
            None => self.composition.into(),
        }
    }
    pub fn with_contract(
        project: &Project,
        composition: CompositionId,
        revision: u64,
        profile: RenderProfile,
        semantic_versions: SemanticVersions,
        mattes: Vec<MatteBinding>,
    ) -> Result<Self, RenderError> {
        let mut snapshot = Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            project: project.clone(),
            composition,
            sequence: None,
            revision,
            semantic_versions,
            profile,
            mattes,
            font_locks: vec![],
        };
        snapshot.validate()?;
        let definitions = snapshot.definitions()?;
        let text_ids: BTreeSet<_> = definitions
            .iter()
            .flat_map(|c| c.nodes.iter())
            .filter_map(|n| match n.kind {
                NodeKind::Text { content_ref } => Some(content_ref),
                _ => None,
            })
            .collect();
        let mut locks = BTreeSet::new();
        for id in text_ids {
            let text = content(&project.texts, id.as_uuid(), |t| t.id.as_uuid())?
                .ok_or(TextError::MissingContent { id })?;
            locks.extend(text.styles.iter().map(|s| s.font.clone()));
        }
        snapshot.font_locks = locks.into_iter().collect();
        Ok(snapshot)
    }
    pub fn profile(&self) -> RenderProfile {
        self.profile
    }
    /// Explicit transient matte inputs; does not alter the saved Project.
    pub fn with_mattes(mut self, mattes: Vec<MatteBinding>) -> Self {
        self.mattes = mattes;
        self
    }
    pub fn composition(&self) -> CompositionId {
        self.composition
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn semantic_versions(&self) -> &SemanticVersions {
        &self.semantic_versions
    }
    pub fn font_locks(&self) -> &[FontRef] {
        &self.font_locks
    }
    /// Same sorted-object compact UTF-8/SHA-256 convention as STORE-001. Arrays
    /// retain authored order; every lock, profile, version and revision is hashed.
    pub fn content_hash(&self) -> Result<String, RenderError> {
        let value = serde_json::to_value(self)?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
    }
    /// Rendering content identity, excluding transaction bookkeeping only.
    /// Every document field, lock, profile, and semantic version remains hashed.
    pub fn evaluation_content_hash(&self) -> Result<String, RenderError> {
        let mut value = serde_json::to_value(self)?;
        value
            .as_object_mut()
            .expect("snapshot object")
            .remove("revision");
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if let Some(temporal) = self.profile.temporal {
            temporal.validate()?;
        }
        if self.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(RenderError::UnsupportedSchema(self.schema_version));
        }
        if self.project.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(RenderError::UnsupportedSchema(self.project.schema_version));
        }
        self.project
            .validate_storage()
            .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
        // Legacy visibility v1 is equivalent only when every authored node is enabled.
        if self.profile.temporal.is_some()
            && self.semantic_versions.temporal != Some(TEMPORAL_VERSION)
        {
            return Err(RenderError::UnsupportedFeature(
                "temporal profile requires an explicit supported temporal semantic version".into(),
            ));
        }
        if self.profile.hdr.is_some()
            && (self.semantic_versions.hdr != Some(crate::HDR_VERSION)
                || self.profile.working_space != ColorSpace::LinearRec2020)
        {
            return Err(RenderError::UnsupportedFeature(
                "HDR requires pinned hdr version 1 and LinearRec2020 working space".into(),
            ));
        }
        self.project
            .validate_mattes()
            .map_err(|e| RenderError::Backend {
                code: e.code(),
                message: e.to_string(),
            })?;
        let mut supported_versions = SemanticVersions::current(self.project.semantic_version);
        if (1..=EXPRESSION_SUPPORTED_VERSION).contains(&self.semantic_versions.expression)
            && self.project.expressions.iter().all(|e| {
                matches!(e,
                DocumentObject::Known(e) if e.version <= self.semantic_versions.expression)
            })
        {
            supported_versions.expression = self.semantic_versions.expression;
        }
        if self.project.expressions.iter().any(|e| matches!(e, DocumentObject::Known(e) if e.version > self.semantic_versions.expression)) { return Err(RenderError::UnsupportedFeature("expression exceeds pinned semantic version".into())); }
        let has_blending = self.project.compositions.iter().any(|c| matches!(c, DocumentObject::Known(c) if c.nodes.iter().any(|n| n.properties.iter().any(|p| p.descriptor().key.as_str() == BLEND_KEY))))
            || self.project.sequences.iter().any(|s| matches!(s, DocumentObject::Known(s) if s.tracks.iter().flat_map(|t| &t.clips).any(|c| c.properties.iter().any(|p| p.descriptor().key.as_str() == BLEND_KEY))));
        if self.semantic_versions.simulation.is_none() && self.project.simulations.is_empty() {
            supported_versions.simulation = None;
        }
        self.project
            .validate_simulations()
            .map_err(|e| RenderError::Backend {
                code: e.code(),
                message: e.to_string(),
            })?;
        if self.semantic_versions.repeater.is_none() && self.project.repeaters.is_empty() {
            supported_versions.repeater = None;
        }
        self.project
            .validate_repeaters()
            .map_err(|e| RenderError::Backend {
                code: e.code(),
                message: e.to_string(),
            })?;
        if self.semantic_versions.blend.is_none() && !has_blending {
            supported_versions.blend = None;
        }
        if self.semantic_versions.document_matte.is_none() && self.project.mattes.is_empty() {
            supported_versions.document_matte = None;
        }
        if self.semantic_versions.advanced_text.is_none()
            && !self.project.texts.iter().any(|object| {
                matches!(object,
                DocumentObject::Known(text) if text.layout_version == TEXT_ADVANCED_LAYOUT_VERSION)
            })
        {
            supported_versions.advanced_text = None;
        }
        if self.semantic_versions.path_operations.is_none()
            && !self.project.shapes.iter().any(|object| matches!(object,
                DocumentObject::Known(shape) if matches!(shape.geometry, ShapeGeometry::MorphPath { .. } | ShapeGeometry::TrimmedPath { .. })))
        {
            supported_versions.path_operations = None;
        }
        if self.semantic_versions.hdr.is_none() && self.profile.hdr.is_none() {
            supported_versions.hdr = None;
        }
        if self.semantic_versions.composition_media.is_none() {
            supported_versions.composition_media = None;
        }
        if self.semantic_versions.temporal.is_none() && self.profile.temporal.is_none() {
            supported_versions.temporal = None;
        }
        if self.semantic_versions.stroke_geometry == LEGACY_STROKE_VERSION {
            supported_versions.stroke_geometry = LEGACY_STROKE_VERSION.into();
        }
        if self.semantic_versions.visibility == 1
            && self
                .project
                .compositions
                .iter()
                .all(|c| matches!(c, DocumentObject::Known(c) if c.nodes.iter().all(|n| n.enabled)))
        {
            supported_versions.visibility = 1;
        }
        for (id, version) in &mut supported_versions.effects {
            if self.semantic_versions.effects.get(id) == Some(&EFFECT_VERSION) {
                *version = EFFECT_VERSION;
            }
        }
        if self.project.semantic_version != PROJECT_SEMANTIC_VERSION
            || self.semantic_versions != supported_versions
        {
            return Err(RenderError::UnsupportedFeature(
                "snapshot semantic versions".into(),
            ));
        }
        if !self.project.unknown_fields.is_empty() {
            return Err(RenderError::UnsupportedFeature(
                "unknown project fields".into(),
            ));
        }
        if self.profile.working_space == ColorSpace::Srgb
            || !self.profile.flatten_tolerance_px.is_finite()
            || self.profile.flatten_tolerance_px <= 0.0
        {
            return Err(RenderError::InvalidInput(
                "working space must be linear and flatten tolerance positive".into(),
            ));
        }
        if let Some(id) = self.sequence {
            if self.composition.as_uuid() != id.as_uuid() {
                return Err(RenderError::InvalidInput("sequence target mismatch".into()));
            }
            let root = crate::sequence::lower_sequence(&self.project, id)?;
            let sequence = self
                .project
                .sequences
                .iter()
                .find_map(|s| match s {
                    DocumentObject::Known(s) if s.id == id => Some(s),
                    _ => None,
                })
                .expect("lowering validated the sequence target");
            if self.profile.working_space != sequence.working_space {
                return Err(RenderError::InvalidInput(
                    "sequence working space mismatch".into(),
                ));
            }
            for node in root.nodes {
                if let NodeKind::CompositionInstance(i) = node.kind {
                    kronello_template::validate_reachable(&self.project, i.definition_ref)?;
                }
            }
        } else {
            kronello_template::validate_reachable(&self.project, self.composition)?;
        }
        self.definitions()?;
        Ok(())
    }
    pub(crate) fn definitions(&self) -> Result<Vec<Composition>, RenderError> {
        self.definitions_at(None)
    }
    pub(crate) fn definitions_at(
        &self,
        time: Option<Time>,
    ) -> Result<Vec<Composition>, RenderError> {
        let mut definitions = Vec::new();
        let mut pending = vec![self.composition];
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if seen.len() > 1024 {
                return Err(RenderError::UnsupportedFeature(
                    "composition budget exceeded".into(),
                ));
            }
            let lowered;
            let c = if let Some(sequence) = self.sequence.filter(|_| id == self.composition) {
                lowered = match time {
                    Some(time) => {
                        crate::sequence::lower_sequence_at(&self.project, sequence, time)?
                    }
                    None => crate::sequence::lower_sequence(&self.project, sequence)?,
                };
                &lowered
            } else {
                content(&self.project.compositions, id.as_uuid(), |c| c.id.as_uuid())?
                    .ok_or(kronello_eval::EvaluationError::CompositionNotFound(id))?
            };
            let c =
                self.project
                    .lower_repeater_composition(c)
                    .map_err(|e| RenderError::Backend {
                        code: e.code(),
                        message: e.to_string(),
                    })?;
            for node in &c.nodes {
                match &node.kind {
                    NodeKind::CompositionInstance(i) => pending.push(i.definition_ref),
                    NodeKind::Simulation { content_ref } => pending.push(
                        self.project
                            .simulation(*content_ref)
                            .map_err(|e| RenderError::Backend {
                                code: e.code(),
                                message: e.to_string(),
                            })?
                            .source
                            .composition,
                    ),
                    _ => (),
                }
            }
            definitions.push(c.clone());
        }
        Ok(definitions)
    }
}

fn content<T>(
    objects: &[DocumentObject<T>],
    id: uuid::Uuid,
    key: impl Fn(&T) -> uuid::Uuid,
) -> Result<Option<&T>, RenderError> {
    for object in objects {
        match object {
            DocumentObject::Known(v) if key(v) == id => return Ok(Some(v)),
            DocumentObject::Opaque(v) if v.id == id => {
                return Err(RenderError::UnsupportedFeature(format!(
                    "opaque content {id}"
                )));
            }
            _ => (),
        }
    }
    Ok(None)
}
fn content_asset(project: &Project, id: AssetId) -> Result<&Asset, RenderError> {
    content(&project.assets, id.as_uuid(), |a| a.id.as_uuid())?.ok_or_else(|| {
        RenderError::Backend {
            code: "ASSET_MISSING",
            message: id.to_string(),
        }
    })
}

/// Resolved caption draw input: laid-out glyphs plus cue-level attributes that
/// are not part of `TextDocument` (outline ring, block background, synthesized
/// bold/italic flags). Glyph coordinates are text-local; `origin` is the
/// text-local origin in root Composition design_px after anchor/safe-area
/// placement, so node transforms still compose normally.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionDraw {
    pub layout: LayoutResult,
    /// Draw-time flags indexed by each glyph's `style_index`.
    pub span_flags: Vec<kronello_model::CaptionSpanFlags>,
    /// Outer ring width added to synthesized-bold spans, design_px.
    pub bold_width: f64,
    pub outline: Option<kronello_model::CaptionOutline>,
    pub background: Option<kronello_model::Color>,
    pub origin: [f64; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub enum SceneContent {
    Empty,
    Shape {
        definition: Box<Shape>,
        values: BTreeMap<PropertyId, Value>,
        resolved: ResolvedShape,
    },
    Text(LayoutResult),
    Caption(CaptionDraw),
    Video {
        asset: Asset,
        stream_index: u32,
        time: Time,
        reverse_sampling: bool,
        extent: [f64; 2],
    },
}
/// Resolved FX-003 transition operation on one incoming clip node
/// (ADR-0109). Crossfade and dip use post_effect_opacity for the incoming
/// ramp; wipe attaches a reveal rectangle; dip also lowers a color underlay.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneTransition {
    /// Hard-edge reveal rectangle in root design_px (wipe; no feathering).
    Reveal { min: [f64; 2], max: [f64; 2] },
    /// Dip color composited beneath the node at the given opacity.
    Dip { color: Color, opacity: f64 },
}
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNodeIr {
    pub key: SceneKey,
    pub parent: Option<SceneKey>,
    pub world_transform: kronello_eval::Affine2,
    pub opacity: f64,
    pub post_effect_opacity: f64,
    pub transitions: Vec<SceneTransition>,
    pub blend_mode: BlendMode,
    pub effects: Vec<ResolvedEffect>,
    /// Final node values, including template and layout inputs.
    pub properties: BTreeMap<PropertyId, Value>,
    pub text: Option<String>,
    pub content: SceneContent,
    /// All three envelopes in root Composition design_px, before matte clipping.
    pub bounds: crate::LayoutValue,
    pub layout_content_hash: Option<String>,
}
/// Resolution-independent text layout and shape values, in local design_px.
/// Output region/scale appear only in DAG construction. No backend objects.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneIr {
    pub composition: CompositionId,
    pub time: Time,
    pub design_extent: [f64; 2],
    pub nodes: Vec<SceneNodeIr>,
    pub mattes: Vec<MatteBinding>,
}

pub fn build_scene_ir(
    snapshot: &RenderSnapshot,
    time: Time,
    fonts: &[FontData<'_>],
) -> Result<SceneIr, RenderError> {
    build_scene_ir_with_cache(
        snapshot,
        time,
        fonts,
        &mut crate::RenderCache::new(crate::CacheConfig::disabled()),
    )
}

pub fn build_scene_ir_with_cache(
    snapshot: &RenderSnapshot,
    time: Time,
    fonts: &[FontData<'_>],
    cache: &mut crate::RenderCache,
) -> Result<SceneIr, RenderError> {
    snapshot.validate()?;
    let (definitions, simulation_proof) = crate::simulation::lower(snapshot, time, fonts, cache)?;
    let registry = render_registry();
    let mut used_curves = BTreeSet::new();
    for c in &definitions {
        for source in c
            .properties
            .iter()
            .chain(c.nodes.iter().flat_map(|n| n.properties.iter()))
            .map(|p| p.source())
            .chain(
                c.nodes
                    .iter()
                    .filter_map(|n| match &n.kind {
                        NodeKind::CompositionInstance(i) => Some(i.input_bindings.values()),
                        _ => None,
                    })
                    .flatten(),
            )
        {
            if let PropertySource::Curve(id) = source {
                used_curves.insert(*id);
            }
        }
    }
    let mut expressions = vec![];
    let mut used_expressions = BTreeSet::new();
    for c in &definitions {
        for source in c
            .properties
            .iter()
            .chain(c.nodes.iter().flat_map(|n| &n.properties))
            .map(|p| p.source())
            .chain(
                c.nodes
                    .iter()
                    .filter_map(|n| match &n.kind {
                        NodeKind::CompositionInstance(i) => Some(i.input_bindings.values()),
                        _ => None,
                    })
                    .flatten(),
            )
        {
            if let PropertySource::Expression(id) = source {
                used_expressions.insert(*id);
            }
        }
    }
    for id in used_expressions {
        let Some(e) = content(&snapshot.project.expressions, id.as_uuid(), |e| {
            e.id.as_uuid()
        })?
        else {
            continue;
        };
        for n in &e.nodes {
            if let ExpressionNode::CurveSample { curve, .. } = n {
                used_curves.insert(*curve);
            }
        }
        expressions.push(e.clone());
    }
    let mut curves = vec![];
    for id in used_curves {
        let Some(curve) = content(&snapshot.project.curves, id.as_uuid(), |c| c.id().as_uuid())?
        else {
            continue;
        };
        if curve.ensure_supported_version().is_err() {
            return Err(RenderError::UnsupportedFeature(format!(
                "curve {id} interpolation version"
            )));
        }
        curves.push(curve.clone());
    }
    let refs = ReferenceBindings::new();
    let origins = simulation_proof
        .as_ref()
        .map(|proof| proof.definitions.clone())
        .unwrap_or_default();
    let mut templates = crate::template::TemplateRuntime::compile_specialized(
        &snapshot.project,
        &definitions,
        snapshot.composition,
        &origins,
    )?;
    let deps = templates.dependencies.clone();
    let mut audio_analyses = Vec::new();
    for object in &snapshot.project.audio_analyses {
        if let kronello_model::DocumentObject::Known(data) = object {
            if let kronello_model::AudioAnalysisSource::Asset {
                asset,
                content_hash,
                ..
            } = &data.source
            {
                let source = snapshot.project.assets.iter().find_map(|a| match a {
                    kronello_model::DocumentObject::Known(a) if a.id == *asset => Some(a),
                    _ => None,
                });
                if source.is_none_or(|a| a.content_hash != *content_hash) {
                    return Err(RenderError::UnsupportedFeature(
                        "stale audio analysis source".into(),
                    ));
                }
            }
            audio_analyses.push(data.clone());
        }
    }
    let data_assets =
        snapshot
            .project
            .expression_data_inputs()
            .map_err(|e| RenderError::Backend {
                code: "INVALID_DOCUMENT",
                message: e.to_string(),
            })?;
    let evaluation = EvaluationSnapshot {
        expressions: &expressions,
        compositions: &definitions,
        curves: &curves,
        registry: &registry,
        reference_bindings: &refs,
        dependencies: &deps,
        working_space: snapshot.profile.working_space,
    };
    let graph = if let Some(proof) = &simulation_proof {
        DependencyGraph::compile_specialized_with_data(
            evaluation,
            snapshot.composition,
            &audio_analyses,
            &data_assets,
            &proof.borrowed(),
        )?
    } else {
        DependencyGraph::compile_with_data(
            evaluation,
            snapshot.composition,
            &audio_analyses,
            &data_assets,
        )?
    };
    let (seeds, aliases) = snapshot.project.repeater_context();
    let graph = graph.with_repeater_context(seeds, aliases);
    templates.layout_inputs(&snapshot.project, &definitions, &graph, time, fonts, cache)?;
    let identity = snapshot.evaluation_content_hash()?;
    let evaluated = graph.evaluate_scene_with_properties(time, &mut |keys, time| {
        if snapshot.project.template_instances.is_empty() {
            keys.iter()
                .map(|key| Ok((key.clone(), cache.evaluate(&graph, &identity, key, time)?)))
                .collect()
        } else {
            graph.evaluate_properties_with_inputs(keys, time, &templates.inputs)
        }
    })?;
    if evaluated.nodes.len() > 1024 {
        return Err(RenderError::UnsupportedFeature(
            "scene node budget exceeded".into(),
        ));
    }
    let root = definitions
        .iter()
        .find(|c| c.id == snapshot.composition)
        .expect("validated root");
    let mut nodes = vec![];
    let mut used_fonts = BTreeSet::new();
    let mut mattes = snapshot.mattes.clone();
    for n in evaluated.nodes {
        let values: BTreeMap<_, _> = n
            .properties
            .iter()
            .filter_map(|(key, value)| match key {
                kronello_eval::RuntimePropertyKey::Node(key) => Some((key.property, value.clone())),
                _ => None,
            })
            .collect();
        let definition = definitions
            .iter()
            .find(|c| c.id == n.composition)
            .expect("evaluated definition");
        let authored = definition
            .nodes
            .iter()
            .find(|v| v.id == n.key.node)
            .expect("evaluated node");
        if authored.effects.len() > 16 {
            return Err(EffectError::StackBudget.into());
        }
        let effects = authored
            .effects
            .iter()
            .map(|e| {
                let d = e.definition()?;
                if snapshot
                    .semantic_versions
                    .effects
                    .get(&d.effect_id)
                    .is_none_or(|v| d.version > *v)
                {
                    return Err(RenderError::UnsupportedFeature(
                        "effect exceeds pinned snapshot version".into(),
                    ));
                }
                d.validate(&authored.properties, &registry)?;
                Ok(d.resolve(&values)?)
            })
            .collect::<Result<Vec<_>, RenderError>>()?;
        let mut layout_content_hash = None;
        let properties = values.clone();
        let mut evaluated_text = None;
        let mut content = match n.kind {
            NodeKind::Shape { content_ref } => {
                let shape = content(&snapshot.project.shapes, content_ref.as_uuid(), |s| {
                    s.id.as_uuid()
                })?
                .ok_or(ShapeError::MissingContent { id: content_ref })?;
                shape.validate(&authored.properties, &registry)?;
                if shape.stroke.as_ref().is_some_and(|s| s.options.is_some())
                    && snapshot.semantic_versions.stroke_geometry == LEGACY_STROKE_VERSION
                {
                    return Err(RenderError::UnsupportedFeature(
                        "stroke exceeds pinned snapshot version".into(),
                    ));
                }
                SceneContent::Shape {
                    definition: Box::new(shape.clone()),
                    resolved: shape.resolve(&values)?,
                    values,
                }
            }
            NodeKind::Text { content_ref } => {
                let text = content(&snapshot.project.texts, content_ref.as_uuid(), |t| {
                    t.id.as_uuid()
                })?
                .ok_or(TextError::MissingContent { id: content_ref })?;
                text.validate(&authored.properties, &registry)?;
                let mut resolved = text.resolve(&values)?;
                templates.text_override(&n.key, &mut resolved)?;
                evaluated_text = Some(resolved.text.clone());
                layout_content_hash = Some(crate::layout_content_hash(&resolved)?);
                used_fonts.extend(resolved.styles.iter().map(|s| s.font.clone()));
                let layout = cache.layout(&resolved, fonts)?;
                crate::bounds::check_overflow(&n.key, &layout)?;
                SceneContent::Text(layout)
            }
            NodeKind::Media(media) => {
                let asset = content_asset(&snapshot.project, media.asset)?;
                match asset.kind {
                    AssetKind::Audio => SceneContent::Empty,
                    AssetKind::Video | AssetKind::Image => {
                        let stream = asset
                            .streams
                            .iter()
                            .find(|s| s.index == media.stream_index)
                            .ok_or_else(|| {
                                RenderError::InvalidInput("Media stream missing".into())
                            })?;
                        if asset.kind == AssetKind::Video
                            && stream.width.is_none()
                            && stream.height.is_none()
                        {
                            SceneContent::Empty
                        } else {
                            require_composition_media(snapshot)?;
                            let relative =
                                n.local_time.checked_sub(authored.active_range.start())?;
                            let source =
                                media.source_in.checked_add(media.time_map.map(relative)?)?;
                            media_content(asset, media.stream_index, source)?
                        }
                    }
                    AssetKind::Data => {
                        return Err(RenderError::UnsupportedFeature(
                            "Data asset is not visual media".into(),
                        ));
                    }
                }
            }
            _ => SceneContent::Empty,
        };
        if let Some(asset) = templates.media_slots.get(&n.key) {
            require_composition_media(snapshot)?;
            let asset = content_asset(&snapshot.project, *asset)?;
            let stream = asset
                .streams
                .iter()
                .find(|s| s.width.is_some() && s.height.is_some())
                .ok_or_else(|| {
                    RenderError::UnsupportedFeature("MediaSlot has no visual stream".into())
                })?;
            let relative = n.local_time.checked_sub(authored.active_range.start())?;
            let source = stream
                .start_time
                .unwrap_or(Time::ZERO)
                .checked_add(relative)?;
            content = media_content(asset, stream.index, source)?;
        }
        let mut post_effect_opacity = 1.0;
        let mut transitions = Vec::new();
        let mut transition_offset = [0.0; 2];
        if let Some(sequence) = snapshot
            .sequence
            .filter(|_| n.composition == snapshot.composition)
        {
            let sequence = snapshot
                .project
                .sequences
                .iter()
                .find_map(|s| match s {
                    DocumentObject::Known(s) if s.id == sequence => Some(s),
                    _ => None,
                })
                .expect("validated sequence");
            let clip = sequence
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .find(|c| c.id.as_uuid() == n.key.node.as_uuid())
                .expect("lowered clip");
            content = match &clip.source_ref {
                SourceRef::Asset {
                    asset,
                    stream_index,
                } => {
                    let asset = content_asset(&snapshot.project, *asset)?;
                    let stream = asset
                        .streams
                        .iter()
                        .find(|s| s.index == *stream_index)
                        .expect("validated stream");
                    let extent = [stream.width, stream.height].map(|x| x.map(f64::from));
                    let [Some(w), Some(h)] = extent else {
                        return Err(RenderError::UnsupportedFeature(
                            "video dimensions unavailable".into(),
                        ));
                    };
                    SceneContent::Video {
                        asset: asset.clone(),
                        stream_index: *stream_index,
                        time: clip.local_time(time)?,
                        reverse_sampling: clip.reverse_sampling.is_some(),
                        extent: [w, h],
                    }
                }
                SourceRef::Generator { color, .. } => {
                    crate::sequence::solid_content(*color, sequence.extent)?
                }
                SourceRef::Caption { caption } => {
                    let document =
                        self::content(&snapshot.project.captions, caption.as_uuid(), |c| {
                            c.id.as_uuid()
                        })?
                        .ok_or(CaptionError::MissingContent { id: *caption })?;
                    let resolved = document.resolve(&clip.properties, &registry)?;
                    let wrap_width = resolved.placement.wrap_width(sequence.extent);
                    let text = resolved.resolved_text(wrap_width)?;
                    let layout = cache.layout(&text, fonts)?;
                    crate::bounds::check_overflow(&n.key, &layout)?;
                    layout_content_hash = Some(crate::layout_content_hash(&text)?);
                    used_fonts.extend(resolved.fonts());
                    let block = [
                        wrap_width,
                        layout.layout_bounds.max[1] - layout.layout_bounds.min[1],
                    ];
                    let top_left = resolved.placement.origin(sequence.extent, block);
                    // Text-local origin: the layout bounds box is anchored
                    // inside the safe area, so translate by the inverse of the
                    // bounds' own offset.
                    let origin = [
                        top_left[0] - layout.layout_bounds.min[0],
                        top_left[1] - layout.layout_bounds.min[1],
                    ];
                    SceneContent::Caption(CaptionDraw {
                        layout,
                        span_flags: resolved.span_flags,
                        bold_width: resolved.font_size * kronello_model::CAPTION_BOLD_WIDTH_RATIO,
                        outline: resolved.outline,
                        background: resolved.background,
                        origin,
                    })
                }
                _ => content,
            };
            for tr in &sequence.transitions {
                // The transition contributes only when both endpoints are
                // enabled; a disabled clip leaves a hole rather than a
                // partially rendered transition (NLE-005).
                let outgoing_enabled = sequence
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == tr.outgoing)
                    .is_some_and(|c| c.enabled);
                if tr.incoming == clip.id && tr.range.contains(time) && outgoing_enabled {
                    if tr.version != 1 {
                        return Err(RenderError::UnsupportedFeature("transition version".into()));
                    }
                    let progress = time
                        .checked_sub(tr.range.start())?
                        .checked_div(tr.range.duration()?.as_time())?;
                    let p = progress.numerator() as f64 / progress.denominator() as f64;
                    let extent = [sequence.extent.width(), sequence.extent.height()];
                    match tr.kind {
                        TransitionKind::Crossfade => post_effect_opacity *= p,
                        TransitionKind::Wipe => {
                            let Some(TransitionParams::Wipe(wipe)) = tr.params else {
                                return Err(RenderError::InvalidInput(
                                    "wipe transition params".into(),
                                ));
                            };
                            let (min, max) = match wipe.direction {
                                TransitionDirection::Left => {
                                    ([0.0, 0.0], [p * extent[0], extent[1]])
                                }
                                TransitionDirection::Right => {
                                    ([(1.0 - p) * extent[0], 0.0], extent)
                                }
                                TransitionDirection::Up => ([0.0, 0.0], [extent[0], p * extent[1]]),
                                TransitionDirection::Down => ([0.0, (1.0 - p) * extent[1]], extent),
                            };
                            transitions.push(SceneTransition::Reveal { min, max });
                        }
                        TransitionKind::Slide => {
                            let Some(TransitionParams::Slide(slide)) = tr.params else {
                                return Err(RenderError::InvalidInput(
                                    "slide transition params".into(),
                                ));
                            };
                            transition_offset = match slide.direction {
                                TransitionDirection::Left => [-(1.0 - p) * extent[0], 0.0],
                                TransitionDirection::Right => [(1.0 - p) * extent[0], 0.0],
                                TransitionDirection::Up => [0.0, -(1.0 - p) * extent[1]],
                                TransitionDirection::Down => [0.0, (1.0 - p) * extent[1]],
                            };
                        }
                        TransitionKind::Dip => {
                            let Some(TransitionParams::Dip(dip)) = tr.params else {
                                return Err(RenderError::InvalidInput(
                                    "dip transition params".into(),
                                ));
                            };
                            // First half: dip color fades in over the outgoing
                            // clip; second half: the incoming clip fades in
                            // over the fully opaque dip color (ADR-0109).
                            let (under, over) = if p < 0.5 {
                                (2.0 * p, 0.0)
                            } else {
                                (1.0, 2.0 * p - 1.0)
                            };
                            post_effect_opacity *= over;
                            transitions.push(SceneTransition::Dip {
                                color: dip.color,
                                opacity: under,
                            });
                        }
                    }
                }
            }
        }
        for relation in snapshot.project.mattes.iter().filter_map(|m| match m {
            DocumentObject::Known(m)
                if m.composition == n.composition && m.source == n.key.node =>
            {
                Some(m)
            }
            _ => None,
        }) {
            let source: SceneKey = (&n.key).into();
            if mattes.iter().any(|m| m.source == source) {
                return Err(RenderError::Backend {
                    code: "MATTE_DUPLICATE_SOURCE",
                    message: "transient matte collides with document relation".into(),
                });
            }
            let kind = match (relation.kind, relation.invert) {
                (DocumentMatteKind::Alpha, false) => MatteKind::Alpha,
                (DocumentMatteKind::Luminance, false) => MatteKind::Luminance,
                (DocumentMatteKind::Alpha, true) => MatteKind::AlphaInverted,
                (DocumentMatteKind::Luminance, true) => MatteKind::LuminanceInverted,
            };
            mattes.push(MatteBinding {
                source,
                matte: SceneKey {
                    instance_path: n.key.instance_path.clone(),
                    node: relation.matte,
                },
                kind,
                visible: relation.visible,
            });
        }
        BlendMode::from_properties(&authored.properties)
            .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
        let blend_mode = if let Some(property) = authored
            .properties
            .iter()
            .find(|p| p.descriptor().key.as_str() == BLEND_KEY)
        {
            BlendMode::from_value(properties.get(&property.id()).ok_or_else(|| {
                RenderError::InvalidInput("missing resolved blend property".into())
            })?)
            .map_err(|e| RenderError::InvalidInput(e.to_string()))?
        } else {
            BlendMode::Normal
        };
        // Slide translates the whole lowered clip node in root design_px.
        let mut world_transform = n.world_transform;
        world_transform.0[0][2] += transition_offset[0];
        world_transform.0[1][2] += transition_offset[1];
        nodes.push(SceneNodeIr {
            key: (&n.key).into(),
            parent: n.containment_parent.as_ref().map(Into::into),
            world_transform,
            opacity: n.transform.opacity,
            post_effect_opacity,
            transitions,
            blend_mode,
            effects,
            properties,
            text: evaluated_text,
            content,
            bounds: crate::LayoutValue::default(),
            layout_content_hash,
        });
    }
    // Imported snapshots must retain every required lock; no latest-font filling.
    if used_fonts.iter().any(|f| !snapshot.font_locks.contains(f)) {
        return Err(RenderError::InvalidInput(
            "snapshot missing required font lock".into(),
        ));
    }
    crate::bounds::derive_scene_bounds(&mut nodes)?;
    Ok(SceneIr {
        composition: snapshot.composition,
        time,
        design_extent: [root.design_extent.width(), root.design_extent.height()],
        nodes,
        mattes,
    })
}

pub fn render_registry() -> SchemaRegistry {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors()
        .into_iter()
        .chain(text_descriptors())
        .chain(effect_descriptors())
        .chain(kronello_model::simulation_descriptors())
        .chain(kronello_model::caption_descriptors())
    {
        registry
            .register(descriptor)
            .expect("distinct built-in render descriptors");
    }
    registry
}

fn require_composition_media(snapshot: &RenderSnapshot) -> Result<(), RenderError> {
    if snapshot.semantic_versions.composition_media != Some(COMPOSITION_MEDIA_VERSION) {
        return Err(RenderError::UnsupportedFeature("Composition visual Media/MediaSlot requires explicit composition_media semantic version 1".into()));
    }
    Ok(())
}
fn media_content(
    asset: &Asset,
    stream_index: u32,
    source: Time,
) -> Result<SceneContent, RenderError> {
    if !matches!(asset.kind, AssetKind::Video | AssetKind::Image) {
        return Err(RenderError::UnsupportedFeature(
            "MediaSlot requires image/video asset".into(),
        ));
    }
    let stream = asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .ok_or_else(|| RenderError::InvalidInput("visual media stream missing".into()))?;
    let (Some(width), Some(height)) = (stream.width, stream.height) else {
        return Err(RenderError::UnsupportedFeature(
            "visual media dimensions missing".into(),
        ));
    };
    if width == 0 || height == 0 {
        return Err(RenderError::InvalidInput(
            "visual media dimensions must be positive".into(),
        ));
    }
    let time = if asset.kind == AssetKind::Image {
        stream.start_time.unwrap_or(Time::ZERO)
    } else {
        let start = stream.start_time.unwrap_or(Time::ZERO);
        if source < start
            || stream
                .duration
                .is_some_and(|duration| source >= start.checked_add(duration).unwrap_or(start))
        {
            return Err(RenderError::Backend {
                code: "FRAME_NOT_FOUND",
                message: "Composition media source time is outside the locked stream".into(),
            });
        }
        source
    };
    Ok(SceneContent::Video {
        reverse_sampling: false,
        asset: asset.clone(),
        stream_index,
        time,
        extent: [f64::from(width), f64::from(height)],
    })
}
