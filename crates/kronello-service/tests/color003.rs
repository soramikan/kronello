//! COLOR-003/COLOR-004 service coverage (ADR-0113): `lut.import` verifies and
//! persists a `.cube` Data asset, `luts` render inputs bind hash-checked
//! lattices through render/scopes requests, and `inspect.scopes` returns
//! deterministic integer bins with the evaluated revision.
use kronello_model::*;
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn run(request: Json) -> Result<ResultData, ServiceError> {
    service().dispatch(serde_json::from_value(request).unwrap())
}
fn execute(request: Json) -> Json {
    serde_json::to_value(service().execute_json(&request.to_string())).unwrap()
}
fn export(path: &std::path::Path) -> ExportResult {
    let ResultData::Export(r) = run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    *r
}
/// Sequence with one full-alpha solid generator clip, no effects; the clip id
/// is returned for `clip_set_effects` commands.
fn fixture() -> (Project, SequenceId, ClipId) {
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, [0.25, 0.5, 0.75], 1.0).unwrap(),
        },
        timeline_range: TimeRange::new(Time::ZERO, Time::from_integer(1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        properties: vec![],
        markers: vec![],
    };
    let clip_id = clip.id;
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(4.0, 2.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
    };
    let sequence_id = sequence.id;
    let p = Project {
        sequences: vec![DocumentObject::Known(sequence)],
        ..Project::default()
    };
    (p, sequence_id, clip_id)
}
/// A size-2 `.cube` that inverts every channel: corner (r,g,b) ↦ (1-r,1-g,1-b).
fn invert_cube() -> String {
    let mut text = String::from("TITLE \"invert\"\nLUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                text.push_str(&format!("{} {} {}\n", 1 - r, 1 - g, 1 - b));
            }
        }
    }
    text
}
fn hash_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn input(path: &std::path::Path, sequence: SequenceId, luts: Json) -> Json {
    json!({"project":path,"target":RenderTarget::Sequence{sequence},
        "region":{"origin":[0.,0.],"extent":[4.,2.],"pixels":[4,2]},
        "luts":luts})
}
fn frame(
    path: &std::path::Path,
    sequence: SequenceId,
    luts: Json,
) -> Result<ResultData, ServiceError> {
    run(
        json!({"operation":"render.frame","input":input(path, sequence, luts),
        "time":Time::ZERO,"backend":"cpu_reference"}),
    )
}
/// Attach `kronello.color.lut` to the clip through the shared plan/apply path.
fn apply_lut(
    path: &std::path::Path,
    sequence: SequenceId,
    clip: ClipId,
    asset: AssetId,
    intensity: f64,
) {
    let revision = export(path).revision;
    let property = |key: &str, kind: &str, value: Json| {
        json!({"id":Uuid::new_v4(),"descriptor":{"key":key,"version":1},
            "source":{"kind":"constant","value":{"kind":kind,"value":value}},"modifiers":[]})
    };
    let commands = json!([{"timeline":{"clip_set_effects":{
        "sequence":sequence,"clip":clip,
        "properties":[
            property("kronello.effect.lut","asset_ref",json!(asset)),
            property("kronello.effect.intensity","scalar",json!(intensity))],
        "effects":[{"effect_id":"kronello.color.lut","version":1,
            "parameters":{"kind":"color_lut"}}]}}}]);
    // Parameter fields are PropertyIds; fill them after building the properties.
    let mut commands = commands;
    let lut_id = commands[0]["timeline"]["clip_set_effects"]["properties"][0]["id"].clone();
    let intensity_id = commands[0]["timeline"]["clip_set_effects"]["properties"][1]["id"].clone();
    let effect = &mut commands[0]["timeline"]["clip_set_effects"]["effects"][0]["parameters"];
    effect["lut"] = lut_id;
    effect["intensity"] = intensity_id;
    let ResultData::Plan(plan) = run(
        json!({"operation":"edit.plan","project":path,"base_revision":revision,
            "commands":commands}),
    )
    .unwrap() else {
        panic!()
    };
    run(
        json!({"operation":"edit.apply","project":path,"base_revision":revision,
        "commands":commands,"plan_hash":plan.plan_hash,
        "session_id":Uuid::new_v4(),"idempotency_key":Uuid::new_v4().to_string()}),
    )
    .unwrap();
}

#[test]
fn lut_import_verifies_content_and_drives_render_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("color003.kronello");
    let (document, sequence, clip) = fixture();
    run(json!({"operation":"project.create","project":path,"document":document})).unwrap();
    let cube = dir.path().join("invert.cube");
    std::fs::write(&cube, invert_cube()).unwrap();
    let asset = AssetId::new();
    // Import is an edit: base revision, session and idempotency apply.
    run(
        json!({"operation":"lut.import","project":path,"base_revision":"1",
        "session_id":Uuid::new_v4(),"idempotency_key":"import-1",
        "path":cube,"asset":asset}),
    )
    .unwrap();
    let document = export(&path).document;
    let DocumentObject::Known(record) = document
        .assets
        .iter()
        .find(|a| matches!(a, DocumentObject::Known(a) if a.id == asset))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(record.kind, AssetKind::Data);
    assert_eq!(record.content_hash, hash_of(&std::fs::read(&cube).unwrap()));
    apply_lut(&path, sequence, clip, asset, 1.0);
    // The lattice is bound by content hash and applied pointwise: the solid
    // (0.25,0.5,0.75) inverts to (0.75,0.5,0.25) through the size-2 lattice.
    let ResultData::Frame(result) = frame(
        &path,
        sequence,
        json!([{"hash":record.content_hash,"path":cube}]),
    )
    .unwrap() else {
        panic!()
    };
    for p in &result.linear {
        assert!(
            (p[0] - 0.75).abs() < 1e-6 && (p[2] - 0.25).abs() < 1e-6,
            "{p:?}"
        );
        assert_eq!(p[3], 1.0);
    }
    // No supplied lattice: the referenced asset has no verified input.
    let error = frame(&path, sequence, json!([])).unwrap_err();
    assert_eq!(error.code, "LUT_INPUT_MISSING", "{error:?}");
    // A locator whose bytes do not match the declared hash is rejected before
    // any lattice is bound.
    let other = dir.path().join("other.cube");
    std::fs::write(&other, format!("{}\n# touched\n", invert_cube())).unwrap();
    let error = frame(
        &path,
        sequence,
        json!([{"hash":record.content_hash,"path":other}]),
    )
    .unwrap_err();
    assert_eq!(error.code, "ASSET_HASH_MISMATCH", "{error:?}");
    let missing = dir.path().join("missing.cube");
    let error = frame(
        &path,
        sequence,
        json!([{"hash":record.content_hash,"path":missing}]),
    )
    .unwrap_err();
    assert_eq!(error.code, "LUT_MISSING", "{error:?}");
}

#[test]
fn lut_import_rejects_malformed_and_unsupported_documents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("color003.kronello");
    let (document, _, _) = fixture();
    run(json!({"operation":"project.create","project":path,"document":document})).unwrap();
    let import = |name: &str, bytes: &str| {
        let file = dir.path().join(name);
        std::fs::write(&file, bytes).unwrap();
        execute(
            json!({"operation":"lut.import","project":path,"base_revision":"1",
            "session_id":Uuid::new_v4(),"idempotency_key":Uuid::new_v4().to_string(),
            "path":file,"asset":AssetId::new()}),
        )
    };
    for (name, bytes, code) in [
        (
            "one.cube",
            "LUT_1D_SIZE 4\n0\n0.3\n0.7\n1\n",
            "UNSUPPORTED_FEATURE",
        ),
        ("rows.cube", "LUT_3D_SIZE 2\n0 0 0\n", "INVALID_LUT"),
        ("size.cube", "LUT_3D_SIZE 66\n", "UNSUPPORTED_FEATURE"),
        ("text.cube", "LUT_3D_SIZE two\n", "INVALID_LUT"),
    ] {
        let response = import(name, bytes);
        assert_eq!(response["status"], "error", "{response}");
        assert_eq!(response["error"]["code"], code, "{response}");
    }
    let response = execute(
        json!({"operation":"lut.import","project":path,"base_revision":"1",
        "session_id":Uuid::new_v4(),"idempotency_key":Uuid::new_v4().to_string(),
        "path":dir.path().join("absent.cube"),"asset":AssetId::new()}),
    );
    assert_eq!(response["error"]["code"], "LUT_MISSING", "{response}");
}

#[test]
fn inspect_scopes_return_deterministic_bins_with_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("color004.kronello");
    let (document, sequence, _) = fixture();
    run(json!({"operation":"project.create","project":path,"document":document})).unwrap();
    let request = json!({"operation":"inspect.scopes",
        "input":input(&path, sequence, json!([])),"time":Time::ZERO});
    let first = execute(request.clone());
    assert_eq!(first["status"], "success", "{first}");
    assert_eq!(first["result"]["kind"], "scopes", "{first}");
    let first = &first["result"]["value"];
    // Deterministic: identical request, identical bins and revision.
    let second = execute(request.clone());
    assert_eq!(second["result"]["value"], *first);
    assert_eq!(first["revision"], export(&path).revision);
    assert_eq!(first["size"], json!([4, 2]));
    assert_eq!(first["working_space"], "linear_rec709");
    // A flat opaque frame concentrates every scope at one bin; all 8 pixels
    // are counted exactly once per scope family.
    let sum = |v: &Json| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .sum::<u64>()
    };
    assert_eq!(sum(&first["waveform"]["bins"]), 8);
    assert_eq!(sum(&first["vectorscope"]["bins"]), 8);
    assert_eq!(sum(&first["histogram"]["luma"]), 8);
    for channel in ["red", "green", "blue"] {
        assert_eq!(sum(&first["histogram"][channel]), 8, "{channel}");
        assert_eq!(sum(&first["parade"][channel]), 8, "{channel}");
    }
    assert_eq!(first["waveform"]["columns"], 512);
    assert_eq!(first["vectorscope"]["size"], 256);
    assert_eq!(first["histogram"]["levels"], 256);
    // Malformed input (unknown field) is a typed request error, not a panic.
    let mut bad = request.clone();
    bad["input"]["luts"] = json!([{"hash":"not-hex","path":dir.path().join("invert.cube")}]);
    let response = execute(bad);
    assert_eq!(response["status"], "error");
}
