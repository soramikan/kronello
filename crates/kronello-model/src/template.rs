//! Versioned template contracts. Definition IDs identify immutable editions.
use crate::{
    CompositionId, CompositionInstanceId, FiniteF64, NodeId, PropertyId, Value, ValueType,
};
use kronello_time::Duration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateDefinition {
    pub id: Uuid,
    pub template_id: Uuid,
    pub version: String,
    pub composition_ref: CompositionId,
    pub public_inputs: BTreeMap<String, TemplateInput>,
    pub duration_policy: TemplateDurationPolicy,
    pub constraints: TemplateConstraints,
    /// Hash of the complete reachable authoring content, fixed by define.
    pub content_hash: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateInput {
    pub value_type: ValueType,
    pub default: Value,
    pub target: TemplateInputTarget,
    pub minimum: Option<FiniteF64>,
    pub maximum: Option<FiniteF64>,
    #[serde(default)]
    pub choices: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateInputTarget {
    Property { node: NodeId, property: PropertyId },
    Text { node: NodeId },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateDurationPolicy {
    pub intro: Duration,
    pub outro: Duration,
    pub minimum_middle: Duration,
    pub middle_mode: TemplateMiddleMode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateMiddleMode {
    Stretch,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateConstraints {
    #[serde(default)]
    pub bands: Vec<TemplateBandBinding>,
    #[serde(default)]
    pub max_lines: BTreeMap<NodeId, usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateBandBinding {
    pub text_node: NodeId,
    pub band_node: NodeId,
    pub size_property: PropertyId,
    pub position_property: PropertyId,
    pub padding: [FiniteF64; 2],
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateInstance {
    pub id: CompositionInstanceId,
    pub definition_ref: Uuid,
    pub version: String,
    pub duration: Duration,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
}
