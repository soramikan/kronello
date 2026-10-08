//! FX-007 service contracts (ADR-0116): adjustment clips place through the
//! shared `clip_place` command, classify as `ClipKind::Adjustment` in sequence
//! queries, apply effects to the lower composite, and reject typed violations.
use kronello_model::*;
use kronello_render::{OutputRegion, RenderTarget, render_registry};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn property(key: &str, value: Value) -> Property {
    let r = render_registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(r.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &r,
    )
    .unwrap()
}
fn solid(rgb: [u8; 3]) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8(rgb, None),
        },
        timeline_range: range(Time::ZERO, t(4, 1)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
        pan: None,
    }
}
fn adjustment(exposure_ev: f64) -> Clip {
    let exposure = property("kronello.effect.exposure", scalar(exposure_ev));
    let offset = property("kronello.effect.exposure_offset", scalar(0.0));
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Adjustment,
        timeline_range: range(Time::ZERO, t(4, 1)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        effects: vec![Effect::Known(EffectDefinition {
            effect_id: COLOR_EXPOSURE_ID.into(),
            version: 1,
            parameters: EffectParameters::ColorExposure {
                exposure: exposure.id(),
                offset: offset.id(),
            },
        })],
        masks: vec![],
        markers: vec![],
        properties: vec![exposure, offset],
        pan: None,
    }
}
fn sequence(tracks: Vec<Track>) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(2.0, 2.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks,
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }
}
fn track(kind: TrackKind, clips: Vec<Clip>) -> Track {
    Track {
        state: None,
        id: TrackId::new(),
        kind,
        clips,
    }
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fx007.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    (dir, path)
}
fn export(path: &Path) -> ExportResult {
    let ResultData::Export(p) = service()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    *p
}
fn apply(path: &Path, command: TimelineCommand, key: &str) -> kronello_store::Event {
    let commands = vec![EditCommand::Timeline(Box::new(command))];
    let revision = export(path).revision;
    let ResultData::Plan(plan) = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.into(),
            base_revision: revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: key.into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap()
    else {
        panic!()
    };
    event
}
fn reject(path: &Path, command: TimelineCommand, code: &str) {
    let before = export(path);
    let error = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: before.revision.clone(),
            commands: vec![EditCommand::Timeline(Box::new(command))],
        }))
        .unwrap_err();
    assert_eq!(error.code, code, "{error:?}");
    assert_eq!(export(path).document, before.document);
    assert_eq!(export(path).revision, before.revision);
}
fn sequence_query(path: &Path, sequence: SequenceId) -> SequenceQueryResult {
    let ResultData::Timeline(r) = service()
        .dispatch(Request::SequenceQuery(SequenceQueryRequest {
            project: path.into(),
            sequence,
        }))
        .unwrap()
    else {
        panic!()
    };
    r
}

#[test]
fn clip_place_accepts_adjustment_and_query_classifies_it() {
    let base = solid([200, 40, 10]);
    let v1 = track(TrackKind::Video, vec![base]);
    let v2 = track(TrackKind::Video, vec![]);
    let v2_id = v2.id;
    let s = sequence(vec![v1, v2]);
    let sequence_id = s.id;
    let (_dir, path) = setup({
        let mut p = Project::default();
        p.sequences.push(DocumentObject::Known(s));
        p
    });
    let adj = adjustment(1.0);
    let adj_id = adj.id;
    apply(
        &path,
        TimelineCommand::ClipPlace {
            sequence: sequence_id,
            track: v2_id,
            clip: Box::new(adj),
        },
        "place-adjustment",
    );
    let query = sequence_query(&path, sequence_id);
    let placed = query
        .clips
        .iter()
        .find(|c| c.clip.id == adj_id)
        .expect("adjustment clip listed");
    assert_eq!(placed.kind, ClipKind::Adjustment);
    assert!(placed.unsupported_reason.is_none());
    // The clip affects the lower composite: rendering through the service
    // applies the exposure effect to the red clip below.
    let ResultData::Frame(frame) = service()
        .dispatch(Request::RenderFrame(FrameRenderRequest {
            backend: None,
            input: RenderInput {
                project: path.clone(),
                composition: None,
                target: Some(RenderTarget::Sequence {
                    sequence: sequence_id,
                }),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [2.0; 2],
                    pixels: [2; 2],
                },
                profile: Default::default(),
                fonts: vec![],
                media_proxies: kronello_render::MediaProxyMode::Off,
                luts: vec![],
            },
            time: t(1, 1),
        }))
        .unwrap()
    else {
        panic!()
    };
    let rgb = [
        (200.0 / 255.0f32).powf(2.2).max(0.0),
        (40.0 / 255.0f32).powf(2.2).max(0.0),
        (10.0 / 255.0f32).powf(2.2).max(0.0),
    ];
    // sRGB -> linear conversion aside, exposure +1 must double each channel.
    assert!(frame.linear[0][0] > rgb[0] * 1.5, "{:?}", frame.linear[0]);
    assert!(frame.linear[0][3] > 0.99);
}

#[test]
fn adjustment_violations_reject_with_typed_errors() {
    let base = solid([200, 40, 10]);
    let v = track(TrackKind::Video, vec![base]);
    let v2 = track(TrackKind::Video, vec![]);
    let v_id = v2.id;
    let a = track(TrackKind::Audio, vec![]);
    let a_id = a.id;
    let c = track(TrackKind::Caption, vec![]);
    let c_id = c.id;
    let s = sequence(vec![v, v2, a, c]);
    let sequence_id = s.id;
    let (_dir, path) = setup({
        let mut p = Project::default();
        p.sequences.push(DocumentObject::Known(s));
        p
    });
    // Audio and caption tracks reject adjustment placement.
    for track in [a_id, c_id] {
        reject(
            &path,
            TimelineCommand::ClipPlace {
                sequence: sequence_id,
                track,
                clip: Box::new(adjustment(1.0)),
            },
            "INVALID_CLIP",
        );
    }
    // A non-identity time map (retime) is rejected on video tracks too.
    let mut retimed = adjustment(1.0);
    retimed.time_map = TimeMap::linear(Time::ZERO, Rational::new(2, 1).unwrap()).unwrap();
    reject(
        &path,
        TimelineCommand::ClipPlace {
            sequence: sequence_id,
            track: v_id,
            clip: Box::new(retimed),
        },
        "INVALID_CLIP",
    );
    // A nonzero source window is likewise invalid for a payload-free source.
    let mut shifted = adjustment(1.0);
    shifted.source_in = t(1, 1);
    reject(
        &path,
        TimelineCommand::ClipPlace {
            sequence: sequence_id,
            track: v_id,
            clip: Box::new(shifted),
        },
        "INVALID_CLIP",
    );
    // Trimming or stretching an adjustment stays a pure placement resize: it
    // must not mint a source window. The surviving clip keeps identity time.
    let adj = adjustment(1.0);
    let adj_id = adj.id;
    apply(
        &path,
        TimelineCommand::ClipPlace {
            sequence: sequence_id,
            track: v_id,
            clip: Box::new(adj),
        },
        "place-ok",
    );
    apply(
        &path,
        TimelineCommand::ClipTrim {
            sequence: sequence_id,
            clip: adj_id,
            range: range(t(1, 1), t(3, 1)),
        },
        "trim-adjustment",
    );
    let e = export(&path);
    let DocumentObject::Known(s) = &e.document.sequences[0] else {
        panic!()
    };
    let c = s.tracks[1]
        .clips
        .iter()
        .find(|c| c.id == adj_id)
        .expect("trimmed adjustment");
    assert_eq!(c.timeline_range, range(t(1, 1), t(3, 1)));
    assert_eq!(c.source_in, Time::ZERO);
    // Splitting an adjustment keeps both pieces payload-free and identity.
    let right = ClipId::new();
    apply(
        &path,
        TimelineCommand::ClipSplit {
            sequence: sequence_id,
            clip: adj_id,
            time: t(2, 1),
            right_clip: right,
        },
        "split-adjustment",
    );
    let e = export(&path);
    let DocumentObject::Known(s) = &e.document.sequences[0] else {
        panic!()
    };
    assert_eq!(s.tracks[1].clips.len(), 2);
    for piece in &s.tracks[1].clips {
        assert!(matches!(piece.source_ref, SourceRef::Adjustment));
        assert_eq!(piece.source_in, Time::ZERO);
    }
}
