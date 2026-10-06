use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Time, TimeMap, TimeRange};
use std::{collections::BTreeMap, fs::File, path::Path};
fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn duration(n: i64) -> Duration {
    Duration::new(t(n, 1)).unwrap()
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
fn composition(nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: CompositionId::new(),
        duration: duration(4),
        design_extent: DesignExtent::new(2.0, 2.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: nodes.iter().map(|n| n.id).collect(),
        nodes,
        properties: vec![],
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
fn image(path: &Path, pixel: [u16; 4]) -> Asset {
    let mut encoder = png::Encoder::new(File::create(path).unwrap(), 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder.set_source_gamma(png::ScaledFloat::new(1.0));
    let mut writer = encoder.write_header().unwrap();
    let pixel: Vec<_> = pixel.into_iter().flat_map(u16::to_be_bytes).collect();
    writer.write_image_data(&pixel.repeat(4)).unwrap();
    writer.finish().unwrap();
    Asset {
        id: AssetId::new(),
        content_hash: content_hash(path).unwrap(),
        kind: AssetKind::Image,
        locator: AssetLocator {
            relative: Some(path.file_name().unwrap().to_str().unwrap().into()),
            absolute: Some(path.to_str().unwrap().into()),
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "png".into(),
            time_base: t(1, 24),
            duration: None,
            start_time: None,
            width: Some(2),
            height: Some(2),
            pixel_format: Some("rgba64be".into()),
            color_primaries: Some("bt709".into()),
            color_transfer: Some("linear".into()),
            color_matrix: Some("gbr".into()),
            color_range: Some("pc".into()),
        }],
    }
}
fn frame(
    snapshot: &RenderSnapshot,
    path: &Path,
    backend: &dyn RenderBackend,
    time: Time,
) -> RenderedFrame {
    render_frame(
        snapshot,
        &[],
        &VideoRenderBackend {
            backend,
            project_path: path,
        },
        FrameRequest {
            time,
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [2.0; 2],
                pixels: [2; 2],
            },
        },
    )
    .unwrap()
}
fn media_project(asset: Asset) -> (Project, CompositionId) {
    let volume = volume();
    let c = composition(vec![node(
        NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: t(7, 1),
            time_map: TimeMap::linear(Time::ZERO, t(2, 1)).unwrap(),
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
#[test]
fn composition_png_native16_alpha_cpu_gpu_and_asset_locks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.kronello");
    let file = dir.path().join("image.png");
    let asset = image(&file, [1001, 2002, 3003, 32768]);
    let (p, id) = media_project(asset.clone());
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let cpu = frame(&snapshot, &path, &CpuReferenceBackend, t(3, 1));
    let alpha = 32768.0_f32 / 65535.0;
    assert!((cpu.pixels.linear[0][0] - (1001.0 / 65535.0) * alpha).abs() < 1e-7);
    assert_eq!(cpu.pixels.linear[0][3], alpha);
    assert!(cpu.metadata.input_path.starts_with("png_native_8_16bit"));
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required; no fallback");
    let actual = frame(&snapshot, &path, &gpu, t(3, 1));
    for (a, b) in cpu.pixels.linear.iter().zip(actual.pixels.linear) {
        for c in 0..4 {
            assert!((a[c] - b[c]).abs() < 1.0 / 1024.0);
        }
    }
    let bytes = std::fs::read(&file).unwrap();
    std::fs::write(&file, b"different").unwrap();
    let backend = VideoRenderBackend {
        backend: &CpuReferenceBackend,
        project_path: &path,
    };
    let request = FrameRequest {
        time: Time::ZERO,
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [2.0; 2],
            pixels: [2; 2],
        },
    };
    assert_eq!(
        render_frame(&snapshot, &[], &backend, request)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        render_frame(&snapshot, &[], &backend, request)
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    std::fs::write(&file, bytes).unwrap();
    let mut legacy = serde_json::to_value(&snapshot).unwrap();
    legacy["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("composition_media");
    let legacy: RenderSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        render_frame(&legacy, &[], &backend, request)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}
#[test]
fn template_media_slot_final_render_uses_instance_override_and_nested_scope() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.kronello");
    let red = image(&dir.path().join("red.png"), [65535, 0, 0, 65535]);
    let blue = image(&dir.path().join("blue.png"), [0, 0, 65535, 65535]);
    let slot = node(NodeKind::Null, vec![]);
    let slot_id = slot.id;
    let target = composition(vec![slot]);
    let target_id = target.id;
    let instance_id = CompositionInstanceId::new();
    let root = composition(vec![node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: instance_id,
            definition_ref: target_id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            seed: 0,
        }),
        vec![],
    )]);
    let id = root.id;
    let mut p = Project {
        assets: vec![
            DocumentObject::Known(red.clone()),
            DocumentObject::Known(blue.clone()),
        ],
        compositions: vec![DocumentObject::Known(root), DocumentObject::Known(target)],
        ..Project::default()
    };
    let definition = TemplateDefinition {
        id: uuid::Uuid::new_v4(),
        template_id: uuid::Uuid::new_v4(),
        version: "1".into(),
        composition_ref: target_id,
        public_inputs: BTreeMap::from([(
            "logo".into(),
            TemplateInput {
                value_type: ValueType::AssetRef,
                default: Value::AssetRef(red.id),
                target: TemplateInputTarget::MediaSlot { node: slot_id },
                minimum: None,
                maximum: None,
                choices: vec![],
            },
        )]),
        variants: BTreeMap::new(),
        duration_policy: TemplateDurationPolicy {
            intro: Duration::ZERO,
            outro: Duration::ZERO,
            minimum_middle: Duration::ZERO,
            middle_mode: TemplateMiddleMode::Stretch,
        },
        constraints: Default::default(),
        content_hash: kronello_template::authoring_hash(&p, target_id).unwrap(),
    };
    let DocumentObject::Known(root) = &mut p.compositions[0] else {
        panic!()
    };
    let NodeKind::CompositionInstance(placement) = &mut root.nodes[0].kind else {
        panic!()
    };
    placement.local_time_map =
        kronello_template::duration_map(duration(4), duration(4), &definition.duration_policy)
            .unwrap();
    p.template_instances
        .push(DocumentObject::Known(TemplateInstance {
            id: instance_id,
            definition_ref: definition.id,
            version: definition.version.clone(),
            duration: duration(4),
            variant: None,
            inputs: BTreeMap::from([("logo".into(), Value::AssetRef(blue.id))]),
        }));
    p.templates.push(DocumentObject::Known(definition));
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let cpu = frame(&snapshot, &path, &CpuReferenceBackend, Time::ONE);
    assert_eq!(cpu.pixels.linear[0], [0.0, 0.0, 1.0, 1.0]);
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required; no fallback");
    let actual = frame(&snapshot, &path, &gpu, Time::ONE);
    assert_eq!(actual.pixels.linear, cpu.pixels.linear);
}

#[test]
fn composition_video_nested_source_time_and_cpu_gpu_reference() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().join("source.kronello");
    let path = dir.path().join("video.mov");
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
    let source = decoder.decode_at(t(1, 24)).unwrap();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: content_hash(&path).unwrap(),
        kind: AssetKind::Video,
        locator: AssetLocator {
            relative: Some("video.mov".into()),
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
    let volume = volume();
    let mut media = node(
        NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: t(1, 24),
            time_map: TimeMap::linear(Time::ZERO, t(1, 2)).unwrap(),
            volume: volume.id(),
        }),
        vec![volume],
    );
    media.active_range = TimeRange::new(Time::ONE, t(3, 1)).unwrap();
    let target = composition(vec![media]);
    let root = composition(vec![node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: target.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(Time::ONE, t(2, 1)).unwrap(),
            seed: 0,
        }),
        vec![],
    )]);
    let id = root.id;
    let p = Project {
        assets: vec![DocumentObject::Known(asset.clone())],
        compositions: vec![DocumentObject::Known(root), DocumentObject::Known(target)],
        ..Project::default()
    };
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let time = t(1, 96);
    let scene = build_scene_ir(&snapshot, time, &[]).unwrap();
    let source_time = scene
        .nodes
        .iter()
        .find_map(|n| match n.content {
            SceneContent::Video { time, .. } => Some(time),
            _ => None,
        })
        .unwrap();
    assert_eq!(source_time, t(5, 96));
    let reference = runtime
        .decode_video_image(
            &asset,
            &project_path,
            0,
            source_time,
            ColorSpace::LinearRec709,
        )
        .unwrap();
    let cpu = frame(&snapshot, &project_path, &CpuReferenceBackend, time);
    assert_eq!(cpu.pixels.linear[0], reference.pixels[0]);
    assert!(cpu.pixels.linear[0][2] > 0.95);
    assert!(cpu.pixels.linear[0][0] < 0.01);
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required; no fallback");
    let actual = frame(&snapshot, &project_path, &gpu, time);
    for (a, b) in cpu.pixels.linear.iter().zip(actual.pixels.linear) {
        for c in 0..4 {
            assert!((a[c] - b[c]).abs() < 1.0 / 1024.0);
        }
    }
    let backend = VideoRenderBackend {
        backend: &CpuReferenceBackend,
        project_path: &project_path,
    };
    assert_eq!(
        render_frame(
            &snapshot,
            &[],
            &backend,
            FrameRequest {
                time: t(1, 2),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [2.0; 2],
                    pixels: [2; 2]
                }
            }
        )
        .unwrap_err()
        .code(),
        "FRAME_NOT_FOUND"
    );
}

#[test]
fn native_png_color_contract_and_document_audio_visual_streams_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().join("source.kronello");
    let path = dir.path().join("linear.png");
    let asset = image(&path, [1001, 2002, 3003, 32768]);
    let rec709 = decode_image_asset(&asset, &project_path, 0, ColorSpace::LinearRec709).unwrap();
    let rec2020 = decode_image_asset(&asset, &project_path, 0, ColorSpace::LinearRec2020).unwrap();
    assert_ne!(rec709.pixels, rec2020.pixels);
    let mut tags = asset.clone();
    tags.streams[0].color_transfer = Some("smpte2084".into());
    assert_eq!(
        decode_image_asset(&tags, &project_path, 0, ColorSpace::LinearRec709)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut codec = asset.clone();
    codec.streams[0].codec = "jpeg".into();
    assert_eq!(
        decode_image_asset(&codec, &project_path, 0, ColorSpace::LinearRec709)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut format = asset.clone();
    format.streams[0].pixel_format = Some("rgba".into());
    assert_eq!(
        decode_image_asset(&format, &project_path, 0, ColorSpace::LinearRec709)
            .unwrap_err()
            .code(),
        "INVALID_MEDIA_INPUT"
    );
    let mut size = asset.clone();
    size.streams[0].width = Some(3);
    assert_eq!(
        decode_image_asset(&size, &project_path, 0, ColorSpace::LinearRec709)
            .unwrap_err()
            .code(),
        "INVALID_MEDIA_INPUT"
    );
    let (p, id) = media_project(asset);
    let snapshot = RenderSnapshot::new(&p, id, 1, RenderProfile::default()).unwrap();
    let av =
        AvExportSnapshot::with_audio(&snapshot, kronello_audio::AudioSourceMode::Document, vec![])
            .unwrap();
    assert!(av.clips().is_empty());
    let mut old = serde_json::to_value(&snapshot).unwrap();
    old["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("composition_media");
    let restored: RenderSnapshot = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), old);
    old["semantic_versions"]["composition_media"] = serde_json::json!(99);
    let future: RenderSnapshot = serde_json::from_value(old).unwrap();
    assert_eq!(future.validate().unwrap_err().code(), "UNSUPPORTED_FEATURE");
}
#[test]
fn png8_srgb_gray_palette_and_alpha_sources_preserve_numeric_meaning() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("source.kronello");
    for (index, color, format, data) in [
        (0, png::ColorType::Rgba, "rgba", vec![128, 64, 32, 128]),
        (1, png::ColorType::Grayscale, "gray", vec![128]),
        (2, png::ColorType::Indexed, "pal8", vec![0]),
    ] {
        let path = dir.path().join(format!("source-{index}.png"));
        let mut encoder = png::Encoder::new(File::create(&path).unwrap(), 1, 1);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![128, 64, 32]);
            encoder.set_trns(vec![128]);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        let asset = Asset {
            id: AssetId::new(),
            content_hash: content_hash(&path).unwrap(),
            kind: AssetKind::Image,
            locator: AssetLocator {
                relative: None,
                absolute: Some(path.to_str().unwrap().into()),
            },
            streams: vec![StreamMetadata {
                index: 0,
                codec: "png".into(),
                time_base: t(1, 24),
                duration: None,
                start_time: None,
                width: Some(1),
                height: Some(1),
                pixel_format: Some(format.into()),
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            }],
        };
        let image = decode_image_asset(&asset, &project, 0, ColorSpace::LinearRec709).unwrap();
        let alpha = if color == png::ColorType::Grayscale {
            1.0
        } else {
            128.0 / 255.0
        };
        let encoded = (128.0_f64 / 255.0 + 0.055) / 1.055;
        let expected = encoded.powf(2.4) * alpha;
        assert!((f64::from(image.pixels[0][0]) - expected).abs() < 1e-7);
        assert!((f64::from(image.pixels[0][3]) - alpha).abs() < 1e-7);
    }
}

#[test]
fn sequential_product_backend_reuses_exact_decoder_and_revalidates_hash() {
    let runtime = MediaRuntime::load().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/generated/media/cfr-30000-1001.nut")
        .canonicalize()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("video.nut");
    std::fs::copy(&path, &copy).unwrap();
    let metadata = runtime
        .open_video(&copy)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: content_hash(&copy).unwrap(),
        locator: AssetLocator {
            relative: None,
            absolute: Some(copy.to_str().unwrap().into()),
        },
        streams: vec![metadata],
    };
    let (mut project, id) = media_project(asset);
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        unreachable!()
    };
    let NodeKind::Media(media) = &mut c.nodes[0].kind else {
        unreachable!()
    };
    media.source_in = Time::ZERO;
    media.time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    let snapshot = RenderSnapshot::new(&project, id, 1, RenderProfile::default()).unwrap();
    let backend = SequentialVideoRenderBackend::new(&CpuReferenceBackend, dir.path(), Ok(&runtime));
    for time in (0..6)
        .map(|i| t(i * 1001, 30000))
        .chain([t(4004, 30000); 3])
    {
        let expected = frame(&snapshot, dir.path(), &CpuReferenceBackend, time);
        let request = FrameRequest {
            time,
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [2.0; 2],
                pixels: [2; 2],
            },
        };
        let actual = render_frame(&snapshot, &[], &backend, request).unwrap();
        assert_eq!(actual.pixels, expected.pixels);
    }
    assert_eq!(backend.decoder_stats().len(), 1);
    assert_eq!(backend.decoder_stats()[0].seeks, 2);
    assert_eq!(backend.decoder_stats()[0].interval_hits, 2);
    assert_eq!(backend.pool_stats().misses, 1);
    assert_eq!(backend.pool_stats().hits, 8);
    for _ in 0..2 {
        let mut another = project.clone();
        let fresh = AssetId::new();
        let DocumentObject::Known(asset) = &mut another.assets[0] else {
            unreachable!()
        };
        asset.id = fresh;
        let DocumentObject::Known(c) = &mut another.compositions[0] else {
            unreachable!()
        };
        let NodeKind::Media(media) = &mut c.nodes[0].kind else {
            unreachable!()
        };
        media.asset = fresh;
        let snapshot = RenderSnapshot::new(&another, id, 1, RenderProfile::default()).unwrap();
        render_frame(
            &snapshot,
            &[],
            &backend,
            FrameRequest {
                time: Time::ZERO,
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [2.0; 2],
                    pixels: [2; 2],
                },
            },
        )
        .unwrap();
    }
    assert_eq!(backend.pool_stats().active_decoders, 2);
    assert_eq!(backend.pool_stats().evictions, 1);
    assert_eq!(backend.pool_stats().retained_frame_bytes, 1536);
    assert!(backend.pool_stats().peak_retained_frame_bytes <= 128 * 1024 * 1024);
    std::fs::write(&copy, b"changed").unwrap();
    let error = render_frame(
        &snapshot,
        &[],
        &backend,
        FrameRequest {
            time: t(4004, 30000),
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [2.0; 2],
                pixels: [2; 2],
            },
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASSET_HASH_MISMATCH");
}
