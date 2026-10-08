//! FLOW-002/003 document model: bins and shared export presets (ADR-0129/0130).
use kronello_model::*;
use kronello_time::{FrameRate, Rational, TimeRange};

fn asset(id: uuid::Uuid) -> Asset {
    Asset {
        id: AssetId::from_uuid(id),
        content_hash: "ab".repeat(32),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("a.bin".into()),
            absolute: None,
        },
    }
}
fn preset(target: ExportTarget) -> ExportPreset {
    ExportPreset {
        version: EXPORT_PRESET_VERSION,
        id: ExportPresetId::new(),
        name: "web".into(),
        composition: None,
        target: Some(target),
        range: TimeRange::new(Rational::ZERO, Rational::new(1, 24).unwrap()).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: ExportRegion {
            origin: [0.0; 2],
            extent: [64.0; 2],
            pixels: [64; 2],
        },
        profile: ExportProfile::default(),
        output: ExportOutput::ImageSequence,
        required_features: vec![],
    }
}

#[test]
fn project_roundtrips_bins_and_presets_and_omits_empty_collections() {
    let mut project = Project::default();
    let value = serde_json::to_value(&project).unwrap();
    // Backward compatibility: documents without the fields decode with
    // defaults, and empty collections never serialize (ADR-0129/0130).
    assert!(value.get("bins").is_none() && value.get("export_presets").is_none());
    let decoded: Project = serde_json::from_value(value).unwrap();
    assert!(decoded.bins.is_empty() && decoded.export_presets.is_empty());
    let asset = asset(uuid::Uuid::from_u128(7));
    let mut bin = Bin::new(BinId::new(), "shots");
    bin.assets.push(asset.id);
    project.bins.push(bin);
    project.export_presets.push(preset(ExportTarget::Sequence {
        sequence: SequenceId::new(),
    }));
    // The preset above references a missing sequence and must not store.
    assert!(project.validate_storage().is_err());
    project.export_presets.clear();
    project.assets.push(DocumentObject::Known(asset));
    let json = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, project);
    assert_eq!(decoded.bins[0].assets, vec![decoded.bins[0].assets[0]]);
}

#[test]
fn bin_validation_rejects_empty_names_and_duplicate_members() {
    let mut bin = Bin::new(BinId::new(), "  ");
    assert!(bin.validate().is_err());
    bin.name = "selects".into();
    let id = AssetId::new();
    bin.assets = vec![id, id];
    assert!(bin.validate().is_err());
    bin.assets = vec![id];
    bin.validate().unwrap();
}

#[test]
fn bin_membership_must_reference_project_assets() {
    let mut project = Project::default();
    let mut bin = Bin::new(BinId::new(), "shots");
    bin.assets.push(AssetId::new());
    project.bins.push(bin);
    assert!(project.validate_storage().is_err());
    let a = asset(uuid::Uuid::from_u128(9));
    project.bins[0].assets = vec![a.id];
    project.assets.push(DocumentObject::Known(a));
    project.validate_storage().unwrap();
    // Duplicate bin ids are project-level identity violations.
    project.bins.push(Bin::new(
        BinId::from_uuid(project.bins[0].id.as_uuid()),
        "dup",
    ));
    assert!(project.validate_storage().is_err());
}

#[test]
fn opaque_asset_membership_is_allowed() {
    let opaque_id = uuid::Uuid::from_u128(11);
    let mut project = Project::default();
    project.assets.push(DocumentObject::Opaque(OpaqueObject {
        id: opaque_id,
        fields: [("kind".into(), serde_json::json!("future"))]
            .into_iter()
            .collect(),
    }));
    let mut bin = Bin::new(BinId::new(), "future");
    bin.assets.push(AssetId::from_uuid(opaque_id));
    project.bins.push(bin);
    project.validate_storage().unwrap();
}

#[test]
fn preset_field_validation_is_strict_and_versioned() {
    let sequence = SequenceId::new();
    let mut p = preset(ExportTarget::Sequence { sequence });
    p.validate().unwrap();
    p.version = 0;
    assert!(p.validate().is_err());
    p.version = EXPORT_PRESET_VERSION;
    p.name = " ".into();
    assert!(p.validate().is_err());
    p.name = "web".into();
    // Exactly one of composition/target, matching the RenderInput rule.
    p.composition = Some(CompositionId::new());
    assert!(p.validate().is_err());
    p.composition = None;
    // A sidecar preset must target the same sequence it serializes.
    p.output = ExportOutput::CaptionSidecar {
        sequence: SequenceId::new(),
        caption_format: CaptionFormat::Srt,
    };
    assert!(p.validate().is_err());
    p.output = ExportOutput::CaptionSidecar {
        sequence,
        caption_format: CaptionFormat::Srt,
    };
    p.validate().unwrap();
}

#[test]
fn preset_references_validate_against_the_document() {
    let sequence = SequenceId::new();
    let mut p = preset(ExportTarget::Sequence { sequence });
    let mut project = Project::default();
    project.export_presets.push(p.clone());
    assert!(project.validate_storage().is_err());
    project.export_presets.clear();
    // Sequence targets validate against sequences, compositions likewise.
    project.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: kronello_time::SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    p.output = ExportOutput::ProResMov {
        audio: ExportAudioMode::Explicit,
        profile_version: 1,
        clips: vec![ExportAudioClip {
            asset: AssetId::new(),
            stream_index: 0,
            placement: TimeRange::new(Rational::ZERO, Rational::from_integer(1)).unwrap(),
            source_in: Rational::ZERO,
            gain: 1.0,
        }],
        background: [0.0; 3],
    };
    project.export_presets.push(p);
    // The audio clip asset is missing.
    assert!(project.validate_storage().is_err());
    let a = asset(uuid::Uuid::from_u128(21));
    project.assets.push(DocumentObject::Known(a.clone()));
    let mut p = project.export_presets[0].clone();
    if let ExportOutput::ProResMov { clips, .. } = &mut p.output {
        clips[0].asset = a.id;
    }
    project.export_presets[0] = p;
    project.validate_storage().unwrap();
}
