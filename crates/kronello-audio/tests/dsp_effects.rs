//! AUDIO-007/008 integration coverage: typed audio filters and dynamics run
//! through the shared DocumentAudioPlan used by realtime playback and export.
//! Stateful chains evaluate from the placement boundary, so any partition of
//! a request reproduces the exact samples of a continuous mix (ADR-0117).
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
fn volume_property(value: f64) -> Property {
    let registry = SchemaRegistry::with_builtin();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(scalar(value)),
        vec![],
        &registry,
    )
    .unwrap()
}
fn effect(effect_id: &str, parameters: EffectParameters) -> Effect {
    Effect::Known(EffectDefinition {
        effect_id: effect_id.into(),
        version: 1,
        parameters,
    })
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
fn fixture(kind: TrackKind, clip: Clip) -> (Project, SequenceId) {
    let sequence = SequenceId::new();
    let mut p = Project::default();
    p.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    (p, sequence)
}
fn clip_mut(p: &mut Project) -> &mut Clip {
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    &mut s.tracks[0].clips[0]
}
fn advanced(p: &Project, id: SequenceId) -> DocumentAudioPlan {
    DocumentAudioPlan::compile_version(p, AudioTarget::Sequence(id), AUDIO_EVALUATION_VERSION)
        .unwrap()
}
fn rms(frames: &[[f32; 2]], skip: usize) -> f64 {
    let tail = &frames[skip..];
    (tail
        .iter()
        .map(|f| f64::from(f[0]) * f64::from(f[0]))
        .sum::<f64>()
        / tail.len() as f64)
        .sqrt()
}
fn attach(p: &mut Project, parameters: EffectParameters, properties: Vec<Property>) {
    let effect_id = match &parameters {
        EffectParameters::AudioEq { .. } => AUDIO_EQ_ID,
        EffectParameters::AudioHpf { .. } => AUDIO_HPF_ID,
        EffectParameters::AudioLpf { .. } => AUDIO_LPF_ID,
        EffectParameters::AudioCompressor { .. } => AUDIO_COMPRESSOR_ID,
        EffectParameters::AudioLimiter { .. } => AUDIO_LIMITER_ID,
        _ => unreachable!(),
    };
    let clip = clip_mut(p);
    clip.effects.push(effect(effect_id, parameters));
    clip.properties.extend(properties);
}
fn eq_table() -> Value {
    Value::DataTable(DataTable {
        columns: [
            ("kind".to_owned(), ValueType::Enum),
            ("freq_hz".to_owned(), ValueType::Scalar),
            ("gain_db".to_owned(), ValueType::Scalar),
            ("q".to_owned(), ValueType::Scalar),
        ]
        .into(),
        rows: vec![
            [
                ("kind".to_owned(), Value::Enum("peak".into())),
                ("freq_hz".to_owned(), scalar(440.0)),
                ("gain_db".to_owned(), scalar(12.0)),
                ("q".to_owned(), scalar(1.0)),
            ]
            .into(),
        ],
    })
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
fn baseline_rms(range: TimeRange) -> f64 {
    let (p, id) = fixture(TrackKind::Audio, generator_clip());
    rms(
        advanced(&p, id)
            .mix(&AudioSources::new(), range)
            .unwrap()
            .buffer()
            .frames(),
        4_800,
    )
}
#[test]
fn filters_process_chain_and_arbitrary_partitions_are_bit_identical() {
    let range = r(Time::ZERO, t(2, 1));
    let baseline = baseline_rms(range);
    // 4th-order 2 kHz Butterworth HPF removes the 440 Hz tone.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let (cutoff, order) = (
        param("cutoff_hz", scalar(2_000.0)),
        param("order", scalar(4.0)),
    );
    attach(
        &mut p,
        EffectParameters::AudioHpf {
            cutoff_hz: cutoff.id(),
            order: order.id(),
        },
        vec![cutoff, order],
    );
    let plan = advanced(&p, id);
    let filtered = plan.mix(&AudioSources::new(), range).unwrap();
    assert!(rms(filtered.buffer().frames(), 4_800) < baseline * 0.02);
    assert_partitioned(
        &plan,
        range,
        &[t(4096, 48000), t(5000, 48000), t(96000, 48000)],
    );
    // 4th-order 200 Hz Butterworth LPF removes the same tone.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let (cutoff, order) = (
        param("cutoff_hz", scalar(200.0)),
        param("order", scalar(4.0)),
    );
    attach(
        &mut p,
        EffectParameters::AudioLpf {
            cutoff_hz: cutoff.id(),
            order: order.id(),
        },
        vec![cutoff, order],
    );
    let plan = advanced(&p, id);
    let filtered = plan.mix(&AudioSources::new(), range).unwrap();
    assert!(rms(filtered.buffer().frames(), 4_800) < baseline * 0.1);
    assert_partitioned(&plan, range, &[t(5000, 48000)]);
    // Peaking EQ at the tone frequency adds ~12 dB.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let bands = param("eq_bands", eq_table());
    attach(
        &mut p,
        EffectParameters::AudioEq { bands: bands.id() },
        vec![bands],
    );
    let plan = advanced(&p, id);
    let boosted = plan.mix(&AudioSources::new(), range).unwrap();
    let gain = 20.0 * (rms(boosted.buffer().frames(), 4_800) / baseline).log10();
    assert!((gain - 12.0).abs() < 0.5, "eq gain {gain}");
    assert_partitioned(&plan, range, &[t(5000, 48000), t(48001, 48000)]);
}
#[test]
fn dynamics_process_chain_and_arbitrary_partitions_are_bit_identical() {
    let range = r(Time::ZERO, t(2, 1));
    let baseline = baseline_rms(range);
    // Hot input: authored gain 4x ahead of the compressor.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let gain = volume_property(4.0);
    let (threshold, ratio, attack, release, makeup) = (
        param("threshold_db", scalar(-20.0)),
        param("ratio", scalar(4.0)),
        param("attack_ms", scalar(5.0)),
        param("release_ms", scalar(100.0)),
        param("makeup_db", scalar(0.0)),
    );
    let clip = clip_mut(&mut p);
    clip.effects.push(effect(
        AUDIO_GAIN_ID,
        EffectParameters::AudioGain { gain: gain.id() },
    ));
    clip.properties.push(gain);
    clip.effects.push(effect(
        AUDIO_COMPRESSOR_ID,
        EffectParameters::AudioCompressor {
            threshold_db: threshold.id(),
            ratio: ratio.id(),
            attack_ms: attack.id(),
            release_ms: release.id(),
            makeup_db: makeup.id(),
        },
    ));
    clip.properties
        .extend([threshold, ratio, attack, release, makeup]);
    let plan = advanced(&p, id);
    let compressed = plan.mix(&AudioSources::new(), range).unwrap();
    let hot = rms(compressed.buffer().frames(), 48_000);
    // 4x gain then ~-10 dB compression ends near the unity baseline.
    assert!(hot < baseline * 1.8 && hot > baseline * 0.2, "hot {hot}");
    assert_partitioned(&plan, range, &[t(5000, 48000), t(96000, 48000)]);
    // Limiter at -3 dBFS caps the same 4x-hot signal.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let gain = volume_property(4.0);
    let (ceiling, release) = (
        param("ceiling_db", scalar(-3.0)),
        param("release_ms", scalar(50.0)),
    );
    let clip = clip_mut(&mut p);
    clip.effects.push(effect(
        AUDIO_GAIN_ID,
        EffectParameters::AudioGain { gain: gain.id() },
    ));
    clip.properties.push(gain);
    clip.effects.push(effect(
        AUDIO_LIMITER_ID,
        EffectParameters::AudioLimiter {
            ceiling_db: ceiling.id(),
            release_ms: release.id(),
        },
    ));
    clip.properties.extend([ceiling, release]);
    let plan = advanced(&p, id);
    let limited = plan.mix(&AudioSources::new(), range).unwrap();
    let ceiling = 10_f64.powf(-3.0 / 20.0) as f32;
    let peak = limited
        .buffer()
        .frames()
        .iter()
        .skip(4_800)
        .flatten()
        .fold(0.0_f32, |m, v| m.max(v.abs()));
    assert!(peak <= ceiling * 1.001, "peak {peak}");
    assert_partitioned(&plan, range, &[t(5000, 48000)]);
}
#[test]
fn audio_effects_require_audio_tracks_and_typed_parameters() {
    // Audio effects on a video-track composition clip are a typed error. The
    // composition carries a media node with an audio stream, so the clip is
    // audible and reaches the audio-effect contract check.
    let mut p = Project::default();
    let asset = AssetId::new();
    let composition = CompositionId::new();
    p.assets.push(DocumentObject::Known(Asset {
        id: asset,
        content_hash: "a".repeat(64),
        kind: AssetKind::Audio,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_s16le".into(),
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
        locator: AssetLocator {
            relative: Some("tone.wav".into()),
            absolute: None,
        },
    }));
    let volume = volume_property(1.0);
    let node: SceneNode = serde_json::from_value(serde_json::json!({
        "id": NodeId::new(),
        "kind": {"kind":"media","value":{"asset":asset,"stream_index":0,
            "source_in":{"num":"0","den":"1"},
            "time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},
            "volume":volume.id()}},
        "properties": [volume],
        "active_range": {"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}},
        "child_order": [], "containment_parent": null, "transform_parent": null, "effects": []
    }))
    .unwrap();
    p.compositions.push(DocumentObject::Known(Composition {
        id: composition,
        duration: kronello_time::Duration::new(t(2, 1)).unwrap(),
        design_extent: DesignExtent::new(64.0, 32.0).unwrap(),
        edit_rate: FrameRate::new(30000, 1001).unwrap(),
        root_nodes: vec![node.id],
        properties: vec![],
        nodes: vec![node],
    }));
    let mut clip = generator_clip();
    clip.source_ref = SourceRef::Composition { composition };
    clip.audio_retime = AudioRetimePolicy::Reject;
    let cutoff = param("cutoff_hz", scalar(1_000.0));
    let order = param("order", scalar(2.0));
    clip.effects.push(effect(
        AUDIO_HPF_ID,
        EffectParameters::AudioHpf {
            cutoff_hz: cutoff.id(),
            order: order.id(),
        },
    ));
    clip.properties.extend([cutoff, order]);
    let sequence = SequenceId::new();
    p.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
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
        targets: None,
    }));
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(sequence), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    // Out-of-range constants are typed parameter errors.
    for (cutoff_hz, order_value) in [(30_000.0, 2.0), (1_000.0, 9.0), (0.0, 2.0)] {
        let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
        let (cutoff, order) = (
            param("cutoff_hz", scalar(cutoff_hz)),
            param("order", scalar(order_value)),
        );
        attach(
            &mut p,
            EffectParameters::AudioHpf {
                cutoff_hz: cutoff.id(),
                order: order.id(),
            },
            vec![cutoff, order],
        );
        assert_eq!(
            DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
                .unwrap_err()
                .code(),
            "INVALID_AUDIO_INPUT",
            "cutoff_hz={cutoff_hz} order={order_value}"
        );
    }
    // A missing parameter Property reference is a typed error too.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let cutoff = param("cutoff_hz", scalar(1_000.0));
    attach(
        &mut p,
        EffectParameters::AudioHpf {
            cutoff_hz: cutoff.id(),
            order: PropertyId::new(),
        },
        vec![cutoff],
    );
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
    // Non-Constant (animated) filter parameters are an explicit unsupported
    // contract, not an implicit static fallback.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let registry = registry();
    let animated = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.effect.cutoff_hz").unwrap())
                .unwrap(),
        ),
        PropertySource::Curve(CurveId::new()),
        vec![],
        &registry,
    )
    .unwrap();
    let order = param("order", scalar(2.0));
    attach(
        &mut p,
        EffectParameters::AudioHpf {
            cutoff_hz: animated.id(),
            order: order.id(),
        },
        vec![animated, order],
    );
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    // Malformed EQ tables and non-audio effects on audio clips are typed too.
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let bands = param(
        "eq_bands",
        Value::DataTable(DataTable {
            columns: [("x".to_owned(), ValueType::Scalar)].into(),
            rows: vec![],
        }),
    );
    attach(
        &mut p,
        EffectParameters::AudioEq { bands: bands.id() },
        vec![bands],
    );
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
    let (mut p, id) = fixture(TrackKind::Audio, generator_clip());
    let sigma = param("sigma", scalar(1.0));
    let clip = clip_mut(&mut p);
    clip.effects.push(effect(
        GAUSSIAN_BLUR_ID,
        EffectParameters::GaussianBlur { sigma: sigma.id() },
    ));
    clip.properties.push(sigma);
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}
