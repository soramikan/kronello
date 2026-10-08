//! FLOW-002 (ADR-0129): bins, media.query and asset.thumbnail through the
//! shared service surface every transport calls.
use kronello_media::content_hash;
use kronello_model::*;
use kronello_service::*;
use serde_json::Value as Json;
use std::path::PathBuf;
use uuid::Uuid;

fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
/// Assets are written next to this project path so relative locators resolve.
fn setup_at(project: PathBuf, document: Project) -> PathBuf {
    let ResultData::Project(_) = service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: project.clone(),
            document,
        }))
        .unwrap()
    else {
        panic!()
    };
    project
}
fn apply(
    project: &std::path::Path,
    base_revision: &str,
    commands: Vec<EditCommand>,
) -> Result<Json, ServiceError> {
    let plan = match service().dispatch(Request::EditPlan(PlanRequest {
        project: project.to_path_buf(),
        base_revision: base_revision.into(),
        commands: commands.clone(),
    })) {
        Ok(ResultData::Plan(plan)) => plan,
        Err(error) => return Err(error),
        other => panic!("{other:?}"),
    };
    match service().dispatch(Request::EditApply(EditApplyRequest {
        project: project.to_path_buf(),
        base_revision: base_revision.into(),
        plan_hash: plan.plan_hash,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        commands,
    })) {
        Ok(ResultData::Edit(event)) => Ok(serde_json::to_value(event).unwrap()),
        Err(error) => Err(error),
        other => panic!("{other:?}"),
    }
}
fn revision(project: &std::path::Path) -> String {
    let ResultData::Project(info) = service()
        .dispatch(Request::ProjectInfo(ProjectRequest {
            project: project.to_path_buf(),
        }))
        .unwrap()
    else {
        panic!()
    };
    info.revision
}
fn png_asset(dir: &std::path::Path, name: &str, width: u32, height: u32, rgba: [u8; 4]) -> Asset {
    let path = dir.join(name);
    let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&rgba.repeat((width * height) as usize))
        .unwrap();
    writer.finish().unwrap();
    Asset {
        id: AssetId::new(),
        content_hash: content_hash(&path).unwrap(),
        kind: AssetKind::Image,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "png".into(),
            time_base: kronello_time::Rational::new(1, 24).unwrap(),
            duration: None,
            start_time: None,
            width: Some(width),
            height: Some(height),
            pixel_format: Some("rgba".into()),
            color_primaries: Some("bt709".into()),
            color_transfer: Some("iec61966-2-1".into()),
            color_matrix: Some("gbr".into()),
            color_range: Some("pc".into()),
        }],
        locator: AssetLocator {
            relative: Some(name.into()),
            absolute: None,
        },
    }
}

#[test]
fn bins_edit_through_shared_commands_persist_and_media_query_reports() {
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("a.mov");
    std::fs::write(&present, b"video-bytes").unwrap();
    let a = Asset {
        id: AssetId::new(),
        content_hash: content_hash(&present).unwrap(),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("a.mov".into()),
            absolute: None,
        },
    };
    let b = Asset {
        id: AssetId::new(),
        content_hash: "00".repeat(32),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("missing.mov".into()),
            absolute: None,
        },
    };
    let (a_id, b_id) = (a.id, b.id);
    let document = Project {
        assets: vec![DocumentObject::Known(a), DocumentObject::Known(b)],
        ..Project::default()
    };
    let project = setup_at(dir.path().join("media.kronello"), document);
    let bin = BinId::new();
    apply(
        &project,
        "1",
        vec![EditCommand::BinCreate {
            bin: Bin::new(bin, "素材"),
        }],
    )
    .unwrap();
    // Duplicate bin ids are a typed rejection, not an upsert.
    let error = apply(
        &project,
        "2",
        vec![EditCommand::BinCreate {
            bin: Bin::new(bin, "again"),
        }],
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_EDIT");
    apply(
        &project,
        "2",
        vec![EditCommand::BinAssign {
            bin,
            assets: vec![a_id, b_id],
        }],
    )
    .unwrap();
    let ResultData::Media(media) = service()
        .dispatch(Request::MediaQuery(MediaQueryRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(media.bins.len(), 1);
    assert_eq!(media.bins[0].assets, vec![a_id, b_id]);
    // Assets report in document order with the shared locator probe: the file
    // on disk is present_unverified, the unresolvable one is missing.
    assert_eq!(media.assets.len(), 2);
    assert_eq!(media.assets[0].asset, a_id);
    assert_eq!(
        media.assets[0].availability,
        AssetAvailability::PresentUnverified
    );
    assert_eq!(media.assets[0].size_bytes, Some(11));
    assert_eq!(media.assets[1].asset, b_id);
    assert_eq!(media.assets[1].availability, AssetAvailability::Missing);
    assert!(media.assets[1].error.is_some());
    // Membership rejects unknown assets and duplicates.
    for assets in [vec![a_id, a_id], vec![a_id, AssetId::new()]] {
        let error = apply(&project, "3", vec![EditCommand::BinAssign { bin, assets }]).unwrap_err();
        assert_eq!(error.code, "INVALID_EDIT");
    }
    apply(
        &project,
        "3",
        vec![EditCommand::BinRename {
            bin,
            name: "  ".into(),
        }],
    )
    .unwrap_err();
    apply(
        &project,
        "3",
        vec![EditCommand::BinRename {
            bin,
            name: "ダビング".into(),
        }],
    )
    .unwrap();
    apply(&project, "4", vec![EditCommand::BinDelete { bin }]).unwrap();
    let ResultData::Media(media) = service()
        .dispatch(Request::MediaQuery(MediaQueryRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(media.bins.is_empty());
    assert_eq!(media.assets.len(), 2, "bin deletion never removes assets");
    // Undo restores the deleted bin, proving bins live in the event store.
    let ResultData::Edit(_) = service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: project.clone(),
            base_revision: revision(&project),
            session_id: Uuid::new_v4(),
            idempotency_key: Uuid::new_v4().to_string(),
            event_id: {
                let ResultData::History(history) = service()
                    .dispatch(Request::HistoryList(HistoryRequest {
                        cursor: None,
                        project: project.clone(),
                        since_revision: "0".into(),
                        limit: 100,
                        session_id: None,
                    }))
                    .unwrap()
                else {
                    panic!()
                };
                history.events.last().unwrap().event.id
            },
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Media(media) = service()
        .dispatch(Request::MediaQuery(MediaQueryRequest { project }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(media.bins.len(), 1);
    assert_eq!(media.bins[0].name, "ダビング");
}

#[test]
fn asset_thumbnail_decodes_png_deterministically_and_fails_typed() {
    let dir = tempfile::tempdir().unwrap();
    let image = png_asset(dir.path(), "logo.png", 4, 2, [255, 0, 0, 255]);
    let (image_id, image_path) = (image.id, image.clone());
    let audio = Asset {
        id: AssetId::new(),
        content_hash: "11".repeat(32),
        kind: AssetKind::Audio,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("audio.wav".into()),
            absolute: None,
        },
    };
    let audio_id = audio.id;
    let document = Project {
        assets: vec![
            DocumentObject::Known(image_path),
            DocumentObject::Known(audio),
        ],
        ..Project::default()
    };
    let project = setup_at(dir.path().join("media.kronello"), document);
    let thumbnail = |asset: AssetId, max_size: u32| {
        service().dispatch(Request::AssetThumbnail(AssetThumbnailRequest {
            project: project.clone(),
            asset,
            time: None,
            max_size,
        }))
    };
    let ResultData::Thumbnail(first) = thumbnail(image_id, 16).unwrap() else {
        panic!()
    };
    // Aspect preserved: 4x2 inside 16x16 becomes 16x8, opaque RGBA8.
    assert_eq!((first.width, first.height), (16, 8));
    assert_eq!(first.rgba.len(), 16 * 8 * 4);
    assert!(first.rgba.chunks(4).all(|px| px[3] == 255));
    assert!(first.pts.is_none(), "still images carry no PTS");
    let ResultData::Thumbnail(second) = thumbnail(image_id, 16).unwrap() else {
        panic!()
    };
    assert_eq!(first.rgba, second.rgba, "thumbnails are deterministic");
    // A larger cap scales up with the same aspect ratio.
    let ResultData::Thumbnail(large) = thumbnail(image_id, 32).unwrap() else {
        panic!()
    };
    assert_eq!((large.width, large.height), (32, 16));
    for max_size in [0, 15, 1025] {
        assert_eq!(
            thumbnail(image_id, max_size).unwrap_err().code,
            "INVALID_REQUEST"
        );
    }
    assert_eq!(
        thumbnail(AssetId::new(), 64).unwrap_err().code,
        "ASSET_MISSING"
    );
    assert_eq!(
        thumbnail(audio_id, 64).unwrap_err().code,
        "UNSUPPORTED_FEATURE"
    );
    // A deleted locator reports ASSET_MISSING, never silent pixels.
    std::fs::remove_file(dir.path().join("logo.png")).unwrap();
    assert_eq!(thumbnail(image_id, 64).unwrap_err().code, "ASSET_MISSING");
}
