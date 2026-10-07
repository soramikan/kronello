//! ADR-0119 proxy transcode acceptance on the real encoder path.
use kronello_media::*;
use kronello_time::Rational;

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}

/// ProRes source with a fixed per-frame pattern (luma ramp + frame index).
fn write_source(path: &std::path::Path, count: usize) -> MediaPathReport {
    let runtime = MediaRuntime::load().unwrap();
    runtime
        .encode_video_stream(
            &EncodeRequest {
                output: path.into(),
                codec: EncodeCodec::ProRes,
                width: 32,
                height: 24,
                time_base: r(1, 24),
            },
            count,
            &mut |index| {
                let mut rgba = vec![0u8; 32 * 24 * 4];
                for (p, pixel) in rgba.chunks_exact_mut(4).enumerate() {
                    let (x, y) = (p as u32 % 32, p as u32 / 32);
                    pixel[0] = ((x * 8 + index as u32 * 16) % 256) as u8;
                    pixel[1] = ((y * 10) % 256) as u8;
                    pixel[2] = ((x + y) % 256) as u8;
                    pixel[3] = 255;
                }
                Ok(EncodeFrame {
                    pts: r(index as i64, 24),
                    rgba,
                })
            },
        )
        .unwrap()
}

#[test]
fn encode_proxy_produces_verified_half_scale_prores() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mov");
    write_source(&source, 4);
    let runtime = MediaRuntime::load().unwrap();
    let output = dir.path().join("proxy.mov");
    let mut progressed = Vec::new();
    let result = runtime
        .encode_proxy(&source, 0, 16, 12, &output, &mut |frames| {
            progressed.push(frames);
            Ok(())
        })
        .unwrap();
    assert_eq!(result.frames, 4);
    assert_eq!((result.width, result.height), (16, 12));
    assert_eq!(result.time_base, r(1, 24));
    assert_eq!(result.duration, Some(r(4, 24)));
    assert_eq!(result.content_hash, content_hash(&output).unwrap());
    assert_eq!(progressed, vec![1, 2, 3, 4]);
    // Existing destinations never clobber.
    assert!(matches!(
        runtime.encode_proxy(&source, 0, 16, 12, &output, &mut |_| Ok(())),
        Err(MediaError::OutputExists(_))
    ));
    // The proxy decodes back through the same sequential RGBA8 path.
    let mut decoder = runtime.open_video_stream(&output, 0).unwrap();
    let mut pts = Vec::new();
    while let Some(frame) = decoder.next_rgba().unwrap() {
        assert_eq!((frame.width, frame.height), (16, 12));
        assert_eq!(frame.rgba.len(), 16 * 12 * 4);
        assert!(frame.rgba.chunks_exact(4).all(|p| p[3] == 255));
        pts.push(frame.pts);
    }
    assert_eq!(pts, (0..4).map(|i| r(i, 24)).collect::<Vec<_>>());
    // Source still decodes at full resolution through next_rgba.
    let mut full = runtime.open_video_stream(&source, 0).unwrap();
    let first = full.next_rgba().unwrap().unwrap();
    assert_eq!((first.width, first.height), (32, 24));
}

#[test]
fn export_snapshot_rejects_preview_proxy_mode() {
    // A Prefer-mode snapshot can never enter the export path (ADR-0119).
    let composition = kronello_model::CompositionId::new();
    let document = kronello_model::Project {
        compositions: vec![kronello_model::DocumentObject::Known(
            kronello_model::Composition {
                id: composition,
                duration: kronello_time::Duration::new(r(1, 1)).unwrap(),
                design_extent: kronello_model::DesignExtent::new(16.0, 16.0).unwrap(),
                edit_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
                root_nodes: vec![],
                nodes: vec![],
                properties: vec![],
            },
        )],
        ..Default::default()
    };
    let snapshot =
        kronello_render::RenderSnapshot::new(&document, composition, 1, Default::default())
            .unwrap()
            .with_media_proxies(kronello_render::MediaProxyMode::Prefer);
    let error = AvExportSnapshot::new(&snapshot, vec![]).unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
}

#[test]
fn scale_rgba8_is_deterministic_and_opaque() {
    // 4x2 gradient → 2x1.
    let mut src = Vec::new();
    for y in 0..2u8 {
        for x in 0..4u8 {
            src.extend_from_slice(&[x * 40, y * 100, 200, 255]);
        }
    }
    let a = scale_rgba8(&src, 4, 2, 2, 1).unwrap();
    let b = scale_rgba8(&src, 4, 2, 2, 1).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 8);
    assert!(a.chunks_exact(4).all(|p| p[3] == 255));
    // Edge-to-edge: dst x=0 maps to src x=0; dst x=1 maps to src x=3.
    assert_eq!(a[0], 0);
    assert_eq!(a[4], 120);
    // Identity is a pass-through copy.
    assert_eq!(scale_rgba8(&src, 4, 2, 4, 2).unwrap(), src);
    for (w, h, dw, dh, len) in [(0, 2, 2, 1, 32), (4, 2, 0, 1, 32), (4, 2, 2, 1, 8)] {
        assert!(scale_rgba8(&src[..len], w, h, dw, dh).is_err());
    }
}
