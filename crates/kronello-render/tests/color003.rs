//! COLOR-003 end-to-end render coverage (ADR-0113): a `.cube` lattice supplied
//! through the snapshot `luts` input binds to the clip's `kronello.color.lut`
//! asset reference, applies tetrahedrally in straight working space, and fails
//! with typed errors when the lattice is absent, wrong-typed, or oversized.
use kronello_gpu::color::apply_color;
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
/// Deterministic stand-in for a verified content hash; the render contract
/// only requires a 64-hex document value matching the `luts` input key.
fn hash() -> String {
    "ab".repeat(32)
}
fn asset() -> Asset {
    Asset {
        id: AssetId::from_uuid(uuid::Uuid::new_v4()),
        content_hash: hash(),
        kind: AssetKind::Data,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("luts/test.cube".into()),
            absolute: None,
        },
    }
}
/// Identity `.cube` document of the given edge size, red-fastest row order.
fn identity_cube(size: u32) -> Vec<u8> {
    let mut text = format!("LUT_3D_SIZE {size}\n");
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                text.push_str(&format!(
                    "{} {} {}\n",
                    r as f32 / (size - 1) as f32,
                    g as f32 / (size - 1) as f32,
                    b as f32 / (size - 1) as f32
                ));
            }
        }
    }
    text.into_bytes()
}
/// A 2x2x2 cube that maps channel maxima to swapped endpoints, so sampled
/// output differs visibly from the identity at every non-corner input.
fn swapped_cube() -> CubeLut {
    let mut lut = CubeLut::parse(&identity_cube(2)).unwrap();
    // Write a deterministic non-identity lattice: output = (g, b, r) rotation.
    for b in 0..2usize {
        for g in 0..2usize {
            for r in 0..2usize {
                let i = ((b * 2 + g) * 2 + r) * 3;
                lut.data[i] = g as f32;
                lut.data[i + 1] = b as f32;
                lut.data[i + 2] = r as f32;
            }
        }
    }
    lut
}
/// Clip effect referencing `asset_id`; properties use the shared descriptors.
fn lut_effect(asset_id: AssetId, intensity: f64) -> (Effect, Vec<Property>) {
    let registry = render_registry();
    let property = |key: &str, value: Value| {
        let d = registry.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(d),
            PropertySource::Constant(value),
            vec![],
            &registry,
        )
        .unwrap()
    };
    let lut = property("kronello.effect.lut", Value::AssetRef(asset_id));
    let intensity = property("kronello.effect.intensity", scalar(intensity));
    let ids = [lut.id(), intensity.id()];
    (
        Effect::Known(EffectDefinition {
            effect_id: COLOR_LUT_ID.into(),
            version: 1,
            parameters: EffectParameters::ColorLut {
                lut: ids[0],
                intensity: ids[1],
            },
        }),
        vec![lut, intensity],
    )
}
/// 4x1 sequence whose single solid clip carries the given effects; the Data
/// asset is registered so `luts` binding can find it by content hash.
fn project(
    rgb: [f64; 3],
    alpha: f64,
    fx: Vec<(Effect, Vec<Property>)>,
    assets: Vec<Asset>,
) -> Project {
    let (effects, properties): (Vec<Effect>, Vec<Vec<Property>>) = fx.into_iter().unzip();
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, rgb, alpha).unwrap(),
        },
        timeline_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        effects,
        masks: vec![],
        markers: vec![],
        properties: properties.into_iter().flatten().collect(),
    };
    Project {
        sequences: vec![DocumentObject::Known(Sequence {
            id: SequenceId::new(),
            extent: DesignExtent::new(4.0, 1.0).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: SampleRate::HZ_48000,
            working_space: ColorSpace::LinearRec709,
            tracks: vec![Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![clip],
            }],
            transitions: vec![],
            markers: vec![],
            work_area: None,
            targets: None,
        })],
        assets: assets.into_iter().map(DocumentObject::Known).collect(),
        ..Project::default()
    }
}
fn snapshot(p: &Project, luts: &[(String, CubeLut)]) -> RenderSnapshot {
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: s.id },
        7,
        RenderProfile::default(),
    )
    .unwrap()
    .with_luts(luts.iter().cloned().collect())
}
fn render(p: &Project, luts: &[(String, CubeLut)]) -> Result<Vec<[f32; 4]>, RenderError> {
    render_frame(
        &snapshot(p, luts),
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(1, 2),
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [4.0, 1.0],
                pixels: [4, 1],
            },
        },
    )
    .map(|frame| frame.pixels.linear)
}
fn half(v: f32) -> f32 {
    half::f16::from_f32(v).to_f32()
}
fn premult(rgb: [f64; 3], alpha: f64) -> [f32; 4] {
    [
        half((rgb[0] * alpha) as f32),
        half((rgb[1] * alpha) as f32),
        half((rgb[2] * alpha) as f32),
        half(alpha as f32),
    ]
}

#[test]
fn color003_lut_applies_tetrahedral_in_authored_order_and_preserves_alpha() {
    let asset = asset();
    let lut = swapped_cube();
    let (fx, props) = lut_effect(asset.id, 1.0);
    let p = project(
        [0.25, 0.5, 0.75],
        0.5,
        vec![(fx, props)],
        vec![asset.clone()],
    );
    let input = premult([0.25, 0.5, 0.75], 0.5);
    let want = {
        let applied = apply_color(
            input,
            &PixelEffect::ColorLut {
                lut: lut.clone(),
                intensity: 1.0,
            },
        )
        .map(half);
        if applied[3] == 0.0 { [0.0; 4] } else { applied }
    };
    let rendered = render(&p, &[(asset.content_hash.clone(), lut)]).unwrap();
    for px in &rendered {
        assert_eq!(*px, want);
    }
    // (g,b,r) rotation moves red into blue; alpha is exactly preserved.
    assert!(
        (want[2] - half(0.25 * 0.5)).abs() < 1e-6
            && want[0] == half(0.5 * 0.5)
            && want[3] == half(0.5)
    );
}

#[test]
fn color003_intensity_blends_toward_identity() {
    let asset = asset();
    for (intensity, name) in [(0.0, "identity"), (1.0, "full")] {
        let (fx, props) = lut_effect(asset.id, intensity);
        let p = project(
            [0.25, 0.5, 0.75],
            1.0,
            vec![(fx, props)],
            vec![asset.clone()],
        );
        let rendered = render(&p, &[(asset.content_hash.clone(), swapped_cube())]).unwrap();
        let input = premult([0.25, 0.5, 0.75], 1.0);
        if name == "identity" {
            assert_eq!(rendered[0], input, "intensity 0 must be the identity");
        } else {
            assert_ne!(rendered[0], input, "intensity 1 applies the lattice");
        }
    }
}

#[test]
fn color003_missing_or_broken_lattice_is_a_typed_render_failure() {
    let asset = asset();
    let (fx, props) = lut_effect(asset.id, 1.0);
    // No luts input at all: the resolved reference cannot bind.
    let p = project([0.5; 3], 1.0, vec![(fx, props)], vec![asset.clone()]);
    let error = render(&p, &[]).unwrap_err();
    assert_eq!(error.code(), "LUT_INPUT_MISSING", "{error:?}");
    // A lattice stored under a different hash does not satisfy the asset.
    let error = render(&p, &[("cd".repeat(32), swapped_cube())]).unwrap_err();
    assert_eq!(error.code(), "LUT_INPUT_MISSING");
    // A structurally invalid lattice bound under the right hash fails the
    // render instead of silently passing pixels through.
    let mut broken = swapped_cube();
    broken.data.pop();
    let error = render(&p, &[(asset.content_hash.clone(), broken)]).unwrap_err();
    assert_eq!(error.code(), "INVALID_LUT", "{error:?}");
    // Oversized lattices (parser-accepted >33) are rejected for documents.
    let oversized = CubeLut::parse(&identity_cube(34)).unwrap();
    let error = render(&p, &[(asset.content_hash.clone(), oversized)]).unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE", "{error:?}");
    // A non-Data asset kind cannot back a LUT effect.
    let media = Asset {
        kind: AssetKind::Video,
        ..asset.clone()
    };
    let (fx, props) = lut_effect(media.id, 1.0);
    let p = project([0.5; 3], 1.0, vec![(fx, props)], vec![media]);
    let error = render(&p, &[(hash(), swapped_cube())]).unwrap_err();
    assert_eq!(error.code(), "INVALID_LUT", "{error:?}");
}

#[test]
fn color003_scene_ir_binds_referenced_luts_and_dag_holds_tetrahedral_node() {
    let asset = asset();
    let unused = "ef".repeat(32);
    let (fx, props) = lut_effect(asset.id, 0.5);
    let p = project([0.5; 3], 1.0, vec![(fx, props)], vec![asset.clone()]);
    let snap = snapshot(
        &p,
        &[
            (asset.content_hash.clone(), swapped_cube()),
            (unused, CubeLut::parse(&identity_cube(2)).unwrap()),
        ],
    );
    let scene = build_scene_ir(&snap, t(1, 2), &[]).unwrap();
    assert_eq!(
        scene.nodes[0].effects,
        vec![ResolvedEffect::ColorLut {
            lut: asset.id,
            intensity: 0.5
        }]
    );
    // Only the referenced asset is bound; unreferenced inputs stay unbound.
    assert_eq!(scene.luts.len(), 1);
    assert!(scene.luts.contains_key(&asset.id));
    let dag = build_render_dag(
        &scene,
        RenderProfile::default(),
        OutputRegion {
            origin: [0.0; 2],
            extent: [4.0, 1.0],
            pixels: [4, 1],
        },
    )
    .unwrap();
    assert!(dag.nodes().iter().any(|n| match n {
        DagNode::Effect {
            effect: PixelEffect::ColorLut { lut, intensity },
            ..
        } => lut.size == 2 && *intensity == 0.5,
        _ => false,
    }));
    // Snapshot identity stays hash-stable: lattice bytes never enter the
    // resolved effect value, only the supplied input map changes.
    let other = snapshot(&p, &[]);
    assert_eq!(
        snap.semantic_versions().effects.get(COLOR_LUT_ID),
        other.semantic_versions().effects.get(COLOR_LUT_ID)
    );
}
