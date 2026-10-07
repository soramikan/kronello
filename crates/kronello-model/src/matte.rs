//! Authored, stable same-composition matte relations; independent of draw order.
use crate::{CompositionId, DocumentObject, NodeId, Project};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;
pub const DOCUMENT_MATTE_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentMatteKind {
    Alpha,
    Luminance,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatteRelation {
    pub id: Uuid,
    pub version: u32,
    pub composition: CompositionId,
    pub source: NodeId,
    pub matte: NodeId,
    pub kind: DocumentMatteKind,
    pub invert: bool,
    /// A consumed matte may also be shown in normal composition draw order.
    pub visible: bool,
}
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MatteError {
    #[error("unsupported matte semantics or opaque relation")]
    Unsupported,
    #[error("matte relation {id} references a missing composition or node")]
    Missing { id: Uuid },
    #[error("more than one matte relation targets the same source")]
    DuplicateSource,
    #[error("cyclic containment/matte dependency")]
    Cycle,
    #[error("matte relation budget exceeded")]
    BudgetExceeded,
}
impl MatteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported => "UNSUPPORTED_FEATURE",
            Self::Missing { .. } => "MATTE_MISSING",
            Self::DuplicateSource => "MATTE_DUPLICATE_SOURCE",
            Self::Cycle => "MATTE_CYCLE",
            Self::BudgetExceeded => "MATTE_BUDGET_EXCEEDED",
        }
    }
}
impl Project {
    pub fn validate_mattes(&self) -> Result<(), MatteError> {
        if self.mattes.len() > 4096 {
            return Err(MatteError::BudgetExceeded);
        }
        let mut sources = BTreeSet::new();
        for object in &self.mattes {
            let DocumentObject::Known(relation) = object else {
                return Err(MatteError::Unsupported);
            };
            if relation.version != DOCUMENT_MATTE_VERSION {
                return Err(MatteError::Unsupported);
            }
            let composition = self
                .compositions
                .iter()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id == relation.composition => Some(c),
                    _ => None,
                })
                .ok_or(MatteError::Missing { id: relation.id })?;
            if !composition.nodes.iter().any(|n| n.id == relation.source)
                || !composition.nodes.iter().any(|n| n.id == relation.matte)
            {
                return Err(MatteError::Missing { id: relation.id });
            }
            if !sources.insert((relation.composition, relation.source)) {
                return Err(MatteError::DuplicateSource);
            }
        }
        for object in &self.compositions {
            let DocumentObject::Known(composition) = object else {
                continue;
            };
            let mut edges: BTreeMap<NodeId, Vec<NodeId>> = composition
                .nodes
                .iter()
                .map(|n| (n.id, n.child_order.clone()))
                .collect();
            for relation in self.mattes.iter().filter_map(|m| match m {
                DocumentObject::Known(m) if m.composition == composition.id => Some(m),
                _ => None,
            }) {
                edges
                    .entry(relation.source)
                    .or_default()
                    .push(relation.matte);
            }
            fn visit(
                id: NodeId,
                edges: &BTreeMap<NodeId, Vec<NodeId>>,
                active: &mut BTreeSet<NodeId>,
                done: &mut BTreeSet<NodeId>,
                depth: usize,
            ) -> Result<(), MatteError> {
                if done.contains(&id) {
                    return Ok(());
                }
                if depth > 1024 {
                    return Err(MatteError::BudgetExceeded);
                }
                if !active.insert(id) {
                    return Err(MatteError::Cycle);
                }
                for next in edges.get(&id).into_iter().flatten() {
                    visit(*next, edges, active, done, depth + 1)?;
                }
                active.remove(&id);
                done.insert(id);
                Ok(())
            }
            let mut active = BTreeSet::new();
            let mut done = BTreeSet::new();
            for id in edges.keys() {
                visit(*id, &edges, &mut active, &mut done, 0)?;
            }
        }
        Ok(())
    }
}
