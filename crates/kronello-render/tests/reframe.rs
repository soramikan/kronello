//! AI-003 (ADR-0126) render coverage: media crop properties narrow the video
//! extent with typed bounds checks, and template smart-reframe rules inject
//! tracking-driven crop windows through the shared layout-value path.
use std::collections::BTreeMap;

use kronello_model::*;
use kronello_render::*;
use kronello_template::authoring_hash;
use kronello_time::{Duration, FrameRate, Rational, Time, TimeMap, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn video(id: AssetId, width: u32, height: u32, hash: char) -> Asset {
    Asset {
        id,
        content_hash: hash.to_string().repeat(64),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: r(1, 24),
            duration: Some(r(4, 1)),
            start_time: Some(Time::ZERO),
            width: Some(width),
            height: Some(height),
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
        locator: AssetLocator {
            relative: Some(format!("{id}.mov")),
            absolute: None,
        },
    }
}
fn property(name: &str, value: Value) -> Property {
    let registry = render_registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(name).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
}
fn media_node(asset: AssetId, properties: Vec<Property>) -> SceneNode {
    let mut properties = properties;
    let volume = property("kronello.audio.volume", Value::Scalar(finite(1.0)));
    let volume_id = volume.id();
    properties.push(volume);
    SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        effects: vec![],
        id: NodeId::new(),
        kind: NodeKind::Media(MediaNode {
            asset,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
            volume: volume_id,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, r(3, 1)).unwrap(),
        properties,
    }
}
fn composition(nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: CompositionId::new(),
        duration: Duration::new(r(3, 1)).unwrap(),
        design_extent: DesignExtent::new(320.0, 180.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: nodes.iter().map(|n| n.id).collect(),
        nodes,
        properties: vec![],
    }
}
fn vec2(x: f64, y: f64) -> Value {
    Value::Vec2([finite(x), finite(y)])
}
fn video_crop(scene: &SceneIr) -> ([f64; 2], Option<[f64; 4]>) {
    let node = scene
        .nodes
        .iter()
        .find(|n| matches!(n.content, SceneContent::Video { .. }))
        .expect("video node");
    let SceneContent::Video { extent, crop, .. } = &node.content else {
        unreachable!()
    };
    (*extent, *crop)
}

#[test]
fn crop_properties_narrow_video_extent_and_reject_out_of_bounds() {
    let asset = AssetId::new();
    let mut project = Project::default();
    project
        .assets
        .push(DocumentObject::Known(video(asset, 320, 180, 'a')));
    let node = media_node(
        asset,
        vec![
            property("kronello.media.crop_origin", vec2(8.0, 6.0)),
            property("kronello.media.crop_size", vec2(16.0, 12.0)),
        ],
    );
    let root = composition(vec![node]);
    let id = root.id;
    project.compositions.push(DocumentObject::Known(root));
    let snapshot = RenderSnapshot::new(&project, id, 1, RenderProfile::default()).unwrap();
    let (extent, crop) = video_crop(&build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap());
    // The layout box is the crop window so bounds follow reframed content.
    assert_eq!(crop, Some([8.0, 6.0, 16.0, 12.0]));
    assert_eq!(extent, [16.0, 12.0]);
    // A window outside the locked source bounds is a typed error, not a clamp.
    let DocumentObject::Known(root) = &mut project.compositions[0] else {
        panic!()
    };
    root.nodes[0].properties[0] = property("kronello.media.crop_origin", vec2(310.0, 0.0));
    let snapshot = RenderSnapshot::new(&project, id, 1, RenderProfile::default()).unwrap();
    let error = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
    assert_eq!(error.code(), "RENDER_ERROR", "{error}");
}

/// One-seed tracking asset whose source identity is `hash * 64`.
fn tracking(asset: AssetId, hash: char) -> TrackingDataAsset {
    let mut data = TrackingDataAsset {
        id: AssetId::new(),
        version: TRACKING_VERSION,
        source: TrackingSource {
            asset,
            stream_index: 0,
            content_hash: hash.to_string().repeat(64),
        },
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: finite(0.5),
            y: finite(0.5),
            template_radius: 4,
            search_radius: 8,
        }],
        range: TimeRange::new(Time::ZERO, r(3, 1)).unwrap(),
        sample_rate: r(24, 1),
        frames: vec![TrackedFrame {
            time: Time::ZERO,
            points: vec![TrackedPoint {
                x: finite(0.25),
                y: finite(0.5),
                confidence: finite(1.0),
            }],
            homography: None,
        }],
        content_hash: String::new(),
    };
    data.content_hash = data.computed_hash().unwrap();
    data
}

fn reframe_project(asset_hash: char, tracking_hash: char) -> (Project, CompositionId, AssetId) {
    let asset = AssetId::new();
    let tracking = tracking(asset, tracking_hash);
    let tracking_id = tracking.id;
    // The template composition hosts the reframed media node; crop
    // descriptors are placeholders the rule fills at layout time.
    let origin = property("kronello.media.crop_origin", vec2(0.0, 0.0));
    let size = property("kronello.media.crop_size", vec2(1.0, 1.0));
    let node = media_node(asset, vec![origin.clone(), size.clone()]);
    let node_id = node.id;
    let template_composition = composition(vec![node]);
    let settings = SmartReframeSettings {
        version: SMART_REFRAME_VERSION,
        seeds: vec![],
        smoothing_window: Duration::new(Time::ZERO).unwrap(),
        padding: finite(1.0),
        max_zoom: finite(1.0),
        target_aspect: finite(9.0 / 16.0),
        easing: ReframeEasing::Linear,
    };
    let mut definition = TemplateDefinition {
        id: uuid::Uuid::new_v4(),
        template_id: uuid::Uuid::new_v4(),
        version: "1.0.0".into(),
        composition_ref: template_composition.id,
        public_inputs: BTreeMap::new(),
        variants: BTreeMap::new(),
        duration_policy: TemplateDurationPolicy {
            intro: Duration::new(Time::ZERO).unwrap(),
            outro: Duration::new(Time::ZERO).unwrap(),
            minimum_middle: Duration::new(Time::ZERO).unwrap(),
            middle_mode: TemplateMiddleMode::Hold,
        },
        constraints: TemplateConstraints {
            smart_reframes: vec![SmartReframeRule {
                node: node_id,
                tracking: tracking_id,
                crop_origin_property: origin.id(),
                crop_size_property: size.id(),
                settings,
            }],
            ..TemplateConstraints::default()
        },
        content_hash: String::new(),
    };
    let mut project = Project::default();
    project
        .assets
        .push(DocumentObject::Known(video(asset, 320, 180, asset_hash)));
    project
        .compositions
        .push(DocumentObject::Known(template_composition));
    project
        .tracking_data_assets
        .push(DocumentObject::Known(tracking));
    definition.content_hash = authoring_hash(&project, definition.composition_ref).unwrap();
    // Root composition places one instance of the template composition.
    let instance_id = CompositionInstanceId::new();
    let instance_node = SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        effects: vec![],
        id: NodeId::new(),
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id: instance_id,
            definition_ref: definition.composition_ref,
            input_bindings: BTreeMap::new(),
            local_time_map: kronello_template::duration_map(
                Duration::new(r(3, 1)).unwrap(),
                Duration::new(r(3, 1)).unwrap(),
                &definition.duration_policy,
            )
            .unwrap(),
            seed: 0,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, r(3, 1)).unwrap(),
        properties: vec![],
    };
    let root = composition(vec![instance_node]);
    let root_id = root.id;
    project.compositions.push(DocumentObject::Known(root));
    project
        .templates
        .push(DocumentObject::Known(definition.clone()));
    project
        .template_instances
        .push(DocumentObject::Known(TemplateInstance {
            id: instance_id,
            definition_ref: definition.id,
            version: definition.version,
            duration: Duration::new(r(3, 1)).unwrap(),
            variant: None,
            inputs: BTreeMap::new(),
        }));
    (project, root_id, asset)
}

#[test]
fn smart_reframe_rule_injects_tracking_driven_crop_window() {
    let (project, root, _asset) = reframe_project('a', 'a');
    let snapshot = RenderSnapshot::new(&project, root, 7, RenderProfile::default()).unwrap();
    let (extent, crop) = video_crop(&build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap());
    let DocumentObject::Known(tracking) = &project.tracking_data_assets[0] else {
        panic!()
    };
    let DocumentObject::Known(definition) = &project.templates[0] else {
        panic!()
    };
    let settings = &definition.constraints.smart_reframes[0].settings;
    // The injected window is exactly the pure `reframe_window` result at the
    // media node's mapped source time, and the layout extent follows it.
    let expected =
        kronello_scene::reframe_window(tracking, settings, Time::ZERO, [320.0, 180.0]).unwrap();
    assert_eq!(crop, Some(expected));
    assert_eq!(extent, [expected[2], expected[3]]);
    // Deterministic: a second evaluation produces the identical window.
    let again = video_crop(&build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap());
    assert_eq!(again.1, crop);
}

#[test]
fn smart_reframe_honors_selected_aspect_variant() {
    // Responsive composition (LAYOUT-001): each aspect variant freezes its own
    // composition and constraints, so a reframe rule must follow the variant
    // the instance selects rather than the default edition.
    let (mut project, root, asset) = reframe_project('a', 'a');
    let DocumentObject::Known(tracking) = &project.tracking_data_assets[0] else {
        panic!()
    };
    let tracking_id = tracking.id;
    let origin = property("kronello.media.crop_origin", vec2(0.0, 0.0));
    let size = property("kronello.media.crop_size", vec2(1.0, 1.0));
    let node = media_node(asset, vec![origin.clone(), size.clone()]);
    let node_id = node.id;
    let variant_composition = composition(vec![node]);
    let variant_ref = variant_composition.id;
    project
        .compositions
        .push(DocumentObject::Known(variant_composition));
    let settings = SmartReframeSettings {
        version: SMART_REFRAME_VERSION,
        seeds: vec![],
        smoothing_window: Duration::new(Time::ZERO).unwrap(),
        padding: finite(1.0),
        max_zoom: finite(1.0),
        target_aspect: finite(1.0),
        easing: ReframeEasing::Linear,
    };
    let variant_hash = authoring_hash(&project, variant_ref).unwrap();
    let DocumentObject::Known(definition) = &mut project.templates[0] else {
        panic!()
    };
    definition.variants.insert(
        "square".to_owned(),
        TemplateVariant {
            composition_ref: variant_ref,
            targets: BTreeMap::new(),
            constraints: TemplateConstraints {
                smart_reframes: vec![SmartReframeRule {
                    node: node_id,
                    tracking: tracking_id,
                    crop_origin_property: origin.id(),
                    crop_size_property: size.id(),
                    settings: settings.clone(),
                }],
                ..TemplateConstraints::default()
            },
            content_hash: variant_hash,
        },
    );
    let DocumentObject::Known(instance) = &mut project.template_instances[0] else {
        panic!()
    };
    instance.variant = Some("square".to_owned());
    // The placement pin follows the selected edition's composition.
    let DocumentObject::Known(root_c) = &mut project.compositions[1] else {
        panic!()
    };
    let NodeKind::CompositionInstance(placement) = &mut root_c.nodes[0].kind else {
        panic!()
    };
    placement.definition_ref = variant_ref;
    let snapshot = RenderSnapshot::new(&project, root, 7, RenderProfile::default()).unwrap();
    let (extent, crop) = video_crop(&build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap());
    let DocumentObject::Known(tracking) = &project.tracking_data_assets[0] else {
        panic!()
    };
    // The variant's square aspect drives the window, not the default 9:16.
    let expected =
        kronello_scene::reframe_window(tracking, &settings, Time::ZERO, [320.0, 180.0]).unwrap();
    assert_eq!(crop, Some(expected));
    assert_eq!(extent, [expected[2], expected[3]]);
    assert_eq!(expected[2] / expected[3], 1.0);
}

#[test]
fn smart_reframe_rejects_stale_tracking_source() {
    // The document asset drifted after the tracking asset was produced;
    // document validation already reports the stale source as a typed error.
    let (project, root, _asset) = reframe_project('b', 'a');
    let snapshot = RenderSnapshot::new(&project, root, 7, RenderProfile::default()).unwrap();
    let error = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
    assert_eq!(error.code(), "INVALID_DOCUMENT", "{error}");
}
