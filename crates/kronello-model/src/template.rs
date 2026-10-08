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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variants: BTreeMap<String, TemplateVariant>,
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
    MediaSlot { node: NodeId },
    DataTable { bindings: Vec<TemplateDataBinding> },
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
    Hold,
    Loop,
    Stretch,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateConstraints {
    #[serde(default)]
    pub bands: Vec<TemplateBandBinding>,
    #[serde(default)]
    pub max_lines: BTreeMap<NodeId, usize>,
    /// AI-003 (ADR-0126): tracking-driven crop windows on media nodes, one
    /// layout-derived rule per placement through the shared dependency path.
    #[serde(default)]
    pub smart_reframes: Vec<crate::SmartReframeRule>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateBandBinding {
    /// Explicit bounds stage; omitted legacy bindings keep the layout box.
    #[serde(default, skip_serializing_if = "BoundsStage::is_layout")]
    pub bounds: BoundsStage,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
}

/// Each aspect variant freezes a separate authored Composition and bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateVariant {
    pub composition_ref: CompositionId,
    pub targets: BTreeMap<String, TemplateInputTarget>,
    pub constraints: TemplateConstraints,
    pub content_hash: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateDataBinding {
    pub row: usize,
    pub column: String,
    pub target: TemplateCellTarget,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateCellTarget {
    Property { node: NodeId, property: PropertyId },
    Text { node: NodeId },
}

/// Resolution-independent bounds stage selected by a layout consumer.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BoundsStage {
    #[default]
    Layout,
    Ink,
    Visual,
}
impl BoundsStage {
    pub fn is_layout(&self) -> bool {
        *self == Self::Layout
    }
}
