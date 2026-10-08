//! FX-004 clip mask model contracts and FX-007 adjustment clip validation
//! (ADR-0114, ADR-0116).
use kronello_model::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::collections::BTreeMap;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn scalar(value: f64) -> PropertySource<Value> {
    PropertySource::Constant(Value::Scalar(FiniteF64::new(value).unwrap()))
}
fn path_value(points: &[[f64; 2]]) -> Value {
    let mut segments = vec![];
    for (i, p) in points.iter().enumerate() {
        let point = [FiniteF64::new(p[0]).unwrap(), FiniteF64::new(p[1]).unwrap()];
        segments.push(if i == 0 {
            PathSegment::MoveTo(point)
        } else {
            PathSegment::LineTo(point)
        });
    }
    segments.push(PathSegment::Close);
    Value::Path(Path { segments })
}
fn mask_property(key: &str, source: PropertySource<Value>) -> Property {
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        source,
        vec![],
        &registry,
    )
    .unwrap()
}
/// A valid mask row plus its backing clip properties.
fn mask_with_properties() -> (Mask, Vec<Property>) {
    let path = mask_property(
        MASK_PATH_KEY,
        PropertySource::Constant(path_value(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]])),
    );
    let feather = mask_property(MASK_FEATHER_KEY, scalar(2.0));
    let expansion = mask_property(MASK_EXPANSION_KEY, scalar(0.5));
    let opacity = mask_property(MASK_OPACITY_KEY, scalar(1.0));
    let mask = Mask {
        id: MaskId::new(),
        path: path.id(),
        mode: MaskMode::Add,
        feather: feather.id(),
        expansion: expansion.id(),
        opacity: opacity.id(),
        invert: false,
        closed: true,
    };
    (mask, vec![path, feather, expansion, opacity])
}
fn clip(start: i64, end: i64) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8([128; 3], None),
        },
        timeline_range: TimeRange::new(t(start, 1), t(end, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        enabled: true,
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    }
}
fn sequence_with(kind: TrackKind, clips: Vec<Clip>) -> (Sequence, Project) {
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(4.0, 4.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind,
            clips,
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    };
    (sequence, Project::default())
}

#[test]
fn fx004_mask_mode_wire_names_are_snake_case() {
    for (name, mode) in [
        ("add", MaskMode::Add),
        ("subtract", MaskMode::Subtract),
        ("intersect", MaskMode::Intersect),
        ("difference", MaskMode::Difference),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<MaskMode>(name.into()).unwrap(),
            mode
        );
    }
    assert!(serde_json::from_value::<MaskMode>("xor".into()).is_err());
}

#[test]
fn fx004_mask_serde_defaults_and_clip_field_omission() {
    let (mask, _) = mask_with_properties();
    let raw = serde_json::to_value(&mask).unwrap();
    // `invert`/`closed` are authored fields; absent input must default them.
    let mut minimal = raw.clone();
    let object = minimal.as_object_mut().unwrap();
    object.remove("invert");
    object.remove("closed");
    let decoded: Mask = serde_json::from_value(minimal).unwrap();
    assert_eq!(decoded, mask);
    assert!(!decoded.invert && decoded.closed);
    // Empty clip mask stacks are omitted from the wire entirely.
    let clip_raw = serde_json::to_value(clip(0, 1)).unwrap();
    assert!(!clip_raw.as_object().unwrap().contains_key("masks"));
    let mut masked_clip = clip(0, 1);
    let (mask, properties) = mask_with_properties();
    masked_clip.masks = vec![mask];
    masked_clip.properties = properties;
    let raw = serde_json::to_value(&masked_clip).unwrap();
    assert!(raw.as_object().unwrap().contains_key("masks"));
    let decoded: Clip = serde_json::from_value(raw).unwrap();
    assert_eq!(decoded, masked_clip);
}

#[test]
fn fx004_mask_validation_rejects_bad_references_and_budgets() {
    let registry = SchemaRegistry::with_builtin();
    let (mask, properties) = mask_with_properties();
    validate_clip_masks(std::slice::from_ref(&mask), &properties, &registry).unwrap();
    // Missing property reference.
    let mut missing = mask.clone();
    missing.feather = PropertyId::new();
    assert!(matches!(
        validate_clip_masks(&[missing], &properties, &registry),
        Err(MaskError::InvalidProperty(_))
    ));
    // A property carrying a different descriptor key is incompatible.
    let wrong_key = mask_property(MASK_OPACITY_KEY, scalar(1.0));
    let mut mismatched = mask.clone();
    mismatched.path = wrong_key.id();
    let mut swapped = properties.clone();
    swapped.push(wrong_key);
    assert!(matches!(
        validate_clip_masks(&[mismatched], &swapped, &registry),
        Err(MaskError::InvalidProperty(_))
    ));
    // Duplicate mask ids in one clip. Sharing the same property ids between
    // masks is legal; the duplicate is the mask id itself.
    let duplicate = Mask {
        path: mask.path,
        ..mask.clone()
    };
    assert!(matches!(
        validate_clip_masks(&[mask.clone(), duplicate], &properties, &registry),
        Err(MaskError::DuplicateId(_))
    ));
    // Mask count budget.
    let many: Vec<Mask> = (0..=MASKS_PER_CLIP_MAX)
        .map(|_| Mask {
            id: MaskId::new(),
            ..mask.clone()
        })
        .collect();
    assert!(matches!(
        validate_clip_masks(&many, &properties, &registry),
        Err(MaskError::BudgetExceeded)
    ));
}

#[test]
fn fx004_mask_path_budget_and_topology() {
    let registry = SchemaRegistry::with_builtin();
    // A single path over MASK_PATH_POINTS_MAX anchors is a budget failure.
    let huge: Vec<[f64; 2]> = (0..=MASK_PATH_POINTS_MAX)
        .map(|i| [i as f64, 0.0])
        .collect();
    let path = mask_property(MASK_PATH_KEY, PropertySource::Constant(path_value(&huge)));
    let (mut mask, mut properties) = mask_with_properties();
    mask.path = path.id();
    properties[0] = path;
    assert!(matches!(
        validate_clip_masks(&[mask], &properties, &registry),
        Err(MaskError::BudgetExceeded)
    ));
    // Topology: a segment before any MoveTo is a structural failure.
    let mut bad = mask_with_properties();
    let bad_path = mask_property(
        MASK_PATH_KEY,
        PropertySource::Constant(Value::Path(Path {
            segments: vec![PathSegment::LineTo([
                FiniteF64::new(1.0).unwrap(),
                FiniteF64::new(1.0).unwrap(),
            ])],
        })),
    );
    bad.0.path = bad_path.id();
    bad.1[0] = bad_path;
    assert!(matches!(
        validate_clip_masks(&[bad.0], &bad.1, &registry),
        Err(MaskError::InvalidPath)
    ));
    // Negative constant feather breaks the descriptor range at construction.
    let descriptor = registry
        .lookup(&SchemaKey::new(MASK_FEATHER_KEY).unwrap())
        .unwrap();
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(descriptor),
            scalar(-1.0),
            vec![],
            &registry,
        )
        .is_err()
    );
}

#[test]
fn fx004_mask_resolve_enforces_evaluated_ranges() {
    let (mask, properties) = mask_with_properties();
    let values: BTreeMap<PropertyId, Value> = properties
        .iter()
        .map(|p| match p.source() {
            PropertySource::Constant(v) => (p.id(), v.clone()),
            _ => unreachable!(),
        })
        .collect();
    let resolved = mask.resolve(&values).unwrap();
    assert_eq!(resolved.feather, 2.0);
    assert_eq!(resolved.opacity, 1.0);
    assert!(resolved.closed);
    // Evaluated feather below zero is a typed failure, not a clamp.
    let mut bad = values.clone();
    bad.insert(mask.feather, Value::Scalar(FiniteF64::new(-0.5).unwrap()));
    assert!(matches!(
        mask.resolve(&bad),
        Err(MaskError::OutOfRange(id)) if id == mask.feather
    ));
    let mut over = values.clone();
    over.insert(mask.opacity, Value::Scalar(FiniteF64::new(1.5).unwrap()));
    assert!(matches!(
        mask.resolve(&over),
        Err(MaskError::OutOfRange(id)) if id == mask.opacity
    ));
}

#[test]
fn fx004_masks_validate_through_sequence() {
    let (mask, properties) = mask_with_properties();
    let mut masked = clip(0, 1);
    masked.masks = vec![mask];
    masked.properties = properties;
    let (sequence, project) = sequence_with(TrackKind::Video, vec![masked]);
    sequence.validate(&project).unwrap();
    // The same mask stack on an audio clip is rejected by track kind.
    let mut audio_masked = clip(0, 1);
    let (mask, properties) = mask_with_properties();
    audio_masked.masks = vec![mask];
    audio_masked.properties = properties;
    let (audio_sequence, project) = sequence_with(TrackKind::Audio, vec![audio_masked]);
    assert!(matches!(
        audio_sequence.validate(&project),
        Err(SequenceError::Invalid(_))
    ));
    // Missing mask properties inside a validated clip surface via validation.
    let mut dangling = clip(0, 1);
    let (mask, _) = mask_with_properties();
    dangling.masks = vec![mask];
    let (sequence, project) = sequence_with(TrackKind::Video, vec![dangling]);
    assert!(matches!(
        sequence.validate(&project),
        Err(SequenceError::Invalid(_))
    ));
}

fn adjustment_clip(start: i64, end: i64) -> Clip {
    Clip {
        source_ref: SourceRef::Adjustment,
        ..clip(start, end)
    }
}

#[test]
fn fx007_adjustment_source_ref_wire_shape() {
    let raw = serde_json::to_value(SourceRef::Adjustment).unwrap();
    assert_eq!(raw, serde_json::json!({"kind": "adjustment"}));
    let decoded: SourceRef = serde_json::from_value(raw).unwrap();
    assert_eq!(decoded, SourceRef::Adjustment);
    // Unknown payload fields on the closed variant are rejected.
    let mut extra = serde_json::json!({"kind": "adjustment", "surprise": 1});
    assert!(serde_json::from_value::<SourceRef>(extra.clone()).is_err());
    extra["kind"] = serde_json::json!("warp");
    assert!(serde_json::from_value::<SourceRef>(extra).is_err());
}

#[test]
fn fx007_adjustment_clip_track_and_timing_constraints() {
    // Identity adjustment clip on a video track validates.
    let (sequence, project) = sequence_with(TrackKind::Video, vec![adjustment_clip(0, 1)]);
    sequence.validate(&project).unwrap();
    // Audio and caption tracks reject it.
    for kind in [TrackKind::Audio, TrackKind::Caption] {
        let (sequence, project) = sequence_with(kind, vec![adjustment_clip(0, 1)]);
        assert!(matches!(
            sequence.validate(&project),
            Err(SequenceError::Invalid(_))
        ));
    }
    // Non-zero source window.
    let mut bad = adjustment_clip(0, 1);
    bad.source_in = t(1, 2);
    let (sequence, project) = sequence_with(TrackKind::Video, vec![bad]);
    assert!(sequence.validate(&project).is_err());
    // Non-identity speed is a retime the adjustment cannot express.
    let mut bad = adjustment_clip(0, 1);
    bad.time_map = TimeMap::linear(Time::ZERO, Rational::new(2, 1).unwrap()).unwrap();
    let (sequence, project) = sequence_with(TrackKind::Video, vec![bad]);
    assert!(sequence.validate(&project).is_err());
    let mut bad = adjustment_clip(0, 1);
    bad.time_map = TimeMap::linear(t(1, 2), Rational::ONE).unwrap();
    let (sequence, project) = sequence_with(TrackKind::Video, vec![bad]);
    assert!(sequence.validate(&project).is_err());
    // Audio gain has no meaning on an adjustment pass.
    let mut bad = adjustment_clip(0, 1);
    bad.volume = Some(Box::new(mask_property(MASK_OPACITY_KEY, scalar(0.5))));
    let (sequence, project) = sequence_with(TrackKind::Video, vec![bad]);
    assert!(sequence.validate(&project).is_err());
    // Audio retime policy must stay Reject.
    let mut bad = adjustment_clip(0, 1);
    bad.audio_retime = AudioRetimePolicy::ResampleV1;
    let (sequence, project) = sequence_with(TrackKind::Video, vec![bad]);
    assert!(sequence.validate(&project).is_err());
}

#[test]
fn fx007_adjustment_clip_keeps_mask_and_effect_support() {
    let (mask, properties) = mask_with_properties();
    let mut adjustment = adjustment_clip(0, 1);
    adjustment.masks = vec![mask];
    adjustment.properties = properties;
    let (sequence, project) = sequence_with(TrackKind::Video, vec![adjustment]);
    sequence.validate(&project).unwrap();
}
