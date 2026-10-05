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
            plan_hash: None,
            idempotency_key: None,
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

#[test]
fn delivery_output_wire_versions_audio_and_container_contracts_are_strict() {
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let project_path = temp.path().join("project.kronello");
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let composition = document["compositions"][0]["id"].clone();
    let mut document = document;
    document["compositions"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["compositions"][0]["root_nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document.as_object_mut().unwrap().remove("texts");
    let created = service().execute_json(
        &json!({"operation":"project.create","project":project_path,"document":document})
            .to_string(),
    );
    assert!(matches!(created, Response::Success { .. }), "{created:?}");
    for format in ["av1_mp4", "h264_mov", "hevc_mov"] {
        let output = json!({"format":format,"profile_version":1,"audio":"document","audio_codec":"alac","clips":[],"background":[0.1,0.2,0.3]});
        let typed: JobOutput = serde_json::from_value(output.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), output);
        for field in ["codec", "ffmpeg_args", "encoder", "shell"] {
            let mut invalid = output.clone();
            invalid[field] = json!("arbitrary");
            assert!(serde_json::from_value::<JobOutput>(invalid).is_err());
        }
        let mut no_version = output.clone();
        no_version
            .as_object_mut()
            .unwrap()
            .remove("profile_version");
        assert!(serde_json::from_value::<JobOutput>(no_version).is_err());
        for (version, codec, extension, expected) in [
            (99, "alac", "mov", "UNSUPPORTED_FEATURE"),
            (1, "aac", "mov", "UNSUPPORTED_FEATURE"),
            (
                1,
                "alac",
                if format == "av1_mp4" { "mov" } else { "mp4" },
                "INVALID_MEDIA_INPUT",
            ),
        ] {
            for operation in ["render.export", "render.submit"] {
                let mut output = output.clone();
                output["profile_version"] = json!(version);
                output["audio_codec"] = json!(codec);
                let path = temp.path().join(format!("invalid.{extension}"));
                let req = json!({"operation":operation,"render":{"input":{"project":project_path,"composition":composition,
                    "region":{"origin":[0,0],"extent":[64,64],"pixels":[64,64]}},"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},
                    "frame_rate":{"num":"24","den":"1"},"output_directory":path},"output":output});
                let response =
                    serde_json::to_value(service().execute_json(&req.to_string())).unwrap();
                assert_eq!(response["error"]["code"], expected, "{response}");
                assert!(!path.exists());
            }
        }
        let duplicate = format!(
            "{{\"format\":\"{format}\",\"profile_version\":1,\"profile_version\":1,\"clips\":[],\"background\":[0,0,0]}}"
        );
        assert!(serde_json::from_str::<JobOutput>(&duplicate).is_err());
    }
}
