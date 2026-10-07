//! Retained delivery artifacts and reports for explicit CPU/host acceptance.
use kronello_audio::{
    AudioSourceMode, AudioTarget, ClippingPolicy, DocumentAudioPlan, SAMPLE_RATE,
};
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::{
    AvExportRequest, AvExportSnapshot, ExecutionKind, MediaRuntime, MovieProfile,
};
use kronello_model::*;
use kronello_render::{OutputRegion, RenderSnapshot};
use kronello_time::{FrameRate, Rational, TimeMap, TimeRange};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = std::path::PathBuf::from(args.next().ok_or("fresh output directory required")?);
    let profile = match args.next().as_deref() {
        Some("av1") => MovieProfile::Av1Mp4AlacV1,
        Some("h264") => MovieProfile::H264AlacV1,
        Some("hevc") => MovieProfile::HevcAlacV1,
        _ => return Err("closed profile argument: av1 | h264 | hevc".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    std::fs::create_dir(&output)?;
    let runtime = MediaRuntime::load()?;
    runtime.capabilities().verify_distribution()?;
    let rate = FrameRate::new(30000, 1001)?;
    let range = TimeRange::new(Rational::ZERO, rate.frame_to_time(3)?)?;
    let sequence = SequenceId::new();
    let project = Project {
        sequences: vec![DocumentObject::Known(Sequence {
            id: sequence,
            extent: DesignExtent::new(64.0, 64.0)?,
            frame_rate: rate,
            audio_rate: SAMPLE_RATE,
            working_space: ColorSpace::LinearRec709,
            transitions: vec![],
            tracks: vec![Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Audio,
                clips: vec![Clip {
                    id: ClipId::new(),
                    source_ref: SourceRef::Generator {
                        generator: "kronello.audio.tone440".into(),
                        version: 1,
                        color: Color::from_srgb8([0; 3], None),
                    },
                    timeline_range: range,
                    source_in: Rational::ZERO,
                    time_map: TimeMap::linear(Rational::ZERO, Rational::ONE)?,
                    enabled: true,
                    audio_retime: Default::default(),
                    reverse_sampling: None,
                    volume: None,
                    links: vec![],
                    effects: vec![],
                    properties: vec![],
                    markers: vec![],
                }],
            }],
            markers: vec![],
            work_area: None,
            targets: None,
        })],
        ..Project::default()
    };
    let render = RenderSnapshot::for_target(
        &project,
        kronello_render::RenderTarget::Sequence { sequence },
        0,
        Default::default(),
    )?;
    let snapshot =
        AvExportSnapshot::with_movie_profile(&render, AudioSourceMode::Document, vec![], profile)?;
    let request = AvExportRequest {
        output: output.join(if profile == MovieProfile::Av1Mp4AlacV1 {
            "delivery.mp4"
        } else {
            "delivery.mov"
        }),
        range,
        frame_rate: rate,
        region: OutputRegion {
            origin: [0.0, 0.0],
            extent: [64.0, 64.0],
            pixels: [64, 64],
        },
        background: [0.1, 0.2, 0.3],
        clipping: ClippingPolicy::Reject,
    };
    let report = runtime.export_av(
        &snapshot,
        Path::new("unused.kronello"),
        &[],
        &CpuReferenceBackend,
        &request,
    )?;
    report.probe.verify_movie(profile)?;
    assert_eq!(
        report.video.execution,
        if profile == MovieProfile::Av1Mp4AlacV1 {
            ExecutionKind::Software
        } else {
            ExecutionKind::Hardware
        }
    );
    let bus = DocumentAudioPlan::compile_version(&project, AudioTarget::Sequence(sequence), 2)?
        .mix(&Default::default(), range)?;
    let expected = bus.quantize_pcm24(ClippingPolicy::Reject)?;
    let decoded = runtime.decode_audio(&request.output, 1)?;
    assert_eq!(decoded.source_start, Rational::ZERO);
    assert_eq!(decoded.buffer.frames().len(), 4804);
    for (sample, pcm) in decoded
        .buffer
        .frames()
        .iter()
        .flatten()
        .zip(expected.samples)
    {
        assert_eq!(sample.to_bits(), ((pcm / 256) as f32 / 8388608.0).to_bits());
    }
    let mut decoder = runtime.open_video(&request.output)?;
    for i in 0..3 {
        let frame = decoder.decode_at(rate.frame_to_time(i)?)?;
        assert_eq!(frame.pts, rate.frame_to_time(i)?);
        assert_eq!(frame.end, rate.frame_to_time(i + 1)?);
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"capabilities":runtime.capabilities(),"report":report})
        )?
    );
    Ok(())
}
