//! AI-002/AI-003 model coverage (ADR-0125/0126): SceneBoundaryAsset
//! validation and provenance, smart-reframe settings, project backwards
//! compatibility and the crop builtin descriptors.
use kronello_model::*;
use kronello_time::{Duration, Rational, Time, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn range() -> TimeRange {
    TimeRange::new(Time::ZERO, r(8, 24)).unwrap()
}
fn boundary_asset(boundaries: Vec<(i64, f64)>) -> SceneBoundaryAsset {
    let mut data = SceneBoundaryAsset {
        id: AssetId::new(),
        version: SCENE_BOUNDARY_VERSION,
        source: SceneSource {
            asset: AssetId::new(),
            stream_index: 0,
            content_hash: "b".repeat(64),
        },
        params: SceneDetectionParams::default(),
        range: range(),
        frames_analyzed: 8,
        boundaries: boundaries
            .into_iter()
            .map(|(tick, confidence)| SceneBoundary {
                time: r(tick, 24),
                confidence: finite(confidence),
            })
            .collect(),
        content_hash: String::new(),
    };
    data.content_hash = data.computed_hash().unwrap();
    data
}

#[test]
fn scene_boundary_asset_validates_identity_and_ordering() {
    let data = boundary_asset(vec![(1, 0.8), (4, 0.5)]);
    data.validate().unwrap();
    // Hash cover: mutating any versioned payload field breaks validation.
    for mutate in [
        (|d: &mut SceneBoundaryAsset| d.boundaries[0].time = r(2, 24)),
        (|d: &mut SceneBoundaryAsset| d.frames_analyzed = 7),
        (|d: &mut SceneBoundaryAsset| d.source.stream_index = 1),
        (|d: &mut SceneBoundaryAsset| d.params.min_spacing_frames = 5),
    ] as [fn(&mut SceneBoundaryAsset); 4]
    {
        let mut corrupt = data.clone();
        mutate(&mut corrupt);
        assert!(corrupt.validate().is_err());
    }
    // Recomputing the hash restores validity: the cover is the whole payload.
    let mut modified = data.clone();
    modified.boundaries[0].confidence = finite(0.9);
    modified.content_hash = modified.computed_hash().unwrap();
    modified.validate().unwrap();
    // Unordered, out-of-range and out-of-confidence boundaries reject.
    for boundaries in [
        vec![(4, 0.5), (1, 0.8)],
        vec![(0, 0.5)],
        vec![(8, 0.5)],
        vec![(2, 1.5)],
    ] {
        let mut data = SceneBoundaryAsset {
            boundaries: boundaries
                .into_iter()
                .map(|(tick, confidence)| SceneBoundary {
                    time: r(tick, 24),
                    confidence: finite(confidence),
                })
                .collect(),
            ..boundary_asset(vec![(1, 0.5)])
        };
        data.content_hash = data.computed_hash().unwrap();
        assert!(data.validate().is_err(), "{:?}", data.boundaries);
    }
    // Unsupported version and degenerate bounds reject.
    let mut data = boundary_asset(vec![(1, 0.5)]);
    data.version += 1;
    data.content_hash = data.computed_hash().unwrap();
    assert!(matches!(
        data.validate().unwrap_err(),
        ProjectError::UnsupportedMeaning
    ));
    let mut data = boundary_asset(vec![(1, 0.5)]);
    data.frames_analyzed = 0;
    data.content_hash = data.computed_hash().unwrap();
    assert!(data.validate().is_err());
    let mut data = boundary_asset(vec![]);
    data.boundaries.clear();
    data.content_hash = data.computed_hash().unwrap();
    data.validate().unwrap();
}

fn video_asset(id: AssetId, content_hash: &str) -> Asset {
    Asset {
        id,
        content_hash: content_hash.into(),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: r(1, 24),
            duration: Some(r(8, 24)),
            start_time: Some(Time::ZERO),
            width: Some(16),
            height: Some(16),
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
        locator: AssetLocator {
            relative: Some("clip.mov".into()),
            absolute: None,
        },
    }
}

#[test]
fn scene_boundary_lookup_requires_a_live_locked_source() {
    let source = AssetId::new();
    let mut data = boundary_asset(vec![(2, 0.5)]);
    data.source.asset = source;
    data.source.content_hash = "c".repeat(64);
    data.content_hash = data.computed_hash().unwrap();
    let mut project = Project::default();
    project
        .scene_boundary_assets
        .push(DocumentObject::Known(data.clone()));
    // The asset id is distinct from the source asset id.
    assert!(project.scene_boundary_asset(data.id).is_err());
    project
        .assets
        .push(DocumentObject::Known(video_asset(source, &"c".repeat(64))));
    assert_eq!(project.scene_boundary_asset(data.id).unwrap(), data);
    assert_eq!(project.scene_boundary_inputs().unwrap().len(), 1);
    // Content drift marks the result stale instead of silently applying.
    if let DocumentObject::Known(a) = &mut project.assets[0] {
        a.content_hash = "d".repeat(64);
    }
    assert!(project.scene_boundary_asset(data.id).is_err());
    assert!(project.scene_boundary_inputs().is_err());
    assert!(project.scene_boundary_asset(AssetId::new()).is_err());
}

#[test]
fn smart_reframe_settings_validate_versioned_bounds() {
    let valid = SmartReframeSettings {
        version: SMART_REFRAME_VERSION,
        seeds: vec![0, 2],
        smoothing_window: Duration::new(r(1, 24)).unwrap(),
        padding: finite(0.25),
        max_zoom: finite(2.0),
        target_aspect: finite(9.0 / 16.0),
        easing: ReframeEasing::EaseInOut,
    };
    valid.validate().unwrap();
    for mutate in [
        (|s: &mut SmartReframeSettings| s.version += 1),
        (|s: &mut SmartReframeSettings| s.padding = finite(1.5)),
        (|s: &mut SmartReframeSettings| s.max_zoom = finite(0.5)),
        (|s: &mut SmartReframeSettings| s.target_aspect = finite(0.0)),
        (|s: &mut SmartReframeSettings| s.seeds = vec![1, 1]),
        (|s: &mut SmartReframeSettings| s.seeds = vec![TRACKING_MAX_SEEDS as u32]),
    ] as [fn(&mut SmartReframeSettings); 6]
    {
        let mut invalid = valid.clone();
        mutate(&mut invalid);
        assert!(invalid.validate().is_err());
    }
    for easing in [
        ReframeEasing::Linear,
        ReframeEasing::EaseIn,
        ReframeEasing::EaseOut,
        ReframeEasing::EaseInOut,
    ] {
        let roundtrip: ReframeEasing =
            serde_json::from_str(&serde_json::to_string(&easing).unwrap()).unwrap();
        assert_eq!(roundtrip, easing);
    }
}

#[test]
fn project_roundtrips_scene_assets_and_omits_empty_lists() {
    // Documents written before AI-002 lack the field entirely.
    let value = serde_json::to_value(Project::default()).unwrap();
    assert!(
        !value
            .as_object()
            .unwrap()
            .contains_key("scene_boundary_assets")
    );
    let project: Project = serde_json::from_value(value).unwrap();
    assert!(project.scene_boundary_assets.is_empty());
    // A populated list roundtrips through the document format unchanged.
    let mut project = Project::default();
    project
        .scene_boundary_assets
        .push(DocumentObject::Known(boundary_asset(vec![(2, 0.5)])));
    let json = serde_json::to_string(&project).unwrap();
    let reparsed: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(project, reparsed);
}

#[test]
fn crop_builtins_register_vec2_descriptors() {
    let registry = SchemaRegistry::with_builtin();
    for (id, key) in [
        (MEDIA_CROP_ORIGIN_ID, "kronello.media.crop_origin"),
        (MEDIA_CROP_SIZE_ID, "kronello.media.crop_size"),
    ] {
        let descriptor = registry
            .lookup(&SchemaKey::new(key).unwrap())
            .expect("crop descriptor registered");
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.definition().value_type, ValueType::Vec2);
        assert_eq!(descriptor.definition().unit, Unit::DesignPx);
    }
}
