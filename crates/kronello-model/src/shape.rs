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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Box<Gradient>>,
    /// Color Property: explicitly tagged straight RGB and independent alpha.
    pub color: PropertyId,
    pub rule: FillRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    /// Absent options retain the exact VEC-003 execution path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Box<StrokeOptions>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Box<Gradient>>,
    pub color: PropertyId,
    pub width: PropertyId,
    /// Enum Property: "miter", "round", or "bevel" (Hold interpolation).
    pub join: PropertyId,
    /// Enum Property: "butt", "round", or "square" (Hold interpolation).
    pub cap: PropertyId,
    /// Dimensionless Scalar Property, at least 1.
    pub miter_limit: PropertyId,
}

pub const LEGACY_STROKE_VERSION: &str = "vec003-centered-stroke-v1";
pub const EXTENDED_STROKE_VERSION: &str = "vec005-local-stroke-v2";

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum StrokeAlignment {
    #[default]
    Center,
    Inside,
    Outside,
}

/// Explicit opt-in; lengths are local design_px, offset is an evaluated Property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StrokeOptions {
    pub geometry_version: String,
    pub alignment: StrokeAlignment,
    /// Defines the interior independently of whether the shape has fill paint.
    pub fill_rule: FillRule,
    pub dash_array: Vec<FiniteF64>,
    pub dash_offset: PropertyId,
}
impl StrokeOptions {
    pub fn validate(&self) -> Result<(), ShapeError> {
        if self.geometry_version != EXTENDED_STROKE_VERSION {
            return Err(ShapeError::UnsupportedStrokeVersion);
        }
        if self.dash_array.len() > 256 {
            return Err(ShapeError::StrokeBudgetExceeded);
        }
        validate_dash_array(&self.dash_array.iter().map(|x| x.get()).collect::<Vec<_>>())
    }
}

pub fn validate_dash_array(array: &[f64]) -> Result<(), ShapeError> {
    if array.len() > 256 {
        return Err(ShapeError::StrokeBudgetExceeded);
    }
    if array.iter().any(|x| !x.is_finite() || *x < 0.0)
        || (!array.is_empty() && array.iter().all(|x| *x == 0.0))
        || !(array.iter().sum::<f64>() * if array.len() % 2 == 1 { 2.0 } else { 1.0 }).is_finite()
    {
        return Err(ShapeError::InvalidDashArray);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

/// Versioned paint settings. Missing settings preserve VEC-003 semantics.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GradientSpread {
    #[default]
    Pad,
    Repeat,
    Reflect,
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GradientInterpolation {
    #[default]
    WorkingLinearPremultiplied,
    WorkingLinearStraight,
    SrgbStraight,
    SrgbPremultiplied,
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GradientUnits {
    #[default]
    LocalDesign,
    ObjectBoundingBox,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GradientOptions {
    pub spread: GradientSpread,
    pub interpolation: GradientInterpolation,
    pub interpolation_version: u32,
    pub units: GradientUnits,
    /// Gradient coordinates to unit coordinates; bbox mapping is applied next.
    pub transform: [[FiniteF64; 3]; 2],
}
impl Default for GradientOptions {
    fn default() -> Self {
        let f = |v| FiniteF64::new(v).expect("finite identity");
        Self {
            spread: GradientSpread::Pad,
            interpolation: GradientInterpolation::default(),
            interpolation_version: 1,
            units: GradientUnits::LocalDesign,
            transform: [[f(1.0), f(0.0), f(0.0)], [f(0.0), f(1.0), f(0.0)]],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Gradient {
    Linear {
        start: [FiniteF64; 2],
        end: [FiniteF64; 2],
        stops: Vec<GradientStop>,
        #[serde(default)]
        options: GradientOptions,
    },
    Radial {
        center: [FiniteF64; 2],
        radius: FiniteF64,
        stops: Vec<GradientStop>,
        #[serde(default)]
        options: GradientOptions,
    },
    FocalRadial {
        center: [FiniteF64; 2],
        radius: FiniteF64,
        focal: [FiniteF64; 2],
        focal_radius: FiniteF64,
        stops: Vec<GradientStop>,
        #[serde(default)]
        options: GradientOptions,
    },
    Conic {
        center: [FiniteF64; 2],
        start_angle: FiniteF64,
        sweep_angle: FiniteF64,
        stops: Vec<GradientStop>,
        #[serde(default)]
        options: GradientOptions,
    },
}

// Deserialize fields directly from JSON rather than serde's internally tagged
// Content buffer, which turns arbitrary-precision decimal numbers into maps.
// This also retains strict variant fields and duplicate-field rejection.
impl<'de> Deserialize<'de> for Gradient {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use crate::project::{take_field, unique_fields};
        let mut fields = unique_fields(deserializer)?;
        let kind: String = take_field::<_, D::Error>(&mut fields, "kind")?;
        let stops = take_field::<_, D::Error>(&mut fields, "stops")?;
        let options = if fields.contains_key("options") {
            take_field::<_, D::Error>(&mut fields, "options")?
        } else {
            GradientOptions::default()
        };
        let gradient = match kind.as_str() {
            "linear" => Self::Linear {
                start: take_field::<_, D::Error>(&mut fields, "start")?,
                end: take_field::<_, D::Error>(&mut fields, "end")?,
                stops,
                options,
            },
            "radial" => Self::Radial {
                center: take_field::<_, D::Error>(&mut fields, "center")?,
                radius: take_field::<_, D::Error>(&mut fields, "radius")?,
                stops,
                options,
            },
            "focal_radial" => Self::FocalRadial {
                center: take_field::<_, D::Error>(&mut fields, "center")?,
                radius: take_field::<_, D::Error>(&mut fields, "radius")?,
                focal: take_field::<_, D::Error>(&mut fields, "focal")?,
                focal_radius: take_field::<_, D::Error>(&mut fields, "focal_radius")?,
                stops,
                options,
            },
            "conic" => Self::Conic {
                center: take_field::<_, D::Error>(&mut fields, "center")?,
                start_angle: take_field::<_, D::Error>(&mut fields, "start_angle")?,
                sweep_angle: take_field::<_, D::Error>(&mut fields, "sweep_angle")?,
                stops,
                options,
            },
            _ => return Err(serde::de::Error::custom("unknown gradient kind")),
        };
        if let Some(field) = fields.keys().next() {
            return Err(serde::de::Error::custom(format!(
                "unknown gradient field: {field}"
            )));
        }
        Ok(gradient)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub color: PropertyId,
    pub offset: PropertyId,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedGradient {
    pub options: GradientOptions,
    pub geometry: GradientGeometry,
    pub stops: Vec<ResolvedGradientStop>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GradientGeometry {
    Linear {
        start: [f64; 2],
        end: [f64; 2],
    },
    Radial {
        center: [f64; 2],
        radius: f64,
    },
    FocalRadial {
        center: [f64; 2],
        radius: f64,
        focal: [f64; 2],
        focal_radius: f64,
    },
    Conic {
        center: [f64; 2],
        start_angle: f64,
        sweep_angle: f64,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedGradientStop {
    pub color: Color,
    pub offset: f64,
}
impl Gradient {
    pub fn stops(&self) -> &[GradientStop] {
        match self {
            Self::Linear { stops, .. }
            | Self::Radial { stops, .. }
            | Self::FocalRadial { stops, .. }
            | Self::Conic { stops, .. } => stops,
        }
    }
    pub fn options(&self) -> &GradientOptions {
        match self {
            Self::Linear { options, .. }
            | Self::Radial { options, .. }
            | Self::FocalRadial { options, .. }
            | Self::Conic { options, .. } => options,
        }
    }
    pub fn validate_geometry(&self) -> Result<(), ShapeError> {
        let options = self.options();
        if options.interpolation_version != 1 {
            return Err(ShapeError::UnsupportedGradientVersion);
        }
        let m = options.transform.map(|r| r.map(FiniteF64::get));
        let determinant = m[0][0] * m[1][1] - m[0][1] * m[1][0];
        if !determinant.is_finite() || determinant == 0.0 {
            return Err(ShapeError::InvalidGradient);
        }
        let valid = match self {
            Self::Linear { start, end, .. } => start != end,
            Self::Radial { radius, .. } => radius.get() > 0.0,
            Self::FocalRadial {
                center,
                radius,
                focal,
                focal_radius,
                ..
            } => {
                focal_radius.get() >= 0.0
                    && (center[0].get() - focal[0].get()).hypot(center[1].get() - focal[1].get())
                        + focal_radius.get()
                        < radius.get()
            }
            Self::Conic { sweep_angle, .. } => {
                sweep_angle.get() > 0.0 && sweep_angle.get() <= 360.0
            }
        };
        if !valid || !(2..=256).contains(&self.stops().len()) {
            return Err(ShapeError::InvalidGradient);
        }
        Ok(())
    }
    pub fn validate_properties(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), ShapeError> {
        self.validate_geometry()?;
        let mut constants = BTreeMap::new();
        for stop in self.stops() {
            for (id, parameter) in [
                (stop.color, Parameter::Color),
                (stop.offset, Parameter::Offset),
            ] {
                let property = properties
                    .iter()
                    .find(|p| p.id() == id)
                    .ok_or(ShapeError::MissingProperty { id })?;
                property.validate(registry)?;
                let descriptor = property.descriptor().resolve(registry)?.definition();
                if descriptor.value_type != parameter.value_type()
                    || descriptor.unit != parameter.unit()
                {
                    return Err(ShapeError::InvalidDescriptor { id });
                }
                if !property.modifiers().iter().any(|m| m.enabled)
                    && let PropertySource::Constant(value) = property.source()
                {
                    parameter.validate(id, value)?;
                    constants.insert(id, value.clone());
                }
            }
        }
        if self
            .stops()
            .iter()
            .all(|s| constants.contains_key(&s.color) && constants.contains_key(&s.offset))
        {
            self.resolve(&constants)?;
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        values: &BTreeMap<PropertyId, Value>,
    ) -> Result<ResolvedGradient, ShapeError> {
        self.validate_geometry()?;
        let mut stops = Vec::new();
        let mut previous = -1.0;
        for stop in self.stops() {
            let color_value = values
                .get(&stop.color)
                .ok_or(ShapeError::MissingProperty { id: stop.color })?;
            let offset_value = values
                .get(&stop.offset)
                .ok_or(ShapeError::MissingProperty { id: stop.offset })?;
            Parameter::Color.validate(stop.color, color_value)?;
            Parameter::Offset.validate(stop.offset, offset_value)?;
            let Value::Color(color) = color_value else {
                unreachable!()
            };
            let Value::Scalar(offset) = offset_value else {
                unreachable!()
            };
            if offset.get() < previous {
                return Err(ShapeError::InvalidGradient);
            }
            previous = offset.get();
            stops.push(ResolvedGradientStop {
                color: *color,
                offset: previous,
            });
        }
        let geometry = match self {
            Self::Linear { start, end, .. } => GradientGeometry::Linear {
                start: start.map(FiniteF64::get),
                end: end.map(FiniteF64::get),
            },
            Self::Radial { center, radius, .. } => GradientGeometry::Radial {
                center: center.map(FiniteF64::get),
                radius: radius.get(),
            },
            Self::FocalRadial {
                center,
                radius,
                focal,
                focal_radius,
                ..
            } => GradientGeometry::FocalRadial {
                center: center.map(FiniteF64::get),
                radius: radius.get(),
                focal: focal.map(FiniteF64::get),
                focal_radius: focal_radius.get(),
            },
            Self::Conic {
                center,
                start_angle,
                sweep_angle,
                ..
            } => GradientGeometry::Conic {
                center: center.map(FiniteF64::get),
                start_angle: start_angle.get(),
                sweep_angle: sweep_angle.get(),
            },
        };
        Ok(ResolvedGradient {
            geometry,
            stops,
            options: self.options().clone(),
        })
    }
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
    pub gradient: Option<Box<ResolvedGradient>>,
    pub color: Color,
    pub rule: FillRule,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStroke {
    pub options: Option<Box<ResolvedStrokeOptions>>,
    pub gradient: Option<Box<ResolvedGradient>>,
    pub color: Color,
    pub width: FiniteF64,
    pub join: StrokeJoin,
    pub cap: StrokeCap,
    pub miter_limit: FiniteF64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStrokeOptions {
    pub geometry_version: String,
    pub alignment: StrokeAlignment,
    pub fill_rule: FillRule,
    pub dash_array: Vec<f64>,
    pub dash_offset: f64,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ShapeError {
    #[error("unsupported stroke geometry version")]
    UnsupportedStrokeVersion,
    #[error("dash lengths must be finite, nonnegative and not all zero")]
    InvalidDashArray,
    #[error("stroke work budget exceeded")]
    StrokeBudgetExceeded,
    #[error("inside/outside stroke requires closed contours")]
    OpenStrokeAlignment,
    #[error("unsupported gradient interpolation semantics version")]
    UnsupportedGradientVersion,
    #[error("invalid gradient geometry or stops (2..=256, ordered offsets in [0,1])")]
    InvalidGradient,
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
    Offset,
    DashOffset,
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
            Self::Size | Self::Radius | Self::Path | Self::Width | Self::DashOffset => {
                Unit::DesignPx
            }
            _ => Unit::Dimensionless,
        }
    }
    fn validate(self, id: PropertyId, value: &Value) -> Result<(), ShapeError> {
        let valid = match (self, value) {
            (Self::Size, Value::Vec2(v)) => v.iter().all(|v| v.get() >= 0.0),
            (Self::Radius | Self::Width, Value::Scalar(v)) => v.get() >= 0.0,
            (Self::Miter, Value::Scalar(v)) => v.get() >= 1.0,
            (Self::Offset, Value::Scalar(v)) => (0.0..=1.0).contains(&v.get()),
            (Self::DashOffset, Value::Scalar(_)) => true,
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
            if let Some(options) = &stroke.options {
                refs.push((options.dash_offset, Parameter::DashOffset));
            }
            refs.extend([
                (stroke.color, Parameter::Color),
                (stroke.width, Parameter::Width),
                (stroke.join, Parameter::Join),
                (stroke.cap, Parameter::Cap),
                (stroke.miter_limit, Parameter::Miter),
            ]);
        }
        for gradient in self
            .fill
            .as_ref()
            .and_then(|f| f.gradient.as_ref())
            .into_iter()
            .chain(self.stroke.as_ref().and_then(|s| s.gradient.as_ref()))
        {
            for stop in gradient.stops() {
                refs.extend([
                    (stop.color, Parameter::Color),
                    (stop.offset, Parameter::Offset),
                ]);
            }
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
        if let Some(options) = self.stroke.as_ref().and_then(|s| s.options.as_ref()) {
            options.validate()?;
        }
        for gradient in self
            .fill
            .as_ref()
            .and_then(|f| f.gradient.as_ref())
            .into_iter()
            .chain(self.stroke.as_ref().and_then(|s| s.gradient.as_ref()))
        {
            gradient.validate_geometry()?;
        }
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
        let constants: BTreeMap<_, _> = properties
            .iter()
            .filter_map(|p| match p.source() {
                PropertySource::Constant(v) if !p.modifiers().iter().any(|m| m.enabled) => {
                    Some((p.id(), v.clone()))
                }
                _ => None,
            })
            .collect();
        for gradient in self
            .fill
            .as_ref()
            .and_then(|f| f.gradient.as_ref())
            .into_iter()
            .chain(self.stroke.as_ref().and_then(|s| s.gradient.as_ref()))
        {
            if gradient
                .stops()
                .iter()
                .all(|s| constants.contains_key(&s.color) && constants.contains_key(&s.offset))
            {
                gradient.resolve(&constants)?;
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
        if let Some(options) = self.stroke.as_ref().and_then(|s| s.options.as_ref()) {
            options.validate()?;
        }
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
        let fill_gradient = self
            .fill
            .as_ref()
            .and_then(|f| f.gradient.as_ref())
            .map(|g| g.resolve(values))
            .transpose()?;
        let stroke_gradient = self
            .stroke
            .as_ref()
            .and_then(|s| s.gradient.as_ref())
            .map(|g| g.resolve(values))
            .transpose()?;
        let fill = self.fill.as_ref().map(|f| ResolvedFill {
            gradient: fill_gradient.map(Box::new),
            color: color(f.color),
            rule: f.rule,
        });
        let stroke = self.stroke.as_ref().map(|s| ResolvedStroke {
            options: s.options.as_ref().map(|o| {
                Box::new(ResolvedStrokeOptions {
                    geometry_version: o.geometry_version.clone(),
                    alignment: o.alignment,
                    fill_rule: o.fill_rule,
                    dash_array: o.dash_array.iter().map(|x| x.get()).collect(),
                    dash_offset: scalar(o.dash_offset).get(),
                })
            }),
            gradient: stroke_gradient.map(Box::new),
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
            0x9cf34ec6_523a_4c20_86cf_091c243798ab,
            "dash_offset",
            Unit::DesignPx,
            Value::Scalar(n(0.0)),
        ),
        (
            0x3d761408_24b1_4281_a10c_be02f8b0d876,
            "gradient_color",
            Unit::Dimensionless,
            Value::Color(Color::from_srgb8([0; 3], None)),
        ),
        (
            0x2797a303_a6ae_4ca8_8d35_adf3a21baabb,
            "gradient_offset",
            Unit::Dimensionless,
            Value::Scalar(n(0.0)),
        ),
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
                "gradient_offset" => {
                    definition.range = Some(crate::ValueRange::Scalar(crate::NumericRange {
                        min: Some(crate::NumericBound {
                            value: n(0.0),
                            inclusive: true,
                        }),
                        max: Some(crate::NumericBound {
                            value: n(1.0),
                            inclusive: true,
                        }),
                    }))
                }
                "miter_limit" => definition.range = Some(crate::ValueRange::Scalar(range(1.0))),
                _ => (),
            }
            PropertyDescriptor::new(definition).expect("valid shape descriptor")
        })
        .collect()
}
