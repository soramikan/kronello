//! ADR-0119 render-side proxy substitution: SceneIr decode identity.
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Rational, Time, TimeMap, TimeRange};

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
fn media_node(asset: AssetId, volume: Property) -> SceneNode {
    let id = volume.id();
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
            time_map: TimeMap::linear(Time::ZERO, r(1, 1)).unwrap(),
            volume: id,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, r(3, 1)).unwrap(),
        properties: vec![volume],
    }
}
fn volume() -> Property {
    let registry = render_registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(finite(1.0))),
        vec![],
        &registry,
    )
    .unwrap()
}
struct Fixture {
    project: Project,
    composition: CompositionId,
    original: AssetId,
    proxy: AssetId,
}
fn build_fixture() -> Fixture {
    let original = AssetId::new();
    let proxy = AssetId::new();
    let node = media_node(original, volume());
    let composition = Composition {
        id: CompositionId::new(),
        duration: kronello_time::Duration::new(r(3, 1)).unwrap(),
        design_extent: DesignExtent::new(320.0, 180.0).unwrap(),
        edit_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![node.id],
        nodes: vec![node],
        properties: vec![],
    };
    let mut project = Project {
        compositions: vec![DocumentObject::Known(composition)],
        ..Project::default()
    };
    project
        .assets
        .push(DocumentObject::Known(video(original, 320, 180, 'a')));
    project
        .assets
        .push(DocumentObject::Known(video(proxy, 160, 90, 'b')));
    project.proxies.push(ProxyLink {
        original,
        proxy,
        original_stream_index: 0,
        proxy_stream_index: 0,
        scale: finite(0.5),
        width: 160,
        height: 90,
        source_content_hash: "a".repeat(64),
        source_duration: Some(r(4, 1)),
        job: None,
    });
    Fixture {
        composition: composition_id(&project),
        project,
        original,
        proxy,
    }
}
fn composition_id(project: &Project) -> CompositionId {
    match &project.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    }
}
fn snapshot(fixture: &Fixture, mode: MediaProxyMode) -> RenderSnapshot {
    RenderSnapshot::new(
        &fixture.project,
        fixture.composition,
        7,
        RenderProfile::default(),
    )
    .unwrap()
    .with_media_proxies(mode)
}
fn video_content(snapshot: &RenderSnapshot) -> (AssetId, u32, [f64; 2]) {
    let scene = build_scene_ir(snapshot, Time::ZERO, &[]).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|n| matches!(n.content, SceneContent::Video { .. }))
        .expect("one media node");
    match &node.content {
        SceneContent::Video {
            asset,
            stream_index,
            extent,
            ..
        } => (asset.id, *stream_index, *extent),
        _ => unreachable!(),
    }
}

#[test]
fn prefer_mode_substitutes_a_valid_registered_proxy() {
    let fixture = build_fixture();
    let off = video_content(&snapshot(&fixture, MediaProxyMode::Off));
    assert_eq!(off.0, fixture.original);
    let prefer = video_content(&snapshot(&fixture, MediaProxyMode::Prefer));
    assert_eq!(prefer.0, fixture.proxy);
    assert_eq!(prefer.1, 0);
    // Authored extent (original stream pixels) is preserved on substitution.
    assert_eq!(prefer.2, [320.0, 180.0]);
}

#[test]
fn stale_missing_or_stream_mismatched_links_fall_back_to_original() {
    let mut fixture = build_fixture();
    // Missing: no link at all.
    fixture.project.proxies.clear();
    assert_eq!(
        video_content(&snapshot(&fixture, MediaProxyMode::Prefer)).0,
        fixture.original
    );
    // Stale: content hash drift.
    let mut fixture = build_fixture();
    fixture.project.proxies[0].source_content_hash = "f".repeat(64);
    assert_eq!(
        video_content(&snapshot(&fixture, MediaProxyMode::Prefer)).0,
        fixture.original
    );
    // Different authored stream: link covers stream 0 only.
    let mut fixture = build_fixture();
    if let DocumentObject::Known(c) = &mut fixture.project.compositions[0]
        && let NodeKind::Media(media) = &mut c.nodes[0].kind
    {
        media.stream_index = 1;
    }
    if let DocumentObject::Known(a) = &mut fixture.project.assets[0] {
        a.streams.push(StreamMetadata {
            index: 1,
            codec: "h264".into(),
            time_base: r(1, 24),
            duration: Some(r(4, 1)),
            start_time: Some(Time::ZERO),
            width: Some(320),
            height: Some(180),
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        });
    }
    assert_eq!(
        video_content(&snapshot(&fixture, MediaProxyMode::Prefer)).0,
        fixture.original
    );
    // Proxy asset object removed: substitution cannot resolve, fallback.
    let mut fixture = build_fixture();
    fixture
        .project
        .assets
        .retain(|a| !matches!(a, DocumentObject::Known(a) if a.id == fixture.proxy));
    assert_eq!(
        video_content(&snapshot(&fixture, MediaProxyMode::Prefer)).0,
        fixture.original
    );
}
