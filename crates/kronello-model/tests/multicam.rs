//! ADR-0127 model contracts: multicam groups, stable angle identity, and
//! `SourceRef::Multicam` validation against the owning document.
use kronello_model::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn video(id: AssetId, hash: char, start: Option<Rational>, duration: Option<Rational>) -> Asset {
    Asset {
        id,
        content_hash: hash.to_string().repeat(64),
        kind: AssetKind::Video,
        streams: vec![
            StreamMetadata {
                index: 0,
                codec: "prores".into(),
                time_base: r(1, 24),
                duration,
                start_time: start,
                width: Some(1920),
                height: Some(1080),
                pixel_format: None,
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            },
            StreamMetadata {
                index: 1,
                codec: "pcm".into(),
                time_base: r(1, 48_000),
                duration,
                start_time: start,
                width: None,
                height: None,
                pixel_format: None,
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            },
        ],
        locator: AssetLocator {
            relative: Some(format!("{id}.mov")),
            absolute: None,
        },
    }
}
fn angle(id: AngleId, asset: AssetId, stream: u32, offset: Time) -> MulticamAngle {
    MulticamAngle {
        id,
        asset,
        stream_index: stream,
        sync_offset: offset,
        name: format!("Angle {id}"),
    }
}
fn group() -> (MulticamAsset, AssetId, AssetId) {
    let a = AssetId::new();
    let b = AssetId::new();
    (
        MulticamAsset {
            id: MulticamId::new(),
            name: "Interview".into(),
            angles: vec![
                angle(AngleId::new(), a, 0, Time::ZERO),
                angle(AngleId::new(), b, 0, r(1, 4)),
            ],
        },
        a,
        b,
    )
}
fn document(group: &MulticamAsset, a: AssetId, b: AssetId) -> Project {
    let mut document = Project::default();
    for (id, hash) in [(a, 'a'), (b, 'b')] {
        document.assets.push(DocumentObject::Known(video(
            id,
            hash,
            Some(Time::ZERO),
            Some(r(30, 1)),
        )));
    }
    document.multicams.push(group.clone());
    document
}
fn multicam_clip(source: SourceRef, track_range: TimeRange) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: source,
        timeline_range: track_range,
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        properties: vec![],
        markers: vec![],
    }
}
fn sequence_with(kind: TrackKind, clip: Clip) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
        extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind,
            clips: vec![clip],
        }],
    }
}

#[test]
fn source_ref_multicam_round_trips_strictly() {
    let source = SourceRef::Multicam {
        multicam: MulticamId::new(),
        angle: AngleId::new(),
    };
    let json = serde_json::to_value(&source).unwrap();
    assert_eq!(json["kind"], "multicam");
    assert_eq!(serde_json::from_value::<SourceRef>(json).unwrap(), source);
    // Unknown fields and duplicate identity keys fail strict decoding.
    let mut extra = serde_json::to_value(&source).unwrap();
    extra["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<SourceRef>(extra).is_err());
    assert!(
        serde_json::from_value::<SourceRef>(serde_json::json!({
            "kind": "multicam",
            "multicam": MulticamId::new(),
            "angle": AngleId::new(),
            "stream_index": 0,
        }))
        .is_err()
    );
}

#[test]
fn multicam_asset_validation() {
    let (group, _, _) = group();
    group.validate().unwrap();
    // Duplicate angle id.
    let mut dup = group.clone();
    dup.angles[1].id = dup.angles[0].id;
    assert!(dup.validate().is_err());
    // Angle id aliasing the group id.
    let mut alias = group.clone();
    alias.angles[0].id = AngleId::from_uuid(alias.id.as_uuid());
    assert!(alias.validate().is_err());
    // Angle asset aliasing a multicam identity is ambiguous.
    let mut shadow = group.clone();
    shadow.angles[1].asset = AssetId::from_uuid(shadow.id.as_uuid());
    assert!(shadow.validate().is_err());
    // Empty and oversized angle sets, and name bounds.
    let mut empty = group.clone();
    empty.angles.clear();
    assert!(empty.validate().is_err());
    let mut oversized = group.clone();
    oversized.angles = (0..65)
        .map(|_| angle(AngleId::new(), AssetId::new(), 0, Time::ZERO))
        .collect();
    assert!(oversized.validate().is_err());
    let mut long_name = group.clone();
    long_name.name = "x".repeat(257);
    assert!(long_name.validate().is_err());
    let mut empty_name = group.clone();
    empty_name.name.clear();
    assert!(empty_name.validate().is_err());
    let mut long_angle = group.clone();
    long_angle.angles[0].name = "y".repeat(257);
    assert!(long_angle.validate().is_err());
}

#[test]
fn project_rejects_duplicate_multicam_identity() {
    let (group, a, b) = group();
    let document = document(&group, a, b);
    document.validate_storage().unwrap();
    // A second group sharing the id.
    let mut dup = document.clone();
    dup.multicams.push(group.clone());
    assert!(dup.validate_storage().is_err());
    // A second group sharing an angle id.
    let mut dup_angle = document.clone();
    let mut second = group.clone();
    second.id = MulticamId::new();
    dup_angle.multicams.push(second);
    assert!(dup_angle.validate_storage().is_err());
    // An angle id aliasing an asset identity.
    let mut alias = document.clone();
    alias.multicams[0].angles[0].id = AngleId::from_uuid(a.as_uuid());
    assert!(alias.validate_storage().is_err());
}

#[test]
fn sequence_validates_multicam_clip_references() {
    let (group, a, b) = group();
    let document = document(&group, a, b);
    let clip = multicam_clip(
        SourceRef::Multicam {
            multicam: group.id,
            angle: group.angles[0].id,
        },
        range(Time::ZERO, r(2, 1)),
    );
    // Video track placement is valid; audio/caption tracks reject picture.
    sequence_with(TrackKind::Video, clip.clone())
        .validate(&document)
        .unwrap();
    for kind in [TrackKind::Audio, TrackKind::Caption] {
        assert!(
            sequence_with(kind, clip.clone())
                .validate(&document)
                .is_err()
        );
    }
    // Missing group, missing angle, missing stream.
    let mut missing_group = document.clone();
    missing_group.multicams.clear();
    assert!(matches!(
        sequence_with(TrackKind::Video, clip.clone()).validate(&missing_group),
        Err(SequenceError::MissingSource(_))
    ));
    let bad_angle = multicam_clip(
        SourceRef::Multicam {
            multicam: group.id,
            angle: AngleId::new(),
        },
        range(Time::ZERO, r(2, 1)),
    );
    assert!(matches!(
        sequence_with(TrackKind::Video, bad_angle).validate(&document),
        Err(SequenceError::MissingSource(_))
    ));
    let mut missing_asset = document.clone();
    missing_asset
        .assets
        .retain(|object| !matches!(object, DocumentObject::Known(asset) if asset.id == a));
    assert!(matches!(
        sequence_with(TrackKind::Video, clip.clone()).validate(&missing_asset),
        Err(SequenceError::MissingSource(_))
    ));
    let mut missing_stream = document.clone();
    let DocumentObject::Known(asset) = &mut missing_stream.assets[0] else {
        panic!()
    };
    asset.streams.retain(|s| s.index != 0);
    assert!(matches!(
        sequence_with(TrackKind::Video, clip.clone()).validate(&missing_stream),
        Err(SequenceError::MissingSource(_))
    ));
    // Audio-kind assets never carry multicam picture.
    let mut audio_kind = document.clone();
    let DocumentObject::Known(asset) = &mut audio_kind.assets[0] else {
        panic!()
    };
    asset.kind = AssetKind::Audio;
    assert!(
        sequence_with(TrackKind::Video, clip.clone())
            .validate(&audio_kind)
            .is_err()
    );
}

#[test]
fn multicam_clip_bounds_use_sync_offset() {
    let (group, a, b) = group();
    let mut document = document(&group, a, b);
    // Angle B starts 2s into its PTS clock and lasts 5s: media_time =
    // multicam_time + offset must stay inside [2, 7].
    let DocumentObject::Known(asset) = &mut document.assets[1] else {
        panic!()
    };
    asset.streams[0].start_time = Some(r(2, 1));
    asset.streams[0].duration = Some(r(5, 1));
    let angle_b = group.angles[1].id;
    let source = SourceRef::Multicam {
        multicam: group.id,
        angle: angle_b,
    };
    document.multicams[0].angles[1].sync_offset = r(2, 1);
    // multicam [0, 2) + offset 2 -> media [2, 4] inside [2, 7].
    sequence_with(
        TrackKind::Video,
        multicam_clip(source.clone(), range(Time::ZERO, r(2, 1))),
    )
    .validate(&document)
    .unwrap();
    // Negative offset shifts below the stream start.
    document.multicams[0].angles[1].sync_offset = r(-4, 1);
    assert!(
        sequence_with(
            TrackKind::Video,
            multicam_clip(source.clone(), range(Time::ZERO, r(2, 1))),
        )
        .validate(&document)
        .is_err()
    );
    // media_end beyond origin + duration is rejected.
    document.multicams[0].angles[1].sync_offset = r(2, 1);
    assert!(
        sequence_with(
            TrackKind::Video,
            multicam_clip(source.clone(), range(Time::ZERO, r(6, 1))),
        )
        .validate(&document)
        .is_err()
    );
}

#[test]
fn asset_in_use_covers_multicam_angles() {
    let (group, a, b) = group();
    let document = document(&group, a, b);
    assert!(document.asset_in_use(a));
    assert!(document.asset_in_use(b));
    let unused = AssetId::new();
    assert!(!document.asset_in_use(unused));
}
