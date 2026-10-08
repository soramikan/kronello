use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Rational, Time};
fn hdr_fixture_roundtrip(test_gpu: bool) {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
        let path = dir.path().join(format!("{:?}.mov", transfer));
        let code = transfer.encode([1.0, 1.0, 1.0]).unwrap();
        let mut pixel = Vec::new();
        for c in code {
            pixel.extend(((c * 65535.0).round() as u16).to_le_bytes());
        }
        pixel.extend(u16::MAX.to_le_bytes());
        let request = EncodeRequest {
            output: path.clone(),
            codec: EncodeCodec::ProRes,
            width: 16,
            height: 16,
            time_base: Rational::new(1, 24).unwrap(),
        };
        let report = runtime
            .encode_hdr_video_stream(&request, 2, transfer, &mut |i| {
                Ok(EncodeFrame {
                    pts: Rational::new(i as i64, 24).unwrap(),
                    rgba: pixel.repeat(256),
                })
            })
            .unwrap();
        assert_eq!(report.input_pixel_format, "rgba64le");
        assert_eq!(report.output_pixel_format, "yuv422p10le");
        let probe = runtime.probe(&path).unwrap();
        let stream = &probe.streams[0];
        assert_eq!(stream.color_primaries.as_deref(), Some("bt2020"));
        assert_eq!(stream.color_transfer.as_deref(), Some(transfer.tag()));
        assert_eq!(stream.color_matrix.as_deref(), Some("bt2020nc"));
        assert_eq!(stream.pixel_format.as_deref(), Some("yuv422p10le"));
        let asset = Asset {
            id: AssetId::new(),
            kind: AssetKind::Video,
            content_hash: content_hash(&path).unwrap(),
            locator: AssetLocator {
                relative: None,
                absolute: Some(path.to_str().unwrap().into()),
            },
            streams: vec![StreamMetadata {
                index: 0,
                codec: "prores".into(),
                time_base: request.time_base,
                duration: Some(Rational::new(2, 24).unwrap()),
                start_time: Some(Time::ZERO),
                width: Some(16),
                height: Some(16),
                pixel_format: stream.pixel_format.clone(),
                color_primaries: stream.color_primaries.clone(),
                color_transfer: stream.color_transfer.clone(),
                color_matrix: stream.color_matrix.clone(),
                color_range: stream.color_range.clone(),
            }],
        };
        assert_eq!(
            runtime
                .decode_video_image(&asset, dir.path(), 0, Time::ZERO, ColorSpace::LinearRec2020)
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        let image = runtime
            .decode_video_image_with_hdr(
                &asset,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec2020,
                Some(HdrSettings { transfer }),
            )
            .unwrap();
        for c in &image.pixels[0][..3] {
            assert!((*c - 1.0).abs() < 0.04, "{transfer:?}: {c}");
        }
        assert_eq!(image.pixels[0][3], 1.0);
        let registry = render_registry();
        let descriptor = registry
            .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
            .unwrap();
        let volume = Property::new(
            PropertyId::new(),
            DescriptorRef::new(descriptor),
            PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap())),
            vec![],
            &registry,
        )
        .unwrap();
        let node = SceneNode {
            id: NodeId::new(),
            name: None,
            tags: Default::default(),
            enabled: true,
            kind: NodeKind::Media(MediaNode {
                asset: asset.id,
                stream_index: 0,
                source_in: Time::ZERO,
                time_map: kronello_time::TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                volume: volume.id(),
            }),
            active_range: kronello_time::TimeRange::new(Time::ZERO, Rational::new(2, 24).unwrap())
                .unwrap(),
            properties: vec![volume],
            containment_parent: None,
            transform_parent: None,
            child_order: vec![],
            effects: vec![],
        };
        let composition = Composition {
            id: CompositionId::new(),
            duration: kronello_time::Duration::new(Time::ONE).unwrap(),
            design_extent: DesignExtent::new(16.0, 16.0).unwrap(),
            edit_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
            root_nodes: vec![node.id],
            nodes: vec![node],
            properties: vec![],
        };
        let id = composition.id;
        let project = Project {
            assets: vec![DocumentObject::Known(asset.clone())],
            compositions: vec![DocumentObject::Known(composition)],
            ..Project::default()
        };
        let snapshot = RenderSnapshot::new(
            &project,
            id,
            1,
            RenderProfile {
                working_space: ColorSpace::LinearRec2020,
                hdr: Some(HdrSettings { transfer }),
                ..Default::default()
            },
        )
        .unwrap();
        let mut missing = snapshot.semantic_versions().clone();
        missing.hdr = None;
        assert_eq!(
            RenderSnapshot::with_contract(&project, id, 1, snapshot.profile(), missing, vec![])
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        let cpu = kronello_gpu::render_adapter::CpuReferenceBackend;
        let backend = VideoRenderBackend {
            backend: &cpu,
            project_path: dir.path(),
        };
        let region = OutputRegion {
            origin: [0.0; 2],
            extent: [16.0; 2],
            pixels: [16; 2],
        };
        let rendered = render_frame(
            &snapshot,
            &[],
            &backend,
            FrameRequest {
                time: Time::ZERO,
                region,
            },
        )
        .unwrap();
        assert_eq!(rendered.pixels.linear, image.pixels);
        assert!(rendered.pixels.display[0][0] < rendered.pixels.linear[0][0]);
        if test_gpu {
            let gpu = kronello_gpu::GpuContext::new().expect("real GPU required; no CPU fallback");
            let actual = render_frame(
                &snapshot,
                &[],
                &VideoRenderBackend {
                    backend: &gpu,
                    project_path: dir.path(),
                },
                FrameRequest {
                    time: Time::ZERO,
                    region,
                },
            )
            .unwrap();
            for (expected, actual) in rendered.pixels.linear.iter().zip(actual.pixels.linear) {
                for i in 0..4 {
                    assert!((expected[i] - actual[i]).abs() < 1.0 / 1024.0);
                }
            }
        }
        let movie = match transfer {
            HdrTransfer::Pq => MovieProfile::ProResPqPcm24V1,
            HdrTransfer::Hlg => MovieProfile::ProResHlgPcm24V1,
        };
        let fixed = AvExportSnapshot::with_movie_profile(
            &snapshot,
            kronello_audio::AudioSourceMode::Silence,
            vec![],
            movie,
        )
        .unwrap();
        let restored: AvExportSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&fixed).unwrap()).unwrap();
        assert_eq!(
            fixed.content_hash().unwrap(),
            restored.content_hash().unwrap()
        );
        let output = dir.path().join(format!("final-{transfer:?}.mov"));
        let export = runtime
            .export_av(
                &restored,
                dir.path(),
                &[],
                &backend,
                &AvExportRequest {
                    output: output.clone(),
                    range: kronello_time::TimeRange::new(Time::ZERO, Rational::new(2, 24).unwrap())
                        .unwrap(),
                    frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                    region,
                    background: [0.0; 3],
                    clipping: kronello_audio::ClippingPolicy::Reject,
                    chapters: kronello_media::ChapterPolicy::Transfer,
                    outputs: Vec::new(),
                },
            )
            .unwrap();
        export.probe.verify_movie(movie).unwrap();
        assert_eq!(
            export.probe.render_snapshot_hash,
            snapshot.content_hash().unwrap()
        );
        let v = &export.probe.streams[0];
        assert_eq!(v.color_transfer.as_deref(), Some(transfer.tag()));
        let mut final_asset = asset.clone();
        final_asset.content_hash = content_hash(&output).unwrap();
        final_asset.locator.absolute = Some(output.to_str().unwrap().into());
        let final_image = runtime
            .decode_video_image_with_hdr(
                &final_asset,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec2020,
                Some(HdrSettings { transfer }),
            )
            .unwrap();
        assert!(
            (final_image.pixels[0][0] - 1.0).abs() < 0.05,
            "display transform must not be baked into HDR output"
        );
        let sdr_fixed = AvExportSnapshot::with_movie_profile(
            &snapshot,
            kronello_audio::AudioSourceMode::Silence,
            vec![],
            MovieProfile::ProResSdrFromHdrPcm24V1,
        )
        .unwrap();
        let sdr_output = dir.path().join(format!("explicit-sdr-{transfer:?}.mov"));
        let sdr_report = runtime
            .export_av(
                &sdr_fixed,
                dir.path(),
                &[],
                &backend,
                &AvExportRequest {
                    output: sdr_output.clone(),
                    range: kronello_time::TimeRange::new(Time::ZERO, Rational::new(2, 24).unwrap())
                        .unwrap(),
                    frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                    region,
                    background: [0.0; 3],
                    clipping: kronello_audio::ClippingPolicy::Reject,
                    chapters: kronello_media::ChapterPolicy::Transfer,
                    outputs: Vec::new(),
                },
            )
            .unwrap();
        assert_eq!(
            sdr_report.probe.streams[0].color_transfer.as_deref(),
            Some("bt709")
        );
        let mut sdr_asset = asset.clone();
        sdr_asset.locator.absolute = Some(sdr_output.to_str().unwrap().into());
        sdr_asset.content_hash = content_hash(&sdr_output).unwrap();
        sdr_asset.streams[0].color_primaries = Some("bt709".into());
        sdr_asset.streams[0].color_transfer = Some("bt709".into());
        sdr_asset.streams[0].color_matrix = Some("bt709".into());
        let sdr_image = runtime
            .decode_video_image(
                &sdr_asset,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec709,
            )
            .unwrap();
        assert!((sdr_image.pixels[0][0] - 0.5).abs() < 0.03);
        assert_eq!(
            AvExportSnapshot::with_movie_profile(
                &snapshot,
                kronello_audio::AudioSourceMode::Silence,
                vec![],
                if transfer == HdrTransfer::Pq {
                    MovieProfile::ProResHlgPcm24V1
                } else {
                    MovieProfile::ProResPqPcm24V1
                }
            )
            .unwrap_err()
            .code(),
            "UNSUPPORTED_FEATURE"
        );

        let ramp_path = dir.path().join(format!("ramp-{transfer:?}.mov"));
        let mut ramp = Vec::new();
        for i in 0..1024 {
            let code = ((i as f64 / 1023.0) * 65535.0).round() as u16;
            for _ in 0..3 {
                ramp.extend(code.to_le_bytes());
            }
            ramp.extend(u16::MAX.to_le_bytes());
        }
        runtime
            .encode_hdr_video_stream(
                &EncodeRequest {
                    output: ramp_path.clone(),
                    width: 32,
                    height: 32,
                    ..request.clone()
                },
                1,
                transfer,
                &mut |_| {
                    Ok(EncodeFrame {
                        pts: Time::ZERO,
                        rgba: ramp.clone(),
                    })
                },
            )
            .unwrap();
        let mut ramp_asset = asset.clone();
        ramp_asset.locator.absolute = Some(ramp_path.to_str().unwrap().into());
        ramp_asset.content_hash = content_hash(&ramp_path).unwrap();
        ramp_asset.streams[0].width = Some(32);
        ramp_asset.streams[0].height = Some(32);
        let ramp_image = runtime
            .decode_video_image_with_hdr(
                &ramp_asset,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec2020,
                Some(HdrSettings { transfer }),
            )
            .unwrap();
        let levels: std::collections::BTreeSet<_> =
            ramp_image.pixels.iter().map(|p| p[0].to_bits()).collect();
        assert!(
            levels.len() > 256,
            "native 10-bit must retain more than 8-bit levels: {}",
            levels.len()
        );
        // SDR source white uses the same working unit, hence 203 nits in HDR.
        let sdr_source = dir.path().join(format!("sdr-white-{transfer:?}.mov"));
        runtime
            .encode_video(
                &EncodeRequest {
                    output: sdr_source.clone(),
                    ..request.clone()
                },
                &[EncodeFrame {
                    pts: Time::ZERO,
                    rgba: vec![255; 16 * 16 * 4],
                }],
            )
            .unwrap();
        let mut sdr_white = asset.clone();
        sdr_white.locator.absolute = Some(sdr_source.to_str().unwrap().into());
        sdr_white.content_hash = content_hash(&sdr_source).unwrap();
        sdr_white.streams[0].color_primaries = Some("bt709".into());
        sdr_white.streams[0].color_transfer = Some("bt709".into());
        sdr_white.streams[0].color_matrix = Some("bt709".into());
        let white = runtime
            .decode_video_image_with_hdr(
                &sdr_white,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec2020,
                Some(HdrSettings { transfer }),
            )
            .unwrap();
        assert!((white.pixels[0][0] - 1.0).abs() < 0.02);
        let code = transfer.encode([f64::from(white.pixels[0][0]); 3]).unwrap();
        assert!((transfer.decode(code)[0] * HDR_REFERENCE_WHITE_NITS - 203.0).abs() < 4.0);

        let png_path = dir.path().join(format!("partial-alpha-{transfer:?}.png"));
        let mut encoder = png::Encoder::new(std::fs::File::create(&png_path).unwrap(), 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        encoder.set_source_gamma(png::ScaledFloat::new(1.0));
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 255, 255, 255, 255, 255, 128, 0].repeat(256))
            .unwrap();
        writer.finish().unwrap();
        let mut image_asset = asset.clone();
        image_asset.kind = AssetKind::Image;
        image_asset.content_hash = content_hash(&png_path).unwrap();
        image_asset.locator.absolute = Some(png_path.to_str().unwrap().into());
        let stream = &mut image_asset.streams[0];
        stream.codec = "png".into();
        stream.pixel_format = Some("rgba64be".into());
        stream.color_primaries = Some("bt709".into());
        stream.color_transfer = Some("linear".into());
        stream.color_matrix = Some("gbr".into());
        stream.color_range = Some("pc".into());
        let mut partial_project = project.clone();
        partial_project.assets[0] = DocumentObject::Known(image_asset);
        let partial_snapshot =
            RenderSnapshot::new(&partial_project, id, 1, snapshot.profile()).unwrap();
        let partial = render_frame(
            &partial_snapshot,
            &[],
            &backend,
            FrameRequest {
                time: Time::ZERO,
                region,
            },
        )
        .unwrap();
        let alpha = 32768.0 / 65535.0;
        assert!((partial.pixels.linear[0][0] - alpha).abs() < 1e-6);
        assert_eq!(partial.pixels.linear[0][3], alpha);
        assert!((partial.pixels.display[0][0] - 0.735357).abs() < 1e-4);
        assert_eq!(partial.pixels.display[0][3], alpha);
        let partial_fixed = AvExportSnapshot::with_movie_profile(
            &partial_snapshot,
            kronello_audio::AudioSourceMode::Silence,
            vec![],
            movie,
        )
        .unwrap();
        let partial_output = dir.path().join(format!("partial-final-{transfer:?}.mov"));
        runtime
            .export_av(
                &partial_fixed,
                dir.path(),
                &[],
                &backend,
                &AvExportRequest {
                    output: partial_output.clone(),
                    range: kronello_time::TimeRange::new(Time::ZERO, Rational::new(2, 24).unwrap())
                        .unwrap(),
                    frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                    region,
                    background: [0.0; 3],
                    clipping: kronello_audio::ClippingPolicy::Reject,
                    chapters: kronello_media::ChapterPolicy::Transfer,
                    outputs: Vec::new(),
                },
            )
            .unwrap();
        let mut partial_video = asset.clone();
        partial_video.locator.absolute = Some(partial_output.to_str().unwrap().into());
        partial_video.content_hash = content_hash(&partial_output).unwrap();
        let output_pixels = runtime
            .decode_video_image_with_hdr(
                &partial_video,
                dir.path(),
                0,
                Time::ZERO,
                ColorSpace::LinearRec2020,
                Some(HdrSettings { transfer }),
            )
            .unwrap();
        assert!(
            (output_pixels.pixels[0][0] - alpha).abs() < 0.04,
            "premultiplied HDR alpha must composite once, before transfer"
        );
        // Authored HDR working colors retain negative RGB and >1 values.
        let registry = render_registry();
        let property = |key: &str, value: Value| {
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
                PropertySource::Constant(value),
                vec![],
                &registry,
            )
            .unwrap()
        };
        let size = property(
            "kronello.shape.size",
            Value::Vec2([FiniteF64::new(16.0).unwrap(); 2]),
        );
        let radius = property(
            "kronello.shape.corner_radius",
            Value::Scalar(FiniteF64::new(0.0).unwrap()),
        );
        let fill = property(
            "kronello.fill_color",
            Value::Color(Color::new(ColorSpace::LinearRec2020, [-0.5, 2.0, 5.0], 0.5).unwrap()),
        );
        let shape = Shape {
            id: ContentId::new(),
            geometry: ShapeGeometry::Rectangle {
                size: size.id(),
                corner_radius: radius.id(),
            },
            fill: Some(Fill {
                gradient: None,
                color: fill.id(),
                rule: FillRule::Nonzero,
            }),
            stroke: None,
        };
        let mut negative_project = project.clone();
        negative_project.shapes = vec![DocumentObject::Known(shape.clone())];
        let DocumentObject::Known(composition) = &mut negative_project.compositions[0] else {
            panic!()
        };
        composition.nodes[0].kind = NodeKind::Shape {
            content_ref: shape.id,
        };
        composition.nodes[0].properties = vec![size, radius, fill];
        let negative_snapshot =
            RenderSnapshot::new(&negative_project, id, 1, snapshot.profile()).unwrap();
        let negative = render_frame(
            &negative_snapshot,
            &[],
            &cpu,
            FrameRequest {
                time: Time::ZERO,
                region,
            },
        )
        .unwrap();
        assert_eq!(negative.pixels.linear[0], [-0.25, 1.0, 2.5, 0.5]);
        assert_eq!(negative.pixels.display[0][3], 0.5);
        if test_gpu {
            let gpu = kronello_gpu::GpuContext::new().expect("actual GPU required");
            let actual = render_frame(
                &negative_snapshot,
                &[],
                &gpu,
                FrameRequest {
                    time: Time::ZERO,
                    region,
                },
            )
            .unwrap();
            assert_eq!(actual.pixels.linear[0], negative.pixels.linear[0]);
            let partial_actual = render_frame(
                &partial_snapshot,
                &[],
                &VideoRenderBackend {
                    backend: &gpu,
                    project_path: dir.path(),
                },
                FrameRequest {
                    time: Time::ZERO,
                    region,
                },
            )
            .unwrap();
            for i in 0..4 {
                assert!(
                    (partial_actual.pixels.linear[0][i] - partial.pixels.linear[0][i]).abs()
                        < 1.0 / 1024.0
                );
            }
        }
        let negative_fixed = AvExportSnapshot::with_movie_profile(
            &negative_snapshot,
            kronello_audio::AudioSourceMode::Silence,
            vec![],
            movie,
        )
        .unwrap();
        let negative_output = dir.path().join(format!("reject-negative-{transfer:?}.mov"));
        assert_eq!(
            runtime
                .export_av(
                    &negative_fixed,
                    dir.path(),
                    &[],
                    &cpu,
                    &AvExportRequest {
                        output: negative_output.clone(),
                        range: kronello_time::TimeRange::new(
                            Time::ZERO,
                            Rational::new(2, 24).unwrap()
                        )
                        .unwrap(),
                        frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                        region,
                        background: [0.0; 3],
                        clipping: kronello_audio::ClippingPolicy::Reject,
                        chapters: kronello_media::ChapterPolicy::Transfer,
                        outputs: Vec::new(),
                    }
                )
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        assert!(!negative_output.exists());
        if transfer == HdrTransfer::Hlg {
            let mut saturated_project = negative_project.clone();
            let DocumentObject::Known(composition) = &mut saturated_project.compositions[0] else {
                panic!()
            };
            let color = &mut composition.nodes[0].properties[2];
            *color = Property::new(
                color.id(),
                color.descriptor().clone(),
                PropertySource::Constant(Value::Color(
                    Color::new(ColorSpace::LinearRec2020, [0.0, 0.0, 1000.0 / 203.0], 1.0).unwrap(),
                )),
                vec![],
                &registry,
            )
            .unwrap();
            let saturated =
                RenderSnapshot::new(&saturated_project, id, 1, snapshot.profile()).unwrap();
            let fixed = AvExportSnapshot::with_movie_profile(
                &saturated,
                kronello_audio::AudioSourceMode::Silence,
                vec![],
                movie,
            )
            .unwrap();
            let output = dir.path().join("reject-unrepresentable-saturated-hlg.mov");
            assert_eq!(
                runtime
                    .export_av(
                        &fixed,
                        dir.path(),
                        &[],
                        &cpu,
                        &AvExportRequest {
                            output: output.clone(),
                            range: kronello_time::TimeRange::new(
                                Time::ZERO,
                                Rational::new(2, 24).unwrap()
                            )
                            .unwrap(),
                            frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                            region,
                            background: [0.0; 3],
                            clipping: kronello_audio::ClippingPolicy::Reject,
                            chapters: kronello_media::ChapterPolicy::Transfer,
                            outputs: Vec::new(),
                        }
                    )
                    .unwrap_err()
                    .code(),
                "UNSUPPORTED_FEATURE"
            );
            assert!(!output.exists());
        }
        let mut wrong = asset.clone();
        wrong.streams[0].color_transfer = Some(
            if transfer == HdrTransfer::Pq {
                HdrTransfer::Hlg
            } else {
                HdrTransfer::Pq
            }
            .tag()
            .into(),
        );
        assert_eq!(
            runtime
                .decode_video_image_with_hdr(
                    &wrong,
                    dir.path(),
                    0,
                    Time::ZERO,
                    ColorSpace::LinearRec2020,
                    Some(HdrSettings { transfer })
                )
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        let mut unsupported = request.clone();
        unsupported.output = dir.path().join("unsupported.mov");
        unsupported.codec = EncodeCodec::H264;
        assert_eq!(
            runtime
                .encode_hdr_video_stream(&unsupported, 1, transfer, &mut |_| unreachable!())
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        assert!(!unsupported.output.exists());
    }
}

#[test]
fn native_ten_bit_pq_hlg_precision_and_locked_color() {
    hdr_fixture_roundtrip(false);
}
#[test]
fn native_ten_bit_pq_hlg_actual_gpu_matches_cpu() {
    hdr_fixture_roundtrip(true);
}
