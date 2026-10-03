use kronello_gpu::*;
use kronello_model as model;
use std::collections::BTreeMap;

pub fn paint(rgba: [f32; 4], space: InputSpace) -> Paint {
    Paint { rgba, space }
}
pub fn rectangle(min: [f32; 2], max: [f32; 2], color: Paint) -> DrawNode {
    DrawNode::Path(PathDraw {
        contours: vec![Contour {
            points: vec![min, [max[0], min[1]], max, [min[0], max[1]]],
            closed: true,
        }],
        fill: Some(Fill {
            paint: color,
            rule: FillRule::Nonzero,
        }),
        stroke: None,
    })
}
pub fn isolated() -> DrawScene {
    DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [6.0; 2],
                paint([1.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            rectangle(
                [2.0; 2],
                [8.0; 2],
                paint([0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
            ),
            DrawNode::Group {
                children: vec![0, 1],
                opacity: 0.5,
            },
            DrawNode::Group {
                children: vec![2],
                opacity: 0.5,
            },
        ],
        roots: vec![3],
    }
}
pub fn matte(kind: MaskKind) -> DrawScene {
    DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [8.0; 2],
                paint([0.8, 0.4, 0.2, 0.75], InputSpace::Srgb),
            ),
            rectangle(
                [1.3, 0.7],
                [6.7, 7.3],
                paint([0.1, 0.8, 0.3, 0.5], InputSpace::Srgb),
            ),
            DrawNode::Masked {
                source: 0,
                matte: 1,
                kind,
            },
        ],
        roots: vec![2],
    }
}
pub fn edges() -> DrawScene {
    DrawScene {
        nodes: vec![DrawNode::Path(PathDraw {
            contours: vec![Contour {
                points: vec![[0.3, 0.4], [7.3, 2.1], [2.6, 7.3]],
                closed: true,
            }],
            fill: Some(Fill {
                paint: paint([0.8, 0.3, 0.1, 0.7], InputSpace::Srgb),
                rule: FillRule::Nonzero,
            }),
            stroke: Some(RoundStroke {
                paint: paint([1.0, 0.2, 0.1, 0.6], InputSpace::Srgb),
                width: 0.7,
            }),
        })],
        roots: vec![0],
    }
}
pub fn glyph() -> DrawScene {
    let bytes =
        std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
    let font = kronello_text::pin_font(&bytes, 0).unwrap();
    let f = |v| model::FiniteF64::new(v).unwrap();
    let text = model::ResolvedText {
        layout_version: kronello_text::LAYOUT_VERSION,
        text: "あ".into(),
        styles: vec![model::ResolvedTextStyle {
            range: model::TextRange { start: 0, end: 3 },
            font: font.clone(),
            size: f(25.0),
            fill: model::Color::from_srgb8([255, 80, 20], None),
        }],
        direction: model::TextDirection::Horizontal,
        ruby: vec![],
        wrap_width: f(32.0),
        line_height: f(30.0),
        alignment: model::TextAlignment::Start,
    };
    let layout = kronello_text::layout(
        &text,
        &[kronello_text::FontData {
            identity: &font,
            bytes: &bytes,
        }],
    )
    .unwrap();
    let mut nodes = Vec::new();
    for glyph in layout.glyphs {
        let property = model::PropertyId::new();
        let shape = model::Shape {
            id: model::ContentId::new(),
            geometry: model::ShapeGeometry::BezierPath { path: property },
            fill: None,
            stroke: None,
        };
        let values = BTreeMap::from([(property, model::Value::Path(glyph.outline))]);
        let path = kronello_vector::flatten(
            &shape,
            &values,
            kronello_vector::FlattenRequest::new(1.0, 0.02).unwrap(),
        )
        .unwrap();
        nodes.push(DrawNode::Path(PathDraw {
            contours: path
                .subpaths
                .into_iter()
                .map(|c| Contour {
                    points: c
                        .points
                        .into_iter()
                        .map(|p| [(p[0] + 0.31) as f32, (p[1] + 0.27) as f32])
                        .collect(),
                    closed: c.closed,
                })
                .collect(),
            fill: Some(Fill {
                paint: paint([1.0, 80.0 / 255.0, 20.0 / 255.0, 1.0], InputSpace::Srgb),
                rule: FillRule::Nonzero,
            }),
            stroke: None,
        }));
    }
    let roots = (0..nodes.len()).collect();
    DrawScene { nodes, roots }
}
pub fn fill_rules(rule: FillRule) -> DrawScene {
    let contour = |min, max| match rectangle(min, max, paint([1.0; 4], InputSpace::LinearRec709)) {
        DrawNode::Path(p) => p.contours[0].clone(),
        _ => unreachable!(),
    };
    DrawScene {
        nodes: vec![DrawNode::Path(PathDraw {
            contours: vec![contour([0.3; 2], [7.7; 2]), contour([2.3; 2], [5.7; 2])],
            fill: Some(Fill {
                paint: paint([1.0; 4], InputSpace::LinearRec709),
                rule,
            }),
            stroke: None,
        })],
        roots: vec![0],
    }
}
pub fn scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    vec![
        (
            "isolated-nested-overlap",
            8,
            WorkingSpace::LinearRec709,
            isolated(),
        ),
        (
            "alpha-matte-reference",
            8,
            WorkingSpace::LinearRec709,
            matte(MaskKind::Alpha),
        ),
        (
            "luma-matte-rec709",
            8,
            WorkingSpace::LinearRec709,
            matte(MaskKind::Luminance),
        ),
        (
            "luma-matte-rec2020",
            8,
            WorkingSpace::LinearRec2020,
            matte(MaskKind::Luminance),
        ),
        (
            "coverage-fill-stroke",
            8,
            WorkingSpace::LinearRec709,
            edges(),
        ),
        (
            "coverage-srgb-rec2020",
            8,
            WorkingSpace::LinearRec2020,
            edges(),
        ),
        (
            "fill-evenodd-hole",
            8,
            WorkingSpace::LinearRec709,
            fill_rules(FillRule::Evenodd),
        ),
        (
            "fill-nonzero-winding",
            8,
            WorkingSpace::LinearRec709,
            fill_rules(FillRule::Nonzero),
        ),
        (
            "glyph-outline-edges",
            32,
            WorkingSpace::LinearRec709,
            glyph(),
        ),
    ]
}
