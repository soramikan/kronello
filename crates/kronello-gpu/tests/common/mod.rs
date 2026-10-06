use kronello_gpu::*;
use kronello_model as model;
use std::collections::BTreeMap;

pub fn paint(rgba: [f32; 4], space: InputSpace) -> Paint {
    Paint { rgba, space }
}
pub fn rectangle(min: [f32; 2], max: [f32; 2], color: Paint) -> DrawNode {
    DrawNode::Path(PathDraw {
        stroke_geometry: None,
        fill_gradient: None,
        stroke_gradient: None,
        paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
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
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: vec![Contour {
                points: vec![[0.3, 0.4], [7.3, 2.1], [2.6, 7.3]],
                closed: true,
            }],
            fill: Some(Fill {
                paint: paint([0.8, 0.3, 0.1, 0.7], InputSpace::Srgb),
                rule: FillRule::Nonzero,
            }),
            stroke: Some(RoundStroke {
                join: StrokeJoin::Round,
                cap: StrokeCap::Round,
                miter_limit: 4.0,
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
            gradient: None,
            range: model::TextRange { start: 0, end: 3 },
            font: font.clone(),
            size: f(25.0),
            fill: model::Color::from_srgb8([255, 80, 20], None),
        }],
        direction: model::TextDirection::Horizontal,
        ruby: vec![],
        character_animations: vec![],
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
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
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
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
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
pub fn effect_scene(shadow: bool, working: WorkingSpace) -> DrawScene {
    let color = model::Color::new(model::ColorSpace::Srgb, [0.2, 0.5, 0.9], 0.7).unwrap();
    let effect = if shadow {
        PixelEffect::DropShadow {
            sigma: [1.2, 0.7],
            offset: [2.25, -1.5],
            color,
            opacity: 0.6,
        }
    } else {
        PixelEffect::GaussianBlur { sigma: [1.2, 0.7] }
    };
    let _ = working;
    DrawScene {
        nodes: vec![
            rectangle(
                [5.25, 5.5],
                [10.25, 11.5],
                paint([0.8, 0.2, 0.1, 0.65], InputSpace::Srgb),
            ),
            DrawNode::Effect { source: 0, effect },
        ],
        roots: vec![1],
    }
}
pub fn affine_effect_scene(linear: [[f64; 2]; 2], shadow: bool) -> DrawScene {
    let resolved = if shadow {
        model::ResolvedEffect::AffineDropShadow {
            sigma: 1.1,
            linear,
            offset: [
                linear[0][0] * 2.25 - linear[0][1] * 1.5,
                linear[1][0] * 2.25 - linear[1][1] * 1.5,
            ],
            color: model::Color::new(model::ColorSpace::Srgb, [0.2, 0.5, 0.9], 0.7).unwrap(),
            opacity: 0.6,
        }
    } else {
        model::ResolvedEffect::AffineGaussianBlur { sigma: 1.1, linear }
    };
    let effect = PixelEffect::from_design(&resolved, [1.0; 2]).unwrap();
    let mut scene = effect_scene(shadow, WorkingSpace::LinearRec709);
    scene.nodes[1] = DrawNode::Effect { source: 0, effect };
    scene
}
pub fn fx002_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    vec![
        (
            "fx002-rotation",
            16,
            WorkingSpace::LinearRec709,
            affine_effect_scene([[0.8, -0.6], [0.6, 0.8]], false),
        ),
        (
            "fx002-nonuniform-rotation",
            16,
            WorkingSpace::LinearRec709,
            affine_effect_scene([[1.6, -0.6], [1.2, 0.8]], false),
        ),
        (
            "fx002-shear-shadow",
            16,
            WorkingSpace::LinearRec709,
            affine_effect_scene([[1.5, 0.7], [0.0, 0.8]], true),
        ),
        (
            "fx002-reflected-shear-shadow-rec2020",
            16,
            WorkingSpace::LinearRec2020,
            affine_effect_scene([[-1.2, 0.5], [0.2, 0.8]], true),
        ),
    ]
}
pub fn scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    let mut scenes = vec![
        (
            "fx-gaussian-alpha",
            16,
            WorkingSpace::LinearRec709,
            effect_scene(false, WorkingSpace::LinearRec709),
        ),
        (
            "fx-shadow-srgb",
            16,
            WorkingSpace::LinearRec709,
            effect_scene(true, WorkingSpace::LinearRec709),
        ),
        (
            "fx-shadow-rec2020",
            16,
            WorkingSpace::LinearRec2020,
            effect_scene(true, WorkingSpace::LinearRec2020),
        ),
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
    ];
    scenes.extend(vec003_scenes());
    scenes.extend(vec004_scenes());
    scenes.extend(fx002_scenes());
    scenes.extend(vec005_scenes());
    scenes
}

pub fn stroke_styles(caps: bool, fallback: bool) -> DrawScene {
    let mut nodes = Vec::new();
    for i in 0..3 {
        let x = 2.0 + 10.0 * i as f32;
        nodes.push(DrawNode::Path(PathDraw {
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: vec![Contour {
                points: if caps {
                    vec![[x + 2.0, 5.0], [x + 2.0, 25.0]]
                } else {
                    vec![[x, 10.0], [x + 5.0, 10.0], [x + 5.0, 22.0]]
                },
                closed: false,
            }],
            fill: None,
            stroke: Some(RoundStroke {
                paint: paint([0.8, 0.2, 0.7, 0.7], InputSpace::Srgb),
                width: 4.0,
                join: [StrokeJoin::Miter, StrokeJoin::Bevel, StrokeJoin::Round][i],
                cap: if caps {
                    [StrokeCap::Butt, StrokeCap::Square, StrokeCap::Round][i]
                } else {
                    StrokeCap::Butt
                },
                miter_limit: if fallback { 1.0 } else { 4.0 },
            }),
        }));
    }
    DrawScene {
        nodes,
        roots: vec![0, 1, 2],
    }
}
pub fn gradient_scene(radial: bool) -> DrawScene {
    let mut path = match rectangle([1.3, 1.7], [29.2, 29.7], paint([1.0; 4], InputSpace::Srgb)) {
        DrawNode::Path(p) => p,
        _ => unreachable!(),
    };
    let geometry = if radial {
        GradientGeometry::Radial {
            center: [14.0, 15.0],
            radius: 10.0,
        }
    } else {
        GradientGeometry::Linear {
            start: [5.0, 4.0],
            end: [24.0, 25.0],
        }
    };
    let gradient = GradientPaint {
        spread: Default::default(),
        interpolation: Default::default(),
        interpolation_version: 1,
        transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        geometry,
        stops: vec![
            GradientStop {
                offset: 0.1,
                paint: paint([1.0, 0.1, 0.0, 1.0], InputSpace::Srgb),
            },
            GradientStop {
                offset: 0.45,
                paint: paint([0.0, 0.0, 1.0, 0.0], InputSpace::Srgb),
            },
            GradientStop {
                offset: 0.45,
                paint: paint([0.1, 0.8, 0.2, 0.6], InputSpace::LinearRec2020),
            },
            GradientStop {
                offset: 0.85,
                paint: paint([1.8, -0.1, 0.2, 0.8], InputSpace::LinearRec709),
            },
        ],
    };
    path.fill_gradient = Some(Box::new(gradient.clone()));
    path.stroke_gradient = Some(Box::new(gradient));
    path.stroke = Some(RoundStroke {
        paint: paint([1.0; 4], InputSpace::Srgb),
        width: 2.4,
        join: StrokeJoin::Miter,
        cap: StrokeCap::Square,
        miter_limit: 4.0,
    });
    path.paint_transform = [[0.9, 0.1, -0.3], [-0.1, 1.1, 0.7]];
    DrawScene {
        nodes: vec![DrawNode::Path(path)],
        roots: vec![0],
    }
}
pub fn vec003_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    vec![
        (
            "stroke-joins",
            32,
            WorkingSpace::LinearRec709,
            stroke_styles(false, false),
        ),
        (
            "stroke-caps",
            32,
            WorkingSpace::LinearRec709,
            stroke_styles(true, false),
        ),
        (
            "stroke-miter-limit",
            32,
            WorkingSpace::LinearRec709,
            stroke_styles(false, true),
        ),
        (
            "gradient-linear-fill-stroke",
            32,
            WorkingSpace::LinearRec709,
            gradient_scene(false),
        ),
        (
            "gradient-radial-fill-stroke",
            32,
            WorkingSpace::LinearRec2020,
            gradient_scene(true),
        ),
    ]
}

/// Extended paints exercise independent fill/stroke mappings and both spaces.
pub fn vec004_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    let settings = [
        (
            "gradient-repeat",
            GradientSpread::Repeat,
            GradientInterpolation::WorkingLinearPremultiplied,
        ),
        (
            "gradient-reflect",
            GradientSpread::Reflect,
            GradientInterpolation::WorkingLinearPremultiplied,
        ),
        (
            "gradient-focal-radial",
            GradientSpread::Repeat,
            GradientInterpolation::WorkingLinearPremultiplied,
        ),
        (
            "gradient-conic",
            GradientSpread::Reflect,
            GradientInterpolation::WorkingLinearPremultiplied,
        ),
        (
            "gradient-linear-straight",
            GradientSpread::Pad,
            GradientInterpolation::WorkingLinearStraight,
        ),
        (
            "gradient-srgb-straight",
            GradientSpread::Pad,
            GradientInterpolation::SrgbStraight,
        ),
        (
            "gradient-srgb-premultiplied",
            GradientSpread::Pad,
            GradientInterpolation::SrgbPremultiplied,
        ),
        (
            "gradient-text-fill",
            GradientSpread::Reflect,
            GradientInterpolation::SrgbStraight,
        ),
    ];
    settings
        .into_iter()
        .enumerate()
        .map(|(i, (id, spread, interpolation))| {
            let mut scene = if i == 7 {
                glyph()
            } else {
                gradient_scene(false)
            };
            let DrawNode::Path(path) = &mut scene.nodes[0] else {
                panic!("path fixture");
            };
            let mut gradient = if let Some(g) = &path.fill_gradient {
                g.as_ref().clone()
            } else {
                let source = gradient_scene(false);
                let DrawNode::Path(p) = &source.nodes[0] else {
                    unreachable!()
                };
                *p.fill_gradient.clone().unwrap()
            };
            gradient.spread = spread;
            gradient.interpolation = interpolation;
            gradient.transform = [[0.8, 0.15, -2.0], [-0.1, 0.9, 1.0]];
            if i == 2 {
                gradient.geometry = GradientGeometry::FocalRadial {
                    center: [16.0; 2],
                    radius: 12.0,
                    focal: [12.0, 15.0],
                    focal_radius: 2.0,
                };
            } else if i == 3 {
                gradient.geometry = GradientGeometry::Conic {
                    center: [16.0; 2],
                    start_angle: 25.0,
                    sweep_angle: 240.0,
                };
            } else if i == 7 {
                gradient.geometry = GradientGeometry::Linear {
                    start: [0.0; 2],
                    end: [8.0, 0.0],
                };
            } else {
                // The sample grid and paint transform put mapped samples on a
                // 1/1600 px lattice; a 0.0003 px phase keeps hard stops and
                // repeat seams off exact ties, whose side depends on FMA.
                gradient.geometry = GradientGeometry::Linear {
                    start: [0.0003, 0.0],
                    end: [12.0003, 0.0],
                };
            }
            path.fill_gradient = Some(Box::new(gradient.clone()));
            if i != 7 {
                gradient.transform = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
                path.stroke_gradient = Some(Box::new(gradient));
            }
            (
                id,
                32,
                if i % 2 == 0 {
                    WorkingSpace::LinearRec709
                } else {
                    WorkingSpace::LinearRec2020
                },
                scene,
            )
        })
        .collect()
}

pub fn vec005_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    use model::StrokeAlignment;
    [
        "stroke-dashes",
        "stroke-inside-evenodd",
        "stroke-outside-nonzero",
        "stroke-affine-reflected",
    ]
    .into_iter()
    .enumerate()
    .map(|(case, id)| {
        let mut local = vec![Contour {
            points: vec![
                [0.031, 0.019],
                [12.331, 0.019],
                [12.331, 9.719],
                [0.031, 9.719],
            ],
            closed: true,
        }];
        if case == 1 || case == 2 {
            let mut hole = vec![
                [3.137, 3.271],
                [9.113, 3.271],
                [9.113, 6.317],
                [3.137, 6.317],
            ];
            if case == 2 {
                hole.reverse();
            }
            local.push(Contour {
                points: hole,
                closed: true,
            });
        }
        let matrix: [[f32; 3]; 2] = if case == 3 {
            [[-1.3, 0.29, 24.037], [0.17, 0.83, 7.043]]
        } else {
            [[1.17, 0.23, 7.037], [0.13, 1.07, 5.043]]
        };
        let det = matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
        let inverse = [
            [
                matrix[1][1] / det,
                -matrix[0][1] / det,
                (matrix[0][1] * matrix[1][2] - matrix[1][1] * matrix[0][2]) / det,
            ],
            [
                -matrix[1][0] / det,
                matrix[0][0] / det,
                (matrix[1][0] * matrix[0][2] - matrix[0][0] * matrix[1][2]) / det,
            ],
        ];
        let array = if case == 0 {
            vec![0.0, 2.173, 3.317]
        } else if case == 3 {
            vec![3.173, 1.317]
        } else {
            vec![]
        };
        let offset = 0.371;
        let flattened = kronello_vector::FlattenedPath {
            subpaths: local
                .iter()
                .map(|c| kronello_vector::Polyline {
                    points: c.points.iter().map(|p| p.map(f64::from)).collect(),
                    closed: c.closed,
                })
                .collect(),
        };
        let dashed = kronello_vector::dash_path(&flattened, &array, offset).unwrap();
        let alignment = match case {
            1 => StrokeAlignment::Inside,
            2 => StrokeAlignment::Outside,
            _ => StrokeAlignment::Center,
        };
        let rule = if case == 1 {
            FillRule::Evenodd
        } else {
            FillRule::Nonzero
        };
        let draw = PathDraw {
            stroke_geometry: Some(LocalStrokeGeometry {
                version: model::EXTENDED_STROKE_VERSION.into(),
                contours: dashed
                    .subpaths
                    .into_iter()
                    .map(|c| Contour {
                        points: c.points.into_iter().map(|p| p.map(|x| x as f32)).collect(),
                        closed: c.closed,
                    })
                    .collect(),
                output_to_local: inverse,
                alignment,
                fill_rule: rule,
                dash_array: array,
                dash_offset: offset,
            }),
            contours: local
                .into_iter()
                .map(|c| Contour {
                    points: c
                        .points
                        .into_iter()
                        .map(|p| matrix.map(|r| r[0] * p[0] + r[1] * p[1] + r[2]))
                        .collect(),
                    closed: c.closed,
                })
                .collect(),
            fill: None,
            stroke: Some(RoundStroke {
                width: 1.713,
                join: StrokeJoin::Round,
                cap: StrokeCap::Round,
                miter_limit: 4.0,
                paint: paint([0.17, 0.63, 1.3, 0.83], InputSpace::LinearRec709),
            }),
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        };
        (
            id,
            32,
            WorkingSpace::LinearRec709,
            DrawScene {
                nodes: vec![DrawNode::Path(draw)],
                roots: vec![0],
            },
        )
    })
    .collect()
}
