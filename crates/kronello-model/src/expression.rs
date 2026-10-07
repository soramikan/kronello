//! Canonical postorder AST. Indices are local syntax positions, never object IDs.
use crate::{CurveId, ExpressionId, NodeId, PropertyId, Value, ValueType};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Legacy authored expression and absent snapshot pin meaning.
pub const EXPRESSION_VERSION: u32 = 1;
/// Maximum explicitly supported expression semantics in newly captured snapshots.
pub const EXPRESSION_SUPPORTED_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpressionDependency {
    Property {
        node: Option<NodeId>,
        property: PropertyId,
    },
    Curve(CurveId),
    AudioAnalysis(crate::AssetId),
    DataAsset(crate::AssetId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExpressionBudget {
    pub instructions: usize,
    pub memory_bytes: usize,
    pub samples: usize,
    pub nodes: usize,
    pub dependencies: usize,
}
impl Default for ExpressionBudget {
    fn default() -> Self {
        Self {
            instructions: 4096,
            memory_bytes: 1_048_576,
            samples: 64,
            nodes: 1024,
            dependencies: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expression {
    pub id: ExpressionId,
    pub version: u32,
    pub value_type: ValueType,
    #[serde(default)]
    pub budget: ExpressionBudget,
    /// Ordered postorder tree; the last node is the root. No sharing or dead nodes.
    pub nodes: Vec<ExpressionNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionNode {
    Literal(Value),
    /// Seconds in the authored source scope, after its rational time mapping.
    Time,
    AudioFeature {
        asset: crate::AssetId,
        feature: crate::AudioFeature,
        offset: Time,
    },
    /// Stable property in the authored source scope; None means composition input.
    Property {
        node: Option<NodeId>,
        property: PropertyId,
        value_type: ValueType,
    },
    /// Integer row operand and statically typed column in immutable table data.
    DataAssetCell {
        asset: crate::AssetId,
        column: String,
        row: u32,
        value_type: ValueType,
    },
    /// Nonnegative lookback seconds on the root timeline, quantized to 1 ns.
    /// Static identity remains a dependency even when sampling the past.
    PropertySample {
        node: Option<NodeId>,
        property: PropertyId,
        value_type: ValueType,
        lookback: u32,
    },
    /// Explicit rational offset.
    CurveSample {
        curve: CurveId,
        offset: Time,
        value_type: ValueType,
    },
    Add {
        left: u32,
        right: u32,
    },
    Subtract {
        left: u32,
        right: u32,
    },
    Multiply {
        left: u32,
        right: u32,
    },
    Divide {
        left: u32,
        right: u32,
    },
    Clamp {
        value: u32,
        min: u32,
        max: u32,
    },
    Lerp {
        from: u32,
        to: u32,
        amount: u32,
    },
    /// Radians, explicitly dimensionless scalar input.
    Sin {
        input: u32,
    },
    Vec2 {
        x: u32,
        y: u32,
    },
    Vec3 {
        x: u32,
        y: u32,
        z: u32,
    },
    Angle {
        degrees: u32,
    },
    /// Quintic interpolation between adjacent fixed lattice hashes in [-1, 1].
    ContinuousNoise {
        seed: u32,
        element: u32,
        input: u32,
    },
    /// Fixed hash noise in [-1, 1], keyed by seed, instance, element and input.
    Noise {
        seed: u32,
        element: u32,
        input: u32,
    },
}
impl ExpressionNode {
    pub fn operands(&self) -> Vec<u32> {
        match self {
            Self::Add { left, right }
            | Self::Subtract { left, right }
            | Self::Multiply { left, right }
            | Self::Divide { left, right } => vec![*left, *right],
            Self::Clamp { value, min, max } => vec![*value, *min, *max],
            Self::Lerp { from, to, amount } => vec![*from, *to, *amount],
            Self::Sin { input }
            | Self::Noise { input, .. }
            | Self::ContinuousNoise { input, .. } => vec![*input],
            Self::PropertySample { lookback, .. } => vec![*lookback],
            Self::DataAssetCell { row, .. } => vec![*row],
            Self::Vec2 { x, y } => vec![*x, *y],
            Self::Vec3 { x, y, z } => vec![*x, *y, *z],
            Self::Angle { degrees } => vec![*degrees],
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExpressionError {
    #[error("unsupported expression version {0}")]
    UnsupportedVersion(u32),
    #[error("expression budget exhausted: {0}")]
    Budget(&'static str),
    #[error("invalid expression AST: {0}")]
    InvalidAst(&'static str),
    #[error("expression type mismatch at node {0}")]
    TypeMismatch(usize),
    #[error("expression arithmetic failure: {0}")]
    Arithmetic(&'static str),
}
impl ExpressionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedVersion(_) => "UNSUPPORTED_FEATURE",
            Self::Budget(_) => "EXPRESSION_BUDGET_EXCEEDED",
            _ => "EVALUATION_ERROR",
        }
    }
}

/// Conservative live allocation accounting, including owned payloads.
pub fn expression_value_bytes(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::String(s) | Value::Enum(s) => s.len(),
            Value::Path(p) => p
                .segments
                .len()
                .saturating_mul(std::mem::size_of::<crate::PathSegment>()),
            _ => 0,
        }
}
impl Expression {
    /// Complete static references; no runtime name resolution or node search.
    pub fn dependencies(&self) -> std::collections::BTreeSet<ExpressionDependency> {
        self.nodes
            .iter()
            .filter_map(|node| match node {
                ExpressionNode::Property { node, property, .. }
                | ExpressionNode::PropertySample { node, property, .. } => {
                    Some(ExpressionDependency::Property {
                        node: *node,
                        property: *property,
                    })
                }
                ExpressionNode::CurveSample { curve, .. } => {
                    Some(ExpressionDependency::Curve(*curve))
                }
                ExpressionNode::DataAssetCell { asset, .. } => {
                    Some(ExpressionDependency::DataAsset(*asset))
                }
                ExpressionNode::AudioFeature { asset, .. } => {
                    Some(ExpressionDependency::AudioAnalysis(*asset))
                }
                _ => None,
            })
            .collect()
    }
    pub fn validate(&self) -> Result<(), ExpressionError> {
        if !matches!(self.version, 1..=3) {
            return Err(ExpressionError::UnsupportedVersion(self.version));
        }
        if self.version == 1
            && self
                .nodes
                .iter()
                .any(|n| matches!(n, ExpressionNode::AudioFeature { .. }))
        {
            return Err(ExpressionError::UnsupportedVersion(self.version));
        }
        if self.version < 3
            && self.nodes.iter().any(|n| {
                matches!(
                    n,
                    ExpressionNode::PropertySample { .. }
                        | ExpressionNode::ContinuousNoise { .. }
                        | ExpressionNode::DataAssetCell { .. }
                )
            })
        {
            return Err(ExpressionError::UnsupportedVersion(self.version));
        }
        let ceiling = ExpressionBudget::default();
        for (name, requested, maximum) in [
            (
                "instructions",
                self.budget.instructions,
                ceiling.instructions,
            ),
            (
                "memory_bytes",
                self.budget.memory_bytes,
                ceiling.memory_bytes,
            ),
            ("samples", self.budget.samples, ceiling.samples),
            ("nodes", self.budget.nodes, ceiling.nodes),
            (
                "dependencies",
                self.budget.dependencies,
                ceiling.dependencies,
            ),
        ] {
            if requested > maximum {
                return Err(ExpressionError::Budget(name));
            }
        }
        if self.nodes.is_empty() {
            return Err(ExpressionError::InvalidAst("empty tree"));
        }
        if self.nodes.len() > self.budget.nodes {
            return Err(ExpressionError::Budget("nodes"));
        }
        let mut bytes = self.nodes.len().saturating_mul(
            std::mem::size_of::<ExpressionNode>()
                + std::mem::size_of::<ValueType>()
                + std::mem::size_of::<usize>(),
        );
        for node in &self.nodes {
            if let ExpressionNode::DataAssetCell { column, .. } = node {
                bytes = bytes.saturating_add(column.len());
            }
            if let ExpressionNode::Literal(v) = node {
                bytes = bytes.saturating_add(expression_value_bytes(v));
            }
        }
        if bytes > self.budget.memory_bytes {
            return Err(ExpressionError::Budget("memory_bytes"));
        }
        let mut types = Vec::with_capacity(self.nodes.len());
        let mut starts = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let children = node.operands();
            let start = children
                .first()
                .map_or(i, |c| starts.get(*c as usize).copied().unwrap_or(i));
            let mut next = start;
            for child in children {
                let child = child as usize;
                if child >= i || starts[child] != next {
                    return Err(ExpressionError::InvalidAst(
                        "expected ordered postorder tree",
                    ));
                }
                next = child + 1;
                if types[child] != ValueType::Scalar {
                    return Err(ExpressionError::TypeMismatch(i));
                }
            }
            if next != i {
                return Err(ExpressionError::InvalidAst("shared or unreachable nodes"));
            }
            let ty = match node {
                ExpressionNode::Literal(value) => value.value_type(),
                ExpressionNode::Property { value_type, .. }
                | ExpressionNode::PropertySample { value_type, .. } => *value_type,
                ExpressionNode::CurveSample { value_type, .. }
                | ExpressionNode::DataAssetCell { value_type, .. } => *value_type,
                ExpressionNode::Vec2 { .. } => ValueType::Vec2,
                ExpressionNode::Vec3 { .. } => ValueType::Vec3,
                ExpressionNode::Angle { .. } => ValueType::Angle,
                _ => ValueType::Scalar,
            };
            types.push(ty);
            starts.push(start);
        }
        if starts.last() != Some(&0) {
            return Err(ExpressionError::InvalidAst("unreachable nodes"));
        }
        if types.last() != Some(&self.value_type) {
            return Err(ExpressionError::TypeMismatch(self.nodes.len() - 1));
        }
        if self.dependencies().len() > self.budget.dependencies {
            return Err(ExpressionError::Budget("dependencies"));
        }
        Ok(())
    }
}
