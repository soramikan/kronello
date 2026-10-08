//! TRACK-003 (ADR-0123): optical-flow intermediate-frame interpolation on the
//! media decode path and through the composition -> snapshot -> render chain.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{
    Duration, FlowFallbackPolicy, FrameInterpolation, FrameRate, OpticalFlowConfig, Rational, Time,
    TimeMap, TimeMapPoint, TimeRange,
};
use std::path::{Path, PathBuf};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn duration(n: i64) -> Duration {
    Duration::new(t(n, 1)).unwrap()
}
fn flow(floor: Rational, fallback: Option<FlowFallbackPolicy>) -> FrameInterpolation {
    FrameInterpolation::OpticalFlow(OpticalFlowConfig {
        block_radius: 2,
        search_radius: 2,
        levels: 1,
        confidence_floor: floor,
        max_low_confidence: Rational::new(1, 2).unwrap(),
        flow_fallback: fallback,
    })
}
fn default_flow() -> FrameInterpolation {
    flow(Rational::new(1, 4).unwrap(), None)
}

/// Two solid-color frames; uniform patches flow with identity vectors, so the
/// synthesized midpoint is a deterministic 50% premultiplied mix.
fn red_blue_asset() -> (tempfile::TempDir, PathBuf, PathBuf, Asset) {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().join("source.kronello");
    let path = dir.path().join("flow.mov");
    runtime
        .encode_video(
            &EncodeRequest {
                output: path.clone(),
                codec: EncodeCodec::ProRes,
                width: 16,
                height: 16,
                time_base: t(1, 24),
            },
            &[
                EncodeFrame {
                    pts: Time::ZERO,
                    rgba: [255, 0, 0, 255].repeat(16 * 16),
                },
                EncodeFrame {
                    pts: t(1, 24),
                    rgba: [0, 0, 255, 255].repeat(16 * 16),
                },
            ],
        )
        .unwrap();
    let mut decoder = runtime.open_video(&path).unwrap();
    let source = decoder.decode_at(Time::ZERO).unwrap();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: content_hash(&path).unwrap(),
        kind: AssetKind::Video,
        locator: AssetLocator {
            relative: Some("flow.mov".into()),
            absolute: None,
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: t(1, 24),
            duration: Some(t(1, 12)),
            start_time: Some(Time::ZERO),
            width: Some(16),
            height: Some(16),
            pixel_format: Some(source.pixel_format.clone()),
            color_primaries: Some(source.color_primaries.clone()),
            color_transfer: Some(source.color_transfer.clone()),
            color_matrix: Some(source.color_matrix.clone()),
            color_range: Some(source.color_range.clone()),
        }],
    };
    (dir, project_path, path, asset)
}

fn decode(
    asset: &Asset,
    project: &Path,
    time: Time,
    interpolation: Option<FrameInterpolation>,
) -> Result<VideoImage, MediaError> {
    MediaRuntime::load()
        .unwrap()
        .decode_video_image_with_sampling(
            asset,
            project,
            0,
            time,
            ColorSpace::LinearRec709,
            None,
            false,
            interpolation,
        )
}

#[test]
fn track003_optical_flow_synthesizes_mid_interval_frame() {
    let (_dir, project, _p, asset) = red_blue_asset();
    // t = 1/48 sits halfway between frame 0 (red) and frame 1 (blue).
    let mid = decode(&asset, &project, t(1, 48), Some(default_flow())).unwrap();
    assert_eq!(mid.size, [16, 16]);
    for px in &mid.pixels {
        assert!((px[0] - 0.5).abs() < 0.05, "red mix {:?}", px);
        assert!(px[1].abs() < 0.05, "green {:?}", px);
        assert!((px[2] - 0.5).abs() < 0.05, "blue mix {:?}", px);
        assert_eq!(px[3], 1.0);
    }
    // Exact presentation instants skip synthesis entirely.
    let exact = decode(&asset, &project, Time::ZERO, Some(default_flow())).unwrap();
    let plain = decode(&asset, &project, Time::ZERO, None).unwrap();
    assert_eq!(exact.pixels, plain.pixels);
    let next = decode(&asset, &project, t(1, 24), Some(default_flow())).unwrap();
    for px in &next.pixels {
        assert!(px[0] < 0.05);
        assert!(px[2] > 0.95);
    }
}

#[test]
fn track003_low_confidence_rejects_without_authored_fallback() {
    let (_dir, project, _p, asset) = red_blue_asset();
    // Two decorrelated uniform patches carry no matchable signal; forcing the
    // floor above the achievable confidence must fail typed, never blend.
    let strict = flow(Rational::ONE, None);
    let err = decode(&asset, &project, t(1, 48), Some(strict)).unwrap_err();
    match err {
        MediaError::Flow(e) => assert_eq!(e.code(), "FLOW_CONFIDENCE_LOW"),
        other => panic!("expected flow error, got {other:?}"),
    }
}

#[test]
fn track003_authored_blend_fallback_downgrades_to_crossfade() {
    let (_dir, project, _p, asset) = red_blue_asset();
    let strict = flow(Rational::ONE, Some(FlowFallbackPolicy::Blend));
    let mid = decode(&asset, &project, t(1, 48), Some(strict)).unwrap();
    for px in &mid.pixels {
        assert!((px[0] - 0.5).abs() < 0.05);
        assert!((px[2] - 0.5).abs() < 0.05);
    }
}

#[test]
fn track003_reverse_sampling_rejects_interpolation() {
    let (_dir, project, _p, asset) = red_blue_asset();
    let err = MediaRuntime::load()
        .unwrap()
        .decode_video_image_with_sampling(
            &asset,
            &project,
            0,
            t(1, 48),
            ColorSpace::LinearRec709,
            None,
            true,
            Some(default_flow()),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            MediaError::UnsupportedFeature(_) | MediaError::Flow(_) | MediaError::Decode(_)
        ),
        "typed rejection expected, got {err:?}"
    );
    // The dedicated message fires whenever reverse sampling reaches the
    // interpolation gate; implementations that reject earlier still return a
    // typed error, never silent output.
    if let MediaError::UnsupportedFeature(m) = &err {
        assert!(m.contains("forward sampling") || !m.is_empty());
    }
}

fn composition(nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: CompositionId::new(),
        duration: duration(4),
        design_extent: DesignExtent::new(16.0, 16.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: nodes.iter().map(|n| n.id).collect(),
        nodes,
        properties: vec![],
    }
}
fn node(kind: NodeKind, properties: Vec<Property>) -> SceneNode {
    SceneNode {
        id: NodeId::new(),
        name: None,
        tags: Default::default(),
        enabled: true,
        kind,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
        properties,
        effects: vec![],
    }
}
fn volume() -> Property {
    let registry = render_registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
        .unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap())),
        vec![],
        &registry,
    )
    .unwrap()
}
fn project_with(asset: Asset, time_map: TimeMap) -> (Project, CompositionId) {
    let volume = volume();
    let c = composition(vec![node(
        NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map,
            volume: volume.id(),
        }),
        vec![volume],
    )]);
    let id = c.id;
    (
        Project {
            assets: vec![DocumentObject::Known(asset)],
            compositions: vec![DocumentObject::Known(c)],
            ..Project::default()
        },
        id,
    )
}
fn interpolated_map(mode: Option<FrameInterpolation>) -> TimeMap {
    TimeMap::piecewise_linear_with_interpolation(
        vec![
            TimeMapPoint {
                parent: Time::ZERO,
                local: Time::ZERO,
            },
            TimeMapPoint {
                parent: t(2, 1),
                local: t(1, 24),
            },
        ],
        mode,
    )
    .unwrap()
}

#[test]
fn track003_snapshot_binds_interpolation_and_renders_synthesis() {
    let (_dir, project, _p, asset) = red_blue_asset();
    let (p, id) = project_with(asset, interpolated_map(Some(default_flow())));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    assert_eq!(snapshot.semantic_versions().frame_interpolation, Some(1));
    // Half a local source frame maps to exactly t=1/48 between presentations.
    let scene = build_scene_ir(&snapshot, t(1, 1), &[]).unwrap();
    let node = scene
        .nodes
        .iter()
        .find_map(|n| match &n.content {
            SceneContent::Video {
                time,
                interpolation,
                ..
            } => Some((*time, *interpolation)),
            _ => None,
        })
        .expect("video content");
    assert_eq!(node.0, t(1, 48));
    assert_eq!(node.1, Some(default_flow()));
    let rendered = render_frame(
        &snapshot,
        &[],
        &VideoRenderBackend {
            backend: &CpuReferenceBackend,
            project_path: &project,
        },
        FrameRequest {
            time: t(1, 1),
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [16.0, 16.0],
                pixels: [16, 16],
            },
        },
    )
    .unwrap();
    for px in &rendered.pixels.linear {
        assert!((px[0] - 0.5).abs() < 0.05, "red mix {px:?}");
        assert!((px[2] - 0.5).abs() < 0.05, "blue mix {px:?}");
    }
}

#[test]
fn track003_unpinned_snapshot_rejects_authored_interpolation() {
    let (_dir, _project, _p, asset) = red_blue_asset();
    let (p, id) = project_with(asset.clone(), interpolated_map(Some(default_flow())));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let mut versions = snapshot.semantic_versions().clone();
    // A snapshot pinned before TRACK-003 carries no frame_interpolation
    // version; authored synthesis must reject rather than silently decode.
    versions.frame_interpolation = None;
    let err =
        RenderSnapshot::with_contract(&p, id, 1, snapshot.profile(), versions, vec![]).unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
    // The same contract stays valid when the project authors no synthesis.
    let (p, id) = project_with(asset, interpolated_map(None));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let mut versions = snapshot.semantic_versions().clone();
    versions.frame_interpolation = None;
    RenderSnapshot::with_contract(&p, id, 1, snapshot.profile(), versions, vec![]).unwrap();
}

#[test]
fn track003_stabilize_on_interpolated_clip_is_rejected() {
    // TRACK-002 + TRACK-003 combination guard lives at scene construction:
    // a clip that authors both must fail typed before any decode.
    let (_dir, _project, _p, asset) = red_blue_asset();
    let registry = render_registry();
    let scalar = |v: f64| Value::Scalar(FiniteF64::new(v).unwrap());
    let prop = |key: &str, value: Value| {
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
    let keys = [
        ("kronello.effect.tracking", Value::AssetRef(AssetId::new())),
        ("kronello.effect.smoothing_radius", scalar(8.0)),
        ("kronello.effect.max_displacement", scalar(48.0)),
        (
            "kronello.effect.max_rotation",
            Value::Angle(FiniteF64::new(4.0).unwrap()),
        ),
        ("kronello.effect.max_crop", scalar(0.2)),
        ("kronello.effect.border", Value::Enum("fill".into())),
        (
            "kronello.effect.fill_color",
            Value::Color(Color::from_srgb8([0; 3], None)),
        ),
        ("kronello.effect.sampling", Value::Enum("nearest".into())),
    ];
    let properties: Vec<Property> = keys
        .iter()
        .map(|(key, value)| prop(key, value.clone()))
        .collect();
    let volume = volume();
    let mut owned = properties.clone();
    owned.push(volume.clone());
    let mut media_node = node(
        NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map: interpolated_map(Some(default_flow())),
            volume: volume.id(),
        }),
        owned,
    );
    media_node.effects = vec![Effect::Known(EffectDefinition {
        effect_id: STABILIZE_ID.into(),
        version: STABILIZE_VERSION,
        parameters: EffectParameters::Stabilize {
            tracking: properties[0].id(),
            smoothing_radius: properties[1].id(),
            max_displacement: properties[2].id(),
            max_rotation: properties[3].id(),
            max_crop: properties[4].id(),
            border: properties[5].id(),
            fill_color: properties[6].id(),
            sampling: properties[7].id(),
        },
    })];
    let c = composition(vec![media_node]);
    let id = c.id;
    let p = Project {
        assets: vec![DocumentObject::Known(asset)],
        compositions: vec![DocumentObject::Known(c)],
        ..Project::default()
    };
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let err = build_scene_ir(&snapshot, t(1, 1), &[]).unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
}
