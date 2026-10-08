//! FX-008 audio effects (ADR-0137/0139): delay, reverb, gate run inside the
//! deterministic DocumentAudioPlan DSP chain; pitch executes at the source
//! stage via the shared WSOLA cursor. Stateful chains always evaluate from
//! the placement boundary, so any partition reproduces the continuous mix.
use kronello_audio::*;
use kronello_model::*;
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn r(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn registry() -> SchemaRegistry {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in effect_descriptors() {
        registry.register(descriptor).unwrap();
    }
    registry
}
fn param(key: &str, value: Value) -> Property {
    let registry = registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new(format!("kronello.effect.{key}")).unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
}
fn generator_clip() -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: AUDIO_GENERATOR_TONE.into(),
            version: 1,
            color: Color::from_srgb8([0; 3], None),
        },
        timeline_range: r(Time::ZERO, t(2, 1)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::ResampleV1,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        properties: vec![],
        effects: vec![],
        markers: vec![],
        masks: vec![],
    }
}
fn asset_clip(asset: AssetId) -> Clip {
    Clip {
        source_ref: SourceRef::Asset {
            asset,
            stream_index: 0,
        },
        // Half a second leaves headroom for rate-scaled WSOLA reads.
        timeline_range: r(Time::ZERO, t(1, 2)),
        ..generator_clip()
    }
}
fn fixture(clip: Clip) -> (Project, SequenceId) {
    let sequence = SequenceId::new();
    let mut p = Project::default();
    p.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Audio,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    (p, sequence)
}
fn asset_fixture() -> (Project, SequenceId, AssetId) {
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: "a".repeat(64),
        locator: AssetLocator {
            relative: Some("source.wav".into()),
            absolute: None,
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_f32le".into(),
            time_base: t(1, 48000),
            duration: Some(t(2, 1)),
            start_time: None,
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
    };
    let asset_id = asset.id;
    let (mut p, sequence) = fixture(asset_clip(asset_id));
    p.assets.push(DocumentObject::Known(asset));
    (p, sequence, asset_id)
}
fn clip_mut(p: &mut Project) -> &mut Clip {
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    &mut s.tracks[0].clips[0]
}
fn plan(p: &Project, id: SequenceId) -> DocumentAudioPlan {
    DocumentAudioPlan::compile_version(p, AudioTarget::Sequence(id), AUDIO_EVALUATION_VERSION)
        .unwrap()
}
fn plan_error(p: &Project, id: SequenceId) -> AudioError {
    DocumentAudioPlan::compile_version(p, AudioTarget::Sequence(id), AUDIO_EVALUATION_VERSION)
        .unwrap_err()
}
fn rms(frames: &[[f32; 2]], skip: usize) -> f64 {
    let tail = &frames[skip.min(frames.len())..];
    (tail
        .iter()
        .map(|f| f64::from(f[0]) * f64::from(f[0]))
        .sum::<f64>()
        / tail.len().max(1) as f64)
        .sqrt()
}
/// Arbitrary partitions of a stateful mix reproduce the continuous bus.
fn assert_partitioned(plan: &DocumentAudioPlan, range: TimeRange, cuts: &[Time]) {
    let src = AudioSources::new();
    let full = plan.mix(&src, range).unwrap();
    let mut joined: Vec<[f32; 2]> = Vec::new();
    let mut start = range.start();
    for cut in cuts.iter().copied().chain([range.end()]) {
        let bus = plan.mix(&src, r(start, cut)).unwrap();
        joined.extend_from_slice(bus.buffer().frames());
        start = cut;
    }
    assert_eq!(joined, full.buffer().frames());
}
/// 240 Hz stereo sine, 200 samples per period, for WSOLA pitch checks.
fn sine(frames: usize) -> AudioBuffer {
    AudioBuffer::new(
        (0..frames)
            .map(|i| [0.5 * (std::f64::consts::TAU * i as f64 / 200.0).sin() as f32; 2])
            .collect(),
    )
    .unwrap()
}
/// Dominant period in samples by normalized autocorrelation.
fn best_lag(samples: &[f32], lo: usize, hi: usize) -> usize {
    let mut best = lo;
    let mut best_score = f64::NEG_INFINITY;
    for lag in lo..=hi.min(samples.len() / 2) {
        let mut dot = 0.0_f64;
        let mut ea = 0.0_f64;
        let mut eb = 0.0_f64;
        for i in 0..samples.len() - lag {
            let a = f64::from(samples[i]);
            let b = f64::from(samples[i + lag]);
            dot += a * b;
            ea += a * a;
            eb += b * b;
        }
        let score = if ea > 0.0 && eb > 0.0 {
            dot / (ea * eb).sqrt()
        } else {
            0.0
        };
        if score > best_score {
            best_score = score;
            best = lag;
        }
    }
    best
}
fn attach_delay(p: &mut Project, delay_ms: f64, feedback_db: f64, wet: f64, dry: f64) {
    let (ms, fb, w, d) = (
        param("delay_ms", scalar(delay_ms)),
        param("feedback_db", scalar(feedback_db)),
        param("wet", scalar(wet)),
        param("dry", scalar(dry)),
    );
    let clip = clip_mut(p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_DELAY_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioDelay {
            delay_ms: ms.id(),
            feedback_db: fb.id(),
            wet: w.id(),
            dry: d.id(),
        },
    }));
    clip.properties.extend([ms, fb, w, d]);
}

#[test]
fn fx008_delay_echoes_then_decays_and_partitions_match() {
    let range = r(Time::ZERO, t(2, 1));
    // Baseline generator tone without effects.
    let (p, id) = fixture(generator_clip());
    let baseline = plan(&p, id).mix(&AudioSources::new(), range).unwrap();
    // 250 ms == 12_000 samples; wet-only output is silent for the first
    // wrap then replays the source, with a quieter -6 dB second echo.
    let (mut p, id) = fixture(generator_clip());
    attach_delay(&mut p, 250.0, -6.0, 1.0, 0.0);
    let plan = plan(&p, id);
    let delayed = plan.mix(&AudioSources::new(), range).unwrap();
    let frames = delayed.buffer().frames();
    assert!(frames[..12_000].iter().all(|f| *f == [0.0; 2]));
    assert_eq!(
        &frames[12_000..24_000],
        &baseline.buffer().frames()[..12_000]
    );
    // Each 12_000-sample wrap adds fb x the previous echo; on a continuous
    // tone the recirculation stays bounded (~2x at -6 dB) while building.
    let first = rms(&frames[12_000..24_000], 0);
    let second = rms(&frames[24_000..36_000], 0);
    assert!(
        second > first * 1.2,
        "feedback echo must add (first {first}, second {second})"
    );
    assert!(second < first * 2.2, "feedback must stay bounded");
    assert_partitioned(&plan, range, &[t(5000, 48000), t(24001, 48000)]);
}

#[test]
fn fx008_reverb_tail_is_deterministic_and_damped() {
    let range = r(Time::ZERO, t(2, 1));
    // Wet-only reverb: silent until the shortest comb wraps, then a
    // deterministic decaying tail.
    let (mut p, id) = fixture(generator_clip());
    let (decay, damping, wet, dry) = (
        param("decay_ms", scalar(800.0)),
        param("damping", scalar(0.3)),
        param("wet", scalar(1.0)),
        param("dry", scalar(0.0)),
    );
    let clip = clip_mut(&mut p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_REVERB_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioReverb {
            decay_ms: decay.id(),
            damping: damping.id(),
            wet: wet.id(),
            dry: dry.id(),
        },
    }));
    clip.properties.extend([decay, damping, wet, dry]);
    let plan = plan(&p, id);
    let reverbed = plan.mix(&AudioSources::new(), range).unwrap();
    let frames = reverbed.buffer().frames();
    assert!(frames[..1_100].iter().all(|f| *f == [0.0; 2]));
    assert!(frames[2_000..].iter().any(|f| *f != [0.0; 2]));
    // Repeat evaluation is bit-identical and partitions match exactly.
    let repeat = plan.mix(&AudioSources::new(), range).unwrap();
    assert_eq!(frames, repeat.buffer().frames());
    assert_partitioned(&plan, range, &[t(1157, 48000), t(48_000, 48000)]);
}

#[test]
fn fx008_gate_opens_on_the_tone_and_mutes_below_threshold() {
    let range = r(Time::ZERO, t(1, 1));
    let gate = |p: &mut Project, threshold: f64| {
        let (threshold_db, attack, release, hysteresis) = (
            param("threshold_db", scalar(threshold)),
            param("attack_ms", scalar(1.0)),
            param("release_ms", scalar(50.0)),
            param("hysteresis_db", scalar(6.0)),
        );
        let clip = clip_mut(p);
        clip.effects.push(Effect::Known(EffectDefinition {
            effect_id: AUDIO_GATE_ID.into(),
            version: 1,
            parameters: EffectParameters::AudioGate {
                threshold_db: threshold_db.id(),
                attack_ms: attack.id(),
                release_ms: release.id(),
                hysteresis_db: hysteresis.id(),
            },
        }));
        clip.properties
            .extend([threshold_db, attack, release, hysteresis]);
    };
    let (p, id) = fixture(generator_clip());
    let baseline = rms(
        plan(&p, id)
            .mix(&AudioSources::new(), range)
            .unwrap()
            .buffer()
            .frames(),
        4_800,
    );
    // The 0.25-amplitude tone peaks near -12 dB: a -20 dB threshold opens.
    let (mut p, id) = fixture(generator_clip());
    gate(&mut p, -20.0);
    let open_plan = plan(&p, id);
    let open = open_plan.mix(&AudioSources::new(), range).unwrap();
    assert!(rms(open.buffer().frames(), 4_800) > baseline * 0.8);
    assert_partitioned(&open_plan, range, &[t(5000, 48000)]);
    // A -3 dB threshold never opens: output is exactly muted.
    let (mut p, id) = fixture(generator_clip());
    gate(&mut p, -3.0);
    let closed = plan(&p, id).mix(&AudioSources::new(), range).unwrap();
    assert!(closed.buffer().frames().iter().all(|f| *f == [0.0; 2]));
}

#[test]
fn fx008_pitch_shifts_at_the_source_stage() {
    let (mut p, id, asset) = asset_fixture();
    let semitones = param("semitones", scalar(12.0));
    let clip = clip_mut(&mut p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_PITCH_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioPitch {
            semitones: semitones.id(),
        },
    }));
    clip.properties.push(semitones);
    clip.audio_retime = AudioRetimePolicy::ResampleV1;
    let sources = AudioSources::from([((asset, 0), sine(96_000))]);
    let range = clip_mut(&mut p).timeline_range;
    let shifted = plan(&p, id).mix(&sources, range).unwrap();
    // +12 semitones strides the WSOLA windows by 2x: the 200-sample source
    // period becomes a 100-sample output period while the clip duration
    // stays fixed.
    let mono: Vec<f32> = shifted.buffer().frames()[4096..20_000]
        .iter()
        .map(|f| f[0])
        .collect();
    assert_eq!(best_lag(&mono, 40, 400), 100);
    // Two stacked pitch entries compose by summing semitones: -12 cancels.
    let cancel = param("semitones", scalar(-12.0));
    let clip = clip_mut(&mut p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_PITCH_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioPitch {
            semitones: cancel.id(),
        },
    }));
    clip.properties.push(cancel);
    let restored = plan(&p, id).mix(&sources, range).unwrap();
    let mono: Vec<f32> = restored.buffer().frames()[4096..20_000]
        .iter()
        .map(|f| f[0])
        .collect();
    assert_eq!(best_lag(&mono, 40, 400), 200);
    // Source-stage determinism: partitioned requests reproduce the mix.
    let cut = t(1001, 48000);
    let left = plan(&p, id).mix(&sources, r(range.start(), cut)).unwrap();
    let right = plan(&p, id).mix(&sources, r(cut, range.end())).unwrap();
    let joined: Vec<_> = left
        .buffer()
        .frames()
        .iter()
        .chain(right.buffer().frames())
        .copied()
        .collect();
    assert_eq!(joined, restored.buffer().frames());
}

#[test]
fn fx008_pitch_requires_an_asset_source_and_forward_retime() {
    // The built-in generator has no source stream to rate-scale: a typed
    // unsupported error, not silence or substitution.
    let (mut p, id) = fixture(generator_clip());
    let semitones = param("semitones", scalar(7.0));
    let clip = clip_mut(&mut p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_PITCH_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioPitch {
            semitones: semitones.id(),
        },
    }));
    clip.properties.push(semitones);
    let error = plan_error(&p, id);
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    // An Asset source behind reverse resampling is likewise unsupported:
    // the WSOLA cursor only ever reads forward.
    let (mut p, id, _asset) = asset_fixture();
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::ReverseResampleV1;
    let semitones = param("semitones", scalar(7.0));
    let clip = clip_mut(&mut p);
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_PITCH_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioPitch {
            semitones: semitones.id(),
        },
    }));
    clip.properties.push(semitones);
    let error = plan_error(&p, id);
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
}
