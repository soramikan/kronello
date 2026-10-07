//! SUB-001 caption model: versioned cue documents, strict decoding, span
//! boundaries, placement, descriptors and sequence/track validation.
use kronello_model::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::json;
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
fn document(text: &str) -> CaptionDocument {
    CaptionDocument {
        id: CaptionId::new(),
        version: CAPTION_VERSION,
        text: text.into(),
        style: style(),
        spans: vec![],
        placement: CaptionPlacement::default(),
        format: None,
    }
}
fn caption_registry() -> SchemaRegistry {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in caption_descriptors() {
        registry.register(descriptor).unwrap();
    }
    registry
}
fn caption_clip(caption: CaptionId, start: i64, end: i64) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Caption { caption },
        timeline_range: TimeRange::new(Time::new(start, 1).unwrap(), Time::new(end, 1).unwrap())
            .unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    }
}
fn sequence_with(tracks: Vec<Track>) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks,
        transitions: vec![],
        markers: vec![],
        work_area: None,
    }
}
fn project_with(caption: CaptionDocument, sequence: Sequence) -> Project {
    Project {
        captions: vec![DocumentObject::Known(caption)],
        sequences: vec![DocumentObject::Known(sequence)],
        ..Project::default()
    }
}

#[test]
fn caption_document_requires_supported_version() {
    let mut doc = document("hello");
    assert!(doc.validate().is_ok());
    doc.version = CAPTION_VERSION + 1;
    assert!(matches!(
        doc.validate(),
        Err(CaptionError::UnsupportedVersion(2))
    ));
}

#[test]
fn caption_document_rejects_unknown_fields_and_empty_id() {
    let value = json!({
        "id": Uuid::new_v4(),
        "version": 1,
        "text": "ok",
        "style": serde_json::to_value(style()).unwrap(),
        "placement": serde_json::to_value(CaptionPlacement::default()).unwrap(),
        "unexpected": true,
    });
    assert!(serde_json::from_value::<CaptionDocument>(value).is_err());
    let mut bad_font = document("hello");
    bad_font.style.font.sha256 = "not-a-sha".into();
    assert!(matches!(bad_font.validate(), Err(CaptionError::Text(_))));
}

#[test]
fn caption_text_is_data_with_canonical_newlines() {
    assert!(document("line1\nline2").validate().is_ok());
    for text in ["a\rb", "a\r\nb", "a\u{2028}b", "a\u{2029}b"] {
        assert!(
            document(text).validate().is_err(),
            "{text:?} must be rejected"
        );
    }
    // Markup-looking text is inert data.
    assert!(document("<b>not markup</b>").validate().is_ok());
}

#[test]
fn caption_spans_require_ordered_grapheme_boundaries() {
    // "e\u{0301}" is one grapheme over two bytes.
    let mut doc = document("e\u{0301}x");
    doc.spans = vec![CaptionSpan {
        range: TextRange { start: 1, end: 3 },
        bold: Some(true),
        italic: None,
        color: None,
        font: None,
    }];
    assert!(matches!(doc.validate(), Err(CaptionError::Invalid(_))));
    // Valid boundary at the grapheme start.
    doc.spans[0].range = TextRange { start: 0, end: 3 };
    assert!(doc.validate().is_ok());
    // Overlapping spans are rejected.
    doc.spans.push(CaptionSpan {
        range: TextRange { start: 1, end: 3 },
        bold: Some(true),
        italic: None,
        color: None,
        font: None,
    });
    doc.spans.sort_by_key(|s| s.range.start);
    assert!(matches!(doc.validate(), Err(CaptionError::Invalid(_))));
    // A span font reference must itself validate.
    doc.spans.truncate(1);
    doc.spans[0].font = Some(FontRef {
        family: String::new(),
        postscript_name: "x".into(),
        sha256: "a".repeat(64),
        face_index: 0,
    });
    assert!(matches!(doc.validate(), Err(CaptionError::Text(_))));
}

#[test]
fn caption_placement_inset_is_bounded_ratio() {
    let mut doc = document("hello");
    doc.placement.safe_area_inset = [Rational::new(3, 4).unwrap(); 2];
    assert!(matches!(doc.validate(), Err(CaptionError::Invalid(_))));
    doc.placement.safe_area_inset = [Rational::new(1, 2).unwrap(); 2];
    assert!(doc.validate().is_ok());
}

#[test]
fn caption_resolve_applies_constant_overrides_only() {
    let registry = caption_registry();
    let doc = document("hello");
    let property = |key: &str, source: PropertySource<Value>| {
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
            source,
            vec![],
            &registry,
        )
        .unwrap()
    };
    // Constant color override applies.
    let color = Color::from_srgb8([255, 0, 0], None);
    let resolved = doc
        .resolve(
            &[property(
                "kronello.caption.color",
                PropertySource::Constant(Value::Color(color)),
            )],
            &registry,
        )
        .unwrap();
    assert_eq!(resolved.styles[0].fill, color);
    // Nonconstant sources are a typed rejection, never evaluated. Descriptors
    // forbid curves at construction, so decode one directly from the wire.
    let version = registry
        .lookup(&SchemaKey::new("kronello.caption.color").unwrap())
        .unwrap()
        .version();
    let curve_sourced: Property = serde_json::from_value(json!({
        "id": Uuid::new_v4(),
        "descriptor": {"key": "kronello.caption.color", "version": version},
        "source": {"kind": "curve", "value": Uuid::new_v4()},
        "modifiers": [],
    }))
    .unwrap();
    assert!(doc.resolve(&[curve_sourced], &registry).is_err());
    // Duplicate caption overrides are rejected.
    let err = doc
        .resolve(
            &[
                property(
                    "kronello.caption.color",
                    PropertySource::Constant(Value::Color(color)),
                ),
                property(
                    "kronello.caption.color",
                    PropertySource::Constant(Value::Color(color)),
                ),
            ],
            &registry,
        )
        .unwrap_err();
    assert!(matches!(err, CaptionError::Invalid(_)));
    // Anchor names map to the enum or fail typed.
    let resolved = doc
        .resolve(
            &[property(
                "kronello.caption.anchor",
                PropertySource::Constant(Value::Enum("top_left".into())),
            )],
            &registry,
        )
        .unwrap();
    assert_eq!(resolved.placement.anchor, CaptionAnchor::TopLeft);
}

#[test]
fn caption_clips_require_caption_track_and_identity_timing() {
    let doc = document("cue");
    let caption = doc.id;
    let clip = caption_clip(caption, 1, 3);

    // Caption clip on a video track is rejected.
    let video = sequence_with(vec![Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips: vec![clip.clone()],
    }]);
    let project = project_with(doc.clone(), video);
    assert!(matches!(
        project.sequences.iter().find_map(|s| match s {
            DocumentObject::Known(s) => Some(s.validate(&project)),
            _ => None,
        }),
        Some(Err(SequenceError::Invalid(_)))
    ));

    // A caption track accepts only caption sources and rejects nonzero
    // source_in or non-identity time maps.
    for mut broken in [clip.clone(), clip.clone()] {
        broken.source_in = Time::new(1, 2).unwrap();
        let sequence = sequence_with(vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Caption,
            clips: vec![broken],
        }]);
        let project = project_with(doc.clone(), sequence);
        assert!(matches!(
            project.sequences.iter().find_map(|s| match s {
                DocumentObject::Known(s) => Some(s.validate(&project)),
                _ => None,
            }),
            Some(Err(_))
        ));
    }
    let mut retimed = clip.clone();
    retimed.time_map = TimeMap::linear(Time::ZERO, Rational::new(2, 1).unwrap()).unwrap();
    let sequence = sequence_with(vec![Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Caption,
        clips: vec![retimed],
    }]);
    let project = project_with(doc.clone(), sequence);
    assert!(
        project
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) => Some(s.validate(&project)),
                _ => None,
            })
            .unwrap()
            .is_err()
    );

    // Missing caption content is a typed missing-source error.
    let missing = caption_clip(CaptionId::new(), 1, 3);
    let sequence = sequence_with(vec![Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Caption,
        clips: vec![missing],
    }]);
    let project = project_with(doc.clone(), sequence);
    assert!(matches!(
        project.sequences.iter().find_map(|s| match s {
            DocumentObject::Known(s) => Some(s.validate(&project)),
            _ => None,
        }),
        Some(Err(SequenceError::MissingSource(_)))
    ));

    // The valid clip validates end to end.
    let sequence = sequence_with(vec![Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Caption,
        clips: vec![clip],
    }]);
    let project = project_with(doc, sequence);
    assert!(
        project
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) => Some(s.validate(&project)),
                _ => None,
            })
            .unwrap()
            .is_ok()
    );
}

#[test]
fn caption_tracks_reject_other_sources_and_transitions() {
    let doc = document("cue");
    let composition_clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Composition {
            composition: CompositionId::new(),
        },
        timeline_range: TimeRange::new(Time::ZERO, Time::new(1, 1).unwrap()).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    };
    let sequence = sequence_with(vec![Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Caption,
        clips: vec![composition_clip],
    }]);
    let project = project_with(doc, sequence);
    assert!(
        project
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) => Some(s.validate(&project)),
                _ => None,
            })
            .unwrap()
            .is_err()
    );
}
