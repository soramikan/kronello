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
        path: None,
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
    scenes.extend(color002_scenes());
    scenes.extend(fx003_scenes());
    scenes.extend(fx005006_scenes());
    scenes.extend(fx008_scenes());
    scenes
}

/// COLOR-002 scenes (ADR-0108): one pointwise correction per scene over a
/// mid-alpha solid so alpha preservation and extended range stay observable.
pub fn color002_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    let scene = |effect: PixelEffect| DrawScene {
        nodes: vec![
            rectangle(
                [1.0; 2],
                [15.0; 2],
                paint([0.72, 0.45, 0.18, 0.875], InputSpace::Srgb),
            ),
            DrawNode::Effect { source: 0, effect },
        ],
        roots: vec![1],
    };
    vec![
        (
            "color002-exposure-rec709",
            16,
            WorkingSpace::LinearRec709,
            scene(PixelEffect::ColorExposure {
                exposure: -0.75,
                offset: 0.04,
            }),
        ),
        (
            "color002-levels-rec2020",
            16,
            WorkingSpace::LinearRec2020,
            scene(PixelEffect::ColorLevels {
                in_black: 0.1,
                in_white: 0.9,
                gamma: 1.8,
                out_black: -0.05,
                out_white: 1.1,
            }),
        ),
        (
            "color002-curves-rec709",
            16,
            WorkingSpace::LinearRec709,
            scene(PixelEffect::ColorCurves {
                points: vec![[0.0, 0.0], [0.3, 0.6], [0.7, 0.75], [1.0, 1.0]],
            }),
        ),
        (
            "color002-hsl-rec709",
            16,
            WorkingSpace::LinearRec709,
            scene(PixelEffect::ColorHsl {
                hue_shift: 72.0,
                saturation: 0.6,
                lightness: 0.04,
            }),
        ),
    ]
}

/// FX-003 scenes (ADR-0109): the W3C blend-mode set tiled per mode, then one
/// parameterized scene per transition kind. Inputs avoid the dodge/burn
/// denominator ties where binary16 rounding would diverge between backends.
fn blend_tiles(modes: &[model::BlendMode]) -> DrawScene {
    // 4 columns of 8px tiles with 4px gutters inside a 48px canvas. The
    // source tile is shifted right/down so each tile shows the backdrop, the
    // blend overlap, and the partial-alpha source edge at once.
    let mut nodes = Vec::new();
    let mut roots = Vec::new();
    for (index, mode) in modes.iter().enumerate() {
        let (column, row) = (index % 4, index / 4);
        let (x, y) = (2.0 + column as f32 * 12.0, 2.0 + row as f32 * 12.0);
        nodes.push(rectangle(
            [x, y],
            [x + 8.0, y + 8.0],
            paint([0.55, 0.35, 0.2, 1.0], InputSpace::LinearRec709),
        ));
        nodes.push(rectangle(
            [x + 2.0, y + 2.0],
            [x + 10.0, y + 10.0],
            paint([0.3, 0.6, 0.45, 0.85], InputSpace::LinearRec709),
        ));
        let blend = nodes.len();
        nodes.push(DrawNode::Blend {
            source: blend - 1,
            backdrop: blend - 2,
            mode: *mode,
        });
        roots.push(blend);
    }
    DrawScene { nodes, roots }
}

pub fn fx003_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    use model::BlendMode::*;
    let separable = [
        Multiply,
        Screen,
        Darken,
        Lighten,
        ColorDodge,
        ColorBurn,
        HardLight,
        SoftLight,
        Difference,
        Exclusion,
        Overlay,
        LinearDodge,
        LinearBurn,
        VividLight,
        LinearLight,
    ];
    let nonseparable = [Hue, Saturation, Color, Luminosity];
    let wipe = DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([1.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
            ),
            // Wipe midpoint reveal: hard-edged left-half matte (ADR-0109).
            rectangle(
                [0.0; 2],
                [8.0, 16.0],
                paint([1.0; 4], InputSpace::LinearRec709),
            ),
            DrawNode::Masked {
                source: 1,
                matte: 2,
                kind: MaskKind::Alpha,
            },
        ],
        roots: vec![0, 3],
    };
    let slide = DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([1.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            // Slide-from-right midpoint: the incoming clip is translated and
            // covers the right half; the outgoing clip stays put.
            rectangle(
                [8.0, 0.0],
                [24.0, 16.0],
                paint([0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
            ),
        ],
        roots: vec![0, 1],
    };
    let dip = DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([1.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            // Dip past midpoint: the dip color is fully opaque underneath and
            // the incoming clip fades in over it.
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([0.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [16.0; 2],
                paint([0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
            ),
            DrawNode::Group {
                children: vec![2],
                opacity: 0.5,
            },
        ],
        roots: vec![0, 1, 3],
    };
    vec![
        (
            "fx003-blend-separable",
            48,
            WorkingSpace::LinearRec709,
            blend_tiles(&separable),
        ),
        (
            "fx003-blend-nonseparable",
            48,
            WorkingSpace::LinearRec709,
            blend_tiles(&nonseparable),
        ),
        ("fx003-wipe", 16, WorkingSpace::LinearRec709, wipe),
        ("fx003-slide", 16, WorkingSpace::LinearRec709, slide),
        ("fx003-dip", 16, WorkingSpace::LinearRec709, dip),
    ]
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
/// FX-005/FX-006 scenes (ADR-0115): matte keying over two-color inputs,
/// kernel-based glow/sharpen, and the spatial vignette/warp effects. The
/// corner pin quad is stated explicitly because DrawScene-level effects carry
/// their source bounds directly.
pub fn fx005006_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    let two_tones = |a: [f32; 4], b: [f32; 4], space: InputSpace| DrawScene {
        nodes: vec![
            rectangle([0.0; 2], [8.0, 16.0], paint(a, space)),
            rectangle([8.0, 0.0], [16.0; 2], paint(b, space)),
            DrawNode::Group {
                children: vec![0, 1],
                opacity: 1.0,
            },
        ],
        roots: vec![2],
    };
    let chroma = {
        let mut scene = two_tones(
            [0.0, 0.694, 0.251, 1.0],
            [0.8, 0.15, 0.1, 1.0],
            InputSpace::Srgb,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::ChromaKey {
                key_color: model::Color::from_srgb8([0, 177, 64], None),
                similarity: 0.4,
                edge_shrink: [0.0; 2],
                edge_feather: [0.0; 2],
                spill: 0.5,
            },
        });
        scene.roots = vec![3];
        scene
    };
    let chroma_edges = {
        let mut scene = two_tones(
            [0.0, 0.694, 0.251, 1.0],
            [0.8, 0.15, 0.1, 1.0],
            InputSpace::Srgb,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::ChromaKey {
                key_color: model::Color::from_srgb8([0, 177, 64], None),
                similarity: 0.45,
                edge_shrink: [1.4, 0.6],
                edge_feather: [0.9, 0.4],
                spill: 1.0,
            },
        });
        scene.roots = vec![3];
        scene
    };
    let luma = {
        let mut scene = two_tones(
            [0.02, 0.02, 0.02, 1.0],
            [0.85, 0.6, 0.2, 1.0],
            InputSpace::LinearRec709,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::LumaKey {
                key_luma: 0.05,
                tolerance: 0.15,
                edge_shrink: [0.75, 0.0],
                edge_feather: [0.6, 0.6],
            },
        });
        scene.roots = vec![3];
        scene
    };
    let glow = {
        let mut scene = two_tones(
            [0.95, 0.9, 0.4, 1.0],
            [0.05, 0.05, 0.08, 1.0],
            InputSpace::LinearRec709,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::Glow {
                threshold: 0.5,
                radius: [1.6, 0.9],
                intensity: 0.8,
            },
        });
        scene.roots = vec![3];
        scene
    };
    let sharpen = {
        let mut scene = two_tones(
            [0.85, 0.2, 0.1, 1.0],
            [0.1, 0.15, 0.7, 1.0],
            InputSpace::LinearRec709,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::Sharpen {
                amount: 1.5,
                radius: [1.1, 0.8],
            },
        });
        scene.roots = vec![3];
        scene
    };
    let vignette = {
        let mut scene = two_tones(
            [0.6, 0.5, 0.4, 1.0],
            [0.3, 0.35, 0.5, 1.0],
            InputSpace::LinearRec709,
        );
        scene.nodes.push(DrawNode::Effect {
            source: 2,
            effect: PixelEffect::Vignette {
                amount: 0.85,
                midpoint: 0.2,
                feather: 0.7,
                roundness: 0.6,
            },
        });
        scene.roots = vec![3];
        scene
    };
    let corner_pin = DrawScene {
        nodes: vec![
            rectangle(
                [2.0; 2],
                [14.0; 2],
                paint([0.7, 0.3, 0.55, 1.0], InputSpace::LinearRec709),
            ),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::CornerPin {
                    pins: [[3.0, 1.0], [14.0, 3.0], [12.0, 14.0], [1.0, 13.0]],
                    // The drawn quad's conservative visual bounds.
                    source: Some(kronello_render::PixelBounds {
                        min: [1.0, 1.0],
                        max: [15.0, 15.0],
                    }),
                },
            },
        ],
        roots: vec![1],
    };
    vec![
        ("fx005-chroma-key", 16, WorkingSpace::LinearRec709, chroma),
        (
            "fx005-chroma-key-edges",
            16,
            WorkingSpace::LinearRec709,
            chroma_edges,
        ),
        ("fx005-luma-key", 16, WorkingSpace::LinearRec2020, luma),
        ("fx006-glow", 16, WorkingSpace::LinearRec709, glow),
        ("fx006-sharpen", 16, WorkingSpace::LinearRec2020, sharpen),
        ("fx006-vignette", 16, WorkingSpace::LinearRec709, vignette),
        (
            "fx006-corner-pin",
            16,
            WorkingSpace::LinearRec709,
            corner_pin,
        ),
    ]
}

/// FX-008 scenes (ADR-0137): the nine versioned standard effects. Sharp
/// two-tone content keeps cell, mixing and warp geometry observable; the
/// displace map and the generate leaf exercise the FX-008 node kinds.
pub fn fx008_scenes() -> Vec<(&'static str, u32, WorkingSpace, DrawScene)> {
    let two_tones = |a: [f32; 4], b: [f32; 4], space: InputSpace| DrawScene {
        nodes: vec![
            rectangle([0.0; 2], [8.0, 16.0], paint(a, space)),
            rectangle([8.0, 0.0], [16.0; 2], paint(b, space)),
            DrawNode::Group {
                children: vec![0, 1],
                opacity: 1.0,
            },
        ],
        roots: vec![2],
    };
    let with_effect = |mut scene: DrawScene, effect: PixelEffect| {
        scene.nodes.push(DrawNode::Effect { source: 2, effect });
        scene.roots = vec![3];
        scene
    };
    let grain = with_effect(
        two_tones(
            [0.55, 0.4, 0.2, 0.85],
            [0.15, 0.3, 0.65, 0.85],
            InputSpace::Srgb,
        ),
        PixelEffect::Grain {
            amount: 0.4,
            size: 2.0,
            monochrome: false,
            seed: 17,
        },
    );
    let mosaic = with_effect(
        two_tones(
            [0.9, 0.3, 0.2, 1.0],
            [0.1, 0.5, 0.8, 1.0],
            InputSpace::LinearRec709,
        ),
        PixelEffect::Mosaic {
            block_size: 3.0,
            basis: model::MosaicBasis::Center,
        },
    );
    let mixer = with_effect(
        two_tones(
            [0.7, 0.25, 0.15, 1.0],
            [0.2, 0.6, 0.4, 0.75],
            InputSpace::LinearRec2020,
        ),
        PixelEffect::ChannelMixer {
            // Swap red/blue, halve green, keep alpha.
            matrix: [
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.5, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
    );
    let mut invert = mixer.clone();
    invert.nodes.push(DrawNode::Effect {
        source: 3,
        effect: PixelEffect::Invert {
            channel: model::InvertChannel::Green,
        },
    });
    invert.roots = vec![4];
    let tint = with_effect(
        two_tones(
            [0.05, 0.05, 0.05, 1.0],
            [0.85, 0.7, 0.4, 1.0],
            InputSpace::LinearRec709,
        ),
        PixelEffect::Tint {
            map_black: model::Color::from_srgb8([20, 30, 80], None),
            map_white: model::Color::from_srgb8([240, 200, 60], None),
            amount: 0.85,
        },
    );
    let directional = with_effect(
        two_tones(
            [0.85, 0.2, 0.1, 1.0],
            [0.05, 0.1, 0.6, 1.0],
            InputSpace::LinearRec709,
        ),
        PixelEffect::DirectionalBlur {
            direction: [0.6, -0.8],
            length: 5.0,
        },
    );
    let radial = with_effect(
        two_tones(
            [0.9, 0.85, 0.3, 1.0],
            [0.1, 0.1, 0.55, 1.0],
            InputSpace::LinearRec2020,
        ),
        PixelEffect::RadialBlur {
            mode: model::RadialBlurMode::Spin,
            amount: 25.0,
            center: [8.0, 8.0],
        },
    );
    let displace = {
        let mut scene = two_tones(
            [0.8, 0.2, 0.5, 1.0],
            [0.1, 0.45, 0.75, 1.0],
            InputSpace::LinearRec709,
        );
        scene.nodes.push(DrawNode::Generate {
            effect: PixelEffect::Generate {
                generator: model::GenerateKind::GradientLinear,
                color_a: model::Color::from_srgb8([0, 0, 0], None),
                color_b: model::Color::from_srgb8([255, 255, 255], None),
                point_a: [0.0, 0.0],
                point_b: [16.0, 0.0],
                cell_size: 2.0,
                line_width: 1.0,
            },
        });
        scene.nodes.push(DrawNode::EffectMap {
            source: 2,
            map: 3,
            effect: PixelEffect::Displace {
                channel_x: model::DisplaceChannel::Luminance,
                channel_y: model::DisplaceChannel::Luminance,
                displacement: [[3.0, 0.0], [0.0, -2.0]],
            },
        });
        scene.roots = vec![4];
        scene
    };
    let generate = DrawScene {
        nodes: vec![
            DrawNode::Generate {
                effect: PixelEffect::Generate {
                    generator: model::GenerateKind::Checkerboard,
                    color_a: model::Color::from_srgb8([230, 230, 230], None),
                    color_b: model::Color::from_srgb8([40, 40, 40], None),
                    point_a: [0.0, 0.0],
                    point_b: [16.0, 16.0],
                    cell_size: 4.0,
                    line_width: 1.0,
                },
            },
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::Invert {
                    channel: model::InvertChannel::Blue,
                },
            },
        ],
        roots: vec![1],
    };
    vec![
        ("fx008-grain", 16, WorkingSpace::LinearRec709, grain),
        ("fx008-mosaic", 16, WorkingSpace::LinearRec709, mosaic),
        (
            "fx008-channel-mixer",
            16,
            WorkingSpace::LinearRec2020,
            mixer,
        ),
        ("fx008-invert", 16, WorkingSpace::LinearRec2020, invert),
        ("fx008-tint", 16, WorkingSpace::LinearRec709, tint),
        (
            "fx008-directional-blur",
            16,
            WorkingSpace::LinearRec709,
            directional,
        ),
        ("fx008-radial-blur", 16, WorkingSpace::LinearRec2020, radial),
        ("fx008-displace", 16, WorkingSpace::LinearRec709, displace),
        ("fx008-generate", 16, WorkingSpace::LinearRec709, generate),
    ]
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
