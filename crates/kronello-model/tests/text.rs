use kronello_model::*;
use kronello_time::{Duration, FrameRate, Time, TimeRange};
use serde_json::json;
use std::collections::BTreeMap;

fn f(x: f64) -> FiniteF64 {
    FiniteF64::new(x).unwrap()
}
fn fixture() -> (Project, SchemaRegistry) {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in text_descriptors() {
        registry.register(descriptor).unwrap();
    }
    let properties: Vec<_> = [
        ("kronello.text.font_size", Value::Scalar(f(32.0))),
        (
            "kronello.fill_color",
            Value::Color(Color::from_srgb8([10, 20, 30], Some(100))),
        ),
        ("kronello.text.wrap_width", Value::Scalar(f(100.0))),
        ("kronello.text.line_height", Value::Scalar(f(48.0))),
        ("kronello.text.alignment", Value::Enum("center".into())),
    ]
    .into_iter()
    .map(|(key, value)| {
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
            PropertySource::Constant(value),
            vec![],
            &registry,
        )
        .unwrap()
    })
    .collect();
    let text = TextDocument {
        id: ContentId::new(),
        layout_version: TEXT_LAYOUT_VERSION,
        text: "か\u{3099}日本語".into(),
        styles: vec![TextStyleSpan {
            gradient: None,
            range: TextRange { start: 0, end: 15 },
            font: FontRef {
                family: "Noto Sans CJK JP".into(),
                postscript_name: "NotoSansCJKjp-Regular".into(),
                sha256: "0".repeat(64),
                face_index: 0,
            },
            size: properties[0].id(),
            fill: properties[1].id(),
        }],
        direction: TextDirection::Horizontal,
        ruby: vec![],
        character_animations: vec![],
        wrap_width: properties[2].id(),
        line_height: properties[3].id(),
        alignment: properties[4].id(),
        path: None,
    };
    let node = SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        effects: vec![],
        id: NodeId::new(),
        kind: NodeKind::Text {
            content_ref: text.id,
        },
        containment_parent: None,
        child_order: vec![],
        transform_parent: None,
        active_range: TimeRange::new(Time::ZERO, Time::new(10, 1).unwrap()).unwrap(),
        properties,
    };
    let composition = Composition {
        id: CompositionId::new(),
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        duration: Duration::new(Time::new(10, 1).unwrap()).unwrap(),
        properties: vec![],
        root_nodes: vec![node.id],
        nodes: vec![node],
    };
    (
        Project {
            compositions: vec![DocumentObject::Known(composition)],
            texts: vec![DocumentObject::Known(text)],
            ..Project::default()
        },
        registry,
    )
}
fn text(project: &Project) -> &TextDocument {
    match &project.texts[0] {
        DocumentObject::Known(t) => t,
        _ => panic!("known text"),
    }
}
fn text_mut(project: &mut Project) -> &mut TextDocument {
    match &mut project.texts[0] {
        DocumentObject::Known(t) => t,
        _ => panic!("known text"),
    }
}
fn node(project: &Project) -> &SceneNode {
    match &project.compositions[0] {
        DocumentObject::Known(c) => &c.nodes[0],
        _ => panic!("known composition"),
    }
}
fn values(project: &Project) -> BTreeMap<PropertyId, Value> {
    node(project)
        .properties
        .iter()
        .map(|p| {
            (
                p.id(),
                match p.source() {
                    PropertySource::Constant(v) => v.clone(),
                    _ => panic!("constant"),
                },
            )
        })
        .collect()
}

#[test]
fn text_content_project_roundtrip_reference_validation_and_property_resolution() {
    let (project, registry) = fixture();
    project.ensure_editable().unwrap();
    validate_text_contents(&project, &registry).unwrap();
    let encoded = serde_json::to_string(&project).unwrap();
    assert_eq!(serde_json::from_str::<Project>(&encoded).unwrap(), project);
    assert_eq!(text(&project).property_ids().len(), 5);
    let resolved = text(&project).resolve(&values(&project)).unwrap();
    assert_eq!(resolved.text, "か\u{3099}日本語");
    assert_eq!(resolved.styles[0].size, f(32.0));
    assert_eq!(resolved.wrap_width, f(100.0));
    assert_eq!(resolved.line_height, f(48.0));
    assert_eq!(resolved.alignment, TextAlignment::Center);
}
#[test]
fn empty_text_is_valid_without_styles_and_legacy_project_omits_texts() {
    let mut project = Project::default();
    let encoded = serde_json::to_value(&project).unwrap();
    assert!(encoded.get("texts").is_none());
    assert_eq!(
        serde_json::from_value::<Project>(encoded.clone()).unwrap(),
        project
    );
    project.texts.clear();
    assert_eq!(serde_json::to_value(project).unwrap(), encoded);
    let (mut project, registry) = fixture();
    let t = text_mut(&mut project);
    t.text.clear();
    t.styles.clear();
    validate_text_contents(&project, &registry).unwrap();
    text(&project).resolve(&values(&project)).unwrap();
}
#[test]
fn span_gaps_overlap_out_of_range_and_grapheme_splitting_are_rejected() {
    for range in [
        TextRange { start: 1, end: 15 },
        TextRange { start: 0, end: 16 },
        TextRange { start: 0, end: 3 },
    ] {
        let (mut project, registry) = fixture();
        text_mut(&mut project).styles[0].range = range;
        assert_eq!(
            validate_text_contents(&project, &registry).unwrap_err(),
            TextError::InvalidSpans
        );
    }
    let (mut project, registry) = fixture();
    let duplicate = text(&project).styles[0].clone();
    text_mut(&mut project).styles.push(duplicate);
    assert_eq!(
        validate_text_contents(&project, &registry).unwrap_err(),
        TextError::InvalidSpans
    );
}
#[test]
fn missing_properties_wrong_units_and_post_evaluation_constraints_are_rejected() {
    let (mut project, registry) = fixture();
    text_mut(&mut project).wrap_width = PropertyId::new();
    assert!(matches!(
        validate_text_contents(&project, &registry),
        Err(TextError::MissingProperty { .. })
    ));
    let (project, registry) = fixture();
    let mut invalid = text(&project).clone();
    invalid.wrap_width = node(&project).properties[1].id();
    assert!(matches!(
        invalid.validate(&node(&project).properties, &registry),
        Err(TextError::InvalidDescriptor { .. })
    ));
    let mut evaluated = values(&project);
    evaluated.insert(text(&project).wrap_width, Value::Scalar(f(0.0)));
    assert!(matches!(
        text(&project).resolve(&evaluated),
        Err(TextError::InvalidParameter { .. })
    ));
    evaluated = values(&project);
    evaluated.insert(text(&project).alignment, Value::Enum("justify".into()));
    assert!(matches!(
        text(&project).resolve(&evaluated),
        Err(TextError::InvalidParameter { .. })
    ));
    let mut properties = node(&project).properties.clone();
    properties.push(properties[0].clone());
    assert!(matches!(
        text(&project).validate(&properties, &registry),
        Err(TextError::DuplicateProperty { .. })
    ));
}
#[test]
fn font_lock_and_ruby_ranges_are_validated_without_reading_fonts() {
    let (mut project, registry) = fixture();
    text_mut(&mut project).styles[0].font.sha256 = "XYZ".into();
    assert_eq!(
        validate_text_contents(&project, &registry).unwrap_err(),
        TextError::InvalidFontRef
    );
    let (mut project, registry) = fixture();
    text_mut(&mut project).ruby.push(RubyAssociation {
        base: TextRange { start: 0, end: 3 },
        text: "か".into(),
    });
    assert_eq!(
        validate_text_contents(&project, &registry).unwrap_err(),
        TextError::InvalidRubyRange
    );
}
#[test]
fn missing_duplicate_and_cross_kind_content_ids_are_rejected() {
    let (mut project, registry) = fixture();
    project.texts.clear();
    assert!(matches!(
        validate_text_contents(&project, &registry),
        Err(TextError::MissingContent { .. })
    ));
    let (mut project, registry) = fixture();
    project.texts.push(project.texts[0].clone());
    assert!(matches!(
        validate_text_contents(&project, &registry),
        Err(TextError::DuplicateContent { .. })
    ));
    assert!(project.validate_storage().is_err());
    let (mut project, _) = fixture();
    let id = project.id;
    text_mut(&mut project).id = ContentId::from_uuid(id);
    assert!(project.validate_storage().is_err());
}
#[test]
fn future_text_fields_roundtrip_as_opaque_and_future_meaning_is_not_editable() {
    let (project, registry) = fixture();
    let mut encoded = serde_json::to_value(&project).unwrap();
    encoded["texts"][0]["future"] = json!({"value": 12});
    let future: Project = serde_json::from_value(encoded.clone()).unwrap();
    assert!(matches!(future.texts[0], DocumentObject::Opaque(_)));
    assert_eq!(serde_json::to_value(&future).unwrap(), encoded);
    assert!(future.ensure_editable().is_err());
    assert_eq!(
        validate_text_contents(&future, &registry).unwrap_err(),
        TextError::UnsupportedContent
    );
    let mut version = project;
    text_mut(&mut version).layout_version = 999;
    version.validate_storage().unwrap();
    assert!(version.ensure_editable().is_err());
}
#[test]
fn text_descriptors_use_design_px_positive_ranges_and_hold_alignment() {
    let (_, registry) = fixture();
    for key in ["font_size", "wrap_width", "line_height"] {
        let descriptor = registry
            .lookup(&SchemaKey::new(format!("kronello.text.{key}")).unwrap())
            .unwrap();
        assert_eq!(descriptor.definition().unit, Unit::DesignPx);
        assert!(descriptor.validate_value(&Value::Scalar(f(0.0))).is_err());
        assert!(descriptor.validate_value(&Value::Scalar(f(2.0))).is_ok());
    }
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.text.alignment").unwrap())
        .unwrap();
    assert_eq!(descriptor.definition().value_type, ValueType::Enum);
    assert_eq!(
        descriptor.definition().interpolation_modes,
        std::collections::BTreeSet::from([InterpolationMode::Hold])
    );
}

#[test]
fn text002_selector_captures_source_and_rejects_stale_same_length_edits() {
    let (mut project, registry) = fixture();
    let offset = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_offset").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Vec2([f(2.0), f(0.0)])),
        vec![],
        &registry,
    )
    .unwrap();
    let opacity = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_opacity").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(f(0.0))),
        vec![],
        &registry,
    )
    .unwrap();
    text_mut(&mut project).layout_version = 2;
    text_mut(&mut project).character_animations = vec![CharacterAnimation {
        source: TextRange { start: 0, end: 6 },
        expected_text: "か\u{3099}".into(),
        offset: offset.id(),
        opacity: opacity.id(),
        scale: None,
        rotation: None,
        fill: None,
        mode: AnimatorMode::Step,
        seed: None,
        follow_smoothing: None,
    }];
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.nodes[0].properties.extend([offset, opacity]);
    validate_text_contents(&project, &registry).unwrap();
    assert_eq!(
        text(&project)
            .resolve(&values(&project))
            .unwrap()
            .character_animations[0]
            .opacity,
        f(0.0)
    );
    let encoded = serde_json::to_vec(&project).unwrap();
    let restored: Project = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(text(&restored), text(&project));
    text_mut(&mut project)
        .text
        .replace_range(0..6, "き\u{3099}");
    assert!(matches!(
        validate_text_contents(&project, &registry),
        Err(TextError::InvalidCharacterAnimation)
    ));
    assert!(matches!(
        text(&project).resolve(&values(&project)),
        Err(TextError::InvalidCharacterAnimation)
    ));
}

#[test]
fn vec006_path_and_animator_fields_roundtrip_and_default_when_absent() {
    let (mut project, registry) = fixture();
    let path_property = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.path").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo([f(0.0), f(0.0)]),
                PathSegment::LineTo([f(300.0), f(0.0)]),
            ],
        })),
        vec![],
        &registry,
    )
    .unwrap();
    let smoothing = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_follow_smoothing").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(f(0.5))),
        vec![],
        &registry,
    )
    .unwrap();
    let scale = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_scale").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Vec2([f(2.0), f(2.0)])),
        vec![],
        &registry,
    )
    .unwrap();
    let rotation = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_rotation").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Angle(f(45.0))),
        vec![],
        &registry,
    )
    .unwrap();
    let fill = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_fill").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Color(Color::from_srgb8([255, 0, 0], None))),
        vec![],
        &registry,
    )
    .unwrap();
    let offset = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_offset").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Vec2([f(0.0), f(0.0)])),
        vec![],
        &registry,
    )
    .unwrap();
    let opacity = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_opacity").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(f(1.0))),
        vec![],
        &registry,
    )
    .unwrap();
    {
        let t = text_mut(&mut project);
        t.layout_version = 2;
        t.path = Some(path_property.id());
        t.character_animations = vec![CharacterAnimation {
            source: TextRange { start: 0, end: 6 },
            expected_text: "か\u{3099}".into(),
            offset: offset.id(),
            opacity: opacity.id(),
            scale: Some(scale.id()),
            rotation: Some(rotation.id()),
            fill: Some(fill.id()),
            mode: AnimatorMode::Follow,
            seed: Some(42),
            follow_smoothing: Some(smoothing.id()),
        }];
    }
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.nodes[0].properties.extend([
        path_property,
        smoothing,
        scale,
        rotation,
        fill,
        offset,
        opacity,
    ]);
    validate_text_contents(&project, &registry).unwrap();
    let resolved = text(&project).resolve(&values(&project)).unwrap();
    assert_eq!(resolved.path.unwrap().segments.len(), 2);
    let animation = &resolved.character_animations[0];
    assert_eq!(animation.scale, Some([f(2.0), f(2.0)]));
    assert_eq!(animation.rotation, Some(f(45.0)));
    assert_eq!(animation.mode, AnimatorMode::Follow);
    assert_eq!(animation.seed, Some(42));
    assert_eq!(animation.follow_smoothing, Some(f(0.5)));
    // Binary roundtrip preserves every new field.
    let encoded = serde_json::to_vec(&project).unwrap();
    assert_eq!(
        text(&serde_json::from_slice::<Project>(&encoded).unwrap()),
        text(&project)
    );
    // Documents authored before VEC-006 omit the fields and still load.
    let mut legacy = serde_json::to_value(&project).unwrap();
    let animation_json = &mut legacy["texts"][0]["character_animations"][0];
    for key in [
        "scale",
        "rotation",
        "fill",
        "mode",
        "seed",
        "follow_smoothing",
    ] {
        animation_json.as_object_mut().unwrap().remove(key);
    }
    legacy["texts"][0].as_object_mut().unwrap().remove("path");
    let mut restored: Project = serde_json::from_value(legacy).unwrap();
    let animation = &text(&restored).character_animations[0];
    assert_eq!(animation.mode, AnimatorMode::Step);
    assert_eq!(animation.seed, None);
    assert!(animation.scale.is_none() && animation.rotation.is_none() && animation.fill.is_none());
    assert!(text(&restored).path.is_none());
    // Re-serializing restores the pre-VEC-006 shape: defaults stay omitted.
    let reencoded = serde_json::to_value(&restored).unwrap();
    assert!(reencoded["texts"][0].get("path").is_none());
    assert!(
        reencoded["texts"][0]["character_animations"][0]
            .get("mode")
            .is_none()
    );
    // A legacy document validates and resolves exactly as before.
    text_mut(&mut restored).character_animations.clear();
    validate_text_contents(&restored, &registry).unwrap();
    let resolved = text(&restored).resolve(&values(&restored)).unwrap();
    assert!(resolved.path.is_none() && resolved.character_animations.is_empty());
}

#[test]
fn vec006_follow_requires_smoothing_and_unit_interval_range() {
    let (mut project, registry) = fixture();
    let offset = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_offset").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Vec2([f(0.0), f(0.0)])),
        vec![],
        &registry,
    )
    .unwrap();
    let opacity = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_opacity").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(f(1.0))),
        vec![],
        &registry,
    )
    .unwrap();
    let t = text_mut(&mut project);
    t.layout_version = 2;
    t.character_animations = vec![CharacterAnimation {
        source: TextRange { start: 0, end: 3 },
        expected_text: "か".into(),
        offset: offset.id(),
        opacity: opacity.id(),
        scale: None,
        rotation: None,
        fill: None,
        mode: AnimatorMode::Follow,
        seed: None,
        follow_smoothing: None,
    }];
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.nodes[0].properties.extend([offset, opacity]);
    assert_eq!(
        validate_text_contents(&project, &registry).unwrap_err(),
        TextError::InvalidCharacterAnimation
    );
    // An out-of-range smoothing constant is rejected the same way opacity is.
    let smoothing = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.character_follow_smoothing").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(f(1.5))),
        vec![],
        &registry,
    )
    .unwrap();
    let t = text_mut(&mut project);
    t.character_animations[0].follow_smoothing = Some(smoothing.id());
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.nodes[0].properties.push(smoothing);
    assert_eq!(
        validate_text_contents(&project, &registry).unwrap_err(),
        TextError::InvalidCharacterAnimation
    );
}

#[test]
fn vec006_path_property_requires_a_valid_path_value() {
    let (mut project, registry) = fixture();
    // A Path property whose segments never open a subpath is rejected.
    let bad = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.text.path").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Path(Path {
            segments: vec![PathSegment::LineTo([f(1.0), f(1.0)])],
        })),
        vec![],
        &registry,
    )
    .unwrap();
    let t = text_mut(&mut project);
    t.path = Some(bad.id());
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.nodes[0].properties.push(bad);
    assert!(matches!(
        validate_text_contents(&project, &registry),
        Err(TextError::InvalidParameter { .. })
    ));
    // A non-Path property cannot stand in for the guide.
    let (project, registry) = fixture();
    let mut wrong = text(&project).clone();
    wrong.path = Some(wrong.wrap_width);
    assert!(matches!(
        wrong.validate(&node(&project).properties, &registry),
        Err(TextError::InvalidDescriptor { .. })
    ));
}
