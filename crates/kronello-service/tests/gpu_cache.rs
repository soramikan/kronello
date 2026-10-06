//! Actual shared-service cache validation; subprocess env avoids global mutation.
use kronello_model::{DocumentObject, Project};
use kronello_render::OutputRegion;
use kronello_service::*;
use kronello_time::Time;

#[test]
#[ignore = "requires actual GPU adapter and writable external cache; invoked by parent evidence test"]
fn cache003_service_child() {
    let project = std::path::PathBuf::from(
        std::env::var_os("KRONELLO_CACHE003_PROJECT").expect("parent evidence test required"),
    );
    let ResultData::Export(export) = Service::new(BackendSelection::CpuReference)
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let DocumentObject::Known(composition) = &export.document.compositions[0] else {
        panic!()
    };
    let request = || {
        Request::RenderFrame(FrameRenderRequest {
            input: RenderInput {
                project: project.clone(),
                composition: Some(composition.id),
                target: None,
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [1920.0, 1080.0],
                    pixels: [32, 18],
                },
                profile: Default::default(),
                fonts: vec![],
            },
            time: Time::ZERO,
            backend: Some(BackendSelection::Gpu),
        })
    };
    if std::env::var_os("KRONELLO_CACHE003_EXPECT_PROJECT_REJECTION").is_some() {
        let error = Service::new(BackendSelection::CpuReference)
            .dispatch(request())
            .unwrap_err();
        assert_eq!(error.code, "CACHE_CONFIGURATION");
        eprintln!("CACHE003 project cache rejected: {error:?}");
        return;
    }
    let frame = || {
        let ResultData::Frame(frame) = Service::new(BackendSelection::CpuReference)
            .dispatch(request())
            .unwrap()
        else {
            panic!()
        };
        *frame
    };
    let cold = frame();
    let warm = frame();
    assert!(cold.linear.iter().any(|pixel| pixel[3] > 0.0));
    assert_eq!(cold.linear, warm.linear);
    assert_eq!(cold.display, warm.display);
    let cold_transfers = cold.metadata.transfer_stats.unwrap();
    let warm_transfers = warm.metadata.transfer_stats.unwrap();
    if std::env::var_os("KRONELLO_CACHE003_DEFAULT_OVERLAP").is_some() {
        assert_eq!(
            cold.metadata
                .resource_cache_stats
                .unwrap()
                .persistent_disk_policy,
            kronello_render::PersistentRasterCachePolicy::DefaultDirectoryOverlapsProject
        );
        assert_eq!(
            warm.metadata.resource_cache_stats.unwrap().disk_raster.hits,
            0
        );
        eprintln!("CACHE003 broad HOME parent renders with explicit memory-only metadata");
        return;
    }
    let concurrent = std::env::var_os("KRONELLO_CACHE003_CONCURRENT").is_some();
    if !concurrent {
        assert_eq!(cold_transfers.cpu_upload_pixel_bytes, 0);
        assert!(cold_transfers.gpu_readback_bytes > warm_transfers.gpu_readback_bytes);
    }
    assert!(warm_transfers.cpu_upload_pixel_bytes > 0);
    assert!(warm.metadata.resource_cache_stats.unwrap().disk_raster.hits > 0);
    let directory =
        std::path::PathBuf::from(std::env::var_os("KRONELLO_RASTER_CACHE_ROOT").unwrap());
    if concurrent {
        for _ in 0..8 {
            let repeated = frame();
            assert_eq!(cold.linear, repeated.linear);
            assert_eq!(cold.display, repeated.display);
        }
        eprintln!(
            "CACHE003 actual concurrent service process={} cold={cold_transfers:?} warm={warm_transfers:?}",
            std::process::id()
        );
    } else {
        std::fs::remove_dir_all(&directory).unwrap();
        let deleted = frame();
        assert_eq!(cold.linear, deleted.linear);
        assert_eq!(cold.display, deleted.display);
        eprintln!(
            "CACHE003 actual service fresh GPU context cold={cold_transfers:?} diskWarm={warm_transfers:?} deleted={:?}",
            deleted.metadata.transfer_stats
        );
    }
}

#[test]
#[ignore = "requires actual selected GPU; parent of isolated shared-service acceptance process"]
fn cache003_actual_service_external_disk_fresh_context_and_deletion() {
    let temporary = tempfile::tempdir().unwrap();
    let project_directory = temporary.path().join("project");
    std::fs::create_dir(&project_directory).unwrap();
    let path = project_directory.join("cache.kronello");
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    document.texts.clear();
    Service::new(BackendSelection::CpuReference)
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
            plan_hash: None,
            idempotency_key: None,
        }))
        .unwrap();
    let child = |directory: &std::path::Path, reject: bool| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "cache003_service_child",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env("KRONELLO_CACHE003_PROJECT", &path)
            .env("KRONELLO_RASTER_CACHE_ROOT", directory);
        if reject {
            command.env("KRONELLO_CACHE003_EXPECT_PROJECT_REJECTION", "1");
        }
        let output = command.output().unwrap();
        eprintln!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success());
    };
    child(&temporary.path().join("external-cache"), false);
    let overlap = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "cache003_service_child",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("KRONELLO_CACHE003_PROJECT", &path)
        .env("KRONELLO_CACHE003_DEFAULT_OVERLAP", "1")
        .env_remove("KRONELLO_RASTER_CACHE_ROOT")
        .env("HOME", &project_directory)
        .env("LOCALAPPDATA", &project_directory)
        .env("XDG_CACHE_HOME", &project_directory)
        .output()
        .unwrap();
    eprintln!(
        "{}{}",
        String::from_utf8_lossy(&overlap.stdout),
        String::from_utf8_lossy(&overlap.stderr)
    );
    assert!(overlap.status.success());
    let concurrent_directory = temporary.path().join("concurrent-cache");
    let mut workers = vec![];
    for _ in 0..2 {
        let worker = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "cache003_service_child",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env("KRONELLO_CACHE003_PROJECT", &path)
            .env("KRONELLO_RASTER_CACHE_ROOT", &concurrent_directory)
            .env("KRONELLO_CACHE003_CONCURRENT", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        workers.push(worker);
    }
    for worker in workers {
        let output = worker.wait_with_output().unwrap();
        eprintln!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success());
    }
    child(&project_directory.join("forbidden-cache"), true);
    assert!(!project_directory.join("forbidden-cache").exists());
}

#[test]
#[ignore = "actual GPU nonblocking typed error must survive the shared service error API"]
fn perf001_nonblocking_shared_gpu_busy_has_stable_service_error() {
    use kronello_render::{
        RenderBackend, RenderProfile, RenderSnapshot, build_render_dag, build_scene_ir,
    };
    use std::sync::{Arc, mpsc};
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    let profile = RenderProfile::default();
    let snapshot = RenderSnapshot::new(&document, id, 1, profile).unwrap();
    let scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
    let dag = build_render_dag(
        &scene,
        profile,
        OutputRegion {
            origin: [0.0; 2],
            extent: [1920.0, 1080.0],
            pixels: [32, 18],
        },
    )
    .unwrap();
    let gpu = Arc::new(kronello_gpu::GpuContext::new().unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = gpu.clone();
    let holder = std::thread::spawn(move || {
        let _scope = peer.observation_scope().unwrap();
        // Zero-timeout nesting is valid on the owning thread.
        let _nested = peer.try_observation_scope().unwrap();
        ready_tx.send(()).unwrap();
        release_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
    });
    ready_rx.recv().unwrap();
    let error = gpu.try_execute(&dag).unwrap_err();
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    let error = ServiceError::from(error);
    assert_eq!(error.code, "RENDER_BACKEND_BUSY");
    assert_eq!(
        serde_json::to_value(&error).unwrap()["code"],
        "RENDER_BACKEND_BUSY"
    );
    assert!(gpu.execute(&dag).is_ok());
}
