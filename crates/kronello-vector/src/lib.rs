//! Pure geometry derivation from evaluated vector content. Output scale and
//! tolerance are request inputs, never document fields or stored raster data.
use kronello_model::{PathSegment, PropertyId, ResolvedGeometry, Shape, ShapeError, Value};
use kurbo::{BezPath, Ellipse, PathEl, Point, RoundedRect, Shape as KurboShape};
use std::collections::BTreeMap;
use thiserror::Error;
mod dash;
mod offset;
mod svg;
mod trim;
pub use dash::{MAX_DASH_SEGMENTS, dash_path};
pub use offset::offset_path;
pub use svg::*;
pub use trim::trim_path;

/// Uniform magnification (including the node scale) and error in output pixels.
/// For nonuniform/affine transforms pass a conservative maximum magnification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlattenRequest {
    output_scale: f64,
    tolerance_px: f64,
}
impl FlattenRequest {
    pub fn new(output_scale: f64, tolerance_px: f64) -> Result<Self, VectorError> {
        if !output_scale.is_finite()
            || output_scale <= 0.0
            || !tolerance_px.is_finite()
            || tolerance_px <= 0.0
        {
            return Err(VectorError::InvalidRequest);
        }
        let tolerance = tolerance_px / output_scale;
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(VectorError::InvalidRequest);
        }
        Ok(Self {
            output_scale,
            tolerance_px,
        })
    }
    /// For a matching aspect ratio. Aspect changes require explicit relayout.
    pub fn for_output(
        design: kronello_model::DesignExtent,
        width: u32,
        height: u32,
        tolerance_px: f64,
    ) -> Result<Self, VectorError> {
        if width == 0 || height == 0 {
            return Err(VectorError::InvalidRequest);
        }
        let x = f64::from(width) / design.width();
        let y = f64::from(height) / design.height();
        if (x - y).abs() > 1e-12 * x.max(y) {
            return Err(VectorError::AspectMismatch);
        }
        Self::new(x, tolerance_px)
    }
    pub fn tolerance_design(self) -> f64 {
        self.tolerance_px / self.output_scale
    }
}

/// Non-serialized derived local design-space polyline. Multiple subpaths and
/// explicit closure are retained. This is geometry, not stroke tessellation.
#[derive(Debug, Clone, PartialEq)]
pub struct FlattenedPath {
    pub subpaths: Vec<Polyline>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
}

#[derive(Debug, Error)]
pub enum VectorError {
    #[error(transparent)]
    Shape(#[from] ShapeError),
    #[error("scale/tolerance/output dimensions must be positive and finite")]
    InvalidRequest,
    #[error("output aspect ratio differs from design extent; explicit relayout is required")]
    AspectMismatch,
    #[error("requested geometry exceeds the conservative flattening work budget")]
    GeometryBudgetExceeded,
    #[error("derived geometry is non-finite")]
    NonFiniteGeometry,
}

/// The immutable document and final evaluated property values are borrowed.
/// kurbo flattening is an approximation; tolerance is not a strict error bound.
pub fn flatten(
    shape: &Shape,
    values: &BTreeMap<PropertyId, Value>,
    request: FlattenRequest,
) -> Result<FlattenedPath, VectorError> {
    let resolved = shape.resolve(values)?;
    let tolerance = request.tolerance_design();
    check_work_budget(&resolved.geometry, tolerance)?;
    let trim = match &resolved.geometry {
        ResolvedGeometry::TrimmedPath {
            start, end, offset, ..
        } => Some((*start, *end, *offset)),
        _ => None,
    };
    let path: BezPath = match resolved.geometry {
        ResolvedGeometry::Rectangle {
            size,
            corner_radius,
        } => RoundedRect::new(0.0, 0.0, size[0].get(), size[1].get(), corner_radius.get())
            .to_path(tolerance),
        ResolvedGeometry::Ellipse { size } => {
            let mut path = Ellipse::new(
                (size[0].get() / 2.0, size[1].get() / 2.0),
                (size[0].get() / 2.0, size[1].get() / 2.0),
                0.0,
            )
            .to_path(tolerance);
            // kurbo's ellipse returns coincident endpoints without ClosePath;
            // explicitly close it so stroke caps cannot change ellipse semantics.
            path.close_path();
            path
        }
        ResolvedGeometry::BezierPath(path) | ResolvedGeometry::TrimmedPath { path, .. } => {
            let mut bez = BezPath::new();
            let p = |v: [kronello_model::FiniteF64; 2]| Point::new(v[0].get(), v[1].get());
            for segment in path.segments {
                bez.push(match segment {
                    PathSegment::MoveTo(v) => PathEl::MoveTo(p(v)),
                    PathSegment::LineTo(v) => PathEl::LineTo(p(v)),
                    PathSegment::QuadTo { control, end } => PathEl::QuadTo(p(control), p(end)),
                    PathSegment::CubicTo {
                        control1,
                        control2,
                        end,
                    } => PathEl::CurveTo(p(control1), p(control2), p(end)),
                    PathSegment::Close => PathEl::ClosePath,
                });
            }
            bez
        }
    };
    if !path.is_finite() {
        return Err(VectorError::NonFiniteGeometry);
    }
    let mut subpaths: Vec<Polyline> = Vec::new();
    let mut finite = true;
    kurbo::flatten(path, tolerance, |element| {
        match element {
            PathEl::MoveTo(p) => subpaths.push(Polyline {
                points: vec![[p.x, p.y]],
                closed: false,
            }),
            PathEl::LineTo(p) => {
                if let Some(current) = subpaths.last_mut() {
                    current.points.push([p.x, p.y]);
                }
            }
            PathEl::ClosePath => {
                if let Some(current) = subpaths.last_mut() {
                    current.closed = true;
                }
            }
            _ => unreachable!("flatten emits only line commands"),
        }
        if let PathEl::MoveTo(p) | PathEl::LineTo(p) = element {
            finite &= p.x.is_finite() && p.y.is_finite();
        }
    });
    if !finite {
        return Err(VectorError::NonFiniteGeometry);
    }
    let flattened = FlattenedPath { subpaths };
    if let Some((start, end, offset)) = trim {
        trim_path(&flattened, start, end, offset)
    } else {
        Ok(flattened)
    }
}

// Conservative preflight before kurbo allocates/subdivides. The allowance is
// based on coordinate magnitude and input command count, not output resolution
// stored in the document. This also excludes overflowing intermediate math.
fn check_work_budget(geometry: &ResolvedGeometry, tolerance: f64) -> Result<(), VectorError> {
    let mut magnitude: f64 = 1.0;
    let mut add = |v: [kronello_model::FiniteF64; 2]| {
        for coordinate in v {
            magnitude = magnitude.max(coordinate.get().abs());
        }
    };
    let commands = match geometry {
        ResolvedGeometry::Rectangle { size, .. } | ResolvedGeometry::Ellipse { size } => {
            add(*size);
            10
        }
        ResolvedGeometry::BezierPath(path) | ResolvedGeometry::TrimmedPath { path, .. } => {
            for element in &path.segments {
                match element {
                    PathSegment::MoveTo(p) | PathSegment::LineTo(p) => add(*p),
                    PathSegment::QuadTo { control, end } => {
                        add(*control);
                        add(*end);
                    }
                    PathSegment::CubicTo {
                        control1,
                        control2,
                        end,
                    } => {
                        add(*control1);
                        add(*control2);
                        add(*end);
                    }
                    PathSegment::Close => (),
                }
            }
            path.segments.len()
        }
    };
    let ratio = magnitude / tolerance;
    if magnitude > 1e100
        || !ratio.is_finite()
        || ratio > 1e8
        || commands as f64 * ratio.sqrt().max(1.0) > 1e6
    {
        return Err(VectorError::GeometryBudgetExceeded);
    }
    Ok(())
}

/// Analytic centerline/fill envelope in local design_px, independent of raster
/// tolerance. Stroke expansion belongs to the consumer's paint contract.
pub type GeometryBounds = ([f64; 2], [f64; 2]);

pub fn geometry_bounds(geometry: &ResolvedGeometry) -> Result<Option<GeometryBounds>, VectorError> {
    check_work_budget(geometry, 0.02)?;
    let result = match geometry {
        ResolvedGeometry::Rectangle { size, .. } | ResolvedGeometry::Ellipse { size } => {
            Some(([0.0; 2], size.map(kronello_model::FiniteF64::get)))
        }
        ResolvedGeometry::BezierPath(path) | ResolvedGeometry::TrimmedPath { path, .. } => {
            let mut bez = BezPath::new();
            let p = |v: [kronello_model::FiniteF64; 2]| Point::new(v[0].get(), v[1].get());
            for segment in &path.segments {
                bez.push(match *segment {
                    PathSegment::MoveTo(v) => PathEl::MoveTo(p(v)),
                    PathSegment::LineTo(v) => PathEl::LineTo(p(v)),
                    PathSegment::QuadTo { control, end } => PathEl::QuadTo(p(control), p(end)),
                    PathSegment::CubicTo {
                        control1,
                        control2,
                        end,
                    } => PathEl::CurveTo(p(control1), p(control2), p(end)),
                    PathSegment::Close => PathEl::ClosePath,
                });
            }
            if bez.segments().next().is_none() {
                None
            } else {
                let r = bez.bounding_box();
                Some(([r.x0, r.y0], [r.x1, r.y1]))
            }
        }
    };
    if result.is_some_and(|(min, max)| !min.into_iter().chain(max).all(f64::is_finite)) {
        return Err(VectorError::NonFiniteGeometry);
    }
    Ok(result)
}
