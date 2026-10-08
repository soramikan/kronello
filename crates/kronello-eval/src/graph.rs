use std::collections::{BTreeMap, BTreeSet};

use kronello_model::{
    AnimationCurve, ColorSpace, Composition, CompositionId, CompositionInstance, CurveId,
    InstancePath, ModelError, NodeKind, Property, PropertyKey, PropertySource, SchemaRegistry,
    Value, validate_compositions,
};
use kronello_time::Time;

use crate::{EvaluationError, RuntimePropertyKey};

/// Transient compiled input overrides, not an extension of PropertySource.
/// Targets must be composition properties with an existing placement binding;
/// sources must be properties in that placement's immediate parent scope.
pub type ReferenceBindings = BTreeMap<RuntimePropertyKey, RuntimePropertyKey>;
/// Additional static edges (dependent -> upstreams), for example layout inputs.
/// Expression property edges are derived directly from the canonical AST.
pub type DependencyDeclarations = BTreeMap<RuntimePropertyKey, Vec<RuntimePropertyKey>>;

/// Borrowed immutable semantic input. The caller resolves storage/version/asset
/// compatibility before constructing this typed input; no Project/store API or
/// implicit lookup of a latest document is involved.
pub struct EvaluationSnapshot<'a> {
    pub compositions: &'a [Composition],
    pub expressions: &'a [kronello_model::Expression],
    pub curves: &'a [AnimationCurve],
    pub registry: &'a SchemaRegistry,
    pub reference_bindings: &'a ReferenceBindings,
    pub dependencies: &'a DependencyDeclarations,
    pub working_space: ColorSpace,
}

pub(crate) struct Scope<'a> {
    pub composition: &'a Composition,
    pub parent: Option<(
        InstancePath,
        &'a CompositionInstance,
        kronello_model::NodeId,
    )>,
}
pub(crate) struct Entry<'a> {
    pub(crate) property: &'a Property,
    source: &'a PropertySource<Value>,
    // An authored placement binding runs in the parent's time domain. The
    // definition's own curve runs in its mapped local time domain.
    pub(crate) source_scope: InstancePath,
}

/// Immutable reusable compiled graph. Memoization is local to a single query.
pub struct DependencyGraph<'a> {
    pub(crate) root: CompositionId,
    pub(crate) scopes: BTreeMap<InstancePath, Scope<'a>>,
    entries: BTreeMap<RuntimePropertyKey, Entry<'a>>,
    edges: BTreeMap<RuntimePropertyKey, BTreeSet<RuntimePropertyKey>>,
    v3_roots: BTreeSet<RuntimePropertyKey>,
    current_edges: BTreeMap<RuntimePropertyKey, BTreeSet<RuntimePropertyKey>>,
    references: ReferenceBindings,
    pub(crate) curves: BTreeMap<CurveId, &'a AnimationCurve>,
    pub(crate) registry: &'a SchemaRegistry,
    pub(crate) working_space: ColorSpace,
    pub(crate) audio_analyses:
        BTreeMap<kronello_model::AssetId, &'a kronello_model::AudioAnalysisDataAsset>,
    repeat_seeds: BTreeMap<kronello_model::CompositionInstanceId, u64>,
    expansion_noise_aliases:
        BTreeMap<kronello_model::CompositionInstanceId, kronello_model::CompositionInstanceId>,
    pub(crate) data_assets:
        BTreeMap<kronello_model::AssetId, &'a kronello_model::ExpressionDataAsset>,
    expressions: BTreeMap<kronello_model::ExpressionId, &'a kronello_model::Expression>,
}

impl<'a> DependencyGraph<'a> {
    pub fn compile(
        snapshot: EvaluationSnapshot<'a>,
        root: CompositionId,
    ) -> Result<Self, EvaluationError> {
        Self::compile_with_audio(snapshot, root, &[])
    }
    pub fn with_repeater_context(
        mut self,
        seeds: BTreeMap<kronello_model::CompositionInstanceId, u64>,
        aliases: BTreeMap<
            kronello_model::CompositionInstanceId,
            kronello_model::CompositionInstanceId,
        >,
    ) -> Self {
        self.repeat_seeds = seeds;
        self.expansion_noise_aliases = aliases;
        self
    }
    pub(crate) fn noise_context_len(&self, path: &InstancePath) -> usize {
        path.ids().len().saturating_mul(16)
            + path
                .ids()
                .iter()
                .filter(|id| self.repeat_seeds.contains_key(id))
                .count()
                .saturating_mul(8)
    }
    pub(crate) fn visit_noise_context(&self, path: &InstancePath, mut byte: impl FnMut(u8)) {
        for id in path.ids() {
            let alias = self.expansion_noise_aliases.get(id).unwrap_or(id);
            for b in alias.as_uuid().into_bytes() {
                byte(b);
            }
            if let Some(seed) = self.repeat_seeds.get(id) {
                for b in seed.to_le_bytes() {
                    byte(b);
                }
            }
        }
    }
    pub(crate) fn noise_context_bytes(&self, path: &InstancePath) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(path.ids().len().saturating_mul(24));
        for id in path.ids() {
            let alias = self.expansion_noise_aliases.get(id).unwrap_or(id);
            bytes.extend_from_slice(&alias.as_uuid().into_bytes());
            if let Some(seed) = self.repeat_seeds.get(id) {
                bytes.extend_from_slice(&seed.to_le_bytes());
            }
        }
        bytes
    }
    pub fn compile_with_audio(
        snapshot: EvaluationSnapshot<'a>,
        root: CompositionId,
        audio_analyses: &'a [kronello_model::AudioAnalysisDataAsset],
    ) -> Result<Self, EvaluationError> {
        Self::compile_with_data(snapshot, root, audio_analyses, &[])
    }
    pub fn compile_with_data(
        snapshot: EvaluationSnapshot<'a>,
        root: CompositionId,
        audio_analyses: &'a [kronello_model::AudioAnalysisDataAsset],
        data_assets: &'a [kronello_model::ExpressionDataAsset],
    ) -> Result<Self, EvaluationError> {
        Self::compile_internal(snapshot, root, audio_analyses, data_assets, None)
    }
    /// Internal upper compiler path; persisted/API definitions use strict compile.
    #[doc(hidden)]
    pub fn compile_specialized_with_data(
        snapshot: EvaluationSnapshot<'a>,
        root: CompositionId,
        audio_analyses: &'a [kronello_model::AudioAnalysisDataAsset],
        data_assets: &'a [kronello_model::ExpressionDataAsset],
        provenance: &crate::SpecializationProvenance<'_>,
    ) -> Result<Self, EvaluationError> {
        Self::compile_internal(
            snapshot,
            root,
            audio_analyses,
            data_assets,
            Some(provenance),
        )
    }
    fn compile_internal(
        snapshot: EvaluationSnapshot<'a>,
        root: CompositionId,
        audio_analyses: &'a [kronello_model::AudioAnalysisDataAsset],
        data_assets: &'a [kronello_model::ExpressionDataAsset],
        provenance: Option<&crate::SpecializationProvenance<'_>>,
    ) -> Result<Self, EvaluationError> {
        if let Some(proof) = provenance {
            crate::specialization::validate(snapshot.compositions, root, snapshot.registry, proof)?;
        } else {
            validate_compositions(snapshot.compositions, snapshot.registry)
                .map_err(EvaluationError::InvalidCompositions)?;
        }
        if snapshot.working_space == ColorSpace::Srgb {
            return Err(EvaluationError::InvalidWorkingColorSpace);
        }
        let definitions: BTreeMap<_, _> = snapshot.compositions.iter().map(|c| (c.id, c)).collect();
        let composition = *definitions
            .get(&root)
            .ok_or(EvaluationError::CompositionNotFound(root))?;
        let mut graph = Self {
            root,
            scopes: BTreeMap::new(),
            entries: BTreeMap::new(),
            edges: BTreeMap::new(),
            v3_roots: BTreeSet::new(),
            current_edges: BTreeMap::new(),
            references: snapshot.reference_bindings.clone(),
            curves: BTreeMap::new(),
            audio_analyses: audio_analyses.iter().map(|a| (a.id, a)).collect(),
            repeat_seeds: BTreeMap::new(),
            expansion_noise_aliases: BTreeMap::new(),
            data_assets: data_assets.iter().map(|d| (d.id, d)).collect(),
            expressions: snapshot.expressions.iter().map(|e| (e.id, e)).collect(),
            registry: snapshot.registry,
            working_space: snapshot.working_space,
        };
        if graph.data_assets.len() != data_assets.len()
            || graph.audio_analyses.len() != audio_analyses.len()
            || graph
                .data_assets
                .keys()
                .any(|id| graph.audio_analyses.contains_key(id))
        {
            return Err(EvaluationError::DuplicateDataAssetId);
        }
        if graph.expressions.len() != snapshot.expressions.len() {
            return Err(EvaluationError::DuplicateExpressionId);
        }
        for curve in snapshot.curves {
            if graph.curves.insert(curve.id(), curve).is_some() {
                return Err(EvaluationError::DuplicateCurveId(curve.id()));
            }
        }
        let mut pending = vec![(
            InstancePath::root(),
            Scope {
                composition,
                parent: None,
            },
        )];
        while let Some((path, scope)) = pending.pop() {
            for property in &scope.composition.properties {
                let key = RuntimePropertyKey::Composition {
                    instance_path: path.clone(),
                    composition: scope.composition.id,
                    property: property.id(),
                };
                let (source, source_scope) = scope
                    .parent
                    .as_ref()
                    .and_then(|(parent, placement, _)| {
                        placement
                            .input_bindings
                            .get(&property.id())
                            .map(|source| (source, parent.clone()))
                    })
                    .unwrap_or((property.source(), path.clone()));
                graph.entries.insert(
                    key,
                    Entry {
                        property,
                        source,
                        source_scope,
                    },
                );
            }
            for node in &scope.composition.nodes {
                let mut descriptors = BTreeSet::new();
                for property in &node.properties {
                    // Gradient stops are keyed by PropertyId, not descriptor name.
                    // Singleton transform/opacity descriptors retain their ambiguity check.
                    if !descriptors.insert(property.descriptor().key.clone())
                        && !graph
                            .registry
                            .lookup(&property.descriptor().key)
                            .is_ok_and(|d| d.definition().repeatable)
                        && !matches!(
                            property.descriptor().key.as_str(),
                            "kronello.shape.gradient_color" | "kronello.shape.gradient_offset"
                        )
                    {
                        return Err(EvaluationError::DuplicateNodeDescriptor {
                            node: crate::NodeKey {
                                instance_path: path.clone(),
                                node: node.id,
                            },
                            descriptor: property.descriptor().key.to_string(),
                        });
                    }
                    let key = PropertyKey {
                        instance_path: path.clone(),
                        node: node.id,
                        property: property.id(),
                    }
                    .into();
                    graph.entries.insert(
                        key,
                        Entry {
                            property,
                            source: property.source(),
                            source_scope: path.clone(),
                        },
                    );
                }
                if let NodeKind::CompositionInstance(instance) = &node.kind {
                    pending.push((
                        path.child(instance.id),
                        Scope {
                            composition: definitions[&instance.definition_ref],
                            parent: Some((path.clone(), instance, node.id)),
                        },
                    ));
                }
            }
            graph.scopes.insert(path, scope);
        }
        graph.edges = graph
            .entries
            .keys()
            .cloned()
            .map(|key| (key, BTreeSet::new()))
            .collect();
        for target in snapshot.dependencies.keys() {
            if let RuntimePropertyKey::LayoutValue {
                instance_path,
                text,
                ..
            } = target
            {
                // Text nodes feed responsive layout bands; Media and Null
                // (media-slot) nodes carry AI-003 smart-reframe crop windows
                // through the same layout-input path (ADR-0126).
                if !graph.scopes.get(instance_path).is_some_and(|scope| {
                    scope.composition.nodes.iter().any(|node| {
                        node.id == *text
                            && matches!(
                                node.kind,
                                NodeKind::Text { .. } | NodeKind::Media(_) | NodeKind::Null
                            )
                    })
                }) {
                    return Err(EvaluationError::PropertyNotFound(target.clone()));
                }
                graph.edges.insert(target.clone(), BTreeSet::new());
            }
        }
        for (target, sources) in snapshot.dependencies {
            graph.require_key(target)?;
            for source in sources {
                graph.require_key(source)?;
                graph.edges.get_mut(target).unwrap().insert(source.clone());
            }
        }
        for (target, source) in snapshot.reference_bindings {
            graph.require_key(target)?;
            graph.require_key(source)?;
            let valid = match target {
                RuntimePropertyKey::Composition {
                    instance_path,
                    property,
                    ..
                } => graph.scopes[instance_path].parent.as_ref().is_some_and(
                    |(parent, placement, _)| {
                        placement.input_bindings.contains_key(property)
                            && source.instance_path() == parent
                    },
                ),
                _ => false,
            };
            if !valid || !graph.entries.contains_key(target) || !graph.entries.contains_key(source)
            {
                return Err(EvaluationError::InvalidReferenceBinding(target.clone()));
            }
            let target_contract = self_contract(&graph.entries[target], snapshot.registry);
            let source_contract = self_contract(&graph.entries[source], snapshot.registry);
            if target_contract.value_type != source_contract.value_type
                || target_contract.unit != source_contract.unit
                || target_contract.coordinate_space != source_contract.coordinate_space
            {
                return Err(EvaluationError::InvalidReferenceBinding(target.clone()));
            }
            graph.edges.get_mut(target).unwrap().insert(source.clone());
        }
        for (key, entry) in &graph.entries {
            if graph.references.contains_key(key) {
                continue;
            }
            if let PropertySource::Expression(id) = entry.source {
                let expression =
                    graph
                        .expressions
                        .get(id)
                        .ok_or_else(|| EvaluationError::InvalidValue {
                            key: key.clone(),
                            source: ModelError::ExpressionNotFound { id: *id },
                        })?;
                expression
                    .validate()
                    .map_err(|source| crate::expression::error(key, source))?;
                if expression.value_type != self_contract(entry, snapshot.registry).value_type {
                    return Err(crate::expression::error(
                        key,
                        kronello_model::ExpressionError::TypeMismatch(expression.nodes.len() - 1),
                    ));
                }
                for node in &expression.nodes {
                    match node {
                        kronello_model::ExpressionNode::Property {
                            node,
                            property,
                            value_type,
                        }
                        | kronello_model::ExpressionNode::PropertySample {
                            node,
                            property,
                            value_type,
                            ..
                        } => {
                            let source =
                                graph.expression_key(&entry.source_scope, *node, *property);
                            graph.require_key(&source)?;
                            let upstream =
                                self_contract(&graph.entries[&source], snapshot.registry);
                            let consumer = self_contract(entry, snapshot.registry);
                            if upstream.value_type != *value_type
                                || upstream.unit != consumer.unit
                                || upstream.coordinate_space != consumer.coordinate_space
                            {
                                return Err(crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "reference type, unit or coordinate space mismatch",
                                    ),
                                ));
                            }
                            graph.edges.get_mut(key).unwrap().insert(source);
                        }
                        kronello_model::ExpressionNode::CurveSample {
                            curve, value_type, ..
                        } => {
                            let curve = graph.curves.get(curve).ok_or_else(|| {
                                EvaluationError::InvalidValue {
                                    key: key.clone(),
                                    source: ModelError::CurveNotFound { id: *curve },
                                }
                            })?;
                            curve.ensure_supported_version().map_err(|_| {
                                EvaluationError::UnsupportedFeature {
                                    key: key.clone(),
                                    feature: "expression curve interpolation version".into(),
                                }
                            })?;
                            if curve.value_type() != *value_type {
                                return Err(crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "curve type mismatch",
                                    ),
                                ));
                            }
                        }
                        kronello_model::ExpressionNode::DataAssetCell {
                            asset,
                            column,
                            value_type,
                            ..
                        } => {
                            let data = graph.data_assets.get(asset).ok_or_else(|| {
                                crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "DataAsset missing",
                                    ),
                                )
                            })?;
                            data.validate().map_err(|_| {
                                crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "invalid DataAsset hash or table",
                                    ),
                                )
                            })?;
                            if data.table.columns.get(column) != Some(value_type) {
                                return Err(crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "DataAsset column type mismatch",
                                    ),
                                ));
                            }
                        }
                        kronello_model::ExpressionNode::AudioFeature { asset, feature, .. } => {
                            let data = graph.audio_analyses.get(asset).ok_or_else(|| {
                                crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "audio analysis missing",
                                    ),
                                )
                            })?;
                            data.validate().map_err(|_| {
                                crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "invalid audio analysis",
                                    ),
                                )
                            })?;
                            if let kronello_model::AudioFeature::BandEnergy { band } = feature
                                && *band as usize >= data.config.bands.len()
                            {
                                return Err(crate::expression::error(
                                    key,
                                    kronello_model::ExpressionError::InvalidAst(
                                        "audio band missing",
                                    ),
                                ));
                            }
                        }
                        _ => (),
                    }
                }
            }
        }
        let full_order = graph.order(graph.entries.keys().cloned())?;
        for key in full_order {
            let own_v3 = graph
                .entries
                .get(&key)
                .is_some_and(|entry| match entry.source {
                    PropertySource::Expression(id) => graph.expressions[id].version >= 3,
                    _ => false,
                });
            if own_v3
                || graph.edges[&key]
                    .iter()
                    .any(|source| graph.v3_roots.contains(source))
            {
                graph.v3_roots.insert(key);
            }
        }
        graph.current_edges = graph.edges.clone();
        // Past-only references are static cycle edges, not current-time reads.
        for (key, entry) in &graph.entries {
            if graph.references.contains_key(key) {
                continue;
            }
            if let PropertySource::Expression(id) = entry.source {
                for node in &graph.expressions[id].nodes {
                    if let kronello_model::ExpressionNode::PropertySample {
                        node, property, ..
                    } = node
                    {
                        let source = graph.expression_key(&entry.source_scope, *node, *property);
                        let regular = graph.expressions[id].nodes.iter().any(|n| {
                            matches!(n,
                            kronello_model::ExpressionNode::Property { node: n, property: p, .. }
                            if graph.expression_key(&entry.source_scope, *n, *p) == source)
                        });
                        let declared = snapshot
                            .dependencies
                            .get(key)
                            .is_some_and(|deps| deps.contains(&source));
                        if !regular && !declared {
                            graph.current_edges.get_mut(key).unwrap().remove(&source);
                        }
                    }
                }
            }
        }

        for (key, entry) in &graph.entries {
            if let PropertySource::Expression(id) = entry.source {
                for node in &graph.expressions[id].nodes {
                    if let kronello_model::ExpressionNode::PropertySample {
                        node, property, ..
                    } = node
                    {
                        let source = graph.expression_key(&entry.source_scope, *node, *property);
                        if graph
                            .order(std::iter::once(source))?
                            .iter()
                            .any(|k| matches!(k, RuntimePropertyKey::LayoutValue { .. }))
                        {
                            return Err(crate::expression::error(
                                key,
                                kronello_model::ExpressionError::InvalidAst(
                                    "past layout sampling unsupported",
                                ),
                            ));
                        }
                    }
                }
            }
        }

        Ok(graph)
    }

    fn require_key(&self, key: &RuntimePropertyKey) -> Result<(), EvaluationError> {
        if self.edges.contains_key(key) {
            Ok(())
        } else {
            Err(EvaluationError::PropertyNotFound(key.clone()))
        }
    }
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &RuntimePropertyKey> {
        self.entries.keys()
    }
    pub fn dependencies(
        &self,
        key: &RuntimePropertyKey,
    ) -> Result<&BTreeSet<RuntimePropertyKey>, EvaluationError> {
        self.edges
            .get(key)
            .ok_or_else(|| EvaluationError::PropertyNotFound(key.clone()))
    }
    /// Static upstream-first schedule, including externally supplied layout
    /// projections. Upper compilers use it to produce inputs before consumers.
    pub fn dependency_order(
        &self,
        keys: &[RuntimePropertyKey],
    ) -> Result<Vec<RuntimePropertyKey>, EvaluationError> {
        self.order(keys.iter().cloned())
    }
    // Iterative DFS: active stack preserves the full closed cycle and convergent
    // dependencies are distinguished from back edges.
    fn order(
        &self,
        keys: impl IntoIterator<Item = RuntimePropertyKey>,
    ) -> Result<Vec<RuntimePropertyKey>, EvaluationError> {
        self.order_budgeted(keys, None, false)
    }
    fn order_budgeted(
        &self,
        keys: impl IntoIterator<Item = RuntimePropertyKey>,
        mut budget: Option<(&mut crate::expression::Usage, &RuntimePropertyKey)>,
        current_only: bool,
    ) -> Result<Vec<RuntimePropertyKey>, EvaluationError> {
        let edges = if current_only {
            &self.current_edges
        } else {
            &self.edges
        };
        let mut charge = |key: &RuntimePropertyKey| -> Result<(), EvaluationError> {
            if let Some((usage, owner)) = &mut budget {
                usage
                    .charge(
                        1,
                        3 * std::mem::size_of::<RuntimePropertyKey>()
                            + 192
                            + 3 * key.instance_path().ids().len().saturating_mul(16),
                        0,
                        Default::default(),
                    )
                    .map_err(|e| crate::expression::error(owner, e))?;
            }
            Ok(())
        };
        let mut finished = BTreeSet::new();
        let mut order = Vec::new();
        for key in keys {
            self.require_key(&key)?;
            if finished.contains(&key) {
                continue;
            }
            charge(&key)?;
            let mut stack = vec![(key.clone(), edges[&key].iter())];
            let mut active = BTreeMap::from([(key, 0)]);
            while let Some((_, dependencies)) = stack.last_mut() {
                if let Some(dependency) = dependencies.next() {
                    if let Some(index) = active.get(dependency) {
                        let mut path: Vec<_> =
                            stack[*index..].iter().map(|(key, _)| key.clone()).collect();
                        path.push(dependency.clone());
                        return Err(EvaluationError::DependencyCycle { path });
                    }
                    if !finished.contains(dependency) {
                        charge(dependency)?;
                        active.insert(dependency.clone(), stack.len());
                        stack.push((dependency.clone(), edges[dependency].iter()));
                    }
                } else {
                    let (key, _) = stack.pop().unwrap();
                    active.remove(&key);
                    finished.insert(key.clone());
                    order.push(key);
                }
            }
        }
        Ok(order)
    }

    pub fn local_time(&self, path: &InstancePath, time: Time) -> Result<Time, EvaluationError> {
        // Resolve only the requested scope, so inactive instances do not cause
        // out-of-domain maps to run during a scene query.
        let mut maps = Vec::new();
        let mut current = path;
        loop {
            let scope = self
                .scopes
                .get(current)
                .ok_or_else(|| EvaluationError::InstancePathNotFound(path.clone()))?;
            match &scope.parent {
                Some((parent, placement, _)) => {
                    maps.push(&placement.local_time_map);
                    current = parent;
                }
                None => break,
            }
        }
        maps.into_iter().rev().try_fold(time, |time, map| {
            map.map(time)
                .map_err(|source| EvaluationError::TimeMapping {
                    instance_path: path.clone(),
                    source,
                })
        })
    }

    pub fn evaluate_property(
        &self,
        key: &RuntimePropertyKey,
        time: Time,
    ) -> Result<Value, EvaluationError> {
        Ok(self
            .evaluate_properties(std::slice::from_ref(key), time)?
            .remove(key)
            .unwrap())
    }
    /// Return requested values in stable key order regardless of request order.
    /// Each call evaluates the dependency closure with fresh local memoization.
    pub fn evaluate_properties(
        &self,
        keys: &[RuntimePropertyKey],
        time: Time,
    ) -> Result<BTreeMap<RuntimePropertyKey, Value>, EvaluationError> {
        self.evaluate_properties_with_inputs(keys, time, &BTreeMap::new())
    }

    /// Upper compilation supplies semantic layout results and instance inputs.
    /// Layout consumers must declare their upstream text property dependencies
    /// when compiling this graph; the evaluator imports no text/template crate.
    /// Inputs replace values only for this immutable query, never document data.
    pub fn evaluate_properties_with_inputs(
        &self,
        keys: &[RuntimePropertyKey],
        time: Time,
        inputs: &BTreeMap<RuntimePropertyKey, Value>,
    ) -> Result<BTreeMap<RuntimePropertyKey, Value>, EvaluationError> {
        if self.expressions.is_empty() {
            return self.evaluate_closure(keys, time, inputs);
        }
        // A budget belongs to one requested property's dependency closure.
        // Batching, renderer caches and sibling requests cannot change it.
        let mut result = BTreeMap::new();
        for key in keys.iter().collect::<BTreeSet<_>>() {
            let value = self
                .evaluate_closure(std::slice::from_ref(key), time, inputs)?
                .remove(key)
                .unwrap();
            result.insert(key.clone(), value);
        }
        Ok(result)
    }

    fn evaluate_closure(
        &self,
        keys: &[RuntimePropertyKey],
        time: Time,
        inputs: &BTreeMap<RuntimePropertyKey, Value>,
    ) -> Result<BTreeMap<RuntimePropertyKey, Value>, EvaluationError> {
        self.evaluate_closure_shared(keys, time, inputs, &mut crate::expression::Usage::default())
    }

    pub(crate) fn evaluate_closure_shared(
        &self,
        keys: &[RuntimePropertyKey],
        time: Time,
        inputs: &BTreeMap<RuntimePropertyKey, Value>,
        usage: &mut crate::expression::Usage,
    ) -> Result<BTreeMap<RuntimePropertyKey, Value>, EvaluationError> {
        for (key, value) in inputs {
            self.require_key(key)?;
            if matches!(key, RuntimePropertyKey::LayoutValue { .. }) {
                if !matches!(value, Value::Vec2(_)) {
                    return Err(EvaluationError::MissingLayoutInput(key.clone()));
                }
                continue;
            }
            self.entries[key]
                .property
                .validate_final_value(value, self.registry)
                .map_err(|source| EvaluationError::InvalidValue {
                    key: key.clone(),
                    source,
                })?;
        }
        usage.bounded_schedule |= keys.iter().any(|key| self.v3_roots.contains(key));
        let order = if usage.bounded_schedule && !keys.is_empty() {
            self.order_budgeted(keys.iter().cloned(), Some((usage, &keys[0])), true)?
        } else {
            self.order_budgeted(keys.iter().cloned(), None, true)?
        };
        let expression_key = order
            .iter()
            .find(|key| {
                self.entries
                    .get(*key)
                    .is_some_and(|e| matches!(e.source, PropertySource::Expression(_)))
            })
            .cloned()
            .or_else(|| {
                (!self.expressions.is_empty())
                    .then(|| keys.first().cloned())
                    .flatten()
            });
        let mut values: BTreeMap<RuntimePropertyKey, Value> = BTreeMap::new();
        for key in order {
            if let Some(expression_key) = &expression_key {
                let payload = if let Some(value) = inputs.get(&key) {
                    kronello_model::expression_value_bytes(value)
                } else if let Some(source) = self.references.get(&key) {
                    kronello_model::expression_value_bytes(&values[source])
                } else if let Some(entry) = self.entries.get(&key) {
                    match entry.source {
                        PropertySource::Constant(v) => kronello_model::expression_value_bytes(v),
                        PropertySource::Curve(id) => {
                            let curve = self.curves.get(id).ok_or_else(|| {
                                EvaluationError::InvalidValue {
                                    key: key.clone(),
                                    source: ModelError::CurveNotFound { id: *id },
                                }
                            })?;
                            usage
                                .charge(65 + curve.keys().len(), 0, 1, Default::default())
                                .map_err(|e| crate::expression::error(expression_key, e))?;
                            curve
                                .keys()
                                .iter()
                                .map(|k| kronello_model::expression_value_bytes(&k.value))
                                .max()
                                .unwrap_or(0)
                        }
                        _ => 0,
                    }
                } else {
                    std::mem::size_of::<Value>()
                };
                usage
                    .charge(
                        1,
                        payload
                            + std::mem::size_of::<RuntimePropertyKey>()
                            + 64
                            + key.instance_path().ids().len().saturating_mul(16),
                        0,
                        Default::default(),
                    )
                    .map_err(|e| crate::expression::error(expression_key, e))?;
            }
            if matches!(key, RuntimePropertyKey::LayoutValue { .. }) {
                let value = inputs
                    .get(&key)
                    .ok_or_else(|| EvaluationError::MissingLayoutInput(key.clone()))?;
                values.insert(key, value.clone());
                continue;
            }
            let entry = &self.entries[&key];
            if let Some(modifier) = entry.property.modifiers().iter().find(|m| m.enabled) {
                return Err(EvaluationError::UnsupportedFeature {
                    key,
                    feature: format!("modifier {} version {}", modifier.key, modifier.version),
                });
            }
            let value = if let Some(value) = inputs.get(&key) {
                value.clone()
            } else if let Some(source) = self.edges[&key]
                .iter()
                .find(|source| matches!(source, RuntimePropertyKey::LayoutValue { .. }))
            {
                if self.edges[&key].len() != 1 {
                    return Err(EvaluationError::MissingLayoutInput(key.clone()));
                }
                values[source].clone()
            } else if let Some(source) = self.references.get(&key) {
                values[source].clone()
            } else {
                match entry.source {
                    PropertySource::Constant(value) => value.clone(),
                    PropertySource::Expression(id) => self
                        .run_expression(
                            self.expressions[id],
                            entry,
                            crate::expression::ExpressionContext {
                                instance: key.instance_path(),
                                time: self.local_time(&entry.source_scope, time)?,
                                root_time: time,
                                upstream: &values,
                                inputs,
                            },
                            usage,
                        )
                        .map_err(|source| crate::expression::error(&key, source))?,
                    PropertySource::Curve(id) => {
                        let curve =
                            self.curves
                                .get(id)
                                .ok_or_else(|| EvaluationError::InvalidValue {
                                    key: key.clone(),
                                    source: ModelError::CurveNotFound { id: *id },
                                })?;
                        let space = entry
                            .property
                            .descriptor()
                            .resolve(self.registry)
                            .map_err(|source| EvaluationError::InvalidValue {
                                key: key.clone(),
                                source,
                            })?
                            .definition()
                            .color_interpolation_space
                            .unwrap_or(self.working_space);
                        kronello_animation::sample_in_space(
                            curve,
                            self.local_time(&entry.source_scope, time)?,
                            space,
                        )
                        .map_err(|source| match source {
                            kronello_animation::AnimationError::Curve(
                                kronello_model::CurveError::UnsupportedInterpolationVersion {
                                    ..
                                },
                            ) => EvaluationError::UnsupportedFeature {
                                key: key.clone(),
                                feature: format!(
                                    "curve {id} interpolation version {}",
                                    curve.interpolation_version()
                                ),
                            },
                            source => EvaluationError::Animation {
                                key: key.clone(),
                                source,
                            },
                        })?
                    }
                }
            };
            entry
                .property
                .validate_final_value(&value, self.registry)
                .map_err(|source| EvaluationError::InvalidValue {
                    key: key.clone(),
                    source,
                })?;
            values.insert(key, value);
        }
        Ok(keys
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            // Move requested payloads out of the memo table. Cloning here
            // would allocate a second large output outside the memory charge.
            .map(|key| values.remove_entry(key).expect("evaluated requested key"))
            .collect())
    }
}

fn self_contract<'a>(
    entry: &Entry<'_>,
    registry: &'a SchemaRegistry,
) -> &'a kronello_model::DescriptorDefinition {
    // Composition validation already resolved every descriptor.
    entry
        .property
        .descriptor()
        .resolve(registry)
        .expect("validated descriptor")
        .definition()
}
