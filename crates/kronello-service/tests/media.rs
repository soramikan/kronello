use kronello_media::{content_hash, resolve_asset};
use kronello_model::{Asset, AssetId, AssetKind, AssetLocator, DocumentObject, Project};
use kronello_service::*;
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
#[test]
fn shared_relink_revision_and_portable_sqlite_collect() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("input.bin");
    std::fs::write(&original, b"content").unwrap();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: content_hash(&original).unwrap(),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("input.bin".into()),
            absolute: Some(original.to_str().unwrap().into()),
        },
    };
    let id = asset.id;
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(asset));
    let path = temp.path().join("source.kronello");
    let ResultData::Project(created) = service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
        }))
        .unwrap()
    else {
        panic!()
    };
    let search = temp.path().join("search");
    std::fs::create_dir(&search).unwrap();
    std::fs::rename(&original, search.join("renamed.bin")).unwrap();
    let request = RelinkRequest {
        project: path.clone(),
        base_revision: created.revision.clone(),
        asset: id,
        search_directory: search,
    };
    let ResultData::Project(updated) = service()
        .dispatch(Request::AssetRelink(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(updated.revision, created.revision);
    assert_eq!(
        service()
            .dispatch(Request::AssetRelink(request))
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    let output = temp.path().join("collect");
    let ResultData::Collected(collected) = service()
        .dispatch(Request::ProjectCollect(CollectRequest {
            project: path.clone(),
            output_directory: output.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(collected.asset_count, 1);
    let moved = temp.path().join("moved");
    std::fs::rename(output, &moved).unwrap();
    std::fs::remove_dir_all(temp.path().join("search")).unwrap();
    let ResultData::Export(export) = service()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: moved.join("project.kronello"),
        }))
        .unwrap()
    else {
        panic!()
    };
    let DocumentObject::Known(asset) = &export.document.assets[0] else {
        panic!()
    };
    assert_eq!(asset.id, id);
    assert!(asset.locator.absolute.is_none());
    assert!(resolve_asset(asset, &moved.join("project.kronello")).is_ok());
    let ResultData::Project(source) = service()
        .dispatch(Request::ProjectInfo(ProjectRequest { project: path }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(source.revision, updated.revision);
}
#[test]
fn media_wire_roundtrips_and_capabilities_are_shared() {
    let request: Request = serde_json::from_str(r#"{"operation":"capabilities.get"}"#).unwrap();
    let response = service().execute(request);
    let wire = serde_json::to_string(&response).unwrap();
    let response: Response = serde_json::from_str(&wire).unwrap();
    assert!(matches!(
        response,
        Response::Success {
            result: ResultData::Capabilities(_)
        }
    ));
    assert!(
        serde_json::from_str::<Request>(r#"{"operation":"capabilities.get","ffmpeg_args":"-x"}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<Request>(
            r#"{"operation":"capabilities.get","operation":"capabilities.get"}"#
        )
        .is_err()
    );
}
