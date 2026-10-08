//! MEDIA-004 (ADR-0133): chapter transfer, one-render multi-output fan-out and
//! the added DNx/GIF/MP3/FLAC delivery profiles against the real FFmpeg runtime.
use kronello_audio::{AudioSourceMode, ClippingPolicy};
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::path::{Path, PathBuf};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn marker(time: Time, role: MarkerRole, title: Option<&str>) -> Marker {
    Marker {
        id: MarkerId::new(),
        time,
        color: MarkerColor::Green,
        role,
        title: title.map(str::to_owned),
        // Comments stay authoring annotations and never cross into containers.
        comment: Some("editorial note".into()),
    }
}
/// A two-second sequence with three chapter markers (one exactly at the
/// content end, outside the exported range) and one non-chapter annotation.
fn project(width: u32, height: u32) -> (Project, SequenceId) {
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(width as f64, height as f64).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![Clip {
                id: ClipId::new(),
                source_ref: SourceRef::Generator {
                    generator: SOLID_GENERATOR_ID.into(),
                    version: 1,
                    color: Color::from_srgb8([40, 90, 120], None),
                },
                timeline_range: range(t(0, 1), t(2, 1)),
                source_in: Time::ZERO,
                time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                enabled: true,
                audio_retime: AudioRetimePolicy::Reject,
                reverse_sampling: None,
                volume: None,
                links: vec![],
                properties: vec![],
                effects: vec![],
                masks: vec![],
                markers: vec![],
            }],
        }],
        transitions: vec![],
        markers: vec![
            marker(t(0, 1), MarkerRole::Chapter, Some("Intro")),
            marker(t(1, 2), MarkerRole::Standard, Some("not a chapter")),
            marker(t(1, 1), MarkerRole::Chapter, Some("Middle")),
            marker(t(2, 1), MarkerRole::Chapter, Some("Outside")),
        ],
        work_area: None,
        targets: None,
    };
    let id = sequence.id;
    let mut project = Project::default();
    project.sequences.push(DocumentObject::Known(sequence));
    (project, id)
}
fn render_at(width: u32, height: u32) -> RenderSnapshot {
    let (project, sequence) = project(width, height);
    RenderSnapshot::for_target(
        &project,
        RenderTarget::Sequence { sequence },
        0,
        RenderProfile::default(),
    )
    .unwrap()
}
fn render() -> RenderSnapshot {
    render_at(64, 64)
}
fn request_at(
    output: PathBuf,
    width: u32,
    height: u32,
    chapters: ChapterPolicy,
    outputs: Vec<DeliveryOutput>,
) -> AvExportRequest {
    let rate = FrameRate::new(24, 1).unwrap();
    AvExportRequest {
        output,
        range: range(t(0, 1), t(2, 1)),
        frame_rate: rate,
        region: OutputRegion {
            origin: [0.0, 0.0],
            extent: [width as f64, height as f64],
            pixels: [width, height],
        },
        background: [0.1, 0.2, 0.3],
        clipping: ClippingPolicy::Reject,
        chapters,
        outputs,
    }
}
fn request(
    output: PathBuf,
    chapters: ChapterPolicy,
    outputs: Vec<DeliveryOutput>,
) -> AvExportRequest {
    request_at(output, 64, 64, chapters, outputs)
}
fn leg(path: &Path, profile: MovieProfile) -> DeliveryOutput {
    DeliveryOutput {
        output: path.to_path_buf(),
        profile,
        background: [0.1, 0.2, 0.3],
        chapters: ChapterPolicy::Transfer,
    }
}
fn leg_snapshot(render: &RenderSnapshot, profile: MovieProfile) -> AvExportSnapshot {
    AvExportSnapshot::with_movie_profile(render, AudioSourceMode::Silence, vec![], profile).unwrap()
}
/// Legacy ProRes/PCM24 defaults through the pre-MEDIA-004 constructor.
fn primary_snapshot(render: &RenderSnapshot) -> AvExportSnapshot {
    AvExportSnapshot::with_audio(render, AudioSourceMode::Silence, vec![]).unwrap()
}
/// Expected chapters for the [0, 2) export range: marker-to-marker, rebased to
/// output time, non-chapter roles and range-end markers excluded.
fn expected_chapters() -> Vec<MediaChapter> {
    let chapter = |id: i64, a: i64, b: i64, title: &str| MediaChapter {
        id,
        start: Rational::new(a, 1).unwrap(),
        end: Rational::new(b, 1).unwrap(),
        title: title.into(),
    };
    vec![chapter(0, 0, 1, "Intro"), chapter(1, 1, 2, "Middle")]
}

#[test]
fn chapter_markers_transfer_into_mov_and_probe_roundtrips() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let render = render();
    // Silent stereo PCM keeps the movie leg's required A/V pair.
    let snapshot = primary_snapshot(&render);
    let output = dir.path().join("chapters.mov");
    let report = runtime
        .export_av(
            &snapshot,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(output.clone(), ChapterPolicy::Transfer, vec![]),
        )
        .unwrap();
    assert_eq!(report.outputs.len(), 1);
    let chapters = &report.outputs[0].chapters;
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "Intro");
    assert_eq!(chapters[1].title, "Middle");
    // Clipped to the range and rebased: [0,1) and [1,2) in output time.
    assert_eq!(chapters[0].start, Rational::ZERO);
    assert_eq!(chapters[0].end, Rational::ONE);
    assert_eq!(chapters[1].start, Rational::ONE);
    assert_eq!(chapters[1].end, Rational::new(2, 1).unwrap());
    let probe = runtime.probe(&output).unwrap();
    probe.verify_chapters(&expected_chapters()).unwrap();
    // Marker colors and comments never reach the container; only titles do.
    assert_eq!(
        probe
            .chapters
            .iter()
            .map(|c| c.title.as_str())
            .collect::<Vec<_>>(),
        ["Intro", "Middle"]
    );
}

#[test]
fn chapters_drop_with_typed_warning_on_incapable_containers() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let render = render();
    // GIF cannot carry chapters: Transfer records a typed leg warning.
    let gif = leg_snapshot(&render, MovieProfile::GifV1);
    let output = dir.path().join("chapters.gif");
    let report = runtime
        .export_delivery(
            &[&gif],
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(output.clone(), ChapterPolicy::Transfer, vec![]),
        )
        .unwrap();
    assert_eq!(report.outputs[0].chapters, vec![]);
    assert_eq!(report.outputs[0].warnings.len(), 1);
    assert_eq!(report.outputs[0].warnings[0].code, "CHAPTERS_DROPPED");
    assert!(runtime.probe(&output).unwrap().chapters.is_empty());
    // Explicit omission suppresses the warning.
    let omitted = dir.path().join("omitted.gif");
    let report = runtime
        .export_delivery(
            &[&gif],
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(omitted.clone(), ChapterPolicy::Omit, vec![]),
        )
        .unwrap();
    assert!(report.outputs[0].warnings.is_empty());
    assert!(runtime.probe(&omitted).unwrap().chapters.is_empty());
}

#[test]
fn one_render_fans_out_to_mov_gif_mp3_and_flac() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let render = render();
    let primary = primary_snapshot(&render);
    let legs = [
        (dir.path().join("movie.gif"), MovieProfile::GifV1),
        (dir.path().join("sound.mp3"), MovieProfile::Mp3V1),
        (dir.path().join("sound.flac"), MovieProfile::FlacV1),
    ];
    let snapshots: Vec<_> = legs
        .iter()
        .map(|(_, profile)| leg_snapshot(&render, *profile))
        .collect();
    let outputs: Vec<_> = legs.iter().map(|(p, profile)| leg(p, *profile)).collect();
    let movie = dir.path().join("movie.mov");
    let all: Vec<&AvExportSnapshot> = std::iter::once(&primary).chain(snapshots.iter()).collect();
    let report = runtime
        .export_delivery(
            &all,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(movie.clone(), ChapterPolicy::Transfer, outputs),
        )
        .unwrap();
    assert_eq!(report.outputs.len(), 4);
    // One render pass fed every leg.
    assert_eq!(report.frames.len(), 48);
    let profiles = [
        MovieProfile::ProResPcm24,
        MovieProfile::GifV1,
        MovieProfile::Mp3V1,
        MovieProfile::FlacV1,
    ];
    let destinations = [
        movie.as_path(),
        legs[0].0.as_path(),
        legs[1].0.as_path(),
        legs[2].0.as_path(),
    ];
    for (index, leg) in report.outputs.iter().enumerate() {
        assert_eq!(leg.output, destinations[index]);
        assert_eq!(leg.profile, profiles[index]);
        leg.probe
            .verify_delivery(profiles[index], ChannelMask::STEREO)
            .unwrap();
        // Elementary containers carry no embedded snapshot identity; only
        // muxed movie legs bind the hashes into container metadata.
        if profiles[index].shape() == DeliveryShape::Movie {
            assert_eq!(leg.probe.render_snapshot_hash, report.render_snapshot_hash);
            assert_eq!(
                leg.probe.export_snapshot_hash,
                all[index].content_hash().unwrap()
            );
        }
        assert!(destinations[index].is_file());
    }
    // Chapters land only in the MOV leg; the rest warn (Transfer policy).
    assert_eq!(report.outputs[0].chapters.len(), 2);
    report.outputs[0]
        .probe
        .verify_chapters(&expected_chapters())
        .unwrap();
    for leg in &report.outputs[1..] {
        assert!(leg.chapters.is_empty());
        assert_eq!(leg.warnings.len(), 1);
        assert_eq!(leg.warnings[0].code, "CHAPTERS_DROPPED");
    }
}

#[test]
fn legs_validate_before_render_and_never_publish_partially() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let render = render();
    let primary = primary_snapshot(&render);
    let gif = leg_snapshot(&render, MovieProfile::GifV1);
    let snapshots = [&primary, &gif];
    let movie = dir.path().join("movie.mov");
    let gif_out = dir.path().join("movie.gif");
    // An existing leg destination fails before the first frame renders.
    std::fs::write(&gif_out, b"occupied").unwrap();
    let error = runtime
        .export_delivery(
            &snapshots,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(
                movie.clone(),
                ChapterPolicy::Transfer,
                vec![leg(&gif_out, MovieProfile::GifV1)],
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), "OUTPUT_EXISTS");
    assert!(!movie.exists());
    // Duplicate destinations across legs are rejected outright.
    let collision = runtime
        .export_delivery(
            &snapshots,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(
                movie.clone(),
                ChapterPolicy::Transfer,
                vec![leg(&movie, MovieProfile::GifV1)],
            ),
        )
        .unwrap_err();
    assert_eq!(collision.code(), "INVALID_MEDIA_INPUT");
    assert!(!movie.exists());
    // A wrong-container leg extension is rejected before rendering.
    let wrong = dir.path().join("wrong.mov");
    let extension = runtime
        .export_delivery(
            &snapshots,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(
                movie.clone(),
                ChapterPolicy::Transfer,
                vec![leg(&wrong, MovieProfile::GifV1)],
            ),
        )
        .unwrap_err();
    assert_eq!(extension.code(), "INVALID_MEDIA_INPUT");
    // A mid-render failure publishes neither leg.
    std::fs::remove_file(&gif_out).unwrap();
    let error = runtime
        .export_delivery_with_checkpoint(
            &snapshots,
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(
                movie.clone(),
                ChapterPolicy::Transfer,
                vec![leg(&gif_out, MovieProfile::GifV1)],
            ),
            &mut |frame| {
                if frame == 3 {
                    Err(MediaError::Encode("injected cancellation".into()))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENCODE_ERROR");
    assert!(!movie.exists());
    assert!(!gif_out.exists());
}

#[test]
fn dnx_mov_and_mxf_profiles_encode_and_probe() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // The native dnxhd encoder refuses rasters below its minimum geometry
    // even for DNxHR profiles, so this leg renders at a supported size.
    let render = render_at(640, 360);
    for (name, profile, chapter_capable) in [
        ("hq.mov", MovieProfile::DnxhrHqMovPcm24V1, true),
        ("hqx.mov", MovieProfile::DnxhrHqxMovPcm24V1, true),
        ("sq.mxf", MovieProfile::DnxhrSqMxfPcm24V1, false),
    ] {
        let snapshot = leg_snapshot(&render, profile);
        let output = dir.path().join(name);
        let report = runtime
            .export_delivery(
                &[&snapshot],
                Path::new("absent.kronello"),
                &[],
                &CpuReferenceBackend,
                &request_at(output.clone(), 640, 360, ChapterPolicy::Transfer, vec![]),
            )
            .unwrap();
        report
            .probe
            .verify_delivery(profile, ChannelMask::STEREO)
            .unwrap();
        let video = report
            .probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Video)
            .unwrap();
        assert_eq!(video.codec, "dnxhd");
        if chapter_capable {
            // MOV carries the authored chapter markers.
            assert_eq!(report.outputs[0].chapters.len(), 2, "{name}");
            report.probe.verify_chapters(&expected_chapters()).unwrap();
        } else {
            // MXF cannot carry chapters: a typed warning records the drop.
            assert_eq!(report.outputs[0].chapters, Vec::new(), "{name}");
            assert_eq!(report.outputs[0].warnings[0].code, "CHAPTERS_DROPPED");
        }
    }
    // DNxHD (not HR) is locked to the CID raster table by the encoder; an
    // off-table 640x360 raster is a typed encode failure, never a fallback.
    let snapshot = leg_snapshot(&render, MovieProfile::DnxhdMovPcm24V1);
    let invalid = dir.path().join("invalid.mov");
    let error = runtime
        .export_delivery(
            &[&snapshot],
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request_at(invalid.clone(), 640, 360, ChapterPolicy::Transfer, vec![]),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENCODE_ERROR");
    assert!(!invalid.exists());
}

#[test]
fn mp3_and_flac_elementary_deliveries_probe_and_verify() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let render = render();
    for (name, profile, codec) in [
        ("out.mp3", MovieProfile::Mp3V1, "mp3"),
        ("out.flac", MovieProfile::FlacV1, "flac"),
    ] {
        let snapshot = leg_snapshot(&render, profile);
        let output = dir.path().join(name);
        let report = runtime
            .export_delivery(
                &[&snapshot],
                Path::new("absent.kronello"),
                &[],
                &CpuReferenceBackend,
                &request(output.clone(), ChapterPolicy::Transfer, vec![]),
            )
            .unwrap();
        report
            .probe
            .verify_delivery(profile, ChannelMask::STEREO)
            .unwrap();
        assert_eq!(report.probe.streams.len(), 1);
        assert_eq!(report.probe.streams[0].codec, codec);
        assert_eq!(report.probe.streams[0].sample_rate, Some(48_000));
        assert_eq!(report.probe.streams[0].channels, Some(2));
        assert!(report.outputs[0].video.is_none());
        assert!(report.outputs[0].audio.is_some());
    }
    // MP3 is mono/stereo only; a 5.1 layout is a typed rejection.
    let wide = AvExportSnapshot::with_audio_layout(
        &render,
        AudioSourceMode::Silence,
        vec![],
        Some(MovieProfile::Mp3V1),
        ChannelMask::SURROUND_5_1,
    )
    .unwrap();
    let output = dir.path().join("wide.mp3");
    let error = runtime
        .export_delivery(
            &[&wide],
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(output.clone(), ChapterPolicy::Omit, vec![]),
        )
        .unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_CHANNEL_LAYOUT");
    assert!(!output.exists());
    // FLAC keeps the closed multichannel layouts.
    let wide = AvExportSnapshot::with_audio_layout(
        &render,
        AudioSourceMode::Silence,
        vec![],
        Some(MovieProfile::FlacV1),
        ChannelMask::SURROUND_5_1,
    )
    .unwrap();
    let output = dir.path().join("wide.flac");
    let report = runtime
        .export_delivery(
            &[&wide],
            Path::new("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request(output.clone(), ChapterPolicy::Omit, vec![]),
        )
        .unwrap();
    report
        .probe
        .verify_delivery(MovieProfile::FlacV1, ChannelMask::SURROUND_5_1)
        .unwrap();
}

#[test]
fn missing_closed_profile_encoders_are_typed_capability_failures() {
    let capabilities = || MediaRuntime::load().unwrap().capabilities().clone();
    // DNxHD/DNxHR profiles resolve through the codec-class selector.
    let mut missing = capabilities();
    missing.codecs.retain(|c| c.name != "dnxhd");
    let error = missing.select_encoder(EncodeCodec::Dnx).unwrap_err();
    assert_eq!(error.code(), "ENCODER_UNAVAILABLE");
    // GIF, MP3 and FLAC are closed-profile encoders resolved by exact name.
    for name in ["gif", "libmp3lame", "flac"] {
        let mut missing = capabilities();
        missing.codecs.retain(|c| c.name != name);
        let error = missing.require_encoder(name).unwrap_err();
        assert_eq!(error.code(), "ENCODER_UNAVAILABLE", "{name}");
    }
}
