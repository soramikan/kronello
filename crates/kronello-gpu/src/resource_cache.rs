//! Device-owned textures and pool leases; persistent pixels are an explicit boundary.
use crate::{GpuContext, GpuError, TransferStats};
use kronello_render::{CacheCapacity, CacheStats, RasterCacheKey};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, Default)]
pub struct GpuCacheConfig {
    pub textures: CacheCapacity,
    pub pool: CacheCapacity,
    pub disk: Option<DiskRasterConfig>,
}
#[derive(Debug, Clone)]
pub struct DiskRasterConfig {
    pub directory: PathBuf,
    pub capacity: CacheCapacity,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuCacheStats {
    pub textures: CacheStats,
    pub pool: CacheStats,
    pub disk: CacheStats,
    pub corrupt_disk_entries: u64,
    pub persistent_disk_policy: kronello_render::PersistentRasterCachePolicy,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct SurfaceDescriptor {
    size: [u32; 2],
    usage: wgpu::TextureUsages,
}
pub(crate) struct PoolState {
    capacity: CacheCapacity,
    idle: VecDeque<(SurfaceDescriptor, wgpu::Texture, usize)>,
    stats: CacheStats,
}
struct LeaseInner {
    texture: wgpu::Texture,
    descriptor: SurfaceDescriptor,
    context: Arc<()>,
    pool: Weak<Mutex<PoolState>>,
    recycle: AtomicBool,
    _allocation: Option<crate::allocation::AllocationGuard>,
}
impl Drop for LeaseInner {
    fn drop(&mut self) {
        if !self.recycle.load(Ordering::Relaxed) {
            return;
        }
        if let Some(pool) = self.pool.upgrade() {
            let mut pool = pool.lock().unwrap_or_else(|e| e.into_inner());
            let bytes = self.descriptor.size[0] as usize * self.descriptor.size[1] as usize * 8;
            if pool.capacity.entries == 0 || bytes > pool.capacity.bytes {
                return;
            }
            while pool.idle.len() >= pool.capacity.entries
                || pool.stats.bytes + bytes > pool.capacity.bytes
            {
                if let Some((_, _, weight)) = pool.idle.pop_front() {
                    pool.stats.bytes -= weight;
                    pool.stats.evictions += 1;
                } else {
                    break;
                }
            }
            pool.idle
                .push_back((self.descriptor, self.texture.clone(), bytes));
            pool.stats.bytes += bytes;
            pool.stats.entries = pool.idle.len();
            pool.stats.inserts += 1;
        }
    }
}
/// Cloning a lease keeps the surface unavailable to the pool until every user
/// releases it. Submitted commands retain their GPU objects; reuse remains on
/// the same ordered queue. Arbitrary foreign textures cannot create a lease.
#[derive(Clone)]
pub struct SurfaceLease(Arc<LeaseInner>);
impl std::fmt::Debug for SurfaceLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceLease")
            .field("size", &self.0.descriptor.size)
            .finish()
    }
}
impl SurfaceLease {
    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self.0.texture
    }
    pub(crate) fn width(&self) -> u32 {
        self.0.texture.width()
    }
    pub(crate) fn height(&self) -> u32 {
        self.0.texture.height()
    }
    pub(crate) fn create_view(
        &self,
        descriptor: &wgpu::TextureViewDescriptor<'_>,
    ) -> wgpu::TextureView {
        self.0.texture.create_view(descriptor)
    }
    pub fn validate_for(&self, gpu: &GpuContext, size: [u32; 2]) -> Result<(), GpuError> {
        if !Arc::ptr_eq(&self.0.context, &gpu.identity) || self.0.descriptor.size != size {
            return Err(GpuError::InvalidInput("surface context/extent mismatch"));
        }
        Ok(())
    }
    // A public raw texture boundary cannot retain the lease. Remove it from
    // recycling permanently rather than return a caller-owned texture to pool.
    pub(crate) fn detach(self) -> wgpu::Texture {
        self.0.recycle.store(false, Ordering::Relaxed);
        self.0.texture.clone()
    }
    pub(crate) fn external(gpu: &GpuContext, image: &crate::ResidentImage) -> Self {
        let texture = image.texture().clone();
        Self(Arc::new(LeaseInner {
            descriptor: SurfaceDescriptor {
                size: [texture.width(), texture.height()],
                usage: texture.usage(),
            },
            texture,
            context: gpu.identity.clone(),
            pool: Weak::new(),
            recycle: AtomicBool::new(false),
            _allocation: Some(image.allocation_guard()),
        }))
    }
}
pub(crate) struct ResourceCache {
    config: GpuCacheConfig,
    entries: VecDeque<([u8; 32], SurfaceLease, usize)>,
    stats: GpuCacheStats,
    fingerprint: [u8; 32],
    environment_verified: bool,
}
impl ResourceCache {
    pub(crate) fn new((fingerprint, environment_verified): ([u8; 32], bool)) -> Self {
        Self {
            config: GpuCacheConfig::default(),
            entries: VecDeque::new(),
            stats: GpuCacheStats::default(),
            fingerprint,
            environment_verified,
        }
    }
    fn insert(&mut self, key: RasterCacheKey, surface: SurfaceLease) {
        let bytes = surface.width() as usize * surface.height() as usize * 8;
        if let Some(index) = self.entries.iter().position(|(k, _, _)| *k == key.digest()) {
            let (_, _, weight) = self.entries.remove(index).unwrap();
            self.stats.textures.bytes -= weight;
        }
        let cap = self.config.textures;
        if cap.entries == 0 || bytes > cap.bytes {
            self.stats.textures.entries = self.entries.len();
            return;
        }
        while self.entries.len() >= cap.entries || self.stats.textures.bytes + bytes > cap.bytes {
            if let Some((_, _, weight)) = self.entries.pop_front() {
                self.stats.textures.bytes -= weight;
                self.stats.textures.evictions += 1;
            } else {
                break;
            }
        }
        self.entries.push_back((key.digest(), surface, bytes));
        self.stats.textures.bytes += bytes;
        self.stats.textures.entries = self.entries.len();
        self.stats.textures.inserts += 1;
    }
}
impl GpuContext {
    pub(crate) fn create_pool() -> Arc<Mutex<PoolState>> {
        Arc::new(Mutex::new(PoolState {
            capacity: CacheCapacity::default(),
            idle: VecDeque::new(),
            stats: CacheStats::default(),
        }))
    }
    /// Capacity controls retained resources. Live render surfaces additionally
    /// obey the renderer's scene and device limits; they are never evicted from
    /// under a consumer. Reconfiguration releases only cache/pool ownership.
    pub fn configure_cache(&self, config: GpuCacheConfig) -> Result<(), GpuError> {
        let _scope = self.render_scope()?;
        if let Some(disk) = &config.disk
            && (!disk.directory.is_absolute()
                || disk.capacity.entries == 0
                || disk.capacity.entries > 4096
                || disk.capacity.bytes == 0)
        {
            return Err(GpuError::InvalidInput(
                "disk raster cache requires absolute directory and nonzero budget",
            ));
        }
        let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        if config.disk.is_some() && !cache.environment_verified {
            return Err(GpuError::UnsupportedFeature(
                "persistent cache requires verified OS build and adapter fingerprint",
            ));
        }
        cache.entries.clear();
        cache.stats = GpuCacheStats::default();
        cache.stats.persistent_disk_policy = if config.disk.is_some() {
            kronello_render::PersistentRasterCachePolicy::Enabled
        } else {
            kronello_render::PersistentRasterCachePolicy::MemoryOnly
        };
        cache.config = config.clone();
        let mut pool = self.surface_pool.lock().unwrap_or_else(|e| e.into_inner());
        pool.idle.clear();
        pool.stats = CacheStats::default();
        pool.capacity = config.pool;
        Ok(())
    }
    pub(crate) fn strict_namespace(&self) -> String {
        let cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .fingerprint
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect()
    }
    pub fn record_memory_only_policy(
        &self,
        policy: kronello_render::PersistentRasterCachePolicy,
    ) -> Result<(), GpuError> {
        let _scope = self.render_scope()?;
        let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        if cache.config.disk.is_some()
            || policy == kronello_render::PersistentRasterCachePolicy::Enabled
        {
            return Err(GpuError::InvalidInput(
                "memory-only policy cannot override configured disk cache",
            ));
        }
        cache.stats.persistent_disk_policy = policy;
        Ok(())
    }
    pub fn clear_texture_cache(&self) -> Result<(), GpuError> {
        let _scope = self.render_scope()?;
        let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        cache.entries.clear();
        cache.stats.textures.bytes = 0;
        cache.stats.textures.entries = 0;
        Ok(())
    }
    pub fn cache_stats(&self) -> GpuCacheStats {
        let mut result = self
            .resources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats;
        result.pool = self
            .surface_pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats;
        result
    }
    pub fn render_cache_stats(&self) -> kronello_render::RenderResourceCacheStats {
        let stats = self.cache_stats();
        kronello_render::RenderResourceCacheStats {
            gpu_textures: stats.textures,
            gpu_pool: stats.pool,
            disk_raster: stats.disk,
            rejected_disk_entries: stats.corrupt_disk_entries,
            persistent_disk_policy: stats.persistent_disk_policy,
        }
    }
    pub fn acquire_surface(&self, size: [u32; 2]) -> Result<SurfaceLease, GpuError> {
        self.acquire_surface_with_usage(
            size,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        )
    }
    pub(crate) fn acquire_surface_with_usage(
        &self,
        size: [u32; 2],
        usage: wgpu::TextureUsages,
    ) -> Result<SurfaceLease, GpuError> {
        let _scope = self.render_scope()?;
        let descriptor = SurfaceDescriptor { size, usage };
        let mut pool = self.surface_pool.lock().unwrap_or_else(|e| e.into_inner());
        let texture = if let Some(index) = pool.idle.iter().position(|(d, _, _)| *d == descriptor) {
            let (_, texture, weight) = pool.idle.remove(index).unwrap();
            pool.stats.bytes -= weight;
            pool.stats.hits += 1;
            pool.stats.entries = pool.idle.len();
            texture
        } else {
            pool.stats.misses += 1;
            self.texture(size[0], size[1], wgpu::TextureFormat::Rgba16Float, usage)?
        };
        Ok(SurfaceLease(Arc::new(LeaseInner {
            texture,
            descriptor,
            context: self.identity.clone(),
            pool: Arc::downgrade(&self.surface_pool),
            recycle: AtomicBool::new(true),
            _allocation: Some(self.track_resource(
                crate::allocation::ResourceKind::Graph,
                u64::from(size[0]) * u64::from(size[1]) * 8,
            )),
        })))
    }
    pub(crate) fn cached_surface(
        &self,
        key: RasterCacheKey,
        size: [u32; 2],
        stats: &mut TransferStats,
        allow_disk: bool,
    ) -> Result<Option<SurfaceLease>, GpuError> {
        let disk = {
            let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(index) = cache
                .entries
                .iter()
                .position(|(k, _, _)| *k == key.digest())
            {
                let entry = cache.entries.remove(index).unwrap();
                let result = entry.1.clone();
                cache.entries.push_back(entry);
                cache.stats.textures.hits += 1;
                result.validate_for(self, size)?;
                return Ok(Some(result));
            }
            cache.stats.textures.misses += 1;
            if allow_disk {
                cache.config.disk.clone().map(|d| (d, cache.fingerprint))
            } else {
                None
            }
        };
        let Some((config, fingerprint)) = disk else {
            return Ok(None);
        };
        let (entries, bytes, evictions) = trim_disk(&config)?;
        {
            let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
            cache.stats.disk.entries = entries;
            cache.stats.disk.bytes = bytes;
            cache.stats.disk.evictions += evictions;
        }
        let path = config.directory.join(filename(key));
        let expected = size[0] as usize * size[1] as usize * 8;
        if expected + 112 > config.capacity.bytes {
            self.resources
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stats
                .disk
                .misses += 1;
            return Ok(None);
        }
        let Some(bytes) = read_bounded(&path, expected + 112)? else {
            self.resources
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stats
                .disk
                .misses += 1;
            return Ok(None);
        };
        let valid = bytes.len() == 112 + expected
            && &bytes[..8] == b"KRAS0001"
            && bytes[8..40] == key.digest()
            && bytes[40..72] == fingerprint
            && bytes[72..76] == size[0].to_le_bytes()
            && bytes[76..80] == size[1].to_le_bytes()
            && bytes[80..112] == Sha256::digest(&bytes[112..])[..]
            && crate::decode_rgba16f(&bytes[112..]).is_ok();
        if !valid {
            let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
            cache.stats.corrupt_disk_entries += 1;
            cache.stats.disk.misses += 1;
            return Ok(None);
        }
        let surface = self.acquire_surface(size)?;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: surface.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes[112..],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 8),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        stats.cpu_upload_pixel_bytes += expected as u64;
        stats.cpu_upload_pixel_operations += 1;
        let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        cache.stats.disk.hits += 1;
        cache.insert(key, surface.clone());
        Ok(Some(surface))
    }
    pub(crate) fn retain_surface(&self, key: RasterCacheKey, surface: SurfaceLease) {
        self.resources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, surface);
    }
    pub(crate) fn persist_surface(
        &self,
        key: RasterCacheKey,
        surface: &SurfaceLease,
        stats: &mut TransferStats,
    ) -> Result<(), GpuError> {
        let (config, fingerprint) = {
            let cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
            let Some(config) = cache.config.disk.clone() else {
                return Ok(());
            };
            (config, cache.fingerprint)
        };
        let payload_bytes = surface.width() as usize * surface.height() as usize * 8;
        if payload_bytes + 112 > config.capacity.bytes {
            return Ok(());
        }
        std::fs::create_dir_all(&config.directory).map_err(|e| GpuError::CacheIo(e.to_string()))?;
        let path = config.directory.join(filename(key));
        let payload = self.read_texture(surface.texture(), 8, stats)?;
        crate::decode_rgba16f(&payload)?;
        let mut bytes = b"KRAS0001".to_vec();
        bytes.extend(key.digest());
        bytes.extend(fingerprint);
        bytes.extend(surface.width().to_le_bytes());
        bytes.extend(surface.height().to_le_bytes());
        bytes.extend(Sha256::digest(&payload));
        bytes.extend(payload);
        let temp = config.directory.join(format!(
            ".{}.{}.{}.tmp",
            filename(key),
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let published = (|| -> Result<(), GpuError> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| GpuError::CacheIo(e.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| GpuError::CacheIo(e.to_string()))?;
            // Publish without replacing a peer's fully written same-key entry.
            for _ in 0..8 {
                match std::fs::hard_link(&temp, &path) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        if read_bounded(&path, bytes.len())?.as_ref() == Some(&bytes) {
                            return Ok(());
                        }
                        remove_if_present(&path)?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(GpuError::CacheIo(error.to_string())),
                }
            }
            Err(GpuError::CacheIo(
                "cache publication contention exceeded bounded retry".into(),
            ))
        })();
        remove_if_present(&temp)?;
        published?;
        let (entries, total, evictions) = trim_disk(&config)?;
        let mut cache = self.resources.lock().unwrap_or_else(|e| e.into_inner());
        cache.stats.disk.inserts += 1;
        cache.stats.disk.evictions += evictions;
        cache.stats.disk.entries = entries;
        cache.stats.disk.bytes = total;
        Ok(())
    }
}
fn remove_if_present(path: &std::path::Path) -> Result<(), GpuError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(GpuError::CacheIo(error.to_string())),
    }
}
// Opened-file reads are bounded even if a peer replaces the pathname after
// metadata inspection. Missing/evicted entries are misses; real IO remains typed.
fn read_bounded(path: &std::path::Path, length: usize) -> Result<Option<Vec<u8>>, GpuError> {
    use std::io::Read;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Ok(Some(vec![]));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(GpuError::CacheIo(error.to_string())),
        _ => {}
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(GpuError::CacheIo(error.to_string())),
    };
    let metadata = file
        .metadata()
        .map_err(|e| GpuError::CacheIo(e.to_string()))?;
    if metadata.len() != length as u64 {
        return Ok(Some(vec![]));
    }
    let mut bytes = Vec::with_capacity(length);
    file.take(length as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| GpuError::CacheIo(e.to_string()))?;
    Ok(Some(bytes))
}
fn trim_disk(config: &DiskRasterConfig) -> Result<(usize, usize, u64), GpuError> {
    let directory = match std::fs::read_dir(&config.directory) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0, 0)),
        Err(error) => return Err(GpuError::CacheIo(error.to_string())),
    };
    let mut files = vec![];
    let mut total = 0usize;
    let mut evictions = 0;
    for entry in directory {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(GpuError::CacheIo(error.to_string())),
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 69
            || !name.ends_with(".kras")
            || !name.as_bytes()[..64].iter().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(GpuError::CacheIo(error.to_string())),
        };
        if !metadata.is_file() {
            continue;
        }
        let bytes = metadata.len() as usize;
        files.push((entry.path(), bytes, metadata.modified().ok()));
        total = total.saturating_add(bytes);
        // At most configured entries+1 records are retained while enumerating.
        while files.len() > config.capacity.entries || total > config.capacity.bytes {
            let oldest = files
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, _, time))| *time)
                .map(|(index, _)| index)
                .unwrap();
            let (path, bytes, _) = files.swap_remove(oldest);
            remove_if_present(&path)?;
            total = total.saturating_sub(bytes);
            evictions += 1;
        }
    }
    Ok((files.len(), total, evictions))
}
fn filename(key: RasterCacheKey) -> String {
    format!(
        "{}.kras",
        key.digest()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    )
}
pub(crate) fn fingerprint(info: &wgpu::AdapterInfo) -> ([u8; 32], bool) {
    let os = if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/sw_vers")
            .args(["-buildVersion"])
            .output()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
    } else {
        std::process::Command::new("uname").arg("-r").output()
    };
    let os = os
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
        .unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(format!(
        "cache003-rgba16f-v1:wgpu30.0.1:naga30.0.1:{info:?}:{}:{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let verified = !os.is_empty();
    digest.update(os);
    digest.update(crate::SHADER);
    digest.update(crate::SCENE_SHADER);
    digest.update(crate::EFFECT_SHADER);
    digest.update(include_bytes!("scene_gpu.rs"));
    digest.update(include_bytes!("color.rs"));
    digest.update(include_bytes!("renderer.rs"));
    digest.update(include_bytes!("render_adapter.rs"));
    digest.update(include_bytes!("../../../rust-toolchain.toml"));
    digest.update(include_bytes!("../../../Cargo.lock"));
    (digest.finalize().into(), verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DrawNode, DrawScene, RenderSize, WorkingSpace};
    fn config(directory: Option<PathBuf>, entries: usize, bytes: usize) -> GpuCacheConfig {
        GpuCacheConfig {
            textures: CacheCapacity { entries, bytes },
            pool: CacheCapacity {
                entries: 2,
                bytes: 4096,
            },
            disk: directory.map(|directory| DiskRasterConfig {
                directory,
                capacity: CacheCapacity {
                    entries: 8,
                    bytes: 16384,
                },
            }),
        }
    }
    fn keys(gpu: &GpuContext, origin: f64) -> Vec<Option<RasterCacheKey>> {
        [
            "source-pixels-v1-0.25-0.5-0.125-0.5",
            "group-opacity-v1-0.75",
        ]
        .iter()
        .map(|identity| {
            Some(
                RasterCacheKey::external_source(
                    identity,
                    kronello_render::OutputRegion {
                        origin: [origin, 0.0],
                        extent: [8.0; 2],
                        pixels: [8; 2],
                    },
                    kronello_model::ColorSpace::LinearRec709,
                    &gpu.strict_namespace(),
                )
                .unwrap(),
            )
        })
        .collect()
    }
    fn scene() -> DrawScene {
        DrawScene {
            nodes: vec![
                DrawNode::Raster(vec![[0.25, 0.5, 0.125, 0.5]; 64]),
                DrawNode::Group {
                    children: vec![0],
                    opacity: 0.75,
                },
            ],
            roots: vec![1],
        }
    }
    fn render(
        gpu: &GpuContext,
        keys: &[Option<RasterCacheKey>],
        disk: bool,
    ) -> crate::RenderOutput {
        gpu.render_scene_cached(
            RenderSize {
                design_extent: [8.0; 2],
                output_resolution: [8; 2],
            },
            &scene(),
            WorkingSpace::LinearRec709,
            keys,
            disk,
            None,
        )
        .unwrap()
    }
    #[test]
    #[ignore = "requires actual GPU; failed graphs must never publish reusable textures"]
    fn gpu_failed_graph_never_populates_full_or_preview_cache() {
        for preview in [false, true] {
            let gpu = GpuContext::new().unwrap();
            gpu.configure_cache(config(None, 16, 4096)).unwrap();
            let size = RenderSize::pixels(8, 8);
            let scene = DrawScene {
                nodes: vec![
                    DrawNode::Raster(vec![[65504.0, 0.0, 0.0, 1.0]; 64]),
                    DrawNode::Raster(vec![[50000.0, 0.0, 0.0, 0.5]; 64]),
                    DrawNode::Raster(vec![[0.0, 0.0, 0.0, 1.0]; 64]),
                    DrawNode::Group {
                        children: vec![0, 1, 2],
                        opacity: 1.0,
                    },
                ],
                roots: vec![3],
            };
            let keys: Vec<_> = (0..4)
                .map(|index| {
                    Some(
                        RasterCacheKey::external_source(
                            &format!("failed-graph-{index}"),
                            kronello_render::OutputRegion {
                                origin: [0.0; 2],
                                extent: [8.0; 2],
                                pixels: [8; 2],
                            },
                            kronello_model::ColorSpace::LinearRec709,
                            &gpu.strict_namespace(),
                        )
                        .unwrap(),
                    )
                })
                .collect();
            for _ in 0..2 {
                let result = if preview {
                    gpu.render_scene_texture_cached(
                        size,
                        &scene,
                        WorkingSpace::LinearRec709,
                        &keys,
                        crate::OutputTransform {
                            space: crate::InputSpace::Srgb,
                            alpha: crate::OutputAlpha::Straight,
                        },
                    )
                    .map(|_| ())
                } else {
                    gpu.render_scene_cached(
                        size,
                        &scene,
                        WorkingSpace::LinearRec709,
                        &keys,
                        false,
                        None,
                    )
                    .map(|_| ())
                };
                assert!(matches!(result, Err(GpuError::InvalidInput(_))));
                assert_eq!(gpu.cache_stats().textures.inserts, 0);
                assert_eq!(gpu.cache_stats().textures.entries, 0);
            }
        }
    }
    #[test]
    #[ignore = "requires actual selected GPU adapter; exercises real textures and disk transfers"]
    fn gpu_texture_lru_disk_deletion_corruption_and_roi() {
        let gpu = GpuContext::new().unwrap();
        let directory =
            std::env::temp_dir().join(format!("kronello-cache003-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        gpu.configure_cache(config(Some(directory.clone()), 8, 4096))
            .unwrap();
        let identities = keys(&gpu, 0.0);
        let cold = render(&gpu, &identities, true);
        assert!(
            cold.pixels
                .iter()
                .all(|pixel| *pixel == [0.1875, 0.375, 0.09375, 0.375])
        );
        assert!(cold.transfers.cpu_upload_pixel_bytes > 0);
        assert!(cold.transfers.gpu_readback_bytes > cold.rgba16f.len() as u64 + 4);
        let warm = render(&gpu, &identities, true);
        assert_eq!(cold.pixels, warm.pixels);
        assert_eq!(warm.transfers.cpu_upload_pixel_bytes, 0);
        assert_eq!(warm.transfers.gpu_readback_bytes, 2048 + 4);
        assert!(gpu.cache_stats().textures.hits > 0);
        assert!(gpu.cache_stats().textures.bytes <= 4096);
        gpu.clear_texture_cache().unwrap();
        let disk = render(&gpu, &identities, true);
        assert_eq!(cold.pixels, disk.pixels);
        assert!(disk.transfers.cpu_upload_pixel_bytes > 0);
        assert!(gpu.cache_stats().disk.hits > 0);
        gpu.clear_texture_cache().unwrap();
        for entry in std::fs::read_dir(&directory).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let deleted = render(&gpu, &identities, true);
        assert_eq!(cold.pixels, deleted.pixels);
        gpu.clear_texture_cache().unwrap();
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let mut bytes = std::fs::read(&path).unwrap();
            bytes[40] ^= 1;
            std::fs::write(path, bytes).unwrap();
        }
        let mismatched = render(&gpu, &identities, true);
        assert_eq!(cold.pixels, mismatched.pixels);
        assert!(gpu.cache_stats().corrupt_disk_entries > 0);
        gpu.clear_texture_cache().unwrap();
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let mut bytes = std::fs::read(&path).unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            std::fs::write(path, bytes).unwrap();
        }
        assert_eq!(render(&gpu, &identities, true).pixels, cold.pixels);
        let corrupt_stats = gpu.cache_stats();
        assert!(corrupt_stats.corrupt_disk_entries >= 2);
        let misses = gpu.cache_stats().textures.misses;
        assert_ne!(identities, keys(&gpu, 1.0));
        assert_eq!(render(&gpu, &keys(&gpu, 1.0), true).pixels, cold.pixels);
        assert!(gpu.cache_stats().textures.misses > misses);
        gpu.configure_cache(GpuCacheConfig {
            disk: Some(DiskRasterConfig {
                directory: directory.clone(),
                capacity: CacheCapacity {
                    entries: 1,
                    bytes: 1024,
                },
            }),
            ..config(None, 1, 512)
        })
        .unwrap();
        render(&gpu, &identities, true);
        let disk_budget = gpu.cache_stats().disk;
        assert!(disk_budget.entries <= 1 && disk_budget.bytes <= 1024 && disk_budget.evictions > 0);
        gpu.configure_cache(config(None, 1, 512)).unwrap();
        assert_eq!(render(&gpu, &identities, false).pixels, cold.pixels);
        assert!(gpu.cache_stats().textures.evictions > 0);
        assert!(gpu.cache_stats().textures.entries <= 1 && gpu.cache_stats().textures.bytes <= 512);
        gpu.configure_cache(config(Some(directory.clone()), 8, 4096))
            .unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        // Strict resident execution skips even a configured disk store.
        let strict = render(&gpu, &identities, false);
        assert_eq!(strict.pixels, cold.pixels);
        assert_eq!(strict.transfers.gpu_readback_bytes, 2052);
        assert_eq!(gpu.cache_stats().disk.hits, 0);
        assert!(!directory.exists());
        eprintln!("CACHE003 rejected/corrupt={corrupt_stats:?} disk_budget={disk_budget:?}");
        let blocked =
            std::env::temp_dir().join(format!("kronello-cache003-blocked-{}", std::process::id()));
        std::fs::write(&blocked, b"not a directory").unwrap();
        gpu.configure_cache(config(Some(blocked.clone()), 8, 4096))
            .unwrap();
        let io = gpu
            .render_scene_cached(
                RenderSize {
                    design_extent: [8.0; 2],
                    output_resolution: [8; 2],
                },
                &scene(),
                WorkingSpace::LinearRec709,
                &identities,
                true,
                None,
            )
            .unwrap_err();
        assert!(matches!(io, GpuError::CacheIo(_)));
        std::fs::remove_file(blocked).unwrap();
        gpu.resources.lock().unwrap().environment_verified = false;
        assert!(matches!(
            gpu.configure_cache(config(Some(directory.clone()), 8, 4096)),
            Err(GpuError::UnsupportedFeature(_))
        ));
        eprintln!(
            "CACHE003 adapter={:?} cold={:?} warm={:?} disk={:?} stats={:?}",
            gpu.adapter_info,
            cold.transfers,
            warm.transfers,
            disk.transfers,
            gpu.cache_stats()
        );
        let _ = std::fs::remove_dir_all(directory);
    }
    #[test]
    #[ignore = "requires actual GPU contexts and ordered queue surface ownership"]
    fn gpu_surface_pool_clone_context_lifetime_and_budget() {
        let gpu = GpuContext::new().unwrap();
        let other = GpuContext::new().unwrap();
        gpu.configure_cache(config(None, 0, 0)).unwrap();
        let lease = gpu.acquire_surface([8; 2]).unwrap();
        let clone = lease.clone();
        assert!(lease.validate_for(&other, [8; 2]).is_err());
        assert!(lease.validate_for(&gpu, [7, 8]).is_err());
        drop(lease);
        assert_eq!(gpu.cache_stats().pool.entries, 0);
        let another = gpu.acquire_surface([8; 2]).unwrap();
        assert_eq!(gpu.cache_stats().pool.hits, 0);
        drop(clone);
        drop(another);
        assert_eq!(gpu.cache_stats().pool.entries, 2);
        let reused = gpu.acquire_surface([8; 2]).unwrap();
        assert_eq!(gpu.cache_stats().pool.hits, 1);
        let raw = reused.detach();
        assert_eq!(gpu.cache_stats().pool.entries, 1);
        drop(raw);
        for size in [[9, 9], [10, 10], [11, 11]] {
            drop(gpu.acquire_surface(size).unwrap());
        }
        assert!(gpu.cache_stats().pool.entries <= 2 && gpu.cache_stats().pool.bytes <= 4096);
        assert!(gpu.cache_stats().pool.evictions > 0);
        let native =
            crate::ResidentImage::allocate(&gpu, [8; 2], WorkingSpace::LinearRec709).unwrap();
        let mut native_scene = DrawScene {
            nodes: vec![
                DrawNode::GpuRaster(native),
                DrawNode::Group {
                    children: vec![0],
                    opacity: 1.0,
                },
            ],
            roots: vec![1],
        };
        gpu.configure_cache(config(None, 8, 4096)).unwrap();
        let identities = keys(&gpu, 0.0);
        let size = RenderSize {
            design_extent: [8.0; 2],
            output_resolution: [8; 2],
        };
        gpu.render_scene_cached(
            size,
            &native_scene,
            WorkingSpace::LinearRec709,
            &identities,
            false,
            None,
        )
        .unwrap();
        native_scene.nodes[0] = DrawNode::GpuRaster(
            crate::ResidentImage::allocate(&other, [8; 2], WorkingSpace::LinearRec709).unwrap(),
        );
        assert!(
            matches!(
                gpu.render_scene_cached(
                    size,
                    &native_scene,
                    WorkingSpace::LinearRec709,
                    &identities,
                    false,
                    None
                ),
                Err(GpuError::InvalidInput(_))
            ),
            "warm parent cache cannot hide foreign input ownership"
        );
        let held = gpu.acquire_surface([8; 2]).unwrap();
        drop(gpu); // Lease keeps device texture alive; dropped context has no pool.
        assert_eq!(held.width(), 8);
        drop(held);
    }
}
