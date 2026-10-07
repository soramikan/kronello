use kronello_model::*;
use kronello_vector::*;
use std::collections::BTreeMap;
fn f(value: f64) -> FiniteF64 {
    FiniteF64::new(value).unwrap()
}
fn p(x: f64, y: f64) -> [FiniteF64; 2] {
    [f(x), f(y)]
}
fn line(x: f64) -> Path {
    Path {
        segments: vec![
            PathSegment::MoveTo(p(0.0, 0.0)),
            PathSegment::LineTo(p(x, 0.0)),
        ],
    }
}
fn property(key: &str, value: Value) -> Property {
    let mut registry = SchemaRegistry::with_builtin();
    for d in shape_descriptors() {
        registry.register(d).unwrap();
    }
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
}
#[test]
fn morph_interpolates_all_controls_and_rejects_correspondence() {
    let a = Path {
        segments: vec![
            PathSegment::MoveTo(p(0.0, 0.0)),
            PathSegment::QuadTo {
                control: p(5.0, 10.0),
                end: p(10.0, 0.0),
            },
            PathSegment::CubicTo {
                control1: p(10.0, 10.0),
                control2: p(20.0, 10.0),
                end: p(20.0, 0.0),
            },
            PathSegment::Close,
        ],
    };
    let b = Path {
        segments: vec![
            PathSegment::MoveTo(p(10.0, 20.0)),
            PathSegment::QuadTo {
                control: p(15.0, 30.0),
                end: p(20.0, 20.0),
            },
            PathSegment::CubicTo {
                control1: p(20.0, 30.0),
                control2: p(30.0, 30.0),
                end: p(30.0, 20.0),
            },
            PathSegment::Close,
        ],
    };
    let m = morph_paths(&a, &b, 0.5).unwrap();
    assert_eq!(m.segments[0], PathSegment::MoveTo(p(5.0, 10.0)));
    assert_eq!(
        m.segments[1],
        PathSegment::QuadTo {
            control: p(10.0, 20.0),
            end: p(15.0, 10.0)
        }
    );
    assert_eq!(
        m.segments[2],
        PathSegment::CubicTo {
            control1: p(15.0, 20.0),
            control2: p(25.0, 20.0),
            end: p(25.0, 10.0)
        }
    );
    assert_eq!(morph_paths(&a, &b, 0.0).unwrap(), a);
    assert_eq!(morph_paths(&a, &b, 1.0).unwrap(), b);
    let mut bad = b.clone();
    bad.segments[1] = PathSegment::LineTo(p(20.0, 20.0));
    assert!(matches!(
        morph_paths(&a, &bad, 0.0),
        Err(ShapeError::MorphCorrespondence { index: 1 })
    ));
    bad = b.clone();
    bad.segments.pop();
    assert!(matches!(
        morph_paths(&a, &bad, 0.5),
        Err(ShapeError::MorphCorrespondence { index: 3 })
    ));
    bad = b.clone();
    bad.segments[3] = PathSegment::LineTo(p(10.0, 20.0));
    assert!(matches!(
        morph_paths(&a, &bad, 0.5),
        Err(ShapeError::MorphCorrespondence { index: 3 })
    ));
}
#[test]
fn persisted_shape_variants_drive_flattening_and_reject_dynamic_mismatch() {
    let from = property("kronello.shape.path", Value::Path(line(100.0)));
    let to = property("kronello.shape.path", Value::Path(line(200.0)));
    let progress = property("kronello.shape.morph_progress", Value::Scalar(f(0.5)));
    let shape = Shape {
        id: ContentId::new(),
        geometry: ShapeGeometry::MorphPath {
            from: from.id(),
            to: to.id(),
            progress: progress.id(),
        },
        fill: None,
        stroke: None,
    };
    let mut values: BTreeMap<_, _> = [&from, &to, &progress]
        .into_iter()
        .map(|p| {
            (
                p.id(),
                match p.source() {
                    PropertySource::Constant(v) => v.clone(),
                    _ => panic!(),
                },
            )
        })
        .collect();
    let decoded: Shape = serde_json::from_str(&serde_json::to_string(&shape).unwrap()).unwrap();
    assert_eq!(
        flatten(&decoded, &values, FlattenRequest::new(1.0, 0.02).unwrap())
            .unwrap()
            .subpaths[0]
            .points[1],
        [150.0, 0.0]
    );
    values.insert(
        to.id(),
        Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo(p(0.0, 0.0)),
                PathSegment::QuadTo {
                    control: p(5.0, 10.0),
                    end: p(10.0, 0.0),
                },
            ],
        }),
    );
    assert!(matches!(
        flatten(&shape, &values, FlattenRequest::new(1.0, 0.02).unwrap()),
        Err(VectorError::Shape(ShapeError::MorphCorrespondence { .. }))
    ));
    let start = property("kronello.shape.trim_start", Value::Scalar(f(0.25)));
    let end = property("kronello.shape.trim_end", Value::Scalar(f(0.75)));
    let offset = property("kronello.shape.trim_offset", Value::Scalar(f(-1.0)));
    for prop in [&start, &end, &offset] {
        values.insert(
            prop.id(),
            match prop.source() {
                PropertySource::Constant(v) => v.clone(),
                _ => panic!(),
            },
        );
    }
    let shape = Shape {
        id: ContentId::new(),
        geometry: ShapeGeometry::TrimmedPath {
            path: from.id(),
            start: start.id(),
            end: end.id(),
            offset: offset.id(),
        },
        fill: None,
        stroke: None,
    };
    let result = flatten(&shape, &values, FlattenRequest::new(1.0, 0.02).unwrap()).unwrap();
    assert_eq!(result.subpaths[0].points, vec![[25.0, 0.0], [75.0, 0.0]]);
    assert!(!result.subpaths[0].closed);
    values.insert(start.id(), Value::Scalar(f(0.9)));
    assert!(matches!(
        shape.resolve(&values),
        Err(ShapeError::InvalidTrimRange)
    ));
}
#[test]
fn trim_wrap_multiple_contours_zero_and_full_selection() {
    let path = FlattenedPath {
        subpaths: vec![
            Polyline {
                points: vec![[0.0, 0.0], [100.0, 0.0]],
                closed: false,
            },
            Polyline {
                points: vec![[0.0, 10.0], [100.0, 10.0]],
                closed: false,
            },
        ],
    };
    let trimmed = trim_path(&path, 0.25, 0.75, 0.0).unwrap();
    assert_eq!(trimmed.subpaths.len(), 2);
    assert_eq!(trimmed.subpaths[0].points, vec![[50.0, 0.0], [100.0, 0.0]]);
    assert_eq!(trimmed.subpaths[1].points, vec![[0.0, 10.0], [50.0, 10.0]]);
    let wrapped = trim_path(&path, 0.0, 0.5, -0.25).unwrap();
    assert_eq!(
        wrapped.subpaths[0].points,
        vec![[50.0, 10.0], [100.0, 10.0]]
    );
    assert_eq!(wrapped.subpaths[1].points, vec![[0.0, 0.0], [50.0, 0.0]]);
    assert!(trim_path(&path, 0.4, 0.4, 0.0).unwrap().subpaths.is_empty());
    let mut closed = path;
    closed.subpaths[0].closed = true;
    assert_eq!(trim_path(&closed, 0.0, 1.0, 0.3).unwrap(), closed);
    assert!(trim_path(&closed, 0.0, 0.5, f64::INFINITY).is_err());
}
#[test]
fn svg_report_refuses_executable_reference_and_unsupported_features() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><script onload="run()">command</script><image href="https://example.invalid/image.png"/><path d="M0 0L10 10" fill="url(#gradient)" stroke="#fff"/></svg>"##;
    let report = inspect_svg(svg).unwrap();
    assert!(report.unsupported.iter().any(|s| s == "element script"));
    assert!(
        report
            .unsupported
            .iter()
            .any(|s| s == "path attribute stroke")
    );
    assert!(
        report
            .external_references
            .iter()
            .any(|s| s == "https://example.invalid/image.png")
    );
    assert!(matches!(
        report.ensure_supported(),
        Err(SvgError::Unsupported)
    ));
    let entity = inspect_svg(r#"<!DOCTYPE svg SYSTEM "file:///etc/passwd"><svg/>"#).unwrap();
    assert!(entity.ensure_supported().is_err());
    assert!(inspect_svg("<svg><path d='M0 0' d='M1 1'/></svg>").is_err());
    assert!(inspect_svg("<svg><path d='M0 0'></svg>").is_err());
    assert!(matches!(
        inspect_svg(&" ".repeat(SVG_MAX_BYTES + 1)),
        Err(SvgError::BudgetExceeded)
    ));
}
#[test]
fn svg_import_export_preserves_local_paths_controls_and_fill_rules() {
    let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 0 Q20 10 10 20 C5 20 0 10 0 0 Z" fill="#12abef" fill-rule="evenodd"/><path d="M1 1L2 2" fill="none"/></svg>"##;
    let report = inspect_svg(input).unwrap();
    report.ensure_supported().unwrap();
    let output = export_svg(&report.paths).unwrap();
    assert_eq!(inspect_svg(&output).unwrap().paths, report.paths);
    let report = inspect_svg(
        "<svg viewBox='0 0 100 100'><g transform='scale(2)'><path d='M0 0L1 1'/></g></svg>",
    )
    .unwrap();
    assert!(report.ensure_supported().is_err());
    let mut path = inspect_svg("<svg><path d='M0 0L1 1'/></svg>")
        .unwrap()
        .paths
        .remove(0);
    path.fill = Some(Color::new(ColorSpace::LinearRec709, [2.0, 0.0, 0.0], 1.0).unwrap());
    assert!(matches!(export_svg(&[path]), Err(SvgError::Unsupported)));
}

#[test]
fn closed_trim_wrap_preserves_continuous_seam_without_caps() {
    let square = FlattenedPath {
        subpaths: vec![Polyline {
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            closed: true,
        }],
    };
    let result = trim_path(&square, 0.0, 0.5, 0.75).unwrap();
    assert_eq!(result.subpaths.len(), 1);
    assert_eq!(
        result.subpaths[0].points,
        vec![[0.0, 10.0], [0.0, 0.0], [10.0, 0.0]]
    );
    assert!(!result.subpaths[0].closed);
}
