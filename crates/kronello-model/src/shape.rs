//! Resolution-independent vector content. Parameters reference properties owned
//! by the drawing SceneNode, so existing instance-aware evaluation stays shared.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    Color, ContentId, CoordinateSpace, DescriptorDefinition, DescriptorId, DocumentObject,
    FiniteF64, ModelError, NodeKind, Path, PathSegment, Project, Property, PropertyDescriptor,
    PropertyId, PropertySource, SchemaKey, SchemaRegistry, Unit, Value, ValueType,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub id: ContentId,
    pub geometry: ShapeGeometry,
    pub fill: Option<Fill>,
    pub stroke: Option<Stroke>,
}

/// Size is [width, height] in local design_px, with top-left at [0, 0].
/// Ellipses fit that rectangle. Radius is a uniform circular corner radius;
/// derivation caps it to half the smaller dimension without changing the IR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ShapeGeometry {
    Rectangle {
        size: PropertyId,
        corner_radius: PropertyId,
    },
    Ellipse {
        size: PropertyId,
    },
    BezierPath {
        path: PropertyId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    Nonzero,
    Evenodd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fill {
    /// Color Property: explicitly tagged straight RGB and independent alpha.
    pub color: PropertyId,
    pub rule: FillRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: PropertyId,
    pub width: PropertyId,
    /// Enum Property: "miter", "round", or "bevel" (Hold interpolation).
    pub join: PropertyId,
    /// Enum Property: "butt", "round", or "square" (Hold interpolation).
    pub cap: PropertyId,
    /// Dimensionless Scalar Property, at least 1.
    pub miter_limit: PropertyId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

/// Evaluated, validated design-space values, never a serialized bitmap/cache.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShape {
    pub geometry: ResolvedGeometry,
    pub fill: Option<ResolvedFill>,
    pub stroke: Option<ResolvedStroke>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedGeometry {
    Rectangle {
        size: [FiniteF64; 2],
        corner_radius: FiniteF64,
    },
    Ellipse {
        size: [FiniteF64; 2],
    },
    BezierPath(Path),
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFill {
    pub color: Color,
    pub rule: FillRule,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStroke {
    pub color: Color,
    pub width: FiniteF64,
    pub join: StrokeJoin,
    pub cap: StrokeCap,
    pub miter_limit: FiniteF64,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ShapeError {
    #[error("missing shape content {id}")]
    MissingContent { id: ContentId },
    #[error("opaque content is unsupported")]
    UnsupportedContent,
    #[error("duplicate shape content {id}")]
    DuplicateContent { id: ContentId },
    #[error("missing property {id}")]
    MissingProperty { id: PropertyId },
    #[error("duplicate property {id}")]
    DuplicateProperty { id: PropertyId },
    #[error("incompatible descriptor for shape parameter {id}")]
    InvalidDescriptor { id: PropertyId },
    #[error("invalid value for shape parameter {id}: expected {expected:?}")]
    InvalidParameter { id: PropertyId, expected: ValueType },
    #[error("path command {index} has no open subpath; start with MoveTo")]
    InvalidPath { index: usize },
    #[error(transparent)]
    Model(#[from] ModelError),
}

#[derive(Clone, Copy)]
enum Parameter {
    Size,
    Radius,
    Path,
    Color,
    Width,
    Join,
    Cap,
    Miter,
}
impl Parameter {
    fn value_type(self) -> ValueType {
        match self {
            Self::Size => ValueType::Vec2,
            Self::Path => ValueType::Path,
            Self::Color => ValueType::Color,
            Self::Join | Self::Cap => ValueType::Enum,
            _ => ValueType::Scalar,
        }
    }
    fn unit(self) -> Unit {
        match self {
            Self::Size | Self::Radius | Self::Path | Self::Width => Unit::DesignPx,
            _ => Unit::Dimensionless,
        }
    }
    fn validate(self, id: PropertyId, value: &Value) -> Result<(), ShapeError> {
        let valid = match (self, value) {
            (Self::Size, Value::Vec2(v)) => v.iter().all(|v| v.get() >= 0.0),
            (Self::Radius | Self::Width, Value::Scalar(v)) => v.get() >= 0.0,
            (Self::Miter, Value::Scalar(v)) => v.get() >= 1.0,
            (Self::Path, Value::Path(path)) => {
                validate_path(path)?;
                true
            }
            (Self::Color, Value::Color(_)) => true,
            (Self::Join, Value::Enum(v)) => matches!(v.as_str(), "miter" | "round" | "bevel"),
            (Self::Cap, Value::Enum(v)) => matches!(v.as_str(), "butt" | "round" | "square"),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ShapeError::InvalidParameter {
                id,
                expected: self.value_type(),
            })
        }
    }
}

/// Empty paths and open subpaths are valid; every drawing command must follow
/// MoveTo. Close ends that subpath; another MoveTo starts the next one.
pub fn validate_path(path: &Path) -> Result<(), ShapeError> {
    let mut open = false;
    for (index, segment) in path.segments.iter().enumerate() {
        match segment {
            PathSegment::MoveTo(_) => open = true,
            PathSegment::Close if open => open = false,
            _ if !open => return Err(ShapeError::InvalidPath { index }),
            _ => (),
        }
    }
    Ok(())
}

impl Shape {
    fn parameters(&self) -> Vec<(PropertyId, Parameter)> {
        let mut refs = match self.geometry {
            ShapeGeometry::Rectangle {
                size,
                corner_radius,
            } => vec![(size, Parameter::Size), (corner_radius, Parameter::Radius)],
            ShapeGeometry::Ellipse { size } => vec![(size, Parameter::Size)],
            ShapeGeometry::BezierPath { path } => vec![(path, Parameter::Path)],
        };
        if let Some(fill) = &self.fill {
            refs.push((fill.color, Parameter::Color));
        }
        if let Some(stroke) = &self.stroke {
            refs.extend([
                (stroke.color, Parameter::Color),
                (stroke.width, Parameter::Width),
                (stroke.join, Parameter::Join),
                (stroke.cap, Parameter::Cap),
                (stroke.miter_limit, Parameter::Miter),
            ]);
        }
        refs
    }
    /// Stable Property IDs to pass to the instance-aware evaluator. Repeated
    /// references (e.g. one shared fill/stroke color) appear only once.
    pub fn property_ids(&self) -> Vec<PropertyId> {
        self.parameters()
            .into_iter()
            .map(|(id, _)| id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Checks node-local reference closure, descriptor types/units/coordinates,
    /// and final constant values. Curves/expressions/modifiers are not evaluated.
    pub fn validate(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), ShapeError> {
        let mut by_id = BTreeMap::new();
        for property in properties {
            if by_id.insert(property.id(), property).is_some() {
                return Err(ShapeError::DuplicateProperty { id: property.id() });
            }
        }
        for (id, parameter) in self.parameters() {
            let property = by_id.get(&id).ok_or(ShapeError::MissingProperty { id })?;
            property.validate(registry)?;
            let descriptor = property.descriptor().resolve(registry)?.definition();
            if descriptor.value_type != parameter.value_type()
                || descriptor.unit != parameter.unit()
                || (matches!(parameter, Parameter::Size | Parameter::Path)
                    && descriptor.coordinate_space != Some(CoordinateSpace::LocalDesign))
            {
                return Err(ShapeError::InvalidDescriptor { id });
            }
            if !property.modifiers().iter().any(|m| m.enabled)
                && let PropertySource::Constant(value) = property.source()
            {
                parameter.validate(id, value)?;
            }
        }
        Ok(())
    }
    /// Resolves final values for one placement/time after Property evaluation.
    /// Call validate first; the evaluator remains responsible for descriptor
    /// ranges after modifiers. Shape-specific constraints are checked again here.
    pub fn resolve(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedShape, ShapeError> {
        for (id, parameter) in self.parameters() {
            parameter.validate(
                id,
                values.get(&id).ok_or(ShapeError::MissingProperty { id })?,
            )?;
        }
        let size = |id| match values[&id] {
            Value::Vec2(v) => v,
            _ => unreachable!(),
        };
        let scalar = |id| match values[&id] {
            Value::Scalar(v) => v,
            _ => unreachable!(),
        };
        let color = |id| match &values[&id] {
            Value::Color(v) => *v,
            _ => unreachable!(),
        };
        let geometry = match self.geometry {
            ShapeGeometry::Rectangle {
                size: id,
                corner_radius,
            } => ResolvedGeometry::Rectangle {
                size: size(id),
                corner_radius: scalar(corner_radius),
            },
            ShapeGeometry::Ellipse { size: id } => ResolvedGeometry::Ellipse { size: size(id) },
            ShapeGeometry::BezierPath { path } => match &values[&path] {
                Value::Path(p) => ResolvedGeometry::BezierPath(p.clone()),
                _ => unreachable!(),
            },
        };
        let fill = self.fill.as_ref().map(|f| ResolvedFill {
            color: color(f.color),
            rule: f.rule,
        });
        let stroke = self.stroke.as_ref().map(|s| ResolvedStroke {
            color: color(s.color),
            width: scalar(s.width),
            miter_limit: scalar(s.miter_limit),
            join: match &values[&s.join] {
                Value::Enum(v) if v == "miter" => StrokeJoin::Miter,
                Value::Enum(v) if v == "round" => StrokeJoin::Round,
                _ => StrokeJoin::Bevel,
            },
            cap: match &values[&s.cap] {
                Value::Enum(v) if v == "butt" => StrokeCap::Butt,
                Value::Enum(v) if v == "round" => StrokeCap::Round,
                _ => StrokeCap::Square,
            },
        });
        Ok(ResolvedShape {
            geometry,
            fill,
            stroke,
        })
    }
}

/// Checks known Shape references across all known compositions. Opaque shapes
/// can be saved, but cannot be treated as executable known content.
pub fn validate_shape_contents(
    project: &Project,
    registry: &SchemaRegistry,
) -> Result<(), ShapeError> {
    let mut contents = BTreeMap::new();
    for object in &project.shapes {
        let id = match object {
            DocumentObject::Known(s) => s.id,
            DocumentObject::Opaque(o) => ContentId::from_uuid(o.id),
        };
        if contents.insert(id, object).is_some() {
            return Err(ShapeError::DuplicateContent { id });
        }
    }
    for object in &project.compositions {
        let DocumentObject::Known(composition) = object else {
            return Err(ShapeError::UnsupportedContent);
        };
        for node in &composition.nodes {
            if let NodeKind::Shape { content_ref } = node.kind {
                match contents
                    .get(&content_ref)
                    .ok_or(ShapeError::MissingContent { id: content_ref })?
                {
                    DocumentObject::Known(shape) => shape.validate(&node.properties, registry)?,
                    DocumentObject::Opaque(_) => return Err(ShapeError::UnsupportedContent),
                }
            }
        }
    }
    Ok(())
}

/// Additional descriptors, explicitly registered alongside with_builtin().
/// Fill color and width reuse kronello.fill_color/kronello.stroke_width.
/// Separate stroke_color permits independent paint values on one evaluated node.
/// Enum membership is a Shape constraint and is checked after evaluation.
pub fn shape_descriptors() -> Vec<PropertyDescriptor> {
    let n = |v| FiniteF64::new(v).expect("finite descriptor default");
    let entries = [
        (
            0x25e0da11_a016_488f_a65b_0173c694d5ec,
            "stroke_color",
            Unit::Dimensionless,
            Value::Color(Color::from_srgb8([0; 3], None)),
        ),
        (
            0x4b974eff_c701_42d8_a605_7a086edecabb,
            "size",
            Unit::DesignPx,
            Value::Vec2([n(100.0); 2]),
        ),
        (
            0x6b50a3ba_697f_43a0_a8a0_6003e3e04cb4,
            "corner_radius",
            Unit::DesignPx,
            Value::Scalar(n(0.0)),
        ),
        (
            0x58d7a46d_5ffb_4542_b8c5_716d33c23337,
            "path",
            Unit::DesignPx,
            Value::Path(Path { segments: vec![] }),
        ),
        (
            0xa4feff68_8d50_489f_92da_4b7f33d6d01f,
            "stroke_join",
            Unit::Dimensionless,
            Value::Enum("miter".into()),
        ),
        (
            0xaecfd2c0_d385_4ff1_b455_b3af8a606b32,
            "stroke_cap",
            Unit::Dimensionless,
            Value::Enum("butt".into()),
        ),
        (
            0xa5c12996_4c9e_4713_b98f_9a8e65353505,
            "miter_limit",
            Unit::Dimensionless,
            Value::Scalar(n(4.0)),
        ),
    ];
    entries
        .into_iter()
        .map(|(id, suffix, unit, default)| {
            let mut definition = DescriptorDefinition::new(
                DescriptorId::from_uuid(Uuid::from_u128(id)),
                SchemaKey::new(format!("kronello.shape.{suffix}")).expect("valid shape key"),
                suffix,
                default.value_type(),
                unit,
                default,
            );
            let range = |min| crate::NumericRange {
                min: Some(crate::NumericBound {
                    value: n(min),
                    inclusive: true,
                }),
                max: None,
            };
            match suffix {
                "size" => definition.range = Some(crate::ValueRange::Vec2([range(0.0); 2])),
                "corner_radius" => definition.range = Some(crate::ValueRange::Scalar(range(0.0))),
                "miter_limit" => definition.range = Some(crate::ValueRange::Scalar(range(1.0))),
                _ => (),
            }
            PropertyDescriptor::new(definition).expect("valid shape descriptor")
        })
        .collect()
}
