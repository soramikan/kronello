//! Versioned particle source and ordinary authored dynamics Property references.
use crate::*;
use kronello_time::{Duration, Time};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;
pub const SIMULATION_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParticlePropertyInputs {
    pub origin: PropertyId,
    pub velocity: PropertyId,
    pub jitter: PropertyId,
    pub acceleration: PropertyId,
    pub enabled: PropertyId,
    pub birth_count: PropertyId,
}
impl ParticlePropertyInputs {
    pub fn ids(&self) -> [PropertyId; 6] {
        [
            self.origin,
            self.velocity,
            self.jitter,
            self.acceleration,
            self.enabled,
            self.birth_count,
        ]
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParticleSimulation {
    pub id: ContentId,
    pub version: u32,
    pub source: RepeatSource,
    pub start: Time,
    pub step: Duration,
    pub lifetime: Duration,
    pub emission_interval: Duration,
    pub seed: u64,
    pub inputs: ParticlePropertyInputs,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_bindings: BTreeMap<PropertyId, PropertySource<Value>>,
}
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SimulationModelError {
    #[error("unsupported simulation version {0}")]
    UnsupportedVersion(u32),
    #[error("missing or opaque simulation {0}")]
    Missing(ContentId),
    #[error("invalid simulation clock or duration")]
    Clock,
    #[error("invalid simulation source root")]
    Source,
    #[error("simulation source dependency cycle")]
    Cycle,
    #[error("invalid simulation dynamics Property ownership or type")]
    Inputs,
    #[error("invalid or ambiguous simulation identity aliases")]
    Identity,
}
impl SimulationModelError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedVersion(_) => "UNSUPPORTED_FEATURE",
            Self::Missing(_) => "SIMULATION_MISSING",
            Self::Clock => "SIMULATION_CLOCK",
            Self::Source => "SIMULATION_SOURCE",
            Self::Cycle => "SIMULATION_CYCLE",
            Self::Inputs => "SIMULATION_INPUT",
            Self::Identity => "SIMULATION_IDENTITY",
        }
    }
}
pub fn simulation_descriptors() -> Vec<PropertyDescriptor> {
    let zero = || Value::Vec2([FiniteF64::new(0.).unwrap(); 2]);
    let entries = [
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710001,
            "origin",
            Unit::DesignPx,
            zero(),
        ),
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710002,
            "velocity",
            Unit::DesignPxPerSecond,
            zero(),
        ),
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710003,
            "jitter",
            Unit::DesignPxPerSecond,
            zero(),
        ),
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710004,
            "acceleration",
            Unit::DesignPxPerSecondSquared,
            zero(),
        ),
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710005,
            "enabled",
            Unit::Dimensionless,
            Value::Bool(true),
        ),
        (
            0x8d8f6a33_52a1_4ac0_9a7b_2aa1fe710006,
            "birth_count",
            Unit::Dimensionless,
            Value::Scalar(FiniteF64::new(1.).unwrap()),
        ),
    ];
    entries
        .into_iter()
        .map(|(id, key, unit, value)| {
            let mut d = DescriptorDefinition::new(
                DescriptorId::from_uuid(Uuid::from_u128(id)),
                SchemaKey::new(format!("kronello.simulation.{key}")).unwrap(),
                key,
                value.value_type(),
                unit,
                value,
            );
            if key == "birth_count" {
                d.range = Some(ValueRange::Scalar(NumericRange {
                    min: Some(NumericBound {
                        value: FiniteF64::new(0.).unwrap(),
                        inclusive: true,
                    }),
                    max: Some(NumericBound {
                        value: FiniteF64::new(32.).unwrap(),
                        inclusive: true,
                    }),
                }));
            }
            PropertyDescriptor::new(d).unwrap()
        })
        .collect()
}
impl Project {
    pub fn simulation(&self, id: ContentId) -> Result<&ParticleSimulation, SimulationModelError> {
        self.simulations
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) if s.id == id => Some(s),
                _ => None,
            })
            .ok_or(SimulationModelError::Missing(id))
    }
    pub fn simulation_context(
        &self,
    ) -> Result<BTreeMap<ContentId, ContentId>, SimulationModelError> {
        let mut aliases = BTreeMap::new();
        for r in &self.repeaters {
            if let DocumentObject::Known(r) = r {
                for i in &r.instances {
                    if let Some(expanded) = &i.expanded_source {
                        for (key, value) in &expanded.simulation_aliases {
                            if key == value
                                || aliases
                                    .insert(*key, *value)
                                    .is_some_and(|old| old != *value)
                            {
                                return Err(SimulationModelError::Identity);
                            }
                            self.simulation(*key)?;
                            self.simulation(*value)?;
                        }
                    }
                }
            }
        }
        let mut flat = BTreeMap::new();
        for key in aliases.keys() {
            let mut current = *key;
            let mut visited = BTreeSet::new();
            while let Some(target) = aliases.get(&current) {
                if !visited.insert(current) {
                    return Err(SimulationModelError::Identity);
                }
                current = *target;
            }
            flat.insert(*key, current);
        }
        Ok(flat)
    }
    pub fn validate_simulations(&self) -> Result<(), SimulationModelError> {
        self.simulation_context()?;
        let mut ids = BTreeSet::new();
        for object in &self.simulations {
            let DocumentObject::Known(s) = object else {
                return Err(SimulationModelError::UnsupportedVersion(0));
            };
            if s.version != SIMULATION_VERSION {
                return Err(SimulationModelError::UnsupportedVersion(s.version));
            }
            if !ids.insert(s.id)
                || s.step == Duration::ZERO
                || s.lifetime == Duration::ZERO
                || s.emission_interval == Duration::ZERO
            {
                return Err(SimulationModelError::Clock);
            }
            let interval = s
                .emission_interval
                .as_time()
                .checked_div(s.step.as_time())
                .map_err(|_| SimulationModelError::Clock)?;
            if interval.denominator() != 1 {
                return Err(SimulationModelError::Clock);
            }
            let c = self
                .compositions
                .iter()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == s.source.composition => Some(c),
                    _ => None,
                })
                .ok_or(SimulationModelError::Source)?;
            if s.source_bindings
                .keys()
                .any(|id| !c.properties.iter().any(|p| p.id() == *id))
            {
                return Err(SimulationModelError::Inputs);
            }
            if c.root_nodes != [s.source.root]
                || !c.nodes.iter().any(|n| {
                    n.id == s.source.root
                        && n.containment_parent.is_none()
                        && n.transform_parent.is_none()
                })
            {
                return Err(SimulationModelError::Source);
            }
        }
        let definitions = self
            .compositions
            .iter()
            .filter_map(|c| match c {
                DocumentObject::Known(c) => Some(c),
                _ => None,
            })
            .map(|c| {
                self.lower_repeater_composition(c)
                    .map_err(|_| SimulationModelError::Source)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut owners = BTreeSet::new();
        for c in &definitions {
            for n in &c.nodes {
                if let NodeKind::Simulation { content_ref } = n.kind {
                    let s = self.simulation(content_ref)?;
                    if !owners.insert(s.id) || !n.child_order.is_empty() {
                        return Err(SimulationModelError::Inputs);
                    }
                    for (id, key) in s.inputs.ids().into_iter().zip([
                        "origin",
                        "velocity",
                        "jitter",
                        "acceleration",
                        "enabled",
                        "birth_count",
                    ]) {
                        if !n.properties.iter().any(|p| {
                            p.id() == id
                                && p.descriptor().key.as_str()
                                    == format!("kronello.simulation.{key}")
                        }) {
                            return Err(SimulationModelError::Inputs);
                        }
                    }
                }
            }
        }
        fn visit(
            project: &Project,
            id: CompositionId,
            defs: &[Composition],
            visiting: &mut BTreeSet<CompositionId>,
            done: &mut BTreeSet<CompositionId>,
        ) -> Result<(), SimulationModelError> {
            if done.contains(&id) {
                return Ok(());
            }
            if !visiting.insert(id) {
                return Err(SimulationModelError::Cycle);
            }
            let c = defs
                .iter()
                .find(|c| c.id == id)
                .ok_or(SimulationModelError::Source)?;
            for n in &c.nodes {
                let child = match &n.kind {
                    NodeKind::CompositionInstance(i) => Some(i.definition_ref),
                    NodeKind::Simulation { content_ref } => {
                        Some(project.simulation(*content_ref)?.source.composition)
                    }
                    _ => None,
                };
                if let Some(child) = child {
                    visit(project, child, defs, visiting, done)?;
                }
            }
            visiting.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut visiting = BTreeSet::new();
        let mut done = BTreeSet::new();
        for c in &definitions {
            visit(self, c.id, &definitions, &mut visiting, &mut done)?;
        }
        Ok(())
    }
}
