//! TRACK-002 (ADR-0122): stabilization bound from locked tracking data
//! through scene construction into the render DAG.
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Rational, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn duration(n: i64) -> Duration {
    Duration::new(t(n, 1)).unwrap()
}
fn f(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}

fn video_asset() -> Asset {
    Asset {
        id: AssetId::new(),
        content_hash: "a".repeat(64),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: t(1, 24),
            duration: Some(t(4, 1)),
            start_time: Some(Time::ZERO),
            width: Some(16),
            height: Some(16),
            pixel_format: Some("yuv422p10le".into()),
            color_primaries: Some("bt709".into()),
            color_transfer: Some("bt709".into()),
            color_matrix: Some("bt709".into()),
            color_range: Some("tv".into()),
        }],
        locator: AssetLocator {
            relative: Some("clip.mov".into()),
            absolute: None,
        },
    }
}
/// A single seed whose observed position drifts linearly in +x.
fn tracking(asset: &Asset, frames: u32) -> TrackingDataAsset {
    let drift = 0.05;
    let mut data = TrackingDataAsset {
        id: AssetId::new(),
        version: TRACKING_VERSION,
        source: TrackingSource {
            asset: asset.id,
            stream_index: 0,
            content_hash: asset.content_hash.clone(),
        },
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: f(0.5),
            y: f(0.5),
            template_radius: 4,
            search_radius: 8,
        }],
        range: TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
        sample_rate: Rational::new(24, 1).unwrap(),
        frames: (0..frames)
            .map(|i| TrackedFrame {
                time: t(i as i64, 24),
                points: vec![TrackedPoint {
                    x: f(0.5 + drift * i as f64),
                    y: f(0.5),
                    confidence: f(0.9),
                }],
                homography: None,
            })
            .collect(),
        content_hash: String::new(),
    };
    data.content_hash = data.computed_hash().unwrap();
    data
}
fn stabilize_effect(r: &SchemaRegistry, tracking: AssetId) -> (Effect, Vec<Property>) {
    let scalar = |v: f64| Value::Scalar(f(v));
    let prop = |key: &str, value: Value| {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(d),
            PropertySource::Constant(value),
            vec![],
            r,
        )
        .unwrap()
    };
    let keys = [
        ("kronello.effect.tracking", Value::AssetRef(tracking)),
        ("kronello.effect.smoothing_radius", scalar(2.0)),
        ("kronello.effect.max_displacement", scalar(64.0)),
        ("kronello.effect.max_rotation", Value::Angle(f(5.0))),
        ("kronello.effect.max_crop", scalar(0.25)),
        ("kronello.effect.border", Value::Enum("replicate".into())),
        (
            "kronello.effect.fill_color",
            Value::Color(Color::from_srgb8([0; 3], Some(0))),
        ),
        ("kronello.effect.sampling", Value::Enum("bilinear".into())),
    ];
    let properties: Vec<Property> = keys
        .iter()
        .map(|(key, value)| prop(key, value.clone()))
        .collect();
    let effect = Effect::Known(EffectDefinition {
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
    });
    (effect, properties)
}
fn volume() -> Property {
    let registry = render_registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
        .unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(f(1.0))),
        vec![],
        &registry,
    )
    .unwrap()
}
fn media_project(asset: Asset, tracking: Option<TrackingDataAsset>) -> (Project, CompositionId) {
    let registry = render_registry();
    let tracking_id = tracking.as_ref().map(|d| d.id).unwrap_or_default();
    let (effect, mut properties) = stabilize_effect(&registry, tracking_id);
    properties.push(volume());
    let volume_id = properties.last().unwrap().id();
    let media_node = SceneNode {
        id: NodeId::new(),
        name: None,
        tags: Default::default(),
        enabled: true,
        kind: NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
            volume: volume_id,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
        properties,
        effects: vec![effect],
    };
    let c = Composition {
        id: CompositionId::new(),
        duration: duration(4),
        design_extent: DesignExtent::new(16.0, 16.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![media_node.id],
        nodes: vec![media_node],
        properties: vec![],
    };
    let id = c.id;
    (
        Project {
            assets: vec![DocumentObject::Known(asset)],
            compositions: vec![DocumentObject::Known(c)],
            tracking_data_assets: tracking.into_iter().map(DocumentObject::Known).collect(),
            ..Project::default()
        },
        id,
    )
}
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [16.0, 16.0],
        pixels: [16, 16],
    }
}

#[test]
fn track002_scene_binds_inverse_correction_into_dag() {
    let asset = video_asset();
    let data = tracking(&asset, 8);
    let params = kronello_tracking::StabilizeParams {
        smoothing_radius: 2,
        max_displacement: 64.0,
        max_rotation: 5.0,
        max_crop: 0.25,
    };
    // At t=0 the truncated smoothing window disagrees with the raw drift, so
    // the bound inverse is a real (non-identity) per-frame correction.
    let expected =
        kronello_tracking::correction_inverse(&data, Time::ZERO, &params, [16.0, 16.0]).unwrap();
    assert!(expected.iter().flatten().any(|v| v.abs() > 1e-9));
    let (p, id) = media_project(asset, Some(data));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|n| matches!(n.content, SceneContent::Video { .. }))
        .expect("video node");
    let bound = node.effects.iter().find_map(|e| match e {
        ResolvedEffect::Stabilize { inverse, .. } => *inverse,
        _ => None,
    });
    assert_eq!(bound, Some(expected));
    let dag = build_render_dag(&scene, RenderProfile::default(), region()).unwrap();
    let stabilize = dag.nodes().iter().find_map(|n| match n {
        DagNode::Effect {
            effect:
                PixelEffect::Stabilize {
                    frame,
                    unmap,
                    size,
                    border,
                    fill,
                    sampling,
                },
            ..
        } => Some((*frame, *unmap, *size, *border, *fill, *sampling)),
        _ => None,
    });
    let (frame, _unmap, size, border, _fill, sampling) = stabilize.expect("stabilize pixel effect");
    assert_eq!(size, [16.0, 16.0]);
    assert_eq!(border, StabilizeBorder::Replicate);
    assert_eq!(sampling, StabilizeSampling::Bilinear);
    for r in 0..2 {
        for c in 0..3 {
            assert!(
                (frame[r][c] as f64 - expected[r][c]).abs() < 1e-5,
                "frame[{r}][{c}] {} vs {}",
                frame[r][c],
                expected[r][c]
            );
        }
    }
    // The bound inverse is part of the effect identity: a different tracked
    // instant yields a different semantic effect and raster key space.
    let later = build_scene_ir(&snapshot, t(1, 24), &[]).unwrap();
    let bound_later = later
        .nodes
        .iter()
        .find(|n| matches!(n.content, SceneContent::Video { .. }))
        .and_then(|n| {
            n.effects.iter().find_map(|e| match e {
                ResolvedEffect::Stabilize { inverse, .. } => *inverse,
                _ => None,
            })
        })
        .expect("bound inverse");
    assert_ne!(bound_later, bound.unwrap());
}

#[test]
fn track002_missing_or_stale_tracking_data_fails_typed() {
    let asset = video_asset();
    // No tracking data asset at all.
    let (p, id) = media_project(asset.clone(), None);
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let err = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
    assert!(err.code() == "TRACKING_DATA_MISSING", "{err:?}");
    // Locked to a different content hash: the document-level lock check
    // rejects the project before scene construction is even attempted.
    let mut stale = tracking(&asset, 8);
    stale.source.content_hash = "c".repeat(64);
    stale.content_hash = stale.computed_hash().unwrap();
    let (p, id) = media_project(asset.clone(), Some(stale));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let err = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
    assert_eq!(err.code(), "INVALID_DOCUMENT");
    // Locked to a different stream index: stale.
    let mut stale = tracking(&asset, 8);
    stale.source.stream_index = 7;
    stale.content_hash = stale.computed_hash().unwrap();
    let (p, id) = media_project(asset.clone(), Some(stale));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let err = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
    assert!(err.code() == "TRACKING_DATA_STALE", "{err:?}");
}

#[test]
fn track002_source_time_outside_tracking_range_is_typed() {
    let asset = video_asset();
    // Tracking rows cover only the first half-second; asking beyond it fails.
    let (p, id) = media_project(asset.clone(), Some(tracking(&asset, 4)));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let err = build_scene_ir(&snapshot, t(1, 1), &[]).unwrap_err();
    assert!(err.code() == "TRACKING_DATA_MISSING", "{err:?}");
}

#[test]
fn track002_unpinned_snapshot_rejects_authored_stabilize() {
    let asset = video_asset();
    let (p, id) = media_project(asset.clone(), Some(tracking(&asset, 8)));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let mut versions = snapshot.semantic_versions().clone();
    versions.effects.remove(STABILIZE_ID);
    let unpinned =
        RenderSnapshot::with_contract(&p, id, 1, snapshot.profile(), versions, vec![]).unwrap();
    let err = build_scene_ir(&unpinned, Time::ZERO, &[]).unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
}
