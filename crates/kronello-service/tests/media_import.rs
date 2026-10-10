use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime};
use kronello_model::Project;
use kronello_service::{BackendSelection, Service};
use kronello_time::Rational;
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

fn execute(request: Value) -> Value {
    serde_json::to_value(
        Service::new(BackendSelection::CpuReference).execute_json(&request.to_string()),
    )
    .unwrap()
}
fn ok(request: Value) -> Value {
    let response = execute(request);
    assert_eq!(response["status"], "success", "{response}");
    response["result"]["value"].clone()
}
fn video(path: &Path) {
    MediaRuntime::load()
        .unwrap()
        .encode_video_stream(
            &EncodeRequest {
                output: path.into(),
                codec: EncodeCodec::ProRes,
                width: 16,
                height: 16,
                time_base: Rational::new(1, 24).unwrap(),
            },
            2,
            &mut |i| {
                Ok(EncodeFrame {
                    pts: Rational::new(i as i64, 24).unwrap(),
                    rgba: [32, 64, 128, 255].repeat(256),
                })
            },
        )
        .unwrap();
}
fn create(path: &Path) {
    ok(json!({"operation":"project.create","project":path,"document":Project::default()}));
}
fn request(project: &Path, path: &Path, revision: &str) -> Value {
    json!({"operation":"media.import","project":project,"path":path,"base_revision":revision,
        "session_id":Uuid::new_v4(),"idempotency_key":Uuid::new_v4().to_string(),"asset":Uuid::new_v4()})
}
#[test]
fn media_import_probes_hashes_replays_and_undoes() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.kronello");
    let path = dir.path().join("video with spaces.mov");
    video(&path);
    create(&project);
    let request = request(&project, &path, "1");
    let event = ok(request.clone());
    let export = ok(json!({"operation":"project.export","project":project}));
    let asset = &export["document"]["assets"][0];
    assert_eq!(asset["id"], request["asset"]);
    assert_eq!(asset["kind"], "video");
    assert_eq!(
        asset["content_hash"],
        kronello_media::content_hash(&path).unwrap()
    );
    assert_eq!(asset["locator"]["relative"], "video with spaces.mov");
    assert_eq!(
        asset["locator"]["absolute"],
        path.canonicalize().unwrap().to_str().unwrap()
    );
    let probe = MediaRuntime::load().unwrap().probe(&path).unwrap();
    assert_eq!(
        asset["streams"][0]["duration"],
        serde_json::to_value(probe.streams[0].duration).unwrap()
    );
    assert_eq!(asset["streams"][0]["width"], 16);
    assert_eq!(asset["streams"][0]["codec"], "prores");
    assert_eq!(ok(request.clone()), event);
    let mut reused = request.clone();
    reused["asset"] = json!(Uuid::new_v4());
    assert_eq!(execute(reused)["error"]["code"], "IDEMPOTENCY_KEY_REUSED");
    let mut stale = request.clone();
    stale["idempotency_key"] = json!("stale");
    assert_eq!(execute(stale)["error"]["code"], "REVISION_CONFLICT");
    ok(
        json!({"operation":"edit.undo","project":project,"base_revision":"2","event_id":event["id"],
        "session_id":request["session_id"],"idempotency_key":"undo-import"}),
    );
    assert!(
        ok(json!({"operation":"project.export","project":project}))["document"]["assets"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        ok(request),
        event,
        "receipt replay survives removal and subsequent edits"
    );
}
#[test]
fn media_import_audio_image_external_locator_and_typed_failures() {
    let dir = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.kronello");
    create(&project);
    let audio = external.path().join("audio.mov");
    MediaRuntime::load()
        .unwrap()
        .encode_audio(
            &kronello_audio::Bus::new(
                0,
                kronello_audio::AudioBuffer::new(vec![[0.1, 0.1]; 4800]).unwrap(),
            ),
            kronello_audio::ClippingPolicy::Reject,
            &audio,
        )
        .unwrap();
    ok(request(&project, &audio, "1"));
    let image = dir.path().join("image.unknown");
    let mut encoder = png::Encoder::new(std::fs::File::create(&image).unwrap(), 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[255, 0, 0, 255].repeat(4))
        .unwrap();
    ok(request(&project, &image, "2"));
    let export = ok(json!({"operation":"project.export","project":project}));
    let assets = export["document"]["assets"].as_array().unwrap();
    assert_eq!(assets[0]["kind"], "audio");
    assert_eq!(assets[0]["locator"]["relative"], Value::Null);
    assert_eq!(assets[0]["streams"][0]["codec"], "pcm_s24le");
    assert_eq!(assets[1]["kind"], "image");
    assert_eq!(assets[1]["streams"][0]["codec"], "png");
    assert_eq!(assets[1]["streams"][0]["width"], 2);
    let unsupported = dir.path().join("unsupported.png");
    let mut encoder = png::Encoder::new(std::fs::File::create(&unsupported).unwrap(), 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_gamma(png::ScaledFloat::new(0.7));
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[255, 0, 0, 255].repeat(4))
        .unwrap();
    assert_eq!(
        execute(request(&project, &unsupported, "3"))["error"]["code"],
        "UNSUPPORTED_FEATURE"
    );
    let invalid = dir.path().join("invalid.mov");
    std::fs::write(&invalid, b"not media").unwrap();
    assert_eq!(
        execute(request(&project, &invalid, "3"))["error"]["code"],
        "DECODE_ERROR"
    );
    assert_eq!(
        execute(request(&project, dir.path(), "3"))["error"]["code"],
        "INVALID_MEDIA_INPUT"
    );
    assert_eq!(
        execute(request(&project, &dir.path().join("missing"), "3"))["error"]["code"],
        "ASSET_MISSING"
    );
    assert_eq!(
        execute(request(
            &project,
            Path::new("https://example.invalid/video.mov"),
            "3"
        ))["error"]["code"],
        "INVALID_REQUEST"
    );
    assert_eq!(
        ok(json!({"operation":"project.export","project":project}))["revision"],
        "3"
    );
}

#[test]
fn media_import_keeps_both_video_and_audio_streams() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.kronello");
    create(&project);
    let video_path = dir.path().join("video.mov");
    let audio_path = dir.path().join("audio.mov");
    let movie = dir.path().join("movie.mov");
    video(&video_path);
    let runtime = MediaRuntime::load().unwrap();
    runtime
        .encode_audio(
            &kronello_audio::Bus::new(
                0,
                kronello_audio::AudioBuffer::new(vec![[0.1, 0.1]; 4000]).unwrap(),
            ),
            kronello_audio::ClippingPolicy::Reject,
            &audio_path,
        )
        .unwrap();
    runtime
        .mux_av(
            &video_path,
            &audio_path,
            &movie,
            &"a".repeat(64),
            &"b".repeat(64),
        )
        .unwrap();
    ok(request(&project, &movie, "1"));
    let export = ok(json!({"operation":"project.export","project":project}));
    let asset = &export["document"]["assets"][0];
    assert_eq!(asset["kind"], "video");
    let streams = asset["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    for stream in runtime.probe(&movie).unwrap().streams {
        let saved = streams.iter().find(|s| s["index"] == stream.index).unwrap();
        assert_eq!(saved["codec"], stream.codec);
        assert_eq!(
            saved["time_base"],
            serde_json::to_value(stream.time_base).unwrap()
        );
        assert_eq!(
            saved["start_time"],
            serde_json::to_value(stream.start).unwrap()
        );
    }
}
