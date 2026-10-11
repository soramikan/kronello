//! A native editing session retains the same store used by shared requests.
use crate::ServiceError;
use kronello_store::{OpenOptions, ProjectStore, StoreError};
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
};

struct Retained {
    path: PathBuf,
    safe: bool,
    store: RefCell<Option<ProjectStore>>,
    identity: FileIdentity,
    preparation: RefCell<PreparationCache>,
    render_cache: RefCell<kronello_render::RenderCache>,
    audio_sources: Arc<Mutex<kronello_media::AudioSourceCache>>,
    audio_checkpoints: Arc<Mutex<crate::audio_checkpoints::AudioCheckpointCache>>,
}
thread_local! {
    static ACTIVE: RefCell<Option<Rc<Retained>>> = const { RefCell::new(None) };
}

/// Thread-confined native project lifetime. Requests still carry explicit paths.
/// Safe sessions keep exclusivity. Unix normal sessions retain a connection
/// while checking the path's inode before every lease; other platforms keep
/// transient normal opens until equivalent replacement detection is available.
pub struct ProjectSession(Rc<Retained>);
impl ProjectSession {
    pub fn open(path: &Path) -> Result<Self, ServiceError> {
        Self::open_with_options(path, OpenOptions::default())
    }
    pub fn open_with_options(path: &Path, options: OpenOptions) -> Result<Self, ServiceError> {
        if !path.is_file() {
            return Err(ServiceError::new(
                "PROJECT_NOT_FOUND",
                "project file does not exist",
            ));
        }
        let path = path.canonicalize().map_err(StoreError::from)?;
        let store = ProjectStore::open(&path, options)?;
        let safe = store.safe_mode();
        let identity = FileIdentity::read(&path)?;
        let store = if safe || cfg!(unix) {
            Some(store)
        } else {
            store.close()?;
            None
        };
        Ok(Self(Rc::new(Retained {
            path,
            safe,
            store: RefCell::new(store),
            identity,
            preparation: RefCell::new(PreparationCache::default()),
            render_cache: RefCell::new(kronello_render::RenderCache::default()),
            audio_sources: Default::default(),
            audio_checkpoints: Default::default(),
        })))
    }
    /// Session-owned derived preparation observations, excluding GPU/media.
    pub fn preparation_stats(&self) -> PreparationStats {
        self.0.preparation.borrow().stats
    }
    pub fn audio_source_cache_stats(&self) -> kronello_media::AudioSourceCacheStats {
        self.0
            .audio_sources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats()
    }
    pub fn audio_checkpoint_cache_stats(&self) -> crate::AudioCheckpointCacheStats {
        self.0
            .audio_checkpoints
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats()
    }
    /// Execute the existing service on the session's owning thread. The guard
    /// restores previous routing on normal return and unwind alike.
    pub fn scope<T>(&self, operation: impl FnOnce() -> T) -> T {
        struct Restore(Option<Rc<Retained>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                ACTIVE.with(|slot| *slot.borrow_mut() = self.0.take());
            }
        }
        let previous = ACTIVE.with(|slot| slot.replace(Some(self.0.clone())));
        let _restore = Restore(previous);
        operation()
    }
}

pub(crate) struct StoreLease {
    store: Option<ProjectStore>,
    retained: Option<Rc<Retained>>,
}
impl Deref for StoreLease {
    type Target = ProjectStore;
    fn deref(&self) -> &Self::Target {
        self.store.as_ref().expect("live store lease")
    }
}
impl DerefMut for StoreLease {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.store.as_mut().expect("live store lease")
    }
}
impl StoreLease {
    pub fn close(mut self) -> Result<(), StoreError> {
        if self.retained.is_none() {
            self.store.take().expect("live store lease").close()?;
        }
        Ok(())
    }
}
impl Drop for StoreLease {
    fn drop(&mut self) {
        if let Some(retained) = &self.retained {
            *retained.store.borrow_mut() = self.store.take();
        }
    }
}
pub(crate) fn open_existing(path: &Path) -> Result<StoreLease, ServiceError> {
    if !path.is_file() {
        return Err(ServiceError::new(
            "PROJECT_NOT_FOUND",
            "project file does not exist",
        ));
    }
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    let retained = ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|s| (s.safe || cfg!(unix)) && s.path == canonical)
            .cloned()
    });
    if let Some(session) = &retained
        && session.identity != FileIdentity::read(&canonical)?
    {
        return Err(ServiceError::new(
            "PROJECT_REPLACED",
            "project file was replaced; reopen the session",
        ));
    }
    let store =
        if let Some(session) = &retained {
            session.store.borrow_mut().take().ok_or_else(|| {
                ServiceError::new("PROJECT_LOCKED", "session store is already in use")
            })?
        } else {
            ProjectStore::open(path, OpenOptions::default())?
        };
    Ok(StoreLease {
        store: Some(store),
        retained,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: Option<std::time::SystemTime>,
}
impl FileIdentity {
    fn read(path: &Path) -> Result<Self, ServiceError> {
        let metadata = std::fs::metadata(path).map_err(StoreError::from)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                created: metadata.created().ok(),
            })
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PreparationStats {
    pub preparations: u64,
    pub hits: u64,
    pub info_preparations: u64,
}
#[derive(Default)]
struct PreparationCache {
    input: Option<Rc<PreparedInput>>,
    info: Option<crate::ProjectInfo>,
    stats: PreparationStats,
}
pub(crate) struct PreparedInput {
    key: serde_json::Value,
    resources: Vec<ResourceFingerprint>,
    proxy_assets: Vec<kronello_model::Asset>,
    proxy_resources: Vec<Option<(std::path::PathBuf, ResourceFingerprint)>>,
    pub snapshot: kronello_render::ValidatedRenderSnapshot,
    pub bytes: Vec<Vec<u8>>,
}
#[derive(Debug, PartialEq, Eq)]
struct ResourceFingerprint {
    length: u64,
    modified: Option<std::time::SystemTime>,
    created: Option<std::time::SystemTime>,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    changed: (i64, i64),
}
fn fingerprint(path: &Path) -> Option<ResourceFingerprint> {
    let metadata = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Some(ResourceFingerprint {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    })
}
fn resources(input: &crate::RenderInput) -> Option<Vec<ResourceFingerprint>> {
    // Missing font/LUT files must enter their original typed loader paths.
    input
        .fonts
        .iter()
        .map(|f| &f.path)
        .chain(input.luts.iter().map(|l| &l.path))
        .map(|path| fingerprint(path))
        .collect()
}
fn proxy_resources(
    assets: &[kronello_model::Asset],
    project: &Path,
) -> Vec<Option<(std::path::PathBuf, ResourceFingerprint)>> {
    // Preserve relative-before-absolute resolution and watch missing proxies
    // too: reappearance must invalidate the transient pruned snapshot.
    assets
        .iter()
        .map(|asset| {
            let located = kronello_media::locate_asset(asset, project).ok()?;
            let metadata = fingerprint(&located.path)?;
            Some((located.path, metadata))
        })
        .collect()
}
fn active(path: &Path) -> Result<Option<Rc<Retained>>, ServiceError> {
    if !path.is_file() {
        return Ok(None);
    }
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    Ok(ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|session| (session.safe || cfg!(unix)) && session.path == canonical)
            .cloned()
    }))
}
pub(crate) fn prepare_input(
    input: &crate::RenderInput,
    expected_revision: Option<&str>,
) -> Result<Option<Rc<PreparedInput>>, ServiceError> {
    let Some(session) = active(&input.project)? else {
        return Ok(None);
    };
    let store = open_existing(&input.project)?;
    let revision = store.revision()?;
    crate::jobs::check_expected_revision(expected_revision, revision)?;
    let mut key = serde_json::to_value(input)?;
    key.as_object_mut()
        .expect("render input object")
        .remove("region");
    key["project"] = serde_json::json!(session.path);
    let fingerprints = resources(input);
    {
        let mut cache = session.preparation.borrow_mut();
        if let Some(prepared) = &cache.input
            && prepared.snapshot.revision() == revision
            && prepared.key == key
            && fingerprints.as_ref() == Some(&prepared.resources)
            && proxy_resources(&prepared.proxy_assets, &input.project) == prepared.proxy_resources
        {
            let result = prepared.clone();
            cache.stats.hits += 1;
            store.close()?;
            return Ok(Some(result));
        }
        // Release the old document before allocating its replacement.
        cache.input = None;
    }
    let stored = store.snapshot()?;
    store.close()?;
    crate::jobs::check_expected_revision(expected_revision, stored.revision)?;
    let proxy_assets: Vec<_> = if input.media_proxies == kronello_render::MediaProxyMode::Prefer {
        stored
            .document
            .proxies
            .iter()
            .filter_map(|link| {
                stored
                    .document
                    .assets
                    .iter()
                    .find_map(|object| match object {
                        kronello_model::DocumentObject::Known(asset) if asset.id == link.proxy => {
                            Some(asset.clone())
                        }
                        _ => None,
                    })
            })
            .collect()
    } else {
        Vec::new()
    };
    let proxy_fingerprints = proxy_resources(&proxy_assets, &input.project);
    let snapshot =
        crate::freeze_render_input(&stored, input)?.with_luts(crate::load_locked_luts(input)?);
    let bytes = crate::load_locked_fonts(&snapshot, input)?;
    let prepared = Rc::new(PreparedInput {
        key,
        resources: fingerprints.unwrap_or_default(),
        proxy_assets,
        proxy_resources: proxy_fingerprints,
        snapshot: kronello_render::ValidatedRenderSnapshot::new(snapshot)?,
        bytes,
    });
    let mut cache = session.preparation.borrow_mut();
    cache.stats.preparations += 1;
    // Bound this single-entry derived cache by serialized snapshot/font payload.
    // Larger inputs still render through the same prepared value without retention.
    let size = prepared
        .snapshot
        .retained_payload_bytes()?
        .saturating_add(prepared.bytes.iter().map(Vec::len).sum::<usize>())
        .saturating_add(serde_json::to_vec(&prepared.proxy_assets)?.len());
    if size <= 256 * 1024 * 1024
        && resources(input).as_ref() == Some(&prepared.resources)
        && proxy_resources(&prepared.proxy_assets, &input.project) == prepared.proxy_resources
    {
        cache.input = Some(prepared.clone());
    }
    Ok(Some(prepared))
}
pub(crate) fn with_render_cache<T>(
    path: &Path,
    run: impl FnOnce(&mut kronello_render::RenderCache) -> T,
) -> Result<T, ServiceError> {
    if let Some(session) = active(path)? {
        return Ok(run(&mut session.render_cache.borrow_mut()));
    }
    Ok(run(&mut kronello_render::RenderCache::new(
        kronello_render::CacheConfig::disabled(),
    )))
}
pub(crate) fn project_info(path: &Path) -> Result<Option<crate::ProjectInfo>, ServiceError> {
    let Some(session) = active(path)? else {
        return Ok(None);
    };
    let store = open_existing(path)?;
    let revision = store.revision()?.to_string();
    let mut cache = session.preparation.borrow_mut();
    if let Some(info) = &cache.info
        && info.revision == revision
    {
        let result = info.clone();
        store.close()?;
        return Ok(Some(result));
    }
    let info = crate::info(&store)?;
    store.close()?;
    cache.stats.info_preparations += 1;
    cache.info = Some(info.clone());
    Ok(Some(info))
}

pub(crate) fn audio_source_cache(
    path: &Path,
) -> Result<Option<Arc<Mutex<kronello_media::AudioSourceCache>>>, ServiceError> {
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    Ok(ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|session| session.path == canonical)
            .map(|session| session.audio_sources.clone())
    }))
}

pub(crate) fn audio_checkpoint_cache(
    path: &Path,
) -> Result<Option<Arc<Mutex<crate::audio_checkpoints::AudioCheckpointCache>>>, ServiceError> {
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    Ok(ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|session| session.path == canonical)
            .map(|session| session.audio_checkpoints.clone())
    }))
}

/// Preserve snapshot-only opening for stateless callers while a native safe
/// session reads its own retained connection rather than reopening its DB.
pub(crate) fn read_snapshot(path: &Path) -> Result<kronello_store::Snapshot, ServiceError> {
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    let retained = ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|s| s.safe && s.path == canonical)
    });
    if retained {
        let store = open_existing(path)?;
        let snapshot = store.snapshot()?;
        store.close()?;
        Ok(snapshot)
    } else {
        Ok(ProjectStore::read_snapshot(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendSelection, Service};
    use kronello_store::OpenMode;
    use serde_json::json;
    #[cfg(unix)]
    fn create(path: &Path, document: &serde_json::Value) -> Service<'static> {
        let service = Service::new(BackendSelection::CpuReference);
        let response = serde_json::to_value(service.execute_json(
            &json!({"operation":"project.create","project":path,"document":document}).to_string(),
        ))
        .unwrap();
        assert_eq!(response["status"], "success", "{response}");
        service
    }
    #[cfg(unix)]
    fn input(path: &Path, document: &serde_json::Value) -> crate::RenderInput {
        serde_json::from_value(
            json!({"project":path,"composition":document["compositions"][0]["id"],
            "region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]}}),
        )
        .unwrap()
    }
    #[test]
    #[cfg(unix)]
    fn normal_preparation_tracks_external_revision_time_region_and_info() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prepare.kronello");
        let mut document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        let service = create(&path, &document);
        let session = ProjectSession::open(&path).unwrap();
        let mut input = input(&path, &document);
        session.scope(|| {
            let first = prepare_input(&input, Some("1")).unwrap().unwrap();
            let mut invalid = serde_json::to_value(&*first.snapshot).unwrap();
            invalid["semantic_versions"]["document"] = json!(999);
            let invalid: kronello_render::RenderSnapshot = serde_json::from_value(invalid).unwrap();
            assert!(kronello_render::ValidatedRenderSnapshot::new(invalid).is_err());
            input.region.pixels = [32, 16];
            let second = prepare_input(&input, Some("1")).unwrap().unwrap();
            assert!(Rc::ptr_eq(&first, &second));
            let first_info = project_info(&path).unwrap().unwrap();
            assert_eq!(
                first_info.content_hash,
                project_info(&path).unwrap().unwrap().content_hash
            );
            let frame = crate::FrameRenderRequest {
                input: input.clone(),
                time: kronello_time::Time::ZERO,
                backend: None,
            };
            let (_, first_dag) = service.preview_dag_unresolved(&frame).unwrap();
            let (_, later_dag) = service
                .preview_dag_unresolved(&crate::FrameRenderRequest {
                    time: kronello_time::Time::new(1, 1).unwrap(),
                    ..frame
                })
                .unwrap();
            assert_ne!(first_dag, later_dag);
            let reference =
                kronello_render::build_scene_ir(&first.snapshot, kronello_time::Time::ZERO, &[])
                    .unwrap();
            assert_eq!(
                first_dag,
                kronello_render::build_render_dag(
                    &reference,
                    first.snapshot.profile(),
                    input.region
                )
                .unwrap()
            );
            document["name"] = json!("External commit");
            let mut writer = ProjectStore::open(&path, OpenOptions::default()).unwrap();
            writer
                .import_json(1, uuid::Uuid::new_v4(), &document.to_string())
                .unwrap();
            writer.close().unwrap();
            assert_eq!(
                prepare_input(&input, Some("1")).err().unwrap().code,
                "REVISION_CONFLICT"
            );
            let third = prepare_input(&input, Some("2")).unwrap().unwrap();
            assert!(!Rc::ptr_eq(&first, &third));
            assert_eq!(third.snapshot.revision(), 2);
            let next_info = project_info(&path).unwrap().unwrap();
            assert_eq!(next_info.revision, "2");
            assert_ne!(next_info.content_hash, first_info.content_hash);
        });
        assert_eq!(session.preparation_stats().preparations, 2);
        assert_eq!(session.preparation_stats().hits, 3);
        assert_eq!(session.preparation_stats().info_preparations, 2);
    }
    #[test]
    #[cfg(unix)]
    fn cached_lut_revalidates_content_and_missing_file() {
        use sha2::Digest;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lut.kronello");
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        create(&path, &document);
        let cube = directory.path().join("identity.cube");
        let bytes = b"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        std::fs::write(&cube, bytes).unwrap();
        let mut input = input(&path, &document);
        input.luts.push(crate::LutInput {
            hash: format!("{:x}", sha2::Sha256::digest(bytes)),
            path: cube.clone(),
        });
        let session = ProjectSession::open(&path).unwrap();
        session.scope(|| {
            prepare_input(&input, None).unwrap().unwrap();
            prepare_input(&input, None).unwrap().unwrap();
            let mut changed = bytes.to_vec();
            *changed.last_mut().unwrap() = b' ';
            std::fs::write(&cube, changed).unwrap();
            assert_eq!(
                prepare_input(&input, None).err().unwrap().code,
                "ASSET_HASH_MISMATCH"
            );
            std::fs::remove_file(&cube).unwrap();
            assert_eq!(
                prepare_input(&input, None).err().unwrap().code,
                "LUT_MISSING"
            );
        });
    }
    #[test]
    #[cfg(unix)]
    fn cached_font_revalidates_same_length_rewrites_and_missing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("font.kronello");
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        create(&path, &document);
        let font_path = directory.path().join("font.otf");
        let font_bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),
        )
        .unwrap();
        std::fs::write(&font_path, &font_bytes).unwrap();
        let modified = std::fs::metadata(&font_path).unwrap().modified().unwrap();
        let mut input = input(&path, &document);
        input.fonts.push(
            serde_json::from_value(
                json!({"identity":document["texts"][0]["styles"][0]["font"],"path":font_path}),
            )
            .unwrap(),
        );
        let session = ProjectSession::open(&path).unwrap();
        session.scope(|| {
            prepare_input(&input, None).unwrap().unwrap();
            prepare_input(&input, None).unwrap().unwrap();
            let mut changed = font_bytes.clone();
            changed[0] ^= 1;
            std::fs::write(&font_path, changed).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&font_path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
            assert_eq!(
                prepare_input(&input, None).err().unwrap().code,
                "ASSET_HASH_MISMATCH"
            );
            std::fs::write(&font_path, font_bytes).unwrap();
            prepare_input(&input, None).unwrap().unwrap();
            std::fs::remove_file(&font_path).unwrap();
            assert_eq!(
                prepare_input(&input, None).err().unwrap().code,
                "FONT_MISSING"
            );
        });
        assert_eq!(session.preparation_stats().hits, 1);
    }
    #[test]
    #[cfg(unix)]
    fn replaced_project_is_rejected_instead_of_serving_retained_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original.kronello");
        let replacement = directory.path().join("replacement.kronello");
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        create(&path, &document);
        create(&replacement, &document);
        let session = ProjectSession::open(&path).unwrap();
        session.scope(|| {
            prepare_input(&input(&path, &document), None)
                .unwrap()
                .unwrap();
            std::fs::rename(&replacement, &path).unwrap();
            assert_eq!(
                prepare_input(&input(&path, &document), None)
                    .err()
                    .unwrap()
                    .code,
                "PROJECT_REPLACED"
            );
            assert_eq!(project_info(&path).err().unwrap().code, "PROJECT_REPLACED");
        });
    }
    #[test]
    fn safe_session_returns_store_after_unwind_and_preserves_exclusive_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.kronello");
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        let service = Service::new(BackendSelection::CpuReference);
        assert_eq!(
            serde_json::to_value(
                service.execute_json(
                    &json!({"operation":"project.create","project":path,"document":document})
                        .to_string()
                )
            )
            .unwrap()["status"],
            "success"
        );
        let session = ProjectSession::open_with_options(
            &path,
            OpenOptions {
                mode: OpenMode::ForceSafe,
            },
        )
        .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.scope(|| {
                let _lease = open_existing(&path).unwrap();
                panic!("injected request unwind");
            })
        }));
        assert!(result.is_err());
        assert!(matches!(
            ProjectStore::open(&path, OpenOptions::default()),
            Err(StoreError::ProjectLocked)
        ));
        session.scope(|| open_existing(&path).unwrap().close().unwrap());
        assert!(matches!(
            ProjectStore::open(&path, OpenOptions::default()),
            Err(StoreError::ProjectLocked)
        ));
        drop(session);
        ProjectStore::open(&path, OpenOptions::default())
            .unwrap()
            .close()
            .unwrap();
    }
}
