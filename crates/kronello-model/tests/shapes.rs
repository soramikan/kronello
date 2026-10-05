use kronello_model::*;
use kronello_time::{Duration, FrameRate, Time, TimeRange};
use serde_json::json;
use std::collections::BTreeMap;

fn f(x: f64) -> FiniteF64 {
    FiniteF64::new(x).unwrap()
}
fn registry() -> SchemaRegistry {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors() {
        registry.register(descriptor).unwrap();
    }
    registry
}
fn property(registry: &SchemaRegistry, key: &str, value: Value) -> Property {
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        registry,
    )
    .unwrap()
}
fn fixture() -> (Project, SchemaRegistry) {
    let registry = registry();
    let properties = vec![
        property(
            &registry,
            "kronello.shape.size",
            Value::Vec2([f(200.0), f(100.0)]),
        ),
        property(
            &registry,
            "kronello.shape.corner_radius",
            Value::Scalar(f(20.0)),
        ),
        property(
            &registry,
            "kronello.fill_color",
            Value::Color(Color::new(ColorSpace::LinearRec709, [1.2, -0.1, 0.4], 0.0).unwrap()),
        ),
        property(&registry, "kronello.stroke_width", Value::Scalar(f(2.0))),
        property(
            &registry,
            "kronello.shape.stroke_join",
            Value::Enum("round".into()),
        ),
        property(
            &registry,
            "kronello.shape.stroke_cap",
            Value::Enum("square".into()),
        ),
        property(
            &registry,
            "kronello.shape.miter_limit",
            Value::Scalar(f(4.0)),
        ),
        property(
            &registry,
            "kronello.shape.stroke_color",
            Value::Color(Color::from_srgb8([255, 128, 0], Some(128))),
        ),
    ];
    let ids: Vec<_> = properties.iter().map(Property::id).collect();
    let shape = Shape {
        id: ContentId::new(),
        geometry: ShapeGeometry::Rectangle {
            size: ids[0],
            corner_radius: ids[1],
        },
        fill: Some(Fill {
            gradient: None,
            color: ids[2],
            rule: FillRule::Evenodd,
        }),
        stroke: Some(Stroke {
            gradient: None,
            color: ids[7],
            width: ids[3],
            join: ids[4],
            cap: ids[5],
            miter_limit: ids[6],
        }),
    };
    let node = SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        effects: vec![],
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
        registry,
    )
}
fn shape(project: &Project) -> &Shape {
    match &project.shapes[0] {
        DocumentObject::Known(s) => s,
        _ => panic!(),
    }
}
fn values(project: &Project) -> BTreeMap<PropertyId, Value> {
    let DocumentObject::Known(c) = &project.compositions[0] else {
        panic!()
    };
    c.nodes[0]
        .properties
        .iter()
        .map(|p| match p.source() {
            PropertySource::Constant(v) => (p.id(), v.clone()),
            _ => panic!(),
        })
        .collect()
}

#[test]
fn shape_project_roundtrip_preserves_properties_straight_color_and_styles() {
    let (project, registry) = fixture();
    validate_shape_contents(&project, &registry).unwrap();
    project.ensure_editable().unwrap();
    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(project, decoded);
    let s = shape(&decoded).resolve(&values(&decoded)).unwrap();
    let fill = s.fill.unwrap();
    assert_eq!(fill.rule, FillRule::Evenodd);
    assert_eq!(fill.color.space(), ColorSpace::LinearRec709);
    assert_eq!(fill.color.components().r.get(), 1.2);
    assert_eq!(fill.color.components().g.get(), -0.1);
    assert_eq!(fill.color.components().alpha.get(), 0.0);
    let stroke = s.stroke.unwrap();
    assert_eq!(stroke.color, Color::from_srgb8([255, 128, 0], Some(128)));
    assert_eq!(stroke.width.get(), 2.0);
    assert_eq!(stroke.join, StrokeJoin::Round);
    assert_eq!(stroke.cap, StrokeCap::Square);
    assert_eq!(stroke.miter_limit.get(), 4.0);
    assert_eq!(shape(&project).property_ids().len(), 8);
}

#[test]
fn rectangle_ellipse_and_multisubpath_bezier_roundtrip() {
    let (project, registry) = fixture();
    let mut s = shape(&project).clone();
    let ShapeGeometry::Rectangle { size, .. } = s.geometry else {
        panic!()
    };
    s.geometry = ShapeGeometry::Ellipse { size };
    assert_eq!(
        s,
        serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap()
    );
    assert!(matches!(
        s.resolve(&values(&project)).unwrap().geometry,
        ResolvedGeometry::Ellipse { .. }
    ));
    let path = Path {
        segments: vec![
            PathSegment::MoveTo([f(0.0); 2]),
            PathSegment::LineTo([f(1.0); 2]),
            PathSegment::QuadTo {
                control: [f(2.0); 2],
                end: [f(3.0); 2],
            },
            PathSegment::CubicTo {
                control1: [f(4.0); 2],
                control2: [f(5.0); 2],
                end: [f(6.0); 2],
            },
            PathSegment::Close,
            PathSegment::MoveTo([f(-1.0); 2]),
            PathSegment::LineTo([f(-2.0); 2]),
        ],
    };
    validate_path(&path).unwrap();
    let p = property(&registry, "kronello.shape.path", Value::Path(path.clone()));
    s.geometry = ShapeGeometry::BezierPath { path: p.id() };
    s.fill = None;
    s.stroke = None;
    s.validate(std::slice::from_ref(&p), &registry).unwrap();
    let resolved = s
        .resolve(&BTreeMap::from([(p.id(), Value::Path(path.clone()))]))
        .unwrap();
    assert_eq!(resolved.geometry, ResolvedGeometry::BezierPath(path));
    assert_eq!(
        p,
        serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap()
    );
}

#[test]
fn shape_parameters_accept_curves_expressions_and_discrete_style_animation() {
    let (project, registry) = fixture();
    let DocumentObject::Known(c) = &project.compositions[0] else {
        panic!()
    };
    let mut properties = c.nodes[0].properties.clone();
    for (i, p) in properties.iter_mut().enumerate() {
        p.set_source(
            if i % 2 == 0 {
                PropertySource::Curve(CurveId::new())
            } else {
                PropertySource::Expression(ExpressionId::new())
            },
            &registry,
        )
        .unwrap();
    }
    shape(&project).validate(&properties, &registry).unwrap();
    for key in [
        "kronello.shape.stroke_join",
        "kronello.shape.stroke_cap",
        "kronello.shape.path",
    ] {
        let d = registry.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert_eq!(
            d.definition().interpolation_modes,
            std::collections::BTreeSet::from([InterpolationMode::Hold])
        );
    }
}

#[test]
fn invalid_negative_dimensions_radius_width_miter_and_enum_are_typed_errors() {
    let (project, _) = fixture();
    let s = shape(&project);
    let ShapeGeometry::Rectangle {
        size,
        corner_radius,
    } = s.geometry
    else {
        panic!()
    };
    let stroke = s.stroke.as_ref().unwrap();
    for (id, invalid) in [
        (size, Value::Vec2([f(-1.0), f(2.0)])),
        (corner_radius, Value::Scalar(f(-1.0))),
        (stroke.width, Value::Scalar(f(-1.0))),
        (stroke.miter_limit, Value::Scalar(f(0.5))),
        (stroke.join, Value::Enum("unknown".into())),
        (stroke.cap, Value::Enum("unknown".into())),
    ] {
        let mut values = values(&project);
        values.insert(id, invalid);
        assert!(
            matches!(s.resolve(&values), Err(ShapeError::InvalidParameter { id: actual, .. }) if actual == id)
        );
    }
    for nonfinite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            FiniteF64::new(nonfinite),
            Err(ModelError::NonFinite { .. })
        ));
    }
    assert!(
        serde_json::from_str::<Path>(r#"{"segments":[{"kind":"move_to","value":[1e999,0]}]}"#)
            .is_err()
    );
}

#[test]
fn invalid_path_order_is_rejected_without_guessing_subpaths() {
    for segments in [
        vec![PathSegment::LineTo([f(0.0); 2])],
        vec![PathSegment::Close],
        vec![
            PathSegment::MoveTo([f(0.0); 2]),
            PathSegment::Close,
            PathSegment::Close,
        ],
    ] {
        assert!(matches!(
            validate_path(&Path { segments }),
            Err(ShapeError::InvalidPath { .. })
        ));
    }
    validate_path(&Path { segments: vec![] }).unwrap();
}

#[test]
fn shape_reference_closure_descriptor_units_and_duplicate_ids_are_checked() {
    let (mut project, registry) = fixture();
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        panic!()
    };
    let missing = c.nodes[0].properties.remove(0).id();
    assert_eq!(
        validate_shape_contents(&project, &registry),
        Err(ShapeError::MissingProperty { id: missing })
    );
    let (mut project, registry) = fixture();
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        panic!()
    };
    let p = c.nodes[0].properties[0].clone();
    c.nodes[0].properties.push(p.clone());
    assert_eq!(
        validate_shape_contents(&project, &registry),
        Err(ShapeError::DuplicateProperty { id: p.id() })
    );
    let (mut project, registry) = fixture();
    let DocumentObject::Known(s) = &mut project.shapes[0] else {
        panic!()
    };
    let id = ContentId::new();
    s.id = id;
    assert!(matches!(
        validate_shape_contents(&project, &registry),
        Err(ShapeError::MissingContent { .. })
    ));
    let (mut project, registry) = fixture();
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        panic!()
    };
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.transform.scale").unwrap())
        .unwrap();
    let p = Property::new(
        c.nodes[0].properties[0].id(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Vec2([f(1.0); 2])),
        vec![],
        &registry,
    )
    .unwrap();
    c.nodes[0].properties[0] = p;
    assert!(matches!(
        validate_shape_contents(&project, &registry),
        Err(ShapeError::InvalidDescriptor { .. })
    ));
    let (mut project, registry) = fixture();
    project.shapes.push(project.shapes[0].clone());
    assert!(matches!(
        validate_shape_contents(&project, &registry),
        Err(ShapeError::DuplicateContent { .. })
    ));
    assert!(project.validate_storage().is_err());
}

#[test]
fn unknown_shape_fields_variants_and_large_numbers_are_preserved_opaque() {
    let (project, registry) = fixture();
    for change in [
        json!({"future": {"nested": [1,2,3]}}),
        json!({"geometry": {"kind":"future_geometry", "value":{"arbitrary":true}}}),
    ] {
        let mut raw = serde_json::to_value(&project).unwrap();
        for (k, v) in change.as_object().unwrap() {
            raw["shapes"][0][k] = v.clone();
        }
        let encoded = serde_json::to_string(&raw)
            .unwrap()
            .replace("[1,2,3]", "[123456789012345678901234567890,2,3]");
        let original: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        let decoded: Project = serde_json::from_str(&encoded).unwrap();
        decoded.validate_storage().unwrap();
        assert!(matches!(decoded.shapes[0], DocumentObject::Opaque(_)));
        assert!(matches!(
            decoded.ensure_editable(),
            Err(ProjectError::UnsupportedMeaning)
        ));
        assert_eq!(
            validate_shape_contents(&decoded, &registry),
            Err(ShapeError::UnsupportedContent)
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), original);
    }
}

#[test]
fn legacy_project_without_shapes_remains_unchanged_and_shape_extensions_cannot_shadow() {
    let project = Project::default();
    let raw = serde_json::to_value(&project).unwrap();
    assert!(raw.get("shapes").is_none());
    let decoded: Project = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(decoded, project);
    assert_eq!(serde_json::to_value(decoded).unwrap(), raw);
    let mut project = project;
    project.unknown_fields.insert("shapes".into(), json!([]));
    assert!(project.validate_storage().is_err());
}

#[test]
fn fill_rules_and_all_join_cap_variants_are_retained() {
    let (project, _) = fixture();
    let mut s = shape(&project).clone();
    let stroke = s.stroke.clone().unwrap();
    for rule in [FillRule::Nonzero, FillRule::Evenodd] {
        s.fill.as_mut().unwrap().rule = rule;
        for (join, expected_join) in [
            ("miter", StrokeJoin::Miter),
            ("round", StrokeJoin::Round),
            ("bevel", StrokeJoin::Bevel),
        ] {
            for (cap, expected_cap) in [
                ("butt", StrokeCap::Butt),
                ("round", StrokeCap::Round),
                ("square", StrokeCap::Square),
            ] {
                let mut values = values(&project);
                values.insert(stroke.join, Value::Enum(join.into()));
                values.insert(stroke.cap, Value::Enum(cap.into()));
                let resolved = s.resolve(&values).unwrap();
                let line = resolved.stroke.unwrap();
                assert_eq!(line.join, expected_join);
                assert_eq!(line.cap, expected_cap);
                assert_eq!(resolved.fill.unwrap().rule, rule);
            }
        }
    }
}
