//! Export-lifetime immutable preparation and bounded sample scene sharing.
use std::{collections::VecDeque, io::Write, sync::Arc};

use kronello_text::{FontData, VerifiedFonts};
use kronello_time::Time;

use crate::{
    CacheCapacity, CacheConfig, FrameMetadata, FrameRequest, OutputRegion, RenderBackend,
    RenderCache, RenderError, RenderSnapshot, SceneContent, SceneIr, TemporalMetadata,
    ValidatedRenderSnapshot,
};

const SCENE_CAPACITY: CacheCapacity = CacheCapacity {
    entries: 16,
    bytes: 64 * 1024 * 1024,
};

/// This session owns the validation proof and locked font bytes. Only semantic
/// data is retained; device objects and raster/complete-frame caches remain at
/// the existing backend boundary. Dropping the session releases all preparation.
pub struct ExportRenderSession {
    snapshot: ValidatedRenderSnapshot,
    fonts: VerifiedFonts,
    cache: RenderCache,
    scenes: VecDeque<(Time, Arc<SceneIr>, usize)>,
    scene_bytes: usize,
    capacity: CacheCapacity,
    metadata: Option<FrameMetadata>,
}

impl ExportRenderSession {
    pub fn new(snapshot: RenderSnapshot, fonts: &[FontData<'_>]) -> Result<Self, RenderError> {
        Self::with_scene_capacity(snapshot, fonts, SCENE_CAPACITY)
    }

    /// Zero or smaller capacities preserve output and may reevaluate samples.
    /// Bytes mean serialized semantic payload, excluding allocator overhead/RSS.
    pub fn with_scene_capacity(
        snapshot: RenderSnapshot,
        fonts: &[FontData<'_>],
        capacity: CacheCapacity,
    ) -> Result<Self, RenderError> {
        let snapshot = ValidatedRenderSnapshot::new(snapshot)?;
        let fonts = VerifiedFonts::new(snapshot.font_locks(), fonts)?;
        let small = CacheCapacity {
            entries: 256,
            bytes: 8 * 1024 * 1024,
        };
        let medium = CacheCapacity {
            entries: 256,
            bytes: 16 * 1024 * 1024,
        };
        let mut cache = RenderCache::new(CacheConfig {
            values: small,
            layout: medium,
            geometry: medium,
            simulation: small,
            ..CacheConfig::disabled()
        });
        cache.bind_verified_fonts(fonts.clone());
        crate::observe_execution_payload(
            "export_verified_font_bytes",
            fonts.retained_bytes() as u64,
        );
        crate::observe_execution_payload(
            "export_prepared_snapshot",
            snapshot.retained_payload_bytes()? as u64,
        );
        Ok(Self {
            snapshot,
            fonts,
            cache,
            scenes: VecDeque::new(),
            scene_bytes: 0,
            capacity,
            metadata: None,
        })
    }

    pub fn retained_scene_payload(&self) -> (usize, usize) {
        (self.scenes.len(), self.scene_bytes)
    }

    fn scene(&mut self, time: Time) -> Result<Arc<SceneIr>, RenderError> {
        if let Some(index) = self.scenes.iter().position(|(key, _, _)| *key == time) {
            let entry = self.scenes.remove(index).expect("located scene");
            let scene = entry.1.clone();
            self.scenes.push_back(entry);
            crate::count_execution("export_sample_scene_hits", 1);
            return Ok(scene);
        }
        let scene = Arc::new(crate::build_scene_ir_prevalidated(
            &self.snapshot,
            time,
            &self.fonts.data(),
            &mut self.cache,
        )?);
        // Counting directly into a bounded writer avoids a second large JSON
        // allocation. An oversized scene executes without being retained.
        let mut weight = PayloadWeight {
            bytes: 0,
            limit: self.capacity.bytes,
        };
        if self.capacity.entries != 0 && serde_json::to_writer(&mut weight, scene.as_ref()).is_ok()
        {
            while self.scenes.len() >= self.capacity.entries
                || self.scene_bytes > self.capacity.bytes - weight.bytes
            {
                let (_, _, bytes) = self.scenes.pop_front().expect("full scene cache");
                self.scene_bytes -= bytes;
            }
            self.scene_bytes += weight.bytes;
            self.scenes.push_back((time, scene.clone(), weight.bytes));
            crate::observe_execution_payload("export_sample_scenes", self.scene_bytes as u64);
        }
        self.observe_semantic_cache();
        Ok(scene)
    }

    fn observe_semantic_cache(&self) {
        let stats = self.cache.stats();
        crate::observe_execution_payload(
            "export_semantic_cache",
            (stats.values.bytes + stats.layout.bytes + stats.geometry.bytes) as u64,
        );
    }

    fn frame_metadata(
        &mut self,
        scene: &SceneIr,
        backend: &dyn RenderBackend,
        request: FrameRequest,
    ) -> Result<FrameMetadata, RenderError> {
        if self.metadata.is_none() {
            self.metadata = Some(crate::output::frame_metadata_with_hash(
                &self.snapshot,
                scene,
                backend,
                request,
                self.snapshot.canonical_hash(),
            )?);
        }
        let mut metadata = self.metadata.as_ref().expect("prepared metadata").clone();
        metadata.time = request.time;
        metadata.region = request.region;
        metadata.design_to_pixel = request.region.design_to_pixel().0;
        metadata.design_extent = scene.design_extent;
        metadata.backend = backend.name().into();
        metadata.transfer_stats = backend.transfer_stats();
        metadata.resource_cache_stats = backend.resource_cache_stats();
        metadata.input_path = if scene.nodes.iter().any(|node| {
            matches!(&node.content,
            SceneContent::Video { asset, .. } if asset.kind == kronello_model::AssetKind::Image)
        }) {
            backend.image_input_path().into()
        } else if scene
            .nodes
            .iter()
            .any(|node| matches!(node.content, SceneContent::Video { .. }))
        {
            backend.input_path().into()
        } else {
            "semantic_scene".into()
        };
        Ok(metadata)
    }

    fn execute(
        &mut self,
        scene: &SceneIr,
        backend: &dyn RenderBackend,
        region: OutputRegion,
        streaming: bool,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        let dag = crate::build_render_dag_with_cache(
            scene,
            self.snapshot.profile(),
            region,
            &mut self.cache,
        )?;
        crate::output::check_tile_surface_budget(&dag, streaming)?;
        let pixels = backend.execute_linear_with_cache(&dag, &mut self.cache)?;
        if pixels.len() != region.pixels[0] as usize * region.pixels[1] as usize {
            return Err(RenderError::InvalidInput(
                "backend returned wrong linear tile pixel count".into(),
            ));
        }
        crate::output::validate_pixels(&pixels, true)?;
        Ok(pixels)
    }

    pub fn render_linear_tiles(
        &mut self,
        backend: &dyn RenderBackend,
        request: FrameRequest,
        sink: &mut dyn FnMut([u32; 2], OutputRegion, Vec<[f32; 4]>) -> Result<(), RenderError>,
    ) -> Result<FrameMetadata, RenderError> {
        let _scope = backend.begin_observation_scope()?;
        let before = backend.transfer_stats_total();
        request.region.validate()?;
        let central = self.scene(request.time)?;
        let mut metadata = self.frame_metadata(&central, backend, request)?;
        if let Some(settings) = self.snapshot.profile().temporal {
            if backend.requires_gpu_resident() {
                return Err(RenderError::UnsupportedFeature("require_gpu_resident rejects CPU temporal accumulation; select explicit nonresident backend".into()));
            }
            let samples = crate::temporal_samples(&self.snapshot, request.time, settings)?;
            for (offset, tile) in crate::frame_tiles(request.region) {
                // Same sample order, f64 multiply/add and final f32 boundary as
                // the ordinary temporal path; only scene construction moves.
                let count = tile.pixels[0] as usize * tile.pixels[1] as usize;
                let mut accumulation = vec![[0.0_f64; 4]; count];
                for sample in &samples {
                    let _sample_scope = backend.begin_observation_scope()?;
                    let scene = self.scene(sample.time)?;
                    let pixels = self.execute(&scene, backend, tile, false)?;
                    let weight =
                        sample.weight.numerator() as f64 / sample.weight.denominator() as f64;
                    for (sum, pixel) in accumulation.iter_mut().zip(pixels) {
                        for channel in 0..4 {
                            sum[channel] += f64::from(pixel[channel]) * weight;
                        }
                    }
                }
                let linear: Vec<_> = accumulation
                    .into_iter()
                    .map(|pixel| pixel.map(|value| value as f32))
                    .collect();
                crate::output::validate_pixels(&linear, true)?;
                sink(offset, tile, linear)?;
            }
            metadata.temporal = Some(TemporalMetadata {
                settings,
                samples,
                sampling_scope: "root_composition".into(),
            });
        } else {
            for (offset, tile) in crate::frame_tiles(request.region) {
                let linear = self.execute(&central, backend, tile, true)?;
                sink(offset, tile, linear)?;
            }
        }
        metadata.resource_cache_stats = backend.resource_cache_stats();
        crate::output::apply_transfer_delta(&mut metadata, backend, before);
        self.observe_semantic_cache();
        Ok(metadata)
    }
}

struct PayloadWeight {
    bytes: usize,
    limit: usize,
}
impl Write for PayloadWeight {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "scene exceeds retained payload budget",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
