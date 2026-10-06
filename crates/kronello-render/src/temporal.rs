//! Root-scoped exact shutter integration. Nested scenes inherit each root time.
use crate::{
    BackendFrame, FrameMetadata, FrameRequest, RenderBackend, RenderCache, RenderError,
    RenderSnapshot, RenderTarget, RenderedFrame,
};
use kronello_model::{DocumentObject, TrackKind};
use kronello_text::FontData;
use kronello_time::{FrameRate, Rational, Time};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CutPolicy {
    AvoidCrossing,
    AllowCrossing,
}

/// Phase is an exact frame offset of the exposure start, not an angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalSettings {
    pub frame_rate: FrameRate,
    pub shutter_angle: Rational,
    pub shutter_phase: Rational,
    pub samples: u32,
    pub cut_policy: CutPolicy,
}
impl TemporalSettings {
    pub fn validate(self) -> Result<(), RenderError> {
        if self.samples == 0
            || self.samples > 4096
            || self.shutter_angle < Rational::ZERO
            || self.shutter_angle > Rational::from_integer(360)
        {
            return Err(RenderError::InvalidInput(
                "temporal samples must be 1..=4096 and shutter angle 0..=360".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalSample {
    pub time: Time,
    pub weight: Rational,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalMetadata {
    pub settings: TemporalSettings,
    pub samples: Vec<TemporalSample>,
    pub sampling_scope: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalFrame {
    pub frame: RenderedFrame,
    pub temporal: TemporalMetadata,
}

/// Clip the exposure to the root sequence's nearest visual edit boundaries.
/// Midpoint samples never land on the exclusive interval end.
pub fn temporal_samples(
    snapshot: &RenderSnapshot,
    time: Time,
    settings: TemporalSettings,
) -> Result<Vec<TemporalSample>, RenderError> {
    settings.validate()?;
    if settings.shutter_angle == Rational::ZERO {
        return Ok(vec![TemporalSample {
            time,
            weight: Rational::ONE,
        }]);
    }
    let frame = settings.frame_rate.frame_to_time(1)?;
    let width = frame.checked_mul(
        settings
            .shutter_angle
            .checked_div(Rational::from_integer(360))?,
    )?;
    let mut start = time.checked_add(frame.checked_mul(settings.shutter_phase)?)?;
    let mut end = start.checked_add(width)?;
    if settings.cut_policy == CutPolicy::AvoidCrossing
        && let RenderTarget::Sequence { sequence } = snapshot.target()
    {
        let sequence = snapshot
            .project()
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) if s.id == sequence => Some(s),
                _ => None,
            })
            .ok_or_else(|| RenderError::InvalidInput("missing sequence".into()))?;
        for clip in sequence
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .flat_map(|t| &t.clips)
        {
            for boundary in [clip.timeline_range.start(), clip.timeline_range.end()] {
                // Authored crossfades remain continuous at both endpoints.
                if sequence.transitions.iter().any(|tr| {
                    (tr.incoming == clip.id || tr.outgoing == clip.id)
                        && tr.range.start() <= boundary
                        && boundary <= tr.range.end()
                }) {
                    continue;
                }
                if boundary <= time {
                    start = start.max(boundary);
                } else {
                    end = end.min(boundary);
                }
            }
        }
    }
    if end <= start {
        return Ok(vec![TemporalSample {
            time,
            weight: Rational::ONE,
        }]);
    }
    let width = end.checked_sub(start)?;
    let weight = Rational::new(1, i64::from(settings.samples))?;
    let mut unique = BTreeMap::<Time, Rational>::new();
    for i in 0..settings.samples {
        let fraction = Rational::new(2 * i64::from(i) + 1, 2 * i64::from(settings.samples))?;
        let sample = start.checked_add(width.checked_mul(fraction)?)?;
        let prior = unique.get(&sample).copied().unwrap_or(Rational::ZERO);
        unique.insert(sample, prior.checked_add(weight)?);
    }
    Ok(unique
        .into_iter()
        .map(|(time, weight)| TemporalSample { time, weight })
        .collect())
}

/// Sequential whole-composition integration. Only an accumulator and the current
/// frame are retained; no per-layer averaging or nested resampling occurs.
pub fn render_temporal_frame_with_cache(
    snapshot: &RenderSnapshot,
    fonts: &[FontData<'_>],
    backend: &dyn RenderBackend,
    request: FrameRequest,
    settings: TemporalSettings,
    cache: &mut RenderCache,
) -> Result<TemporalFrame, RenderError> {
    let _scope = backend.begin_observation_scope()?;
    let before = backend.transfer_stats_total();
    if backend.requires_gpu_resident() {
        return Err(RenderError::UnsupportedFeature("require_gpu_resident rejects CPU temporal accumulation; select explicit nonresident backend".into()));
    }
    request.region.validate()?;
    let samples = temporal_samples(snapshot, request.time, settings)?;
    let count = u64::from(request.region.pixels[0]) * u64::from(request.region.pixels[1]);
    if count
        .checked_mul(96)
        .is_none_or(|bytes| bytes > 512 * 1024 * 1024)
    {
        return Err(RenderError::UnsupportedFeature(
            "temporal accumulation budget exceeded; use temporal tile streaming".into(),
        ));
    }
    // Compile every sampled dependency before a cache hit, preserving missing
    // fonts/unsupported diagnostics. Video remains backend validated each call.
    let namespace = backend.cache_namespace();
    let mut halos = Vec::new();
    let mut first_metadata = None;
    let mut has_video = false;
    if namespace.is_some() {
        for sample in &samples {
            let scene = crate::build_scene_ir_with_cache(snapshot, sample.time, fonts, cache)?;
            has_video |= scene
                .nodes
                .iter()
                .any(|n| matches!(n.content, crate::SceneContent::Video { .. }));
            first_metadata.get_or_insert(crate::output::frame_metadata(
                snapshot, &scene, backend, request,
            )?);
            for (_, tile) in crate::frame_tiles(request.region) {
                let dag =
                    crate::build_render_dag_with_cache(&scene, snapshot.profile(), tile, cache)?;
                halos.push((sample.time, tile, dag.execution_region()));
            }
        }
    }
    let identity = if has_video {
        None
    } else {
        namespace
            .map(|namespace| -> Result<String, RenderError> {
                use sha2::{Digest, Sha256};
                let value = (
                    crate::TEMPORAL_VERSION,
                    snapshot.evaluation_content_hash()?,
                    settings,
                    &samples,
                    request.region,
                    halos,
                    namespace,
                );
                Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
            })
            .transpose()?
    };
    if let Some(identity) = &identity
        && let Some(pixels) = cache.temporal_get(identity)?
    {
        let mut metadata = first_metadata.expect("compiled nonempty plan");
        metadata.temporal = Some(TemporalMetadata {
            settings,
            samples: samples.clone(),
            sampling_scope: "root_composition".into(),
        });
        crate::output::apply_transfer_delta(&mut metadata, backend, before);
        return Ok(TemporalFrame {
            frame: RenderedFrame { pixels, metadata },
            temporal: TemporalMetadata {
                settings,
                samples,
                sampling_scope: "root_composition".into(),
            },
        });
    }
    let mut accumulation = vec![[0.0_f64; 4]; count as usize];
    let mut metadata: Option<FrameMetadata> = None;
    for sample in &samples {
        let frame = crate::output::render_single_frame_with_cache(
            snapshot,
            fonts,
            backend,
            FrameRequest {
                time: sample.time,
                region: request.region,
            },
            cache,
        )?;
        let weight = sample.weight.numerator() as f64 / sample.weight.denominator() as f64;
        for (sum, pixel) in accumulation.iter_mut().zip(frame.pixels.linear) {
            for channel in 0..4 {
                sum[channel] += f64::from(pixel[channel]) * weight;
            }
        }
        metadata.get_or_insert(frame.metadata);
    }
    let linear: Vec<_> = accumulation
        .into_iter()
        .map(|p| p.map(|v| v as f32))
        .collect();
    crate::output::validate_pixels(&linear, true)?;
    let display = if snapshot.profile().hdr.is_some() {
        crate::hdr::hdr_sdr_display(&linear)
    } else {
        backend.display_from_linear(&linear, snapshot.profile().working_space)?
    };
    if display.len() != linear.len() {
        return Err(RenderError::InvalidInput(
            "temporal display pixel count mismatch".into(),
        ));
    }
    crate::output::validate_pixels(&display, false)?;
    if let Some(identity) = &identity {
        cache.temporal_insert(
            identity,
            BackendFrame {
                linear: linear.clone(),
                display: display.clone(),
            },
        )?;
    }
    let mut metadata = metadata.expect("nonempty sample plan");
    metadata.time = request.time;
    metadata.transfer_stats = backend.transfer_stats();
    metadata.resource_cache_stats = backend.resource_cache_stats();
    crate::output::apply_transfer_delta(&mut metadata, backend, before);
    metadata.temporal = Some(TemporalMetadata {
        settings,
        samples: samples.clone(),
        sampling_scope: "root_composition".into(),
    });
    Ok(TemporalFrame {
        frame: RenderedFrame {
            pixels: BackendFrame { linear, display },
            metadata,
        },
        temporal: TemporalMetadata {
            settings,
            samples,
            sampling_scope: "root_composition".into(),
        },
    })
}

pub fn render_temporal_frame(
    snapshot: &RenderSnapshot,
    fonts: &[FontData<'_>],
    backend: &dyn RenderBackend,
    request: FrameRequest,
    settings: TemporalSettings,
) -> Result<TemporalFrame, RenderError> {
    render_temporal_frame_with_cache(
        snapshot,
        fonts,
        backend,
        request,
        settings,
        &mut RenderCache::new(crate::CacheConfig::disabled()),
    )
}

/// Synchronous tile backpressure bounds both sample and accumulator memory.
pub fn render_temporal_frame_tiles(
    snapshot: &RenderSnapshot,
    fonts: &[FontData<'_>],
    backend: &dyn RenderBackend,
    request: FrameRequest,
    settings: TemporalSettings,
    sink: &mut dyn FnMut([u32; 2], crate::OutputRegion, BackendFrame) -> Result<(), RenderError>,
) -> Result<TemporalMetadata, RenderError> {
    let _scope = backend.begin_observation_scope()?;
    request.region.validate()?;
    let mut cache = RenderCache::new(crate::CacheConfig::disabled());
    let samples = temporal_samples(snapshot, request.time, settings)?;
    for (offset, region) in crate::frame_tiles(request.region) {
        let frame = render_temporal_frame_with_cache(
            snapshot,
            fonts,
            backend,
            FrameRequest {
                time: request.time,
                region,
            },
            settings,
            &mut cache,
        )?;
        sink(offset, region, frame.frame.pixels)?;
    }
    Ok(TemporalMetadata {
        settings,
        samples,
        sampling_scope: "root_composition".into(),
    })
}
