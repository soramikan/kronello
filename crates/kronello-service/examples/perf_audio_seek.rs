//! Revision-pinned long DSP/WSOLA seek preparation probe.
use kronello_media::MediaRuntime;
use kronello_model::*;
use kronello_service::*;
use kronello_time::{Time, TimeMap, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};

fn parameter(key: &str, value: f64) -> Property {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in effect_descriptors() {
        registry.register(descriptor).unwrap();
    }
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new(format!("kronello.effect.{key}")).unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(value).unwrap())),
        vec![],
        &registry,
    )
    .unwrap()
}
fn attach(
    clip: &mut Clip,
    effect_id: &str,
    values: &[(&str, f64)],
    parameters: impl FnOnce(&[Property]) -> EffectParameters,
) {
    let properties: Vec<_> = values
        .iter()
        .map(|(key, value)| parameter(key, *value))
        .collect();
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: effect_id.into(),
        version: 1,
        parameters: parameters(&properties),
    }));
    clip.properties.extend(properties);
}
fn generate(directory: &Path, wave: &Path) {
    std::fs::create_dir(directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    for case in ["dsp", "wsola"] {
        let case_directory = directory.join(case);
        std::fs::create_dir(&case_directory).unwrap();
        let input = case_directory.join("source.wav");
        std::fs::copy(wave, &input).unwrap();
        let asset = Asset {
            id: AssetId::new(),
            kind: AssetKind::Audio,
            content_hash: kronello_media::content_hash(&input).unwrap(),
            streams: runtime
                .probe(&input)
                .unwrap()
                .streams
                .into_iter()
                .map(|s| StreamMetadata {
                    index: s.index,
                    codec: s.codec,
                    time_base: s.time_base,
                    duration: s.duration,
                    start_time: s.start,
                    width: s.width,
                    height: s.height,
                    pixel_format: s.pixel_format,
                    color_primaries: s.color_primaries,
                    color_transfer: s.color_transfer,
                    color_matrix: s.color_matrix,
                    color_range: s.color_range,
                })
                .collect(),
            locator: AssetLocator {
                relative: Some("source.wav".into()),
                absolute: None,
            },
        };
        let mut document: Project =
            serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
        let DocumentObject::Known(sequence) = &mut document.sequences[0] else {
            panic!()
        };
        sequence.tracks.truncate(1);
        sequence.tracks[0].kind = TrackKind::Audio;
        sequence.tracks[0].clips.truncate(1);
        let clip = &mut sequence.tracks[0].clips[0];
        clip.source_ref = SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        };
        clip.timeline_range = TimeRange::new(Time::ZERO, Time::from_integer(180)).unwrap();
        clip.source_in = Time::new(13, 48000).unwrap();
        clip.time_map = TimeMap::linear(
            Time::ZERO,
            if case == "wsola" {
                Time::new(3, 2).unwrap()
            } else {
                Time::ONE
            },
        )
        .unwrap();
        clip.audio_retime = if case == "wsola" {
            AudioRetimePolicy::PitchPreserveV1
        } else {
            AudioRetimePolicy::ResampleV1
        };
        clip.properties.clear();
        clip.effects.clear();
        attach(
            clip,
            AUDIO_HPF_ID,
            &[("cutoff_hz", 70.0), ("order", 2.0)],
            |p| EffectParameters::AudioHpf {
                cutoff_hz: p[0].id(),
                order: p[1].id(),
            },
        );
        attach(
            clip,
            AUDIO_COMPRESSOR_ID,
            &[
                ("threshold_db", -18.0),
                ("ratio", 4.0),
                ("attack_ms", 5.0),
                ("release_ms", 100.0),
                ("makeup_db", 1.0),
            ],
            |p| EffectParameters::AudioCompressor {
                threshold_db: p[0].id(),
                ratio: p[1].id(),
                attack_ms: p[2].id(),
                release_ms: p[3].id(),
                makeup_db: p[4].id(),
            },
        );
        attach(
            clip,
            AUDIO_DELAY_ID,
            &[
                ("delay_ms", 80.0),
                ("feedback_db", -9.0),
                ("wet", 0.35),
                ("dry", 0.8),
            ],
            |p| EffectParameters::AudioDelay {
                delay_ms: p[0].id(),
                feedback_db: p[1].id(),
                wet: p[2].id(),
                dry: p[3].id(),
            },
        );
        attach(
            clip,
            AUDIO_REVERB_ID,
            &[
                ("decay_ms", 800.0),
                ("damping", 0.3),
                ("wet", 0.25),
                ("dry", 0.75),
            ],
            |p| EffectParameters::AudioReverb {
                decay_ms: p[0].id(),
                damping: p[1].id(),
                wet: p[2].id(),
                dry: p[3].id(),
            },
        );
        if case == "dsp" {
            let mut second = sequence.tracks[0].clips[0].clone();
            second.id = ClipId::new();
            second.timeline_range =
                TimeRange::new(Time::from_integer(5), Time::from_integer(95)).unwrap();
            let mut track = sequence.tracks[0].clone();
            track.id = TrackId::new();
            track.clips = vec![second];
            sequence.tracks.push(track);
        }
        document.assets.push(DocumentObject::Known(asset));
        let service = Service::new(BackendSelection::CpuReference);
        let result = service.execute(Request::ProjectCreate(CreateRequest {
            project: case_directory.join("seek.kronello"),
            document,
            plan_hash: None,
            idempotency_key: None,
        }));
        assert!(matches!(result, Response::Success { .. }), "{result:?}");
    }
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "OUTPUT_DIRECTORY FIXTURE_DIRECTORY INPUT_300S_WAV before|after|disabled"
    );
    let output = Path::new(&args[1]);
    let fixtures = Path::new(&args[2]);
    let phase = &args[4];
    assert!(matches!(phase.as_str(), "before" | "after" | "disabled"));
    std::fs::create_dir(output).unwrap();
    if phase == "before" {
        generate(fixtures, Path::new(&args[3]));
    }
    let mut cases = Vec::new();
    for case in ["dsp", "wsola"] {
        let project = fixtures.join(case).join("seek.kronello");
        let stored = kronello_store::ProjectStore::read_snapshot(&project).unwrap();
        let DocumentObject::Known(sequence) = &stored.document.sequences[0] else {
            panic!()
        };
        let target = RenderTarget::Sequence {
            sequence: sequence.id,
        };
        let revision = stored.revision.to_string();
        let mut service = Service::new(BackendSelection::CpuReference);
        if phase == "disabled" {
            service = service
                .with_audio_checkpoint_cache_limits(AudioCheckpointCacheLimits {
                    entries: 0,
                    state_bytes: 0,
                    ..Default::default()
                })
                .unwrap();
        }
        let mut trials = Vec::new();
        let positions = [
            137 * 48000 + 17,
            151 * 48000 + 31,
            137 * 48000 + 17,
            90 * 48000 + 7,
            165 * 48000 + 53,
            151 * 48000 + 31,
            137 * 48000 + 17,
            90 * 48000 + 7,
            165 * 48000 + 53,
        ];
        for (iteration, resume) in positions.into_iter().enumerate() {
            let begin = Instant::now();
            let input = service
                .capture_audio_input(AudioPrepareRequest {
                    project: project.clone(),
                    target,
                    expected_revision: revision.clone(),
                    resume_sample: Some(resume),
                })
                .unwrap();
            let capture_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let prepared = input.prepare().unwrap();
            let ready_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let mut buffer = vec![0.0; 8192];
            let block_begin = Instant::now();
            prepared.render_block(resume, &mut buffer).unwrap();
            let first_block_ms = block_begin.elapsed().as_secs_f64() * 1000.0;
            let first_audio_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let mut digest = Sha256::new();
            for value in &buffer {
                digest.update(value.to_bits().to_le_bytes());
            }
            let meters = prepared
                .render_block_metered(resume + 4096, &mut buffer)
                .unwrap();
            for value in &buffer {
                digest.update(value.to_bits().to_le_bytes());
            }
            prepared.render_block(resume + 8192, &mut buffer).unwrap();
            for value in &buffer {
                digest.update(value.to_bits().to_le_bytes());
            }
            trials.push(json!({"iteration":iteration,"resume_sample":resume,"capture_ms":capture_ms,
                "prepare_ms":ready_ms-capture_ms,"first_block_ms":first_block_ms,
                "start_to_first_block_ms":first_audio_ms,"three_blocks_f32_bits_sha256":format!("{:x}",digest.finalize()),
                "meters":meters,"source_cache":service.audio_source_cache_stats(),
                "checkpoint_cache":service.audio_checkpoint_cache_stats(),"warm_start_sample":prepared.warm_start_sample(),
                "warm_evaluated_frames":prepared.warm_evaluated_frames()}));
            println!("{case} {iteration}: {first_audio_ms:.3}ms");
        }
        let mut warm: Vec<_> = trials[1..]
            .iter()
            .map(|t| t["start_to_first_block_ms"].as_f64().unwrap())
            .collect();
        warm.sort_by(f64::total_cmp);
        cases.push(json!({"case":case,"project":project,"revision":revision,"document_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&stored.document).unwrap())),
            "source_sha256":kronello_media::content_hash(&fixtures.join(case).join("source.wav")).unwrap(),
            "warm_p50_ms":warm[4],"warm_p95_ms":warm[7],"trials":trials}));
    }
    let report = json!({"phase":phase,"quiet_host":false,"scope":"capture + producer preparation through first 4096 stereo frames; 180s stateful placements over 300s PCM; cold1/warm8; subsequent two blocks and meters verified outside start timing; runtime/state cache counters and full source hashes outside timing",
        "source_sha256":{"advanced.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-audio/src/advanced.rs"))),
            "dsp.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-audio/src/dsp.rs"))),
            "wsola.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-audio/src/wsola.rs"))),
            "playback.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/playback.rs"))),
            "audio_checkpoints.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/audio_checkpoints.rs")))},"cases":cases});
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
