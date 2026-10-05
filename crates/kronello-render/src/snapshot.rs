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
    /// Absent legacy snapshots use the initial bounds contract, never latest.
    #[serde(default = "initial_bounds_version")]
    pub bounds: u32,
    pub vector: String,
    pub color: String,
    pub coverage: String,
    pub stroke_geometry: String,
    pub gradient_interpolation: String,
    pub effects: BTreeMap<String, u32>,
    #[serde(default = "generator_versions")]
    pub generators: BTreeMap<String, u32>,
    #[serde(default = "initial_video_version")]
    pub video_input: String,
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
            expression: EXPRESSION_VERSION,
            interpolation: INTERPOLATION_VERSION,
            time_map: 1,
            layout: TEXT_LAYOUT_VERSION,
            bounds: LAYOUT_BOUNDS_VERSION,
            vector: VECTOR_VERSION.into(),
            color: COLOR_VERSION.into(),
            coverage: COVERAGE_VERSION.into(),
            stroke_geometry: STROKE_GEOMETRY_VERSION.into(),
            gradient_interpolation: GRADIENT_INTERPOLATION_VERSION.into(),
            effects: BTreeMap::from([
                (GAUSSIAN_BLUR_ID.into(), AFFINE_EFFECT_VERSION),
                (DROP_SHADOW_ID.into(), AFFINE_EFFECT_VERSION),
            ]),
            generators: generator_versions(),
            video_input: initial_video_version(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderProfile {
    pub working_space: ColorSpace,
    pub flatten_tolerance_px: f64,
}
impl Default for RenderProfile {
    fn default() -> Self {
        Self {
            working_space: ColorSpace::LinearRec709,
            flatten_tolerance_px: 0.02,
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
        let mut supported_versions = SemanticVersions::current(self.project.semantic_version);
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
    fn definitions(&self) -> Result<Vec<Composition>, RenderError> {
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
                lowered = crate::sequence::lower_sequence(&self.project, sequence)?;
                &lowered
            } else {
                content(&self.project.compositions, id.as_uuid(), |c| c.id.as_uuid())?
                    .ok_or(kronello_eval::EvaluationError::CompositionNotFound(id))?
            };
            for node in &c.nodes {
                if let NodeKind::CompositionInstance(i) = &node.kind {
                    pending.push(i.definition_ref);
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

#[derive(Debug, Clone, PartialEq)]
pub enum SceneContent {
    Empty,
    Shape {
        definition: Shape,
        values: BTreeMap<PropertyId, Value>,
        resolved: ResolvedShape,
    },
    Text(LayoutResult),
    Video {
        asset: Asset,
        stream_index: u32,
        time: Time,
        extent: [f64; 2],
    },
}
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNodeIr {
    pub key: SceneKey,
    pub parent: Option<SceneKey>,
    pub world_transform: kronello_eval::Affine2,
    pub opacity: f64,
    pub post_effect_opacity: f64,
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
    let definitions = snapshot.definitions()?;
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
    let mut templates = crate::template::TemplateRuntime::compile(
        &snapshot.project,
        &definitions,
        snapshot.composition,
    )?;
    let deps = templates.dependencies.clone();
    let graph = DependencyGraph::compile(
        EvaluationSnapshot {
            expressions: &expressions,
            compositions: &definitions,
            curves: &curves,
            registry: &registry,
            reference_bindings: &refs,
            dependencies: &deps,
            working_space: snapshot.profile.working_space,
        },
        snapshot.composition,
    )?;
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
    for n in evaluated.nodes {
        if let Some(asset) = templates.media_slots.get(&n.key) {
            return Err(RenderError::UnsupportedFeature(format!(
                "Composition MediaSlot drawing for asset {asset}"
            )));
        }
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
                    definition: shape.clone(),
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
                let asset = content(&snapshot.project.assets, media.asset.as_uuid(), |a| {
                    a.id.as_uuid()
                })?
                .ok_or_else(|| RenderError::UnsupportedFeature("missing media asset".into()))?;
                if asset.kind != kronello_model::AssetKind::Audio {
                    return Err(RenderError::UnsupportedFeature(
                        "Media video/image drawing requires COMP-002".into(),
                    ));
                }
                SceneContent::Empty
            }
            _ => SceneContent::Empty,
        };
        let mut post_effect_opacity = 1.0;
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
                        extent: [w, h],
                    }
                }
                SourceRef::Generator { color, .. } => {
                    crate::sequence::solid_content(*color, sequence.extent)?
                }
                _ => content,
            };
            for tr in &sequence.transitions {
                if tr.incoming == clip.id && tr.range.contains(time) {
                    let progress = time
                        .checked_sub(tr.range.start())?
                        .checked_div(tr.range.duration()?.as_time())?;
                    post_effect_opacity *=
                        progress.numerator() as f64 / progress.denominator() as f64;
                }
            }
        }
        nodes.push(SceneNodeIr {
            key: (&n.key).into(),
            parent: n.containment_parent.as_ref().map(Into::into),
            world_transform: n.world_transform,
            opacity: n.transform.opacity,
            post_effect_opacity,
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
        mattes: snapshot.mattes.clone(),
    })
}

pub fn render_registry() -> SchemaRegistry {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors()
        .into_iter()
        .chain(text_descriptors())
        .chain(effect_descriptors())
    {
        registry
            .register(descriptor)
            .expect("distinct built-in render descriptors");
    }
    registry
}
