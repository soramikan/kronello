//! SVG conversion creates ordinary shared edit plans; import never mutates state
//! outside revision validation, transactions, idempotency and selective undo.
use crate::{EditCommand, EditPlan, PlanRequest, ServiceError};
use kronello_model::{
    CompositionId, ContentId, DescriptorRef, Fill, Property, PropertyId, PropertySource, SchemaKey,
    Shape, ShapeGeometry, Value,
};
use kronello_vector::{SvgError, SvgPath, SvgReport};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgInspectRequest {
    pub svg: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgExportRequest {
    pub paths: Vec<SvgPath>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgExportResult {
    pub svg: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgImportTarget {
    pub shape: ContentId,
    pub path_property: PropertyId,
    pub fill_property: PropertyId,
    pub node: kronello_model::SceneNode,
    pub index: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SvgImportPlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub composition: CompositionId,
    /// Explicit IDs and placement, one destination for each SVG path.
    pub targets: Vec<SvgImportTarget>,
    pub svg: String,
}
pub(crate) fn svg_error(error: SvgError) -> ServiceError {
    ServiceError::new(
        match error {
            SvgError::BudgetExceeded => "SVG_BUDGET_EXCEEDED",
            SvgError::InvalidDocument => "SVG_INVALID_DOCUMENT",
            SvgError::Unsupported => "UNSUPPORTED_FEATURE",
        },
        error.to_string(),
    )
}
pub(crate) fn inspect(request: SvgInspectRequest) -> Result<SvgReport, ServiceError> {
    kronello_vector::inspect_svg(&request.svg).map_err(svg_error)
}
pub(crate) fn export(request: SvgExportRequest) -> Result<SvgExportResult, ServiceError> {
    Ok(SvgExportResult {
        svg: kronello_vector::export_svg(&request.paths).map_err(svg_error)?,
    })
}
pub(crate) fn import_plan(request: SvgImportPlanRequest) -> Result<EditPlan, ServiceError> {
    let report = kronello_vector::inspect_svg(&request.svg).map_err(svg_error)?;
    if report.ensure_supported().is_err() {
        let mut error = ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "SVG import refused: unsupported features or external references",
        );
        error.details =
            Some(serde_json::to_value(report).map_err(|e| ServiceError::invalid(e.to_string()))?);
        return Err(error);
    }
    if report.paths.len() != request.targets.len() {
        return Err(ServiceError::invalid(
            "SVG target count must equal imported path count",
        ));
    }
    let registry = kronello_render::render_registry();
    let make = |id, key: &str, value| -> Result<Property, ServiceError> {
        let descriptor = registry
            .lookup(&SchemaKey::new(key).map_err(|e| ServiceError::invalid(e.to_string()))?)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        Property::new(
            id,
            DescriptorRef::new(descriptor),
            PropertySource::Constant(value),
            vec![],
            &registry,
        )
        .map_err(|e| ServiceError::invalid(e.to_string()))
    };
    let mut commands = vec![];
    for (path, target) in report.paths.into_iter().zip(request.targets) {
        let mut node = target.node;

        if !matches!(node.kind,kronello_model::NodeKind::Shape { content_ref } if content_ref==target.shape)
        {
            return Err(ServiceError::invalid(
                "SVG target node must reference its explicit shape ID",
            ));
        }
        let fill = if let Some(color) = path.fill {
            node.properties.push(make(
                target.fill_property,
                "kronello.fill_color",
                Value::Color(color),
            )?);
            Some(Fill {
                gradient: None,
                color: target.fill_property,
                rule: path.fill_rule,
            })
        } else {
            None
        };
        node.properties.push(make(
            target.path_property,
            "kronello.shape.path",
            Value::Path(path.path),
        )?);
        commands.push(EditCommand::NodeAdd {
            composition: request.composition,
            node,
            index: target.index,
        });
        commands.push(EditCommand::ShapeSet {
            shape: Shape {
                id: target.shape,
                geometry: ShapeGeometry::BezierPath {
                    path: target.path_property,
                },
                fill,
                stroke: None,
            },
        });
    }
    crate::edit::plan(PlanRequest {
        project: request.project,
        base_revision: request.base_revision,
        commands,
    })
}
