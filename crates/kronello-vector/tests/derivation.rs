use kronello_model::*;
use kronello_time::{Duration, FrameRate, Time, TimeRange};
use kronello_vector::{FlattenRequest, VectorError, flatten};
use std::collections::BTreeMap;

fn f(x: f64) -> FiniteF64 {
    FiniteF64::new(x).unwrap()
}
fn fixture() -> (Project, BTreeMap<PropertyId, Value>, SchemaRegistry) {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors() {
        registry.register(descriptor).unwrap();
    }
    let make = |key: &str, value: Value| {
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
            PropertySource::Constant(value),
            vec![],
            &registry,
        )
        .unwrap()
    };
    let size = make("kronello.shape.size", Value::Vec2([f(200.0), f(100.0)]));
    let radius = make("kronello.shape.corner_radius", Value::Scalar(f(20.0)));
    let shape = Shape {
        id: ContentId::new(),
        geometry: ShapeGeometry::Rectangle {
            size: size.id(),
            corner_radius: radius.id(),
        },
        fill: None,
        stroke: None,
    };
    let properties = vec![size, radius];
    let values = properties
        .iter()
        .map(|p| match p.source() {
            PropertySource::Constant(v) => (p.id(), v.clone()),
            _ => panic!(),
        })
        .collect();
    let node = SceneNode {
        id: NodeId::new(),
        kind: NodeKind::Shape {
            content_ref: shape.id,
        },
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, Time::new(10, 1).unwrap()).unwrap(),
        properties,
    };
    let composition = Composition {
        id: CompositionId::new(),
        duration: Duration::new(Time::new(10, 1).unwrap()).unwrap(),
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![node.id],
        nodes: vec![node],
        properties: vec![],
    };
    (
        Project {
            compositions: vec![DocumentObject::Known(composition)],
            shapes: vec![DocumentObject::Known(shape)],
            ..Project::default()
        },
        values,
        registry,
    )
}
fn shape(project: &Project) -> &Shape {
    let DocumentObject::Known(s) = &project.shapes[0] else {
        panic!()
    };
    s
}
fn count(path: &kronello_vector::FlattenedPath) -> usize {
    path.subpaths.iter().map(|s| s.points.len()).sum()
}

#[test]
fn higher_scale_and_output_resolution_refine_geometry_without_changing_document() {
    let (project, values, registry) = fixture();
    validate_shape_contents(&project, &registry).unwrap();
    let before = serde_json::to_string(&project).unwrap();
    let extent = DesignExtent::new(1920.0, 1080.0).unwrap();
    let low = flatten(
        shape(&project),
        &values,
        FlattenRequest::for_output(extent, 1920, 1080, 0.25).unwrap(),
    )
    .unwrap();
    let high = flatten(
        shape(&project),
        &values,
        FlattenRequest::for_output(extent, 7680, 4320, 0.25).unwrap(),
    )
    .unwrap();
    let zoom = flatten(
        shape(&project),
        &values,
        FlattenRequest::new(16.0, 0.25).unwrap(),
    )
    .unwrap();
    assert!(count(&high) > count(&low));
    assert!(count(&zoom) > count(&high));
    assert_eq!(before, serde_json::to_string(&project).unwrap());
    assert!(low.subpaths[0].closed);
    for result in [&low, &high, &zoom] {
        for [x, y] in &result.subpaths[0].points {
            assert!((0.0..=200.0).contains(x));
            assert!((0.0..=100.0).contains(y));
        }
    }
    let decoded: Project = serde_json::from_str(&before).unwrap();
    assert_eq!(decoded, project);
}

#[test]
fn ellipse_and_cubic_quadratic_subpaths_refine_and_retain_closure() {
    let (project, mut values, _) = fixture();
    let mut s = shape(&project).clone();
    let ShapeGeometry::Rectangle { size, .. } = s.geometry else {
        panic!()
    };
    s.geometry = ShapeGeometry::Ellipse { size };
    let low = flatten(&s, &values, FlattenRequest::new(1.0, 0.25).unwrap()).unwrap();
    let high = flatten(&s, &values, FlattenRequest::new(8.0, 0.25).unwrap()).unwrap();
    assert!(count(&high) > count(&low));
    assert!(high.subpaths[0].closed);
    let id = PropertyId::new();
    s.geometry = ShapeGeometry::BezierPath { path: id };
    values.insert(
        id,
        Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo([f(0.0); 2]),
                PathSegment::QuadTo {
                    control: [f(50.0), f(100.0)],
                    end: [f(100.0), f(0.0)],
                },
                PathSegment::CubicTo {
                    control1: [f(100.0), f(-100.0)],
                    control2: [f(200.0), f(-100.0)],
                    end: [f(200.0), f(0.0)],
                },
                PathSegment::Close,
                PathSegment::MoveTo([f(-20.0); 2]),
                PathSegment::LineTo([f(-10.0); 2]),
            ],
        }),
    );
    let low = flatten(&s, &values, FlattenRequest::new(1.0, 0.25).unwrap()).unwrap();
    let high = flatten(&s, &values, FlattenRequest::new(8.0, 0.25).unwrap()).unwrap();
    assert!(count(&high) > count(&low));
    assert_eq!(high.subpaths.len(), 2);
    assert!(high.subpaths[0].closed);
    assert!(!high.subpaths[1].closed);
    assert_eq!(
        high.subpaths[1].points,
        vec![[-20.0, -20.0], [-10.0, -10.0]]
    );
}

#[test]
fn rectangle_large_radius_is_derived_without_rewriting_property() {
    let (project, mut values, _) = fixture();
    let s = shape(&project);
    let ShapeGeometry::Rectangle {
        size,
        corner_radius,
    } = s.geometry
    else {
        panic!()
    };
    values.insert(corner_radius, Value::Scalar(f(1000.0)));
    let before = values.clone();
    let path = flatten(s, &values, FlattenRequest::new(1.0, 0.25).unwrap()).unwrap();
    assert_eq!(values, before);
    values.insert(corner_radius, Value::Scalar(f(50.0)));
    let capped = flatten(s, &values, FlattenRequest::new(1.0, 0.25).unwrap()).unwrap();
    assert_eq!(path, capped);
    values.insert(size, Value::Vec2([f(0.0); 2]));
    let zero = flatten(s, &values, FlattenRequest::new(1.0, 0.25).unwrap()).unwrap();
    assert!(
        zero.subpaths
            .iter()
            .flat_map(|s| &s.points)
            .all(|p| p == &[0.0, 0.0])
    );
}

#[test]
fn invalid_requests_and_final_evaluated_values_return_typed_errors() {
    for (scale, tolerance) in [
        (0.0, 0.25),
        (-1.0, 0.25),
        (f64::INFINITY, 0.25),
        (1.0, f64::NAN),
        (1.0, 0.0),
        (f64::MAX, f64::MIN_POSITIVE),
    ] {
        assert!(matches!(
            FlattenRequest::new(scale, tolerance),
            Err(VectorError::InvalidRequest)
        ));
    }
    assert!(matches!(
        FlattenRequest::for_output(DesignExtent::new(1920.0, 1080.0).unwrap(), 1080, 1920, 0.25),
        Err(VectorError::AspectMismatch)
    ));
    let (project, mut values, _) = fixture();
    let s = shape(&project);
    let ShapeGeometry::Rectangle { corner_radius, .. } = s.geometry else {
        panic!()
    };
    values.insert(corner_radius, Value::Scalar(f(-1.0)));
    assert!(matches!(
        flatten(s, &values, FlattenRequest::new(1.0, 0.25).unwrap()),
        Err(VectorError::Shape(ShapeError::InvalidParameter { .. }))
    ));
}

#[test]
fn existing_evaluator_animates_shape_size_and_radius_without_editing_ir() {
    use kronello_eval::{DependencyGraph, EvaluationSnapshot, RuntimePropertyKey};
    let (mut project, _, registry) = fixture();
    let s = shape(&project).clone();
    let ShapeGeometry::Rectangle {
        size,
        corner_radius,
    } = s.geometry
    else {
        panic!()
    };
    let curve_size = AnimationCurve::new(
        CurveId::new(),
        ValueType::Vec2,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: Value::Vec2([f(100.0); 2]),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::new(1, 1).unwrap(),
                value: Value::Vec2([f(300.0); 2]),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let curve_radius = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: Value::Scalar(f(0.0)),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::new(1, 1).unwrap(),
                value: Value::Scalar(f(40.0)),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        panic!()
    };
    c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.id() == size)
        .unwrap()
        .set_source(PropertySource::Curve(curve_size.id()), &registry)
        .unwrap();
    c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.id() == corner_radius)
        .unwrap()
        .set_source(PropertySource::Curve(curve_radius.id()), &registry)
        .unwrap();
    project.curves = vec![
        DocumentObject::Known(curve_size.clone()),
        DocumentObject::Known(curve_radius.clone()),
    ];
    validate_shape_contents(&project, &registry).unwrap();
    let before = serde_json::to_string(&project).unwrap();
    let DocumentObject::Known(c) = &project.compositions[0] else {
        panic!()
    };
    let compositions = vec![c.clone()];
    let curves = vec![curve_size, curve_radius];
    let bindings = BTreeMap::new();
    let dependencies = BTreeMap::new();
    let graph = DependencyGraph::compile(
        EvaluationSnapshot {
            compositions: &compositions,
            curves: &curves,
            registry: &registry,
            reference_bindings: &bindings,
            dependencies: &dependencies,
            working_space: ColorSpace::LinearRec709,
        },
        c.id,
    )
    .unwrap();
    let scene = graph.evaluate_scene(Time::new(1, 2).unwrap()).unwrap();
    let values: BTreeMap<_, _> = scene.nodes[0]
        .properties
        .iter()
        .map(|(key, value)| match key {
            RuntimePropertyKey::Node(key) => (key.property, value.clone()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(values[&size], Value::Vec2([f(200.0); 2]));
    assert_eq!(values[&corner_radius], Value::Scalar(f(20.0)));
    let resolved = s.resolve(&values).unwrap();
    assert!(
        matches!(resolved.geometry,ResolvedGeometry::Rectangle {corner_radius,..} if corner_radius.get()==20.0)
    );
    let geometry = flatten(&s, &values, FlattenRequest::new(4.0, 0.25).unwrap()).unwrap();
    assert!(count(&geometry) > 4);
    assert_eq!(before, serde_json::to_string(&project).unwrap());
}

#[test]
fn extreme_finite_geometry_and_tolerance_are_rejected_before_subdivision() {
    let (project, mut values, _) = fixture();
    let s = shape(&project);
    let ShapeGeometry::Rectangle { size, .. } = s.geometry else {
        panic!()
    };
    let request = FlattenRequest::new(1.0, 0.25).unwrap();
    values.insert(size, Value::Vec2([f(f64::MAX); 2]));
    assert!(matches!(
        flatten(s, &values, request),
        Err(VectorError::GeometryBudgetExceeded)
    ));
    let (project, values, _) = fixture();
    assert!(matches!(
        flatten(
            shape(&project),
            &values,
            FlattenRequest::new(1.0, 1e-200).unwrap()
        ),
        Err(VectorError::GeometryBudgetExceeded)
    ));
}
