//! FX-008 remaining standard effect model contracts (ADR-0137/0139).
use kronello_model::*;
use std::collections::BTreeMap;

fn registry() -> SchemaRegistry {
    let mut r = SchemaRegistry::with_builtin();
    for d in effect_descriptors() {
        r.register(d).unwrap();
    }
    r
}
fn prop(r: &SchemaRegistry, key: &str, value: Value) -> Property {
    let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(d),
        PropertySource::Constant(value),
        vec![],
        r,
    )
    .unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn angle(v: f64) -> Value {
    Value::Angle(FiniteF64::new(v).unwrap())
}
fn vec2(x: f64, y: f64) -> Value {
    Value::Vec2([FiniteF64::new(x).unwrap(), FiniteF64::new(y).unwrap()])
}
fn enumeration(v: &str) -> Value {
    Value::Enum(v.into())
}
fn color(r: u8, g: u8, b: u8) -> Value {
    Value::Color(Color::from_srgb8([r, g, b], None))
}
fn mixer(rows: [[f64; 4]; 4]) -> Value {
    let f = |v| FiniteF64::new(v).unwrap();
    let columns = BTreeMap::from([
        ("red".to_string(), ValueType::Scalar),
        ("green".to_string(), ValueType::Scalar),
        ("blue".to_string(), ValueType::Scalar),
        ("alpha".to_string(), ValueType::Scalar),
    ]);
    Value::DataTable(DataTable {
        columns,
        rows: rows
            .iter()
            .map(|r| {
                BTreeMap::from([
                    ("red".to_string(), Value::Scalar(f(r[0]))),
                    ("green".to_string(), Value::Scalar(f(r[1]))),
                    ("blue".to_string(), Value::Scalar(f(r[2]))),
                    ("alpha".to_string(), Value::Scalar(f(r[3]))),
                ])
            })
            .collect(),
    })
}
fn definition(
    r: &SchemaRegistry,
    effect_id: &str,
    parameters: &[(&str, Value)],
    build: impl Fn(&[PropertyId]) -> EffectParameters,
) -> (EffectDefinition, Vec<Property>, Vec<PropertyId>) {
    let properties = parameters
        .iter()
        .map(|(key, value)| prop(r, key, value.clone()))
        .collect::<Vec<_>>();
    let ids = properties.iter().map(|p| p.id()).collect::<Vec<_>>();
    (
        EffectDefinition {
            effect_id: effect_id.into(),
            version: 1,
            parameters: build(&ids),
        },
        properties,
        ids,
    )
}
fn resolve_values(ids: &[PropertyId], values: &[Value]) -> BTreeMap<PropertyId, Value> {
    ids.iter().copied().zip(values.iter().cloned()).collect()
}

#[test]
fn fx008_grain_mosaic_invert_resolve_with_bounds() {
    let r = registry();
    let (grain, props, ids) = definition(
        &r,
        GRAIN_ID,
        &[
            ("kronello.effect.grain_amount", scalar(0.25)),
            ("kronello.effect.grain_size", scalar(1.5)),
            ("kronello.effect.monochrome", Value::Bool(true)),
            ("kronello.effect.seed", scalar(0.0)),
        ],
        |ids| EffectParameters::Grain {
            amount: ids[0],
            size: ids[1],
            monochrome: ids[2],
            seed: ids[3],
        },
    );
    grain.validate(&props, &r).unwrap();
    assert_eq!(
        grain
            .resolve(&resolve_values(
                &ids,
                &[scalar(0.5), scalar(3.0), Value::Bool(false), scalar(42.0)]
            ))
            .unwrap(),
        ResolvedEffect::Grain {
            amount: 0.5,
            size: 3.0,
            monochrome: false,
            seed: 42,
        }
    );
    // amount is unit-interval; size strictly positive; seed a signed integer
    // bounded by the 31-bit lattice.
    for (index, bad) in [
        (0, -0.001),
        (0, 1.001),
        (1, 0.0),
        (1, -2.0),
        (3, GRAIN_MAX_SEED + 1.0),
        (3, 0.5),
    ] {
        let mut values = [scalar(0.25), scalar(1.5), Value::Bool(true), scalar(0.0)];
        values[index] = scalar(bad);
        assert!(matches!(
            grain.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    let (mosaic, props, ids) = definition(
        &r,
        MOSAIC_ID,
        &[
            ("kronello.effect.block_size", scalar(8.0)),
            ("kronello.effect.mosaic_basis", enumeration("center")),
        ],
        |ids| EffectParameters::Mosaic {
            block_size: ids[0],
            basis: ids[1],
        },
    );
    mosaic.validate(&props, &r).unwrap();
    assert_eq!(
        mosaic
            .resolve(&resolve_values(&ids, &[scalar(4.0), enumeration("edge")]))
            .unwrap(),
        ResolvedEffect::Mosaic {
            block_size: 4.0,
            basis: MosaicBasis::Edge,
        }
    );
    assert!(matches!(
        mosaic.resolve(&resolve_values(&ids, &[scalar(0.0), enumeration("edge")])),
        Err(EffectError::InvalidParameter(id)) if id == ids[0]
    ));
    assert!(matches!(
        mosaic.resolve(&resolve_values(
            &ids,
            &[scalar(4.0), enumeration("middle")]
        )),
        Err(EffectError::InvalidParameter(id)) if id == ids[1]
    ));
    let (invert, props, ids) = definition(
        &r,
        INVERT_ID,
        &[("kronello.effect.invert_channel", enumeration("rgb"))],
        |ids| EffectParameters::Invert { channel: ids[0] },
    );
    invert.validate(&props, &r).unwrap();
    for (authored, resolved) in [
        ("rgb", InvertChannel::Rgb),
        ("red", InvertChannel::Red),
        ("green", InvertChannel::Green),
        ("blue", InvertChannel::Blue),
        ("alpha", InvertChannel::Alpha),
    ] {
        assert_eq!(
            invert
                .resolve(&resolve_values(&ids, &[enumeration(authored)]))
                .unwrap(),
            ResolvedEffect::Invert { channel: resolved }
        );
    }
    assert!(matches!(
        invert.resolve(&resolve_values(&ids, &[enumeration("luma")])),
        Err(EffectError::InvalidParameter(id)) if id == ids[0]
    ));
}

#[test]
fn fx008_channel_mixer_validates_table_shape_and_budget() {
    let r = registry();
    let identity = mixer([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let (d, props, ids) = definition(
        &r,
        CHANNEL_MIXER_ID,
        &[("kronello.effect.matrix", identity.clone())],
        |ids| EffectParameters::ChannelMixer { matrix: ids[0] },
    );
    d.validate(&props, &r).unwrap();
    let resolved = d
        .resolve(&resolve_values(
            &ids,
            &[mixer([
                [0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 0.5],
            ])],
        ))
        .unwrap();
    assert_eq!(
        resolved,
        ResolvedEffect::ChannelMixer {
            matrix: [
                [0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 0.5],
            ]
        }
    );
    // A coefficient over the 1024 budget, a wrong row count, and a non-table
    // value are all typed parameter errors.
    let over_budget = mixer([
        [1_025.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let Value::DataTable(mut short_table) = identity.clone() else {
        unreachable!()
    };
    short_table.rows.pop();
    for bad in [over_budget, Value::DataTable(short_table), scalar(1.0)] {
        assert!(matches!(
            d.resolve(&resolve_values(&ids, &[bad])),
            Err(EffectError::InvalidParameter(id)) if id == ids[0]
        ));
    }
}

#[test]
fn fx008_tint_and_blurs_resolve() {
    let r = registry();
    let (tint, props, ids) = definition(
        &r,
        TINT_ID,
        &[
            ("kronello.effect.map_black", color(0, 0, 0)),
            ("kronello.effect.map_white", color(255, 255, 255)),
            ("kronello.effect.tint_amount", scalar(1.0)),
        ],
        |ids| EffectParameters::Tint {
            map_black: ids[0],
            map_white: ids[1],
            amount: ids[2],
        },
    );
    tint.validate(&props, &r).unwrap();
    assert_eq!(
        tint.resolve(&resolve_values(
            &ids,
            &[color(10, 20, 30), color(200, 220, 240), scalar(0.75)]
        ))
        .unwrap(),
        ResolvedEffect::Tint {
            map_black: Color::from_srgb8([10, 20, 30], None),
            map_white: Color::from_srgb8([200, 220, 240], None),
            amount: 0.75,
        }
    );
    assert!(matches!(
        tint.resolve(&resolve_values(
            &ids,
            &[color(0, 0, 0), color(255, 255, 255), scalar(1.5)]
        )),
        Err(EffectError::InvalidParameter(id)) if id == ids[2]
    ));
    let (directional, props, ids) = definition(
        &r,
        DIRECTIONAL_BLUR_ID,
        &[
            ("kronello.effect.angle", angle(45.0)),
            ("kronello.effect.length", scalar(16.0)),
        ],
        |ids| EffectParameters::DirectionalBlur {
            angle: ids[0],
            length: ids[1],
        },
    );
    directional.validate(&props, &r).unwrap();
    assert_eq!(
        directional
            .resolve(&resolve_values(&ids, &[angle(90.0), scalar(24.0)]))
            .unwrap(),
        ResolvedEffect::DirectionalBlur {
            angle_degrees: 90.0,
            length: 24.0,
        }
    );
    for (index, bad) in [(0, 1_000_001.0), (1, -1.0)] {
        let mut values = [angle(45.0), scalar(16.0)];
        values[index] = if index == 0 { angle(bad) } else { scalar(bad) };
        assert!(matches!(
            directional.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    let (radial, props, ids) = definition(
        &r,
        RADIAL_BLUR_ID,
        &[
            ("kronello.effect.radial_mode", enumeration("spin")),
            ("kronello.effect.radial_amount", scalar(10.0)),
            ("kronello.effect.radial_center", vec2(0.0, 0.0)),
        ],
        |ids| EffectParameters::RadialBlur {
            mode: ids[0],
            amount: ids[1],
            center: ids[2],
        },
    );
    radial.validate(&props, &r).unwrap();
    assert_eq!(
        radial
            .resolve(&resolve_values(
                &ids,
                &[enumeration("zoom"), scalar(0.5), vec2(32.0, 18.0)]
            ))
            .unwrap(),
        ResolvedEffect::RadialBlur {
            mode: RadialBlurMode::Zoom,
            amount: 0.5,
            center: [32.0, 18.0],
        }
    );
    // Zoom amount is unit-interval; spin amount is a signed ±1e6 extent.
    assert!(matches!(
        radial.resolve(&resolve_values(
            &ids,
            &[enumeration("zoom"), scalar(1.5), vec2(0.0, 0.0)]
        )),
        Err(EffectError::InvalidParameter(id)) if id == ids[1]
    ));
    assert!(
        radial
            .resolve(&resolve_values(
                &ids,
                &[enumeration("spin"), scalar(-360.0), vec2(0.0, 0.0)]
            ))
            .is_ok()
    );
}

#[test]
fn fx008_displace_and_generate_resolve() {
    let r = registry();
    let (displace, props, ids) = definition(
        &r,
        DISPLACE_ID,
        &[
            (
                "kronello.effect.displace_channel_x",
                enumeration("luminance"),
            ),
            (
                "kronello.effect.displace_channel_y",
                enumeration("luminance"),
            ),
            ("kronello.effect.displace_scale_x", scalar(16.0)),
            ("kronello.effect.displace_scale_y", scalar(16.0)),
        ],
        |ids| EffectParameters::Displace {
            channel_x: ids[0],
            channel_y: ids[1],
            scale_x: ids[2],
            scale_y: ids[3],
        },
    );
    displace.validate(&props, &r).unwrap();
    assert_eq!(
        displace
            .resolve(&resolve_values(
                &ids,
                &[
                    enumeration("red"),
                    enumeration("blue"),
                    scalar(24.0),
                    scalar(-8.0),
                ]
            ))
            .unwrap(),
        ResolvedEffect::Displace {
            channel_x: DisplaceChannel::Red,
            channel_y: DisplaceChannel::Blue,
            displacement: [[24.0, 0.0], [0.0, -8.0]],
        }
    );
    assert!(matches!(
        displace.resolve(&resolve_values(
            &ids,
            &[
                enumeration("luminance"),
                enumeration("luminance"),
                scalar(1_000_001.0),
                scalar(0.0),
            ]
        )),
        Err(EffectError::InvalidParameter(id)) if id == ids[2]
    ));
    let (generate, props, ids) = definition(
        &r,
        GENERATE_ID,
        &[
            (
                "kronello.effect.generate_kind",
                enumeration("gradient_linear"),
            ),
            ("kronello.effect.generate_color_a", color(255, 255, 255)),
            ("kronello.effect.generate_color_b", color(0, 0, 0)),
            ("kronello.effect.generate_point_a", vec2(0.0, 0.0)),
            ("kronello.effect.generate_point_b", vec2(64.0, 64.0)),
            ("kronello.effect.generate_cell_size", scalar(16.0)),
            ("kronello.effect.generate_line_width", scalar(1.0)),
        ],
        |ids| EffectParameters::Generate {
            generator: ids[0],
            color_a: ids[1],
            color_b: ids[2],
            point_a: ids[3],
            point_b: ids[4],
            cell_size: ids[5],
            line_width: ids[6],
        },
    );
    generate.validate(&props, &r).unwrap();
    let resolved = generate
        .resolve(&resolve_values(
            &ids,
            &[
                enumeration("checkerboard"),
                color(255, 0, 0),
                color(0, 0, 255),
                vec2(1.0, 2.0),
                vec2(3.0, 4.0),
                scalar(8.0),
                scalar(0.0),
            ],
        ))
        .unwrap();
    assert_eq!(
        resolved,
        ResolvedEffect::Generate {
            generator: GenerateKind::Checkerboard,
            color_a: Color::from_srgb8([255, 0, 0], None),
            color_b: Color::from_srgb8([0, 0, 255], None),
            point_a: [1.0, 2.0],
            point_b: [3.0, 4.0],
            cell_size: 8.0,
            line_width: 0.0,
        }
    );
    // cell_size must be positive; an unknown kind is a typed error.
    for (index, bad) in [(5, 0.0), (5, -4.0)] {
        let mut values = [
            enumeration("grid"),
            color(255, 255, 255),
            color(0, 0, 0),
            vec2(0.0, 0.0),
            vec2(64.0, 64.0),
            scalar(16.0),
            scalar(1.0),
        ];
        values[index] = scalar(bad);
        assert!(matches!(
            generate.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    let mut values = [
        enumeration("grid"),
        color(255, 255, 255),
        color(0, 0, 0),
        vec2(0.0, 0.0),
        vec2(64.0, 64.0),
        scalar(16.0),
        scalar(1.0),
    ];
    values[0] = enumeration("halftone");
    assert!(matches!(
        generate.resolve(&resolve_values(&ids, &values)),
        Err(EffectError::InvalidParameter(id)) if id == ids[0]
    ));
}

#[test]
fn fx008_audio_resolves_with_48khz_quantization() {
    let r = registry();
    let (delay, props, ids) = definition(
        &r,
        AUDIO_DELAY_ID,
        &[
            ("kronello.effect.delay_ms", scalar(250.0)),
            ("kronello.effect.feedback_db", scalar(-12.0)),
            ("kronello.effect.wet", scalar(0.3)),
            ("kronello.effect.dry", scalar(1.0)),
        ],
        |ids| EffectParameters::AudioDelay {
            delay_ms: ids[0],
            feedback_db: ids[1],
            wet: ids[2],
            dry: ids[3],
        },
    );
    delay.validate(&props, &r).unwrap();
    assert_eq!(
        delay
            .resolve_audio(&resolve_values(
                &ids,
                &[scalar(250.0), scalar(-12.0), scalar(0.3), scalar(1.0)]
            ))
            .unwrap(),
        ResolvedAudioEffect::Delay {
            delay_samples: 12_000,
            feedback_db: -12.0,
            wet: 0.3,
            dry: 1.0,
        }
    );
    // Milliseconds round deterministically to integer 48 kHz samples;
    // feedback is constrained to non-positive dB.
    assert_eq!(
        delay
            .resolve_audio(&resolve_values(
                &ids,
                &[scalar(10.49), scalar(-6.0), scalar(0.0), scalar(1.0)]
            ))
            .unwrap(),
        ResolvedAudioEffect::Delay {
            delay_samples: 504,
            feedback_db: -6.0,
            wet: 0.0,
            dry: 1.0,
        }
    );
    for (index, bad) in [(1, 6.0), (2, 1.5), (3, -0.5)] {
        let mut values = [scalar(250.0), scalar(-12.0), scalar(0.3), scalar(1.0)];
        values[index] = scalar(bad);
        assert!(matches!(
            delay.resolve_audio(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    let (reverb, props, ids) = definition(
        &r,
        AUDIO_REVERB_ID,
        &[
            ("kronello.effect.decay_ms", scalar(800.0)),
            ("kronello.effect.damping", scalar(0.3)),
            ("kronello.effect.wet", scalar(0.3)),
            ("kronello.effect.dry", scalar(1.0)),
        ],
        |ids| EffectParameters::AudioReverb {
            decay_ms: ids[0],
            damping: ids[1],
            wet: ids[2],
            dry: ids[3],
        },
    );
    reverb.validate(&props, &r).unwrap();
    assert_eq!(
        reverb
            .resolve_audio(&resolve_values(
                &ids,
                &[scalar(1_250.0), scalar(0.5), scalar(0.4), scalar(0.6)]
            ))
            .unwrap(),
        ResolvedAudioEffect::Reverb {
            decay_s: 1.25,
            damping: 0.5,
            wet: 0.4,
            dry: 0.6,
        }
    );
    assert!(matches!(
        reverb.resolve_audio(&resolve_values(
            &ids,
            &[scalar(800.0), scalar(1.5), scalar(0.3), scalar(1.0)]
        )),
        Err(EffectError::InvalidParameter(id)) if id == ids[1]
    ));
    let (pitch, props, ids) = definition(
        &r,
        AUDIO_PITCH_ID,
        &[("kronello.effect.semitones", scalar(0.0))],
        |ids| EffectParameters::AudioPitch { semitones: ids[0] },
    );
    pitch.validate(&props, &r).unwrap();
    assert_eq!(
        pitch
            .resolve_audio(&resolve_values(&ids, &[scalar(-12.0)]))
            .unwrap(),
        ResolvedAudioEffect::Pitch { semitones: -12.0 }
    );
    for bad in [
        AUDIO_PITCH_MAX_SEMITONES + 0.1,
        -AUDIO_PITCH_MAX_SEMITONES - 0.1,
    ] {
        assert!(matches!(
            pitch.resolve_audio(&resolve_values(&ids, &[scalar(bad)])),
            Err(EffectError::InvalidParameter(id)) if id == ids[0]
        ));
    }
    let (gate, props, ids) = definition(
        &r,
        AUDIO_GATE_ID,
        &[
            ("kronello.effect.threshold_db", scalar(-40.0)),
            ("kronello.effect.attack_ms", scalar(5.0)),
            ("kronello.effect.release_ms", scalar(50.0)),
            ("kronello.effect.hysteresis_db", scalar(6.0)),
        ],
        |ids| EffectParameters::AudioGate {
            threshold_db: ids[0],
            attack_ms: ids[1],
            release_ms: ids[2],
            hysteresis_db: ids[3],
        },
    );
    gate.validate(&props, &r).unwrap();
    assert_eq!(
        gate.resolve_audio(&resolve_values(
            &ids,
            &[scalar(-30.0), scalar(2.0), scalar(80.0), scalar(8.0)]
        ))
        .unwrap(),
        ResolvedAudioEffect::Gate {
            threshold_db: -30.0,
            attack_ms: 2.0,
            release_ms: 80.0,
            hysteresis_db: 8.0,
        }
    );
    for (index, bad) in [(0, 1.0), (3, AUDIO_GATE_MAX_HYSTERESIS_DB + 1.0), (3, -1.0)] {
        let mut values = [scalar(-40.0), scalar(5.0), scalar(50.0), scalar(6.0)];
        values[index] = scalar(bad);
        assert!(matches!(
            gate.resolve_audio(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
}

#[test]
fn fx008_effect_ids_are_versioned_described_and_supported() {
    let r = registry();
    for id in [
        GRAIN_ID,
        MOSAIC_ID,
        INVERT_ID,
        CHANNEL_MIXER_ID,
        TINT_ID,
        DIRECTIONAL_BLUR_ID,
        RADIAL_BLUR_ID,
        DISPLACE_ID,
        GENERATE_ID,
        AUDIO_DELAY_ID,
        AUDIO_REVERB_ID,
        AUDIO_PITCH_ID,
        AUDIO_GATE_ID,
    ] {
        assert!(id.starts_with("kronello."), "{id}");
    }
    // Every new parameter key resolves to a registered descriptor carrying
    // the documented default.
    for (key, default) in [
        ("kronello.effect.grain_amount", scalar(0.25)),
        ("kronello.effect.grain_size", scalar(1.5)),
        ("kronello.effect.monochrome", Value::Bool(true)),
        ("kronello.effect.seed", scalar(0.0)),
        ("kronello.effect.block_size", scalar(8.0)),
        ("kronello.effect.mosaic_basis", enumeration("center")),
        ("kronello.effect.invert_channel", enumeration("rgb")),
        ("kronello.effect.map_black", color(0, 0, 0)),
        ("kronello.effect.map_white", color(255, 255, 255)),
        ("kronello.effect.tint_amount", scalar(1.0)),
        ("kronello.effect.angle", angle(45.0)),
        ("kronello.effect.length", scalar(16.0)),
        ("kronello.effect.radial_mode", enumeration("spin")),
        ("kronello.effect.radial_amount", scalar(10.0)),
        ("kronello.effect.radial_center", vec2(0.0, 0.0)),
        (
            "kronello.effect.displace_channel_x",
            enumeration("luminance"),
        ),
        (
            "kronello.effect.displace_channel_y",
            enumeration("luminance"),
        ),
        ("kronello.effect.displace_scale_x", scalar(16.0)),
        ("kronello.effect.displace_scale_y", scalar(16.0)),
        (
            "kronello.effect.generate_kind",
            enumeration("gradient_linear"),
        ),
        ("kronello.effect.generate_color_a", color(255, 255, 255)),
        ("kronello.effect.generate_color_b", color(0, 0, 0)),
        ("kronello.effect.generate_point_a", vec2(0.0, 0.0)),
        ("kronello.effect.generate_point_b", vec2(64.0, 64.0)),
        ("kronello.effect.generate_cell_size", scalar(16.0)),
        ("kronello.effect.generate_line_width", scalar(1.0)),
        ("kronello.effect.delay_ms", scalar(250.0)),
        ("kronello.effect.feedback_db", scalar(-12.0)),
        ("kronello.effect.wet", scalar(0.3)),
        ("kronello.effect.dry", scalar(1.0)),
        ("kronello.effect.decay_ms", scalar(800.0)),
        ("kronello.effect.damping", scalar(0.3)),
        ("kronello.effect.semitones", scalar(0.0)),
        ("kronello.effect.hysteresis_db", scalar(6.0)),
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert_eq!(d.definition().default, default, "{key}");
    }
    // The 55xx descriptor band keeps every FX-008 key in one contiguous
    // range distinct from earlier effect descriptors.
    for (key, suffix) in [
        ("kronello.effect.grain_amount", 0x01_u64),
        ("kronello.effect.matrix", 0x08),
        ("kronello.effect.hysteresis_db", 0x23),
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        let uuid = d.id().as_uuid().as_u128();
        assert_eq!(uuid >> 64, 0xf000_0000_0010_5500_u128, "{key}");
        assert_eq!(uuid & 0xffff_ffff, u128::from(suffix), "{key}");
    }
    // Version 0 and 2 remain unsupported; version pins are semantic.
    let r2 = registry();
    let (d, properties, _) = definition(
        &r2,
        GRAIN_ID,
        &[
            ("kronello.effect.grain_amount", scalar(0.25)),
            ("kronello.effect.grain_size", scalar(1.5)),
            ("kronello.effect.monochrome", Value::Bool(true)),
            ("kronello.effect.seed", scalar(0.0)),
        ],
        |ids| EffectParameters::Grain {
            amount: ids[0],
            size: ids[1],
            monochrome: ids[2],
            seed: ids[3],
        },
    );
    for version in [0, 2] {
        let mut unsupported = d.clone();
        unsupported.version = version;
        assert!(matches!(
            unsupported.validate(&properties, &r2),
            Err(EffectError::UnsupportedFeature)
        ));
    }
    let mut foreign = d.clone();
    foreign.effect_id = "vendor.grain".into();
    assert!(matches!(
        foreign.validate(&properties, &r2),
        Err(EffectError::UnsupportedFeature)
    ));
    // The serde schema exposes the new kind tags.
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    let variants = serde_json::to_string(&schema["$defs"]["EffectParameters"]).unwrap();
    for kind in [
        "grain",
        "mosaic",
        "invert",
        "channel_mixer",
        "tint",
        "directional_blur",
        "radial_blur",
        "displace",
        "generate",
        "audio_delay",
        "audio_reverb",
        "audio_pitch",
        "audio_gate",
    ] {
        assert!(variants.contains(kind), "{kind} missing from schema");
    }
}

#[test]
fn fx008_audio_effects_reject_video_domain() {
    // Audio-only parameters still fail the generic video resolve with the
    // typed domain error after validating their values (ADR-0117 contract).
    let r = registry();
    let (d, _, ids) = definition(
        &r,
        AUDIO_GATE_ID,
        &[
            ("kronello.effect.threshold_db", scalar(-40.0)),
            ("kronello.effect.attack_ms", scalar(5.0)),
            ("kronello.effect.release_ms", scalar(50.0)),
            ("kronello.effect.hysteresis_db", scalar(6.0)),
        ],
        |ids| EffectParameters::AudioGate {
            threshold_db: ids[0],
            attack_ms: ids[1],
            release_ms: ids[2],
            hysteresis_db: ids[3],
        },
    );
    assert!(matches!(
        d.resolve(&resolve_values(
            &ids,
            &[scalar(-40.0), scalar(5.0), scalar(50.0), scalar(6.0)]
        )),
        Err(EffectError::UnsupportedFeature)
    ));
}
