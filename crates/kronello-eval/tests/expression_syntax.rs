//! EXPR-002 / ADR-0105: the human-readable surface text parses into the
//! canonical postorder AST, formats back losslessly, and evaluates identically
//! to an equivalent AST constructed directly. Metadata is envelope-owned.
use kronello_eval::*;
use kronello_model::*;
use kronello_time::Time;
use std::collections::BTreeMap;

fn scalar(x: f64) -> Value {
    Value::Scalar(FiniteF64::new(x).unwrap())
}
fn lit(x: f64) -> ExpressionNode {
    ExpressionNode::Literal(scalar(x))
}
fn metadata(id: ExpressionId, value_type: ValueType) -> ExpressionMetadata {
    ExpressionMetadata {
        id,
        version: EXPRESSION_SUPPORTED_VERSION,
        value_type,
        budget: ExpressionBudget::default(),
    }
}
fn parse(text: &str, value_type: ValueType) -> Expression {
    parse_expression(text, &metadata(ExpressionId::new(), value_type)).unwrap()
}
/// Parse must equal the hand-built AST, and format must reproduce the AST and
/// be a stable fixed point: format(parse(format(ast))) == format(ast).
fn roundtrip(text: &str, value_type: ValueType, nodes: Vec<ExpressionNode>) -> Expression {
    let parsed = parse(text, value_type);
    let expected = Expression {
        id: parsed.id,
        version: EXPRESSION_SUPPORTED_VERSION,
        value_type,
        budget: ExpressionBudget::default(),
        nodes,
    };
    expected.validate().unwrap();
    assert_eq!(parsed, expected, "parse mismatch for {text}");
    let formatted = format_expression(&expected).unwrap();
    let reparsed = parse_expression(&formatted, &ExpressionMetadata::from(&expected)).unwrap();
    assert_eq!(reparsed, expected, "roundtrip mismatch for {formatted}");
    assert_eq!(format_expression(&reparsed).unwrap(), formatted);
    expected
}
fn fixture(e: &Expression) -> (Composition, RuntimePropertyKey) {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(mut c) = p.compositions[0].clone() else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
    c.nodes[0].properties[0]
        .set_source(
            PropertySource::Expression(e.id),
            &SchemaRegistry::with_builtin(),
        )
        .unwrap();
    let key = PropertyKey {
        instance_path: InstancePath::root(),
        node: c.nodes[0].id,
        property: c.nodes[0].properties[0].id(),
    }
    .into();
    (c, key)
}
fn graph<'a>(
    c: &'a [Composition],
    e: &'a [Expression],
    curves: &'a [AnimationCurve],
    audio: &'a [AudioAnalysisDataAsset],
    data: &'a [ExpressionDataAsset],
    registry: &'a SchemaRegistry,
) -> DependencyGraph<'a> {
    static EMPTY: BTreeMap<RuntimePropertyKey, RuntimePropertyKey> = BTreeMap::new();
    static DEPS: BTreeMap<RuntimePropertyKey, Vec<RuntimePropertyKey>> = BTreeMap::new();
    DependencyGraph::compile_with_data(
        EvaluationSnapshot {
            compositions: c,
            curves,
            expressions: e,
            registry,
            reference_bindings: &EMPTY,
            dependencies: &DEPS,
            working_space: ColorSpace::LinearRec709,
        },
        c[0].id,
        audio,
        data,
    )
    .unwrap()
}
/// Evaluate both the text-parsed and the hand-built expression at `times`
/// through a typed consumer property and require identical values.
fn assert_evaluates_equal(
    text: &str,
    nodes: Vec<ExpressionNode>,
    default: Value,
    times: &[Time],
    curves: &[AnimationCurve],
    audio: &[AudioAnalysisDataAsset],
    data: &[ExpressionDataAsset],
) -> Vec<Value> {
    let parsed = parse(text, default.value_type());
    let manual = Expression {
        id: parsed.id,
        version: EXPRESSION_SUPPORTED_VERSION,
        value_type: default.value_type(),
        budget: ExpressionBudget::default(),
        nodes,
    };
    manual.validate().unwrap();
    assert_eq!(parsed, manual, "{text}");
    let mut registry = SchemaRegistry::with_builtin();
    let unit = if parsed.value_type == ValueType::Angle {
        Unit::Degrees
    } else {
        Unit::Dimensionless
    };
    let descriptor = PropertyDescriptor::new(DescriptorDefinition::new(
        DescriptorId::new(),
        SchemaKey::new("test.expression_result").unwrap(),
        "Result",
        parsed.value_type,
        unit,
        default,
    ))
    .unwrap();
    registry.register(descriptor.clone()).unwrap();
    let run = |e: &Expression| {
        let (mut c, _) = fixture(e);
        c.nodes[0].properties = vec![
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(&descriptor),
                PropertySource::Expression(e.id),
                vec![],
                &registry,
            )
            .unwrap(),
        ];
        let key: RuntimePropertyKey = PropertyKey {
            instance_path: InstancePath::root(),
            node: c.nodes[0].id,
            property: c.nodes[0].properties[0].id(),
        }
        .into();
        let cs = [c];
        let es = [e.clone()];
        let g = graph(&cs, &es, curves, audio, data, &registry);
        times
            .iter()
            .map(|t| g.evaluate_property(&key, *t).unwrap())
            .collect::<Vec<_>>()
    };
    let direct = run(&manual);
    let from_text = run(&parsed);
    assert_eq!(direct, from_text, "evaluation mismatch for {text}");
    direct
}

#[test]
fn scalar_literals_and_arithmetic_match_the_canonical_ast() {
    for (text, nodes) in [
        ("1", vec![lit(1.0)]),
        (
            "  1\n+\t2  ",
            vec![
                lit(1.0),
                lit(2.0),
                ExpressionNode::Add { left: 0, right: 1 },
            ],
        ),
        (
            "1 + 2 * 3",
            vec![
                lit(1.0),
                lit(2.0),
                lit(3.0),
                ExpressionNode::Multiply { left: 1, right: 2 },
                ExpressionNode::Add { left: 0, right: 3 },
            ],
        ),
        (
            "(1 + 2) * 3",
            vec![
                lit(1.0),
                lit(2.0),
                ExpressionNode::Add { left: 0, right: 1 },
                lit(3.0),
                ExpressionNode::Multiply { left: 2, right: 3 },
            ],
        ),
        (
            // left associativity: (10 - 2) - 3, never 10 - (2 - 3)
            "10 - 2 - 3",
            vec![
                lit(10.0),
                lit(2.0),
                ExpressionNode::Subtract { left: 0, right: 1 },
                lit(3.0),
                ExpressionNode::Subtract { left: 2, right: 3 },
            ],
        ),
        (
            "1.5e2 / -0.5",
            vec![
                lit(150.0),
                lit(-0.5),
                ExpressionNode::Divide { left: 0, right: 1 },
            ],
        ),
        (
            "10 + sin(time()) * 5",
            vec![
                lit(10.0),
                ExpressionNode::Time,
                ExpressionNode::Sin { input: 1 },
                lit(5.0),
                ExpressionNode::Multiply { left: 2, right: 3 },
                ExpressionNode::Add { left: 0, right: 4 },
            ],
        ),
        (
            "clamp(time(), 0, 1)",
            vec![
                ExpressionNode::Time,
                lit(0.0),
                lit(1.0),
                ExpressionNode::Clamp {
                    value: 0,
                    min: 1,
                    max: 2,
                },
            ],
        ),
        (
            "lerp(0, 10, 0.25)",
            vec![
                lit(0.0),
                lit(10.0),
                lit(0.25),
                ExpressionNode::Lerp {
                    from: 0,
                    to: 1,
                    amount: 2,
                },
            ],
        ),
        (
            "noise(1, 2, time())",
            vec![
                ExpressionNode::Time,
                ExpressionNode::Noise {
                    seed: 1,
                    element: 2,
                    input: 0,
                },
            ],
        ),
        (
            "continuous_noise(19, 7, time())",
            vec![
                ExpressionNode::Time,
                ExpressionNode::ContinuousNoise {
                    seed: 19,
                    element: 7,
                    input: 0,
                },
            ],
        ),
        (
            "literal(\"{\\\"kind\\\":\\\"scalar\\\",\\\"value\\\":1.5}\")",
            vec![lit(1.5)],
        ),
    ] {
        roundtrip(text, ValueType::Scalar, nodes);
    }
    for (text, value_type, nodes) in [
        (
            "vec2(1, 2)",
            ValueType::Vec2,
            vec![lit(1.0), lit(2.0), ExpressionNode::Vec2 { x: 0, y: 1 }],
        ),
        (
            "vec3(1, 2, 3)",
            ValueType::Vec3,
            vec![
                lit(1.0),
                lit(2.0),
                lit(3.0),
                ExpressionNode::Vec3 { x: 0, y: 1, z: 2 },
            ],
        ),
        (
            "angle(720)",
            ValueType::Angle,
            vec![lit(720.0), ExpressionNode::Angle { degrees: 0 }],
        ),
        (
            "true",
            ValueType::Bool,
            vec![ExpressionNode::Literal(Value::Bool(true))],
        ),
        (
            "\"text\"",
            ValueType::String,
            vec![ExpressionNode::Literal(Value::String("text".into()))],
        ),
        (
            // A constant Value::Vec2 stays literal(...) and never becomes a
            // vec2(...) construction node.
            "literal(\"{\\\"kind\\\":\\\"vec2\\\",\\\"value\\\":[1.0,2.0]}\")",
            ValueType::Vec2,
            vec![ExpressionNode::Literal(Value::Vec2([
                FiniteF64::new(1.0).unwrap(),
                FiniteF64::new(2.0).unwrap(),
            ]))],
        ),
    ] {
        roundtrip(text, value_type, nodes);
    }
    // Signed literals keep their sign, including -0.
    let negative_zero = parse("-0", ValueType::Scalar);
    let ExpressionNode::Literal(Value::Scalar(v)) = negative_zero.nodes[0] else {
        panic!()
    };
    assert!(v.get() == 0.0 && v.get().is_sign_negative());
    assert_eq!(format_expression(&negative_zero).unwrap(), "-0");
    let negative = parse("0 - -0", ValueType::Scalar);
    let ExpressionNode::Literal(Value::Scalar(v)) = negative.nodes[1] else {
        panic!()
    };
    assert!(v.get().is_sign_negative());
}

#[test]
fn reference_constructs_and_fixed_arguments_match_the_canonical_ast() {
    let node = NodeId::new();
    let property = PropertyId::new();
    let curve = CurveId::new();
    let asset = AssetId::new();
    for (text, nodes) in [
        (
            format!(
                "property(\"{}\", \"{}\", \"scalar\")",
                node.as_uuid(),
                property.as_uuid()
            ),
            vec![ExpressionNode::Property {
                node: Some(node),
                property,
                value_type: ValueType::Scalar,
            }],
        ),
        (
            format!("property(null, \"{}\", \"scalar\")", property.as_uuid()),
            vec![ExpressionNode::Property {
                node: None,
                property,
                value_type: ValueType::Scalar,
            }],
        ),
        (
            format!(
                "property_sample(\"{}\", \"{}\", \"scalar\", time() * 0.5)",
                node.as_uuid(),
                property.as_uuid()
            ),
            vec![
                ExpressionNode::Time,
                lit(0.5),
                ExpressionNode::Multiply { left: 0, right: 1 },
                ExpressionNode::PropertySample {
                    node: Some(node),
                    property,
                    value_type: ValueType::Scalar,
                    lookback: 2,
                },
            ],
        ),
        (
            format!(
                "curve(\"{}\", time_offset(\"1\", \"24\"), \"scalar\")",
                curve.as_uuid()
            ),
            vec![ExpressionNode::CurveSample {
                curve,
                offset: Time::new(1, 24).unwrap(),
                value_type: ValueType::Scalar,
            }],
        ),
        (
            // Rational offsets normalize through the shared Time type.
            format!(
                "curve(\"{}\", time_offset(\"2\", \"48\"), \"scalar\")",
                curve.as_uuid()
            ),
            vec![ExpressionNode::CurveSample {
                curve,
                offset: Time::new(1, 24).unwrap(),
                value_type: ValueType::Scalar,
            }],
        ),
        (
            format!(
                "data_cell(\"{}\", \"offset\", 1 + 1, \"scalar\")",
                asset.as_uuid()
            ),
            vec![
                lit(1.0),
                lit(1.0),
                ExpressionNode::Add { left: 0, right: 1 },
                ExpressionNode::DataAssetCell {
                    asset,
                    column: "offset".into(),
                    row: 2,
                    value_type: ValueType::Scalar,
                },
            ],
        ),
        (
            format!(
                "audio_feature(\"{}\", \"rms\", time_offset(\"0\", \"1\"))",
                asset.as_uuid()
            ),
            vec![ExpressionNode::AudioFeature {
                asset,
                feature: AudioFeature::Rms,
                offset: Time::ZERO,
            }],
        ),
        (
            format!(
                "audio_feature(\"{}\", \"onset\", time_offset(\"-1\", \"2\"))",
                asset.as_uuid()
            ),
            vec![ExpressionNode::AudioFeature {
                asset,
                feature: AudioFeature::Onset,
                offset: Time::new(-1, 2).unwrap(),
            }],
        ),
        (
            format!(
                "audio_band(\"{}\", 3, time_offset(\"1\", \"24\"))",
                asset.as_uuid()
            ),
            vec![ExpressionNode::AudioFeature {
                asset,
                feature: AudioFeature::BandEnergy { band: 3 },
                offset: Time::new(1, 24).unwrap(),
            }],
        ),
    ] {
        roundtrip(&text, ValueType::Scalar, nodes);
    }
}

#[test]
fn parsed_text_evaluates_identically_to_hand_built_ast() {
    let times = [
        Time::ZERO,
        Time::new(1, 4).unwrap(),
        Time::new(3, 2).unwrap(),
    ];
    for (text, nodes, expected_at_zero) in [
        (
            "10 + sin(time()) * 5",
            vec![
                lit(10.0),
                ExpressionNode::Time,
                ExpressionNode::Sin { input: 1 },
                lit(5.0),
                ExpressionNode::Multiply { left: 2, right: 3 },
                ExpressionNode::Add { left: 0, right: 4 },
            ],
            scalar(10.0),
        ),
        (
            "clamp(time(), 0, 1) * 4",
            vec![
                ExpressionNode::Time,
                lit(0.0),
                lit(1.0),
                ExpressionNode::Clamp {
                    value: 0,
                    min: 1,
                    max: 2,
                },
                lit(4.0),
                ExpressionNode::Multiply { left: 3, right: 4 },
            ],
            scalar(0.0),
        ),
        (
            "vec2(time(), -0)",
            vec![
                ExpressionNode::Time,
                lit(-0.0),
                ExpressionNode::Vec2 { x: 0, y: 1 },
            ],
            Value::Vec2([FiniteF64::new(0.0).unwrap(), FiniteF64::new(-0.0).unwrap()]),
        ),
        (
            "continuous_noise(19, 7, time()) * 0.5 + 0.5",
            vec![
                ExpressionNode::Time,
                ExpressionNode::ContinuousNoise {
                    seed: 19,
                    element: 7,
                    input: 0,
                },
                lit(0.5),
                ExpressionNode::Multiply { left: 1, right: 2 },
                lit(0.5),
                ExpressionNode::Add { left: 3, right: 4 },
            ],
            // Evaluated on the real evaluator, not hand-computed.
            scalar(0.5),
        ),
    ] {
        let values =
            assert_evaluates_equal(text, nodes, expected_at_zero.clone(), &times, &[], &[], &[]);
        // The first sample is a deterministic cross-check at t = 0.
        if expected_at_zero.value_type() == ValueType::Vec2 {
            assert_eq!(values[0], expected_at_zero, "{text}");
        } else if text.starts_with("continuous_noise") {
            assert!(matches!(values[0], Value::Scalar(v) if v.get() >= 0.0 && v.get() <= 1.0));
        } else {
            assert_eq!(values[0], expected_at_zero, "{text}");
        }
    }
    // sin uses the shared evaluator's semantics: 10 + 5 * sin(1/4).
    let values = assert_evaluates_equal(
        "10 + sin(time()) * 5",
        vec![
            lit(10.0),
            ExpressionNode::Time,
            ExpressionNode::Sin { input: 1 },
            lit(5.0),
            ExpressionNode::Multiply { left: 2, right: 3 },
            ExpressionNode::Add { left: 0, right: 4 },
        ],
        scalar(0.0),
        &[Time::new(1, 4).unwrap()],
        &[],
        &[],
        &[],
    );
    assert_eq!(values, vec![scalar(10.0 + 5.0 * 0.25_f64.sin())]);
}

#[test]
fn parsed_reference_expressions_evaluate_identically() {
    // Upstream node reads its opacity from `source = time() / 2` so both
    // property(...) and property_sample(..., lookback) have observable values.
    let source = parse("sin(time()) / 4 + 0.5", ValueType::Scalar);
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(mut c) = p.compositions[0].clone() else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
    let registry = SchemaRegistry::with_builtin();
    let mut upstream = c.nodes[0].clone();
    upstream.id = NodeId::new();
    let opacity = upstream
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap();
    upstream.properties = vec![
        Property::new(
            PropertyId::new(),
            opacity.descriptor().clone(),
            PropertySource::Expression(source.id),
            vec![],
            &registry,
        )
        .unwrap(),
    ];
    c.root_nodes.push(upstream.id);
    c.nodes.push(upstream);
    let upstream_node = c.nodes[1].id;
    let upstream_property = c.nodes[1].properties[0].id();
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::from_integer(1),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let table = ExpressionDataAsset::new(
        AssetId::new(),
        DataTable {
            columns: [("v".into(), ValueType::Scalar)].into(),
            rows: vec![
                [("v".into(), scalar(0.2))].into(),
                [("v".into(), scalar(0.7))].into(),
            ],
        },
    )
    .unwrap();
    let audio_id = AssetId::new();
    let audio = AudioAnalysisDataAsset {
        id: audio_id,
        source: AudioAnalysisSource::Asset {
            asset: AssetId::new(),
            stream_index: 0,
            content_hash: "a".repeat(64),
        },
        config: AudioAnalysisConfig {
            version: 1,
            sample_rate: 48000,
            window: 8192,
            hop: 8192,
            bands: vec![[0, 1000]],
            time_map: kronello_time::TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE)
                .unwrap(),
        },
        start_sample: 0,
        sample_count: 49152,
        // Six hops cover the evaluated times 0, 1/2 and 1 second.
        frames: [0.25, 0.3, 0.75, 0.4, 0.5, 0.6]
            .iter()
            .enumerate()
            .map(|(i, rms)| AudioAnalysisFrame {
                time: Time::new(i as i64 * 8192, 48000).unwrap(),
                rms: *rms,
                band_energy: vec![rms * 0.8],
                onset: rms / 2.0,
                beat: i % 2 == 0,
            })
            .collect(),
    };
    let times = [Time::ZERO, Time::new(1, 2).unwrap(), Time::from_integer(1)];
    for (text, nodes) in [
        (
            format!(
                "property(\"{}\", \"{}\", \"scalar\")",
                upstream_node.as_uuid(),
                upstream_property.as_uuid()
            ),
            vec![ExpressionNode::Property {
                node: Some(upstream_node),
                property: upstream_property,
                value_type: ValueType::Scalar,
            }],
        ),
        (
            // A Scalar lookback expression; 0.5s past at root time.
            format!(
                "property_sample(\"{}\", \"{}\", \"scalar\", 0.5)",
                upstream_node.as_uuid(),
                upstream_property.as_uuid()
            ),
            vec![
                lit(0.5),
                ExpressionNode::PropertySample {
                    node: Some(upstream_node),
                    property: upstream_property,
                    value_type: ValueType::Scalar,
                    lookback: 0,
                },
            ],
        ),
        (
            format!(
                "curve(\"{}\", time_offset(\"1\", \"4\"), \"scalar\")",
                curve.id().as_uuid()
            ),
            vec![ExpressionNode::CurveSample {
                curve: curve.id(),
                offset: Time::new(1, 4).unwrap(),
                value_type: ValueType::Scalar,
            }],
        ),
        (
            // The row operand is a Scalar subexpression, not a fixed argument.
            format!(
                "data_cell(\"{}\", \"v\", clamp(time() * 2, 0, 1), \"scalar\")",
                table.id.as_uuid()
            ),
            vec![
                ExpressionNode::Time,
                lit(2.0),
                ExpressionNode::Multiply { left: 0, right: 1 },
                lit(0.0),
                lit(1.0),
                ExpressionNode::Clamp {
                    value: 2,
                    min: 3,
                    max: 4,
                },
                ExpressionNode::DataAssetCell {
                    asset: table.id,
                    column: "v".into(),
                    row: 5,
                    value_type: ValueType::Scalar,
                },
            ],
        ),
        (
            format!(
                "audio_feature(\"{}\", \"rms\", time_offset(\"0\", \"1\"))",
                audio_id.as_uuid()
            ),
            vec![ExpressionNode::AudioFeature {
                asset: audio_id,
                feature: AudioFeature::Rms,
                offset: Time::ZERO,
            }],
        ),
        (
            format!(
                "audio_band(\"{}\", 0, time_offset(\"0\", \"1\"))",
                audio_id.as_uuid()
            ),
            vec![ExpressionNode::AudioFeature {
                asset: audio_id,
                feature: AudioFeature::BandEnergy { band: 0 },
                offset: Time::ZERO,
            }],
        ),
    ] {
        let parsed = parse(&text, ValueType::Scalar);
        let manual = Expression {
            id: parsed.id,
            version: EXPRESSION_SUPPORTED_VERSION,
            value_type: ValueType::Scalar,
            budget: ExpressionBudget::default(),
            nodes,
        };
        manual.validate().unwrap();
        assert_eq!(parsed, manual);
        let mut target = c.clone();
        target.nodes[0].properties[0]
            .set_source(PropertySource::Expression(parsed.id), &registry)
            .unwrap();
        let cs = [target];
        let key: RuntimePropertyKey = PropertyKey {
            instance_path: InstancePath::root(),
            node: cs[0].nodes[0].id,
            property: cs[0].nodes[0].properties[0].id(),
        }
        .into();
        let curves = std::slice::from_ref(&curve);
        let audio_assets = std::slice::from_ref(&audio);
        let data_assets = std::slice::from_ref(&table);
        let values = |e: &Expression| {
            let es = [source.clone(), e.clone()];
            let g = graph(&cs, &es, curves, audio_assets, data_assets, &registry);
            times
                .iter()
                .map(|t| g.evaluate_property(&key, *t).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&manual), values(&parsed), "{text}");
    }
}

#[test]
fn typed_diagnostics_cover_position_and_expected_tokens() {
    let diagnostic = |text: &str| -> ExpressionDiagnostic {
        let error =
            parse_expression(text, &metadata(ExpressionId::new(), ValueType::Scalar)).unwrap_err();
        let ExpressionTextError::Syntax(syntax) = error else {
            panic!("expected syntax error for {text}: {error:?}")
        };
        assert_eq!(syntax.code(), "EXPRESSION_SYNTAX");
        syntax.diagnostics[0].clone()
    };
    // Unknown function.
    let d = diagnostic("noise2(1)");
    assert!(d.message.contains("unknown function"), "{d:?}");
    assert_eq!((d.byte_start, d.byte_end), (0, 6));
    assert_eq!((d.line, d.column), (1, 1));
    // Wrong arity surfaces as a fixed-argument expectation.
    assert!(diagnostic("sin(1, 2)").message.contains("')'"));
    assert!(diagnostic("clamp(1, 2)").message.contains("','"));
    // Bad type name lists the public ValueType names.
    let d = diagnostic(&format!(
        "property(null, \"{}\", \"scalar2\")",
        PropertyId::new().as_uuid()
    ));
    assert!(d.message.contains("unknown value type 'scalar2'"), "{d:?}");
    assert!(d.expected.iter().any(|e| e == "scalar"));
    // Dynamic references are rejected: only UUID strings are static.
    let d = diagnostic(&format!(
        "property(\"{}\", \"transform.position\", \"scalar\")",
        NodeId::new().as_uuid()
    ));
    assert!(d.message.contains("UUID"), "{d:?}");
    // Bad rationals: non-decimal, zero denominator, overflow.
    assert!(
        diagnostic(
            "curve(\"00000000-0000-0000-0000-000000000000\", time_offset(\"1.5\", \"2\"), \"scalar\")"
        )
        .message
        .contains("decimal integer")
    );
    assert!(
        diagnostic(
            "curve(\"00000000-0000-0000-0000-000000000000\", time_offset(\"1\", \"0\"), \"scalar\")"
        )
        .message
        .contains("invalid time_offset rational")
    );
    assert!(
        diagnostic(
            "curve(\"00000000-0000-0000-0000-000000000000\", time_offset(\"9223372036854775808\", \"1\"), \"scalar\")"
        )
        .message
        .contains("i64 range")
    );
    // Fixed arguments reject sign, fraction and exponent notation.
    assert!(
        diagnostic("noise(-1, 0, time())")
            .message
            .contains("unsigned decimal integer")
    );
    assert!(
        diagnostic("noise(1.5, 0, time())")
            .expected
            .iter()
            .any(|e| e == "unsigned decimal integer")
    );
    assert!(
        diagnostic(
            "audio_band(\"00000000-0000-0000-0000-000000000000\", 1e3, time_offset(\"0\", \"1\"))"
        )
        .message
        .contains("unsigned decimal integer")
    );
    // time_offset is not a standalone expression.
    assert!(
        diagnostic("time_offset(\"1\", \"2\")")
            .message
            .contains("time_offset is only allowed")
    );
    // `null` is not a standalone literal.
    assert!(diagnostic("null").message.contains("null"));
    // Malformed inputs all produce typed diagnostics with positions.
    for bad in [
        "1 +",
        "(1",
        "sin(",
        "1 2",
        "@",
        "\"unterminated",
        "1..2",
        "0x1",
        "-true",
    ] {
        diagnostic(bad);
    }
    // Limits: bytes, tokens, depth.
    diagnostic(&"1".repeat(EXPRESSION_TEXT_MAX_BYTES + 1));
    diagnostic(&"(".repeat(EXPRESSION_TEXT_MAX_DEPTH + 1));
    let d = diagnostic(&("1 + ".repeat(4100) + "1"));
    assert!(d.message.contains("tokens"), "{d:?}");
    // AST validation failures keep the typed error family.
    let error = parse_expression(
        "vec2(1, 2)",
        &metadata(ExpressionId::new(), ValueType::Scalar),
    )
    .unwrap_err();
    assert!(matches!(error, ExpressionTextError::Invalid(_)));
    assert_eq!(error.code(), "EVALUATION_ERROR");
    // Unsupported ASTs format to typed errors, never a rewritten AST.
    let mut invalid = Expression {
        id: ExpressionId::new(),
        version: 99,
        value_type: ValueType::Scalar,
        budget: ExpressionBudget::default(),
        nodes: vec![lit(1.0)],
    };
    assert!(matches!(
        format_expression(&invalid),
        Err(ExpressionFormatError::Invalid(_))
    ));
    invalid.version = EXPRESSION_SUPPORTED_VERSION;
    invalid.nodes.clear();
    assert!(matches!(
        format_expression(&invalid),
        Err(ExpressionFormatError::Invalid(_))
    ));
    // A tree whose canonical text would exceed the depth limit is diagnosed;
    // the stored AST is returned unchanged.
    let mut deep = invalid;
    deep.nodes = vec![lit(1.0)];
    for _ in 0..EXPRESSION_TEXT_MAX_DEPTH {
        let right = deep.nodes.len() as u32 - 1;
        deep.nodes.push(lit(1.0));
        deep.nodes.push(ExpressionNode::Add {
            left: right,
            right: right + 1,
        });
    }
    deep.validate().unwrap();
    assert_eq!(
        format_expression(&deep),
        Err(ExpressionFormatError::OutputLimit("64 nesting depth"))
    );
}

#[test]
fn formatter_preserves_tree_shape_and_literal_kinds() {
    // The formatter never re-associates a - (b - c) into a - b - c.
    let e = parse("10 - (2 - 3)", ValueType::Scalar);
    assert_eq!(format_expression(&e).unwrap(), "10 - (2 - 3)");
    let e = parse("10 - 2 - 3", ValueType::Scalar);
    assert_eq!(format_expression(&e).unwrap(), "10 - 2 - 3");
    let e = parse("10 / (2 * 5)", ValueType::Scalar);
    assert_eq!(format_expression(&e).unwrap(), "10 / (2 * 5)");
    // literal(Value::Vec2) and vec2(...) stay distinct node kinds.
    let constant = parse(
        "literal(\"{\\\"kind\\\":\\\"vec2\\\",\\\"value\\\":[1.0,2.0]}\")",
        ValueType::Vec2,
    );
    let constructed = parse("vec2(1, 2)", ValueType::Vec2);
    assert!(matches!(
        constant.nodes[0],
        ExpressionNode::Literal(Value::Vec2(_))
    ));
    assert!(matches!(
        constructed.nodes.last().unwrap(),
        ExpressionNode::Vec2 { .. }
    ));
    assert!(
        format_expression(&constant)
            .unwrap()
            .starts_with("literal(")
    );
    assert!(
        format_expression(&constructed)
            .unwrap()
            .starts_with("vec2(")
    );
    // Whitespace is normalized; the AST is unchanged. Parsed ids are
    // envelope-owned, so only the node lists are compared.
    assert_eq!(
        parse("  1\n +\t 2 ", ValueType::Scalar).nodes,
        parse("1 + 2", ValueType::Scalar).nodes
    );
}
