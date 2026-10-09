//! MEDIA-005 / ADR-0136: camera RAW detection, typed vendor rejection, LibRaw
//! stills, CinemaDNG sequences and the ProRes RAW macOS boundary.
use kronello_media::*;
use kronello_model::{Asset, AssetId, AssetKind, AssetLocator, StreamMetadata};
use kronello_testkit::rawmedia;
use kronello_time::{FrameInterpolation, OpticalFlowConfig, Rational};
use std::path::{Path, PathBuf};

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn locked_stream(codec: &str, width: u32, height: u32) -> StreamMetadata {
    StreamMetadata {
        index: 0,
        codec: codec.into(),
        time_base: Rational::new(1, 1).unwrap(),
        duration: None,
        start_time: None,
        width: Some(width),
        height: Some(height),
        pixel_format: Some(RAW_PIXEL_FORMAT_BAYER.into()),
        color_primaries: Some("bt709".into()),
        color_transfer: Some("linear".into()),
        color_matrix: Some("gbr".into()),
        color_range: Some("pc".into()),
    }
}

fn image_asset(path: &Path, codec: &str, width: u32, height: u32) -> Asset {
    Asset {
        id: AssetId::new(),
        kind: AssetKind::Image,
        content_hash: content_hash(path).unwrap(),
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_str().unwrap().into()),
        },
        streams: vec![locked_stream(codec, width, height)],
    }
}

#[test]
fn detection_claims_camera_raw_formats_only() {
    let dir = tempfile::tempdir().unwrap();
    let dng = write(dir.path(), "still.dng", &rawmedia::dng_bayer16(32, 32));
    assert_eq!(
        sniff_camera_raw(&dng).unwrap(),
        Some(CameraRawDetection::LibRawStill("dng"))
    );
    let braw = write(dir.path(), "clip.braw", &rawmedia::braw_fixture());
    assert_eq!(
        sniff_camera_raw(&braw).unwrap(),
        Some(CameraRawDetection::Braw)
    );
    let r3d = write(dir.path(), "clip.r3d", &rawmedia::r3d_fixture());
    assert_eq!(
        sniff_camera_raw(&r3d).unwrap(),
        Some(CameraRawDetection::R3d)
    );
    let aprn = write(
        dir.path(),
        "clip.mov",
        &rawmedia::prores_raw_mov(b"aprn", 64, 64, 24000, 1001, 24),
    );
    assert_eq!(
        sniff_camera_raw(&aprn).unwrap(),
        Some(CameraRawDetection::ProResRaw("prores_raw"))
    );
    let aprh = write(
        dir.path(),
        "clip2.mov",
        &rawmedia::prores_raw_mov(b"aprh", 64, 64, 24, 1, 24),
    );
    assert_eq!(
        sniff_camera_raw(&aprh).unwrap(),
        Some(CameraRawDetection::ProResRaw("prores_raw_hq"))
    );
    // A normal non-RAW QuickTime is not claimed.
    let avc1 = write(
        dir.path(),
        "plain.mov",
        &rawmedia::prores_raw_mov(b"avc1", 64, 64, 24, 1, 24),
    );
    assert_eq!(sniff_camera_raw(&avc1).unwrap(), None);
}

#[test]
fn vendor_formats_are_typed_rejections_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("clip.braw", rawmedia::braw_fixture()),
        ("clip.r3d", rawmedia::r3d_fixture()),
    ] {
        let path = write(dir.path(), name, &bytes);
        let runtime = MediaRuntime::load().unwrap();
        let error = runtime.open_video(&path).err().unwrap();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE", "{name}: {error}");
        let error = runtime.probe(&path).unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE", "{name}: {error}");
    }
    // A DNG still reached through the video door is also a typed rejection,
    // never an FFmpeg attempt.
    let dng = write(dir.path(), "still.dng", &rawmedia::dng_bayer16(32, 32));
    let runtime = MediaRuntime::load().unwrap();
    assert_eq!(
        runtime.open_video(&dng).err().unwrap().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn raw_still_decode_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "still.dng", &rawmedia::dng_bayer16(32, 32));
    let asset = image_asset(&path, "dng", 32, 32);
    if !libraw_available() {
        assert_eq!(
            decode_image_asset(
                &asset,
                dir.path(),
                0,
                kronello_model::ColorSpace::LinearRec2020
            )
            .unwrap_err()
            .code(),
            "UNSUPPORTED_FEATURE"
        );
        return;
    }
    let image = decode_image_asset(
        &asset,
        dir.path(),
        0,
        kronello_model::ColorSpace::LinearRec2020,
    )
    .unwrap();
    assert_eq!(image.size, [32, 32]);
    assert_eq!(image.pixels.len(), 32 * 32);
    let again = decode_image_asset(
        &asset,
        dir.path(),
        0,
        kronello_model::ColorSpace::LinearRec2020,
    )
    .unwrap();
    assert_eq!(
        image.pixels, again.pixels,
        "LibRaw decode must be deterministic"
    );
    // Scene-referred linear output: nonzero, finite, opaque.
    for p in &image.pixels {
        assert!(p[0].is_finite() && p[1].is_finite() && p[2].is_finite());
        assert_eq!(p[3], 1.0);
    }
    assert!(
        image
            .pixels
            .iter()
            .any(|p| p[0] > 0.0 || p[1] > 0.0 || p[2] > 0.0)
    );
}

#[test]
fn raw_still_metadata_is_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "still.dng", &rawmedia::dng_bayer16(32, 32));
    if libraw_available() {
        let info = probe_raw_still(&path).unwrap();
        assert_eq!((info.width, info.height), (32, 32));
        assert_eq!(info.pixel_format, RAW_PIXEL_FORMAT_BAYER);
        assert_ne!(info.dng_version, 0);
        assert_eq!(info.make, "Kronello");
        assert_eq!(info.model, "Synthetic DNG");
    }
    // Probe never touches FFmpeg for camera RAW.
    let runtime = MediaRuntime::load().unwrap();
    let probe = runtime.probe(&path).unwrap();
    assert_eq!(probe.streams[0].codec, "dng");
    assert_eq!(probe.streams[0].kind, StreamKind::Other);
    assert_eq!(probe.streams[0].width, Some(32));
    assert_eq!(
        probe.streams[0].pixel_format.as_deref(),
        Some(RAW_PIXEL_FORMAT_BAYER)
    );
}

#[test]
fn raw_still_rejects_locked_metadata_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "still.dng", &rawmedia::dng_bayer16(32, 32));
    // Locked codec claims DNG but content is not RAW at all.
    let mut asset = image_asset(&path, "dng", 64, 64);
    if libraw_available() {
        assert_eq!(
            decode_image_asset(
                &asset,
                dir.path(),
                0,
                kronello_model::ColorSpace::LinearRec2020
            )
            .unwrap_err()
            .code(),
            "INVALID_MEDIA_INPUT"
        );
    }
    // Hash mismatch is pinned through resolve_asset.
    asset.streams[0].width = Some(32);
    asset.streams[0].height = Some(32);
    asset.content_hash = "0".repeat(64);
    assert_eq!(
        decode_image_asset(
            &asset,
            dir.path(),
            0,
            kronello_model::ColorSpace::LinearRec2020
        )
        .unwrap_err()
        .code(),
        "ASSET_HASH_MISMATCH"
    );
}

#[test]
fn compressed_dng_rejection_follows_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    // Compression=7 is JPEG-in-DNG, rejected when LibRaw lacks JPEG support.
    let path = write(
        dir.path(),
        "compressed.dng",
        &rawmedia::dng_bayer16_with_compression(32, 32, 7),
    );
    assert_eq!(
        sniff_camera_raw(&path).unwrap(),
        Some(CameraRawDetection::LibRawStill("dng"))
    );
    let asset = image_asset(&path, "dng", 32, 32);
    let result = decode_image_asset(
        &asset,
        dir.path(),
        0,
        kronello_model::ColorSpace::LinearRec2020,
    );
    const LIBRAW_CAPS_JPEG: u32 = 1 << 7;
    if libraw_capabilities() & LIBRAW_CAPS_JPEG == 0 {
        // Vendored configuration (JPEG disabled): typed rejection, always.
        let error = result.unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
        assert!(error.to_string().contains("JPEG"), "{error}");
    } else {
        // Development LibRaw may attempt the decode; the fake strip data then
        // fails deterministically inside LibRaw — never a panic, never silent.
        if let Err(error) = result {
            assert!(matches!(
                error.code(),
                "DECODE_ERROR" | "UNSUPPORTED_FEATURE"
            ));
        }
    }
}

#[test]
fn cinemadng_sequence_selects_and_verifies_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut members = Vec::new();
    for (index, seed) in [1u32, 2, 3].iter().enumerate() {
        members.push(write(
            dir.path(),
            &format!("take{:04}.dng", index + 1),
            &rawmedia::dng_bayer16_seeded(32, 32, 1, *seed),
        ));
    }
    let frame = Rational::new(1, 24).unwrap();
    let probe = probe_cinemadng_sequence(&members[0], frame)
        .unwrap()
        .unwrap();
    assert_eq!(probe.manifest.members.len(), 3);
    assert_eq!((probe.width, probe.height), (32, 32));
    assert_eq!(probe.duration, Rational::new(3, 24).unwrap());
    let mut stream = locked_stream(CINEMADNG_CODEC, 32, 32);
    stream.time_base = frame;
    stream.duration = Some(probe.duration);
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: probe.content_hash.clone(),
        locator: AssetLocator {
            relative: None,
            absolute: Some(members[0].to_str().unwrap().into()),
        },
        streams: vec![stream],
    };
    if !libraw_available() {
        assert_eq!(
            decode_video_dispatch(
                &asset,
                dir.path(),
                0,
                Rational::ZERO,
                kronello_model::ColorSpace::LinearRec2020,
                false,
                None,
            )
            .unwrap_err()
            .code(),
            "UNSUPPORTED_FEATURE"
        );
        return;
    }
    // Frames decode from the numbered members; seeds differ so the picked
    // frame is observable.
    let first = decode_video_dispatch(
        &asset,
        dir.path(),
        0,
        Rational::ZERO,
        kronello_model::ColorSpace::LinearRec2020,
        false,
        None,
    )
    .unwrap();
    let third = decode_video_dispatch(
        &asset,
        dir.path(),
        0,
        Rational::new(2, 24).unwrap(),
        kronello_model::ColorSpace::LinearRec2020,
        false,
        None,
    )
    .unwrap();
    assert_eq!(first.size, [32, 32]);
    assert_ne!(
        first.pixels, third.pixels,
        "distinct members must decode distinct frames"
    );
    // Beyond the last frame is a typed FRAME_NOT_FOUND.
    assert_eq!(
        decode_video_dispatch(
            &asset,
            dir.path(),
            0,
            Rational::new(3, 24).unwrap(),
            kronello_model::ColorSpace::LinearRec2020,
            false,
            None,
        )
        .unwrap_err()
        .code(),
        "FRAME_NOT_FOUND"
    );
    // Interpolation is unsupported on the RAW path.
    let interpolation = FrameInterpolation::OpticalFlow(OpticalFlowConfig {
        block_radius: 2,
        search_radius: 4,
        levels: 2,
        confidence_floor: Rational::new(1, 2).unwrap(),
        max_low_confidence: Rational::new(1, 4).unwrap(),
        flow_fallback: None,
    });
    assert_eq!(
        decode_video_dispatch(
            &asset,
            dir.path(),
            0,
            Rational::ZERO,
            kronello_model::ColorSpace::LinearRec2020,
            false,
            Some(interpolation),
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
    // A modified member breaks the manifest hash before decode.
    let mut corrupted = asset.clone();
    std::fs::write(&members[1], rawmedia::dng_bayer16_seeded(32, 32, 1, 9)).unwrap();
    corrupted.content_hash = probe.content_hash.clone(); // registration hash pinned before corruption
    assert_eq!(
        decode_video_dispatch(
            &corrupted,
            dir.path(),
            0,
            Rational::ZERO,
            kronello_model::ColorSpace::LinearRec2020,
            false,
            None,
        )
        .unwrap_err()
        .code(),
        "ASSET_HASH_MISMATCH"
    );
}

#[test]
fn prores_raw_is_content_verified_and_hardware_gated() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "clip.mov",
        &rawmedia::prores_raw_mov(b"aprn", 64, 64, 24000, 1001, 24),
    );
    // Probe reports the rgba64h contract, not FFmpeg's opinion.
    let runtime = MediaRuntime::load().unwrap();
    let probe = runtime.probe(&path).unwrap();
    assert_eq!(probe.streams[0].codec, "prores_raw");
    assert_eq!(probe.streams[0].kind, StreamKind::Video);
    assert_eq!(
        probe.streams[0].pixel_format.as_deref(),
        Some(PRORES_RAW_PIXEL_FORMAT)
    );
    let mut stream = locked_stream("prores_raw", 64, 64);
    stream.pixel_format = Some(PRORES_RAW_PIXEL_FORMAT.into());
    stream.time_base = Rational::new(1, 24000).unwrap();
    stream.duration = probe.streams[0].duration;
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: content_hash(&path).unwrap(),
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_str().unwrap().into()),
        },
        streams: vec![stream],
    };
    // The fixture has no real compressed samples: decode must produce a typed
    // error (capability or decode), never a panic or silent fallback.
    let error = decode_video_dispatch(
        &asset,
        dir.path(),
        0,
        Rational::ZERO,
        kronello_model::ColorSpace::LinearRec2020,
        false,
        None,
    )
    .unwrap_err();
    assert!(
        matches!(error.code(), "UNSUPPORTED_FEATURE" | "DECODE_ERROR"),
        "{error}"
    );
    if !prores_raw_hardware_supported() {
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    }
    // Locked codec claims ProRes RAW but the container is a normal codec:
    // content verification rejects before decode.
    let plain = write(
        dir.path(),
        "plain.mov",
        &rawmedia::prores_raw_mov(b"avc1", 64, 64, 24000, 1001, 24),
    );
    let mut wrong = asset.clone();
    wrong.locator.absolute = Some(plain.to_str().unwrap().into());
    wrong.content_hash = content_hash(&plain).unwrap();
    assert_eq!(
        decode_video_dispatch(
            &wrong,
            dir.path(),
            0,
            Rational::ZERO,
            kronello_model::ColorSpace::LinearRec2020,
            false,
            None,
        )
        .unwrap_err()
        .code(),
        "INVALID_MEDIA_INPUT"
    );
    // FFmpeg must never see the file.
    assert_eq!(
        runtime.open_video(&path).err().unwrap().code(),
        "UNSUPPORTED_FEATURE"
    );
}
