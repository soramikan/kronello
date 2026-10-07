//! SUB-002 caption sidecar interchange: strict SRT/WebVTT/ITT parsing,
//! deterministic serialization, and import/export through the shared
//! plan/apply edit path.
use kronello_model::*;
use kronello_render::{OutputRegion, RenderProfile};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::path::PathBuf;
use uuid::Uuid;

fn font() -> FontRef {
    FontRef {
        family: "TestSans".into(),
        postscript_name: "TestSans-Regular".into(),
        sha256: "a".repeat(64),
        face_index: 0,
    }
}
fn style() -> CaptionStyle {
    CaptionStyle {
        font: font(),
        size: FiniteF64::new(24.0).unwrap(),
        fill: Color::from_srgb8([255, 255, 255], None),
        outline: None,
        background: None,
    }
}
fn engine() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup() -> (tempfile::TempDir, PathBuf, SequenceId) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captions.kronello");
    let sequence = SequenceId::new();
    let service = engine();
    let ResultData::Project(_) = service
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: Project::default(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(_) = service
        .dispatch(Request::SequenceCreate(SequenceCreateRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "seq".into(),
            sequence: Sequence {
                id: sequence,
                extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                audio_rate: SampleRate::HZ_48000,
                working_space: ColorSpace::LinearRec709,
                tracks: vec![],
                transitions: vec![],
                markers: vec![],
                work_area: None,
                targets: None,
            },
        }))
        .unwrap()
    else {
        panic!()
    };
    (dir, path, sequence)
}
fn export_revision(path: &std::path::Path) -> String {
    let ResultData::Export(result) = engine()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.to_path_buf(),
        }))
        .unwrap()
    else {
        panic!()
    };
    result.revision
}
fn import_request(
    path: &std::path::Path,
    sequence: SequenceId,
    format: CaptionFormat,
    content: &str,
    cue_ids: Vec<CaptionCueIds>,
) -> CaptionsImportPlanRequest {
    CaptionsImportPlanRequest {
        project: path.to_path_buf(),
        base_revision: export_revision(path),
        sequence,
        track: TrackId::new(),
        format,
        content: content.into(),
        style: style(),
        cue_ids,
    }
}
fn cue_ids(n: usize) -> Vec<CaptionCueIds> {
    (0..n)
        .map(|_| CaptionCueIds {
            caption: CaptionId::new(),
            clip: ClipId::new(),
        })
        .collect()
}
fn expect_error(request: Request) -> ServiceError {
    let Response::Error { error } = engine().execute(request) else {
        panic!("expected a typed error")
    };
    error
}

// ------------------------------------------------------------- parsers -----

#[test]
fn srt_parses_timing_markup_entities_and_japanese() {
    let cues = kronello_service::parse(
        "1\n00:00:01,000 --> 00:00:02,500\n<i>こんにちは</i> &amp; more\nsecond line\n\n00:00:03,000 --> 00:00:04,000\nplain\n",
        CaptionFormat::Srt,
    )
    .unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start, Time::new(1, 1).unwrap());
    assert_eq!(cues[0].end, Time::new(5, 2).unwrap());
    assert_eq!(cues[0].text, "こんにちは & more\nsecond line");
    assert_eq!(cues[0].spans.len(), 1);
    assert_eq!(
        cues[0].spans[0].range,
        TextRange {
            start: 0,
            end: "こんにちは".len()
        }
    );
    assert_eq!(cues[0].spans[0].italic, Some(true));
    assert_eq!(cues[1].text, "plain");
}

#[test]
fn srt_rejects_malformed_timing_markup_and_overlap() {
    for (input, code) in [
        // end before start
        (
            "1\n00:00:02,000 --> 00:00:01,000\nx\n",
            "INVALID_CAPTION_FORMAT",
        ),
        // malformed timestamp
        (
            "1\n00:00:01.000 --> 00:00:02,000\nx\n",
            "INVALID_CAPTION_FORMAT",
        ),
        // unsupported markup tag
        (
            "1\n00:00:01,000 --> 00:00:02,000\n<u>x</u>\n",
            "UNSUPPORTED_FEATURE",
        ),
        // coordinate suffix is positioning data we do not model
        (
            "1\n00:00:01,000 --> 00:00:02,000 X1:1 X2:2\nx\n",
            "UNSUPPORTED_FEATURE",
        ),
        // overlapping cue intervals
        (
            "1\n00:00:01,000 --> 00:00:03,000\na\n\n2\n00:00:02,000 --> 00:00:04,000\nb\n",
            "CLIP_OVERLAP",
        ),
    ] {
        let error = kronello_service::parse(input, CaptionFormat::Srt).unwrap_err();
        assert_eq!(error.code, code, "{input}");
    }
}

#[test]
fn vtt_parses_signature_identifiers_align_and_skips_notes() {
    let cues = kronello_service::parse(
        "WEBVTT - header comment\n\nNOTE a skipped block\nstill note\n\ncue-id-1\n00:00.500 --> 00:01.250 align:end\n<b>bold</b> tail\n",
        CaptionFormat::Vtt,
    )
    .unwrap();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].start, Time::new(1, 2).unwrap());
    assert_eq!(cues[0].end, Time::new(5, 4).unwrap());
    assert_eq!(cues[0].text, "bold tail");
    assert_eq!(cues[0].spans[0].bold, Some(true));
    assert_eq!(
        cues[0].anchor_column,
        Some(kronello_service::CaptionAnchorColumn::End)
    );
}

#[test]
fn vtt_rejects_missing_signature_and_unsupported_blocks() {
    assert_eq!(
        kronello_service::parse("00:00.000 --> 00:01.000\nx\n", CaptionFormat::Vtt)
            .unwrap_err()
            .code,
        "INVALID_CAPTION_FORMAT"
    );
    for (input, code) in [
        (
            "WEBVTT\n\nSTYLE\n::cue { color: lime }\n\n00:00.000 --> 00:01.000\nx\n",
            "UNSUPPORTED_FEATURE",
        ),
        (
            "WEBVTT\n\nREGION\nid:fred\n\n00:00.000 --> 00:01.000\nx\n",
            "UNSUPPORTED_FEATURE",
        ),
        // cue settings we do not model
        (
            "WEBVTT\n\n00:00.000 --> 00:01.000 line:80%\nx\n",
            "UNSUPPORTED_FEATURE",
        ),
        (
            "WEBVTT\n\n00:00.000 --> 00:01.000 vertical:rl\nx\n",
            "UNSUPPORTED_FEATURE",
        ),
        (
            "WEBVTT\n\n00:00.000 --> 00:01.000 bogus:x\nx\n",
            "INVALID_CAPTION_FORMAT",
        ),
    ] {
        let error = kronello_service::parse(input, CaptionFormat::Vtt).unwrap_err();
        assert_eq!(error.code, code, "{input}");
    }
}

#[test]
fn itt_parses_styles_spans_breaks_and_dur() {
    let cues = kronello_service::parse(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling">
 <head><styling>
  <style xml:id="s1" tts:color="#FF0000FF" tts:fontWeight="bold"/>
 </styling></head>
 <body><div>
  <p begin="00:00:01.000" dur="1.5s" style="s1" tts:textAlign="end">red <span tts:fontStyle="italic">em</span><br/>line2</p>
  <p begin="3000ms" end="4000ms"><span>plain</span></p>
 </div></body>
</tt>"##,
        CaptionFormat::Itt,
    )
    .unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start, Time::new(1, 1).unwrap());
    assert_eq!(cues[0].end, Time::new(5, 2).unwrap());
    assert_eq!(cues[0].text, "red em\nline2");
    assert_eq!(
        cues[0].fill,
        Some(Color::from_srgb8([255, 0, 0], Some(255)))
    );
    assert_eq!(
        cues[0].anchor_column,
        Some(kronello_service::CaptionAnchorColumn::End)
    );
    // Whole-cue bold from the referenced style plus the nested italic span.
    assert!(
        cues[0]
            .spans
            .iter()
            .any(|s| s.range.end == cues[0].text.len() && s.bold == Some(true))
    );
    assert!(
        cues[0]
            .spans
            .iter()
            .any(|s| s.italic == Some(true) && s.bold == Some(true))
    );
    assert_eq!(cues[1].start, Time::new(3, 1).unwrap());
    assert_eq!(cues[1].text, "plain");
}

#[test]
fn itt_rejects_unsupported_constructs() {
    let doc = |inner: &str| {
        format!(
            r##"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling"><body><div>{inner}</div></body></tt>"##
        )
    };
    for (input, code) in [
        // region elements are unsupported placement semantics
        (
            r##"<tt xmlns="http://www.w3.org/ns/ttml"><head><layout><region xml:id="r"/></layout></head><body><div><p begin="00:00:01.000" end="00:00:02.000">x</p></div></body></tt>"##,
            "UNSUPPORTED_FEATURE",
        ),
        // frame/tick timing needs ttp:frameRate which we do not model
        (
            &doc(r##"<p begin="00:00:01:12" end="00:00:02:00">x</p>"##),
            "UNSUPPORTED_FEATURE",
        ),
        // unknown elements are never dropped silently
        (
            &doc(r##"<p begin="00:00:01.000" end="00:00:02.000"><ruby>x</ruby></p>"##),
            "UNSUPPORTED_FEATURE",
        ),
        // unknown styling attributes reject too
        (
            &doc(r##"<p begin="00:00:01.000" end="00:00:02.000" tts:direction="rtl">x</p>"##),
            "UNSUPPORTED_FEATURE",
        ),
        // missing timing
        (
            &doc(r##"<p end="00:00:02.000">x</p>"##),
            "INVALID_CAPTION_FORMAT",
        ),
    ] {
        let error = kronello_service::parse(input, CaptionFormat::Itt).unwrap_err();
        assert_eq!(error.code, code, "{input}");
    }
}

#[test]
fn parsers_normalize_line_endings_and_reject_nul() {
    let crlf = "1\r\n00:00:01,000 --> 00:00:02,000\r\na\rb\r\n";
    let cues = kronello_service::parse(crlf, CaptionFormat::Srt).unwrap();
    assert_eq!(cues[0].text, "a\nb");
    let with_nul = "1\n00:00:01,000 --> 00:00:02,000\na\0b\n";
    assert_eq!(
        kronello_service::parse(with_nul, CaptionFormat::Srt)
            .unwrap_err()
            .code,
        "INVALID_CAPTION_FORMAT"
    );
}

// ------------------------------------------------- service import/export ---

const SRT_FIXTURE: &str = "1\n00:00:01,000 --> 00:00:02,500\n<i>こんにちは</i> world\nsecond line\n\n2\n00:00:03,000 --> 00:00:04,000\nplain &amp; data\n";

#[test]
fn srt_import_plans_caption_documents_and_clips() {
    let (_dir, path, sequence) = setup();
    let plan_request = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(2));
    let ResultData::Plan(plan) = engine()
        .dispatch(Request::CaptionsImportPlan(plan_request.clone()))
        .unwrap()
    else {
        panic!()
    };
    // One track append plus (CaptionSet + ClipPlace) per cue.
    assert_eq!(plan.commands.len(), 5);
    assert!(matches!(
        &plan.commands[0],
        EditCommand::Timeline(cmd)
            if matches!(&**cmd, TimelineCommand::TrackAppend { .. })
    ));
}

#[test]
fn srt_import_applies_and_exports_deterministically() {
    let (_dir, path, sequence) = setup();
    let plan_request = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(2));
    let track = plan_request.track;
    let ResultData::Edit(_) = engine()
        .dispatch(Request::CaptionsImport(CaptionsImportRequest {
            plan: plan_request,
            session_id: Uuid::new_v4(),
            idempotency_key: "import-1".into(),
        }))
        .unwrap()
    else {
        panic!()
    };

    // The timeline reports caption clips through the shared query API.
    let ResultData::Timeline(result) = engine()
        .dispatch(Request::SequenceQuery(SequenceQueryRequest {
            project: path.clone(),
            sequence,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.sequence.tracks.len(), 1);
    assert_eq!(result.sequence.tracks[0].id, track);
    assert_eq!(result.sequence.tracks[0].kind, TrackKind::Caption);
    assert_eq!(result.clips.len(), 2);
    assert!(result.clips.iter().all(|c| c.kind == ClipKind::Caption));

    // SRT export round-trips payload text, markup and CRLF records.
    let ResultData::Captions(result) = engine()
        .dispatch(Request::CaptionsExport(CaptionsExportRequest {
            project: path.clone(),
            sequence,
            format: CaptionFormat::Srt,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(result.content.contains("\r\n"));
    assert!(result.content.contains("00:00:01,000 --> 00:00:02,500"));
    assert!(result.content.contains("<i>こんにちは</i> world"));
    assert!(result.content.contains("plain &amp; data"));

    // The same cues export as WebVTT.
    let ResultData::Captions(result) = engine()
        .dispatch(Request::CaptionsExport(CaptionsExportRequest {
            project: path.clone(),
            sequence,
            format: CaptionFormat::Vtt,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(result.content.starts_with("WEBVTT\n\n"));
    assert!(result.content.contains("00:00:01.000 --> 00:00:02.500"));
    assert!(result.content.contains("plain &amp; data"));
    assert!(!result.content.contains('\r'));

    // And as ITT, emitting span styling and <br/> line breaks.
    let ResultData::Captions(result) = engine()
        .dispatch(Request::CaptionsExport(CaptionsExportRequest {
            project: path.clone(),
            sequence,
            format: CaptionFormat::Itt,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(
        result
            .content
            .contains("xmlns=\"http://www.w3.org/ns/ttml\"")
    );
    assert!(result.content.contains("begin=\"00:00:01.000\""));
    assert!(result.content.contains("tts:fontStyle=\"italic\""));
    assert!(result.content.contains("<br/>"));
    assert!(result.content.contains("こんにちは"));

    // Our own ITT output reimports without loss of text or styling.
    let reparsed = kronello_service::parse(&result.content, CaptionFormat::Itt).unwrap();
    assert_eq!(reparsed.len(), 2);
    assert_eq!(reparsed[0].text, "こんにちは world\nsecond line");
    assert_eq!(reparsed[0].spans[0].italic, Some(true));
    assert_eq!(reparsed[1].text, "plain & data");
}

#[test]
fn captions_import_rejects_mismatched_cue_ids() {
    let (_dir, path, sequence) = setup();
    let plan = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(1));
    let error = expect_error(Request::CaptionsImportPlan(plan));
    assert_eq!(error.code, "INVALID_REQUEST");
}

#[test]
fn captions_import_rejects_non_caption_track() {
    let (_dir, path, sequence) = setup();
    // Append a video track, then try importing onto it.
    let revision = export_revision(&path);
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::TrackAppend {
            sequence,
            track: Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![],
            },
        },
    ))];
    let ResultData::Plan(plan) = engine()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(_) = engine()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.clone(),
            base_revision: revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: "track".into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let video_track = {
        let ResultData::Timeline(t) = engine()
            .dispatch(Request::SequenceQuery(SequenceQueryRequest {
                project: path.clone(),
                sequence,
            }))
            .unwrap()
        else {
            panic!()
        };
        t.sequence.tracks[0].id
    };
    let mut plan = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(2));
    plan.track = video_track;
    let error = expect_error(Request::CaptionsImportPlan(plan));
    assert_eq!(error.code, "INVALID_CLIP");
}

#[test]
fn captions_trim_and_export_timing_precision() {
    let (_dir, path, sequence) = setup();
    let plan = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(2));
    let clip_id = plan.cue_ids[0].clip;
    let caption_id = plan.cue_ids[0].caption;
    let ResultData::Edit(_) = engine()
        .dispatch(Request::CaptionsImport(CaptionsImportRequest {
            plan,
            session_id: Uuid::new_v4(),
            idempotency_key: "import-2".into(),
        }))
        .unwrap()
    else {
        panic!()
    };

    // Trimming a cue shrinks its display interval.
    let revision = export_revision(&path);
    let commands = vec![EditCommand::Timeline(Box::new(TimelineCommand::ClipTrim {
        sequence,
        clip: clip_id,
        range: TimeRange::new(Time::new(1, 1).unwrap(), Time::new(2, 1).unwrap()).unwrap(),
    }))];
    let ResultData::Plan(plan) = engine()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    engine()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.clone(),
            base_revision: revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: "trim".into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap();
    let ResultData::Captions(result) = engine()
        .dispatch(Request::CaptionsExport(CaptionsExportRequest {
            project: path.clone(),
            sequence,
            format: CaptionFormat::Srt,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(result.content.contains("00:00:01,000 --> 00:00:02,000"));

    // A frame-quantized cue that is not a whole millisecond must reject
    // sidecar output rather than round silently.
    let revision = export_revision(&path);
    let new_clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Caption {
            caption: caption_id,
        },
        timeline_range: TimeRange::new(Time::ZERO, Time::new(1, 24).unwrap()).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    };
    // Place the cue at a non-overlapping position: [10s, 10s+1/24s).
    let commands = vec![EditCommand::Timeline(Box::new(
        TimelineCommand::ClipPlace {
            sequence,
            track: {
                let ResultData::Timeline(t) = engine()
                    .dispatch(Request::SequenceQuery(SequenceQueryRequest {
                        project: path.clone(),
                        sequence,
                    }))
                    .unwrap()
                else {
                    panic!()
                };
                t.sequence.tracks[0].id
            },
            clip: Box::new(Clip {
                timeline_range: TimeRange::new(
                    Time::new(10, 1).unwrap(),
                    Time::new(10, 1)
                        .unwrap()
                        .checked_add(Time::new(1, 24).unwrap())
                        .unwrap(),
                )
                .unwrap(),
                ..new_clip
            }),
        },
    ))];
    let ResultData::Plan(plan) = engine()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    engine()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.clone(),
            base_revision: revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: "place".into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap();
    let error = expect_error(Request::CaptionsExport(CaptionsExportRequest {
        project: path.clone(),
        sequence,
        format: CaptionFormat::Srt,
    }));
    assert_eq!(error.code, "TIMING_PRECISION");
}

#[test]
fn captions_export_rejects_uri_locators_and_missing_sequence() {
    let (_dir, path, sequence) = setup();
    let error = expect_error(Request::CaptionsExport(CaptionsExportRequest {
        project: "https://example.invalid/p.kronello".into(),
        sequence,
        format: CaptionFormat::Srt,
    }));
    assert_eq!(error.code, "INVALID_REQUEST");
    let error = expect_error(Request::CaptionsExport(CaptionsExportRequest {
        project: path.clone(),
        sequence: SequenceId::new(),
        format: CaptionFormat::Srt,
    }));
    assert_eq!(error.code, "SOURCE_MISSING");
}

// --------------------------------------------------------- sidecar jobs ----

#[test]
fn caption_sidecar_output_wire_shape_and_validation() {
    let sequence = SequenceId::new();
    let value = serde_json::to_value(JobOutput::CaptionSidecar {
        sequence,
        caption_format: CaptionFormat::Vtt,
    })
    .unwrap();
    assert_eq!(value["format"], "caption_sidecar");
    assert_eq!(value["caption_format"], "vtt");
    let output: JobOutput = serde_json::from_value(value).unwrap();
    assert!(matches!(output, JobOutput::CaptionSidecar { .. }));

    // Discovery lists the sidecar alongside every other output format.
    let caps = CapabilitiesResult::current(None);
    assert!(
        caps.export_profiles
            .iter()
            .any(|p| p.format == "caption_sidecar")
    );

    // render.submit validates the sidecar destination extension.
    let (_dir, path, sequence) = setup();
    let render = |destination: PathBuf| RenderSubmitRequest {
        expected_revision: None,
        render: SequenceRenderRequest {
            input: RenderInput {
                project: path.clone(),
                composition: None,
                target: Some(RenderTarget::Sequence { sequence }),
                region: OutputRegion {
                    origin: [0.0, 0.0],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
                profile: RenderProfile::default(),
                fonts: vec![],
                media_proxies: kronello_render::MediaProxyMode::Off,
            },
            range: TimeRange::new(Time::ZERO, Time::new(1, 24).unwrap()).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            output_directory: destination,
        },
        output: JobOutput::CaptionSidecar {
            sequence,
            caption_format: CaptionFormat::Srt,
        },
        required_features: vec![],
    };
    let error = engine()
        .dispatch(Request::RenderSubmit(render(_dir.path().join("out.mkv"))))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_MEDIA_INPUT");
}

#[test]
fn captions_lower_above_video_in_scene_ir() {
    let (_dir, path, sequence) = setup();
    let font_bytes =
        std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
    let identity = kronello_text::pin_font(&font_bytes, 0).unwrap();
    let mut plan = import_request(&path, sequence, CaptionFormat::Srt, SRT_FIXTURE, cue_ids(2));
    plan.style.font = identity.clone();
    let ResultData::Edit(_) = engine()
        .dispatch(Request::CaptionsImport(CaptionsImportRequest {
            plan,
            session_id: Uuid::new_v4(),
            idempotency_key: "render-1".into(),
        }))
        .unwrap()
    else {
        panic!()
    };

    let ResultData::Export(result) = engine()
        .dispatch(Request::ProjectExport(ProjectRequest { project: path }))
        .unwrap()
    else {
        panic!()
    };
    let mut project = result.document;
    let sequence_object = project
        .sequences
        .iter_mut()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .unwrap();
    // A video track authored below the caption track still composites under it.
    sequence_object.tracks.insert(
        0,
        Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![Clip {
                id: ClipId::new(),
                source_ref: SourceRef::Generator {
                    generator: SOLID_GENERATOR_ID.into(),
                    version: GENERATOR_VERSION,
                    color: Color::from_srgb8([0, 0, 255], None),
                },
                timeline_range: TimeRange::new(Time::ZERO, Time::new(4, 1).unwrap()).unwrap(),
                source_in: Time::ZERO,
                time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                enabled: true,
                audio_retime: AudioRetimePolicy::Reject,
                reverse_sampling: None,
                volume: None,
                links: vec![],
                effects: vec![],
                masks: vec![],
                markers: vec![],
                properties: vec![],
            }],
        },
    );

    let snapshot = kronello_render::RenderSnapshot::for_target(
        &project,
        kronello_render::RenderTarget::Sequence { sequence },
        1,
        RenderProfile::default(),
    )
    .unwrap();
    let fonts = [kronello_text::FontData {
        identity: &identity,
        bytes: &font_bytes,
    }];
    let scene =
        kronello_render::build_scene_ir(&snapshot, Time::new(3, 2).unwrap(), &fonts).unwrap();
    let video = scene
        .nodes
        .iter()
        .position(|n| matches!(n.content, kronello_render::SceneContent::Shape { .. }))
        .expect("solid fill node");
    let caption = scene
        .nodes
        .iter()
        .position(|n| matches!(n.content, kronello_render::SceneContent::Caption(_)))
        .expect("caption node");
    assert!(caption > video, "captions composite above video");
    let kronello_render::SceneContent::Caption(draw) = &scene.nodes[caption].content else {
        unreachable!()
    };
    assert!(!draw.layout.glyphs.is_empty());
    assert!(draw.span_flags.iter().any(|f| f.italic));

    // A caption clip referencing a missing document is a typed error, not a
    // dropped node.
    let DocumentObject::Known(sequence_object) = project
        .sequences
        .iter_mut()
        .find(|s| matches!(s, DocumentObject::Known(s) if s.id == sequence))
        .unwrap()
    else {
        unreachable!()
    };
    let caption_clip = sequence_object
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| matches!(c.source_ref, SourceRef::Caption { .. }))
        .unwrap()
        .id;
    let track = sequence_object
        .tracks
        .iter_mut()
        .find(|t| t.kind == TrackKind::Caption)
        .unwrap();
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id == caption_clip)
        .unwrap();
    clip.source_ref = SourceRef::Caption {
        caption: CaptionId::new(),
    };
    let error = kronello_render::RenderSnapshot::for_target(
        &project,
        kronello_render::RenderTarget::Sequence { sequence },
        1,
        RenderProfile::default(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "SOURCE_MISSING");
}
