use crate::{
    scene::{StrokePrimitive, check_scene_budget, edges, stroke_primitives},
    *,
};
use wgpu::util::DeviceExt;

/// External RGBA16F boundary with explicit transfer/association metadata.
/// Encoded premultiplied RGB is never mislabeled as a working-space image.
#[derive(Debug)]
pub struct ExternalFrame {
    pub width: u32,
    pub height: u32,
    pub transform: OutputTransform,
    pub pixels: Vec<[f32; 4]>,
    pub rgba16f: Vec<u8>,
    pub transfers: TransferStats,
}
#[derive(Debug)]
pub struct SceneFramePair {
    pub linear: Vec<[f32; 4]>,
    pub display: Vec<[f32; 4]>,
    pub transfers: TransferStats,
}
struct ScenePass<'a> {
    gpu: &'a GpuContext,
    size: RenderSize,
    working: WorkingSpace,
    coverage_bounds_enabled: bool,
    pipeline: wgpu::ComputePipeline,
    effect_pipeline: wgpu::ComputePipeline,
    blank: SurfaceLease,
    semantic_keys: Option<&'a [Option<kronello_render::RasterCacheKey>]>,
    allow_disk: bool,
    pending_cache: Vec<(kronello_render::RasterCacheKey, SurfaceLease)>,
    stats: TransferStats,
    validation: wgpu::Buffer,
    controls: Vec<(wgpu::Buffer, crate::allocation::AllocationGuard)>,
    // Drop ownership after all retained resource handles are released.
    _scope: crate::observation::RenderScope,
}
fn space(space: InputSpace) -> u32 {
    match space {
        InputSpace::Srgb => 0,
        InputSpace::LinearRec709 => 1,
        InputSpace::LinearRec2020 => 2,
    }
}
/// Fixed FX-003 scene-pass operation ids (ADR-0109). Ids are stable protocol;
/// operations 2..=4 are the existing opacity/mask/output paths.
fn blend_operation(mode: kronello_model::BlendMode) -> u32 {
    use kronello_model::BlendMode;
    match mode {
        BlendMode::Normal => 1,
        BlendMode::Multiply => 5,
        BlendMode::Screen => 6,
        BlendMode::Darken => 7,
        BlendMode::Lighten => 8,
        BlendMode::ColorDodge => 9,
        BlendMode::ColorBurn => 10,
        BlendMode::HardLight => 11,
        BlendMode::SoftLight => 12,
        BlendMode::Difference => 13,
        BlendMode::Exclusion => 14,
        BlendMode::Overlay => 15,
        BlendMode::LinearDodge => 16,
        BlendMode::LinearBurn => 17,
        BlendMode::VividLight => 18,
        BlendMode::LinearLight => 19,
        BlendMode::Hue => 20,
        BlendMode::Saturation => 21,
        BlendMode::Color => 22,
        BlendMode::Luminosity => 23,
    }
}
// Restrict the fast path to fill-only outlines with a trustworthy finite AABB.
// Strokes and numerically extreme inputs retain the original sample loop.
fn coverage_bounds(path: &PathDraw, scale: [f32; 2]) -> Option<[f32; 4]> {
    if path.stroke.is_some()
        || path.stroke_geometry.is_some()
        || path.fill_gradient.is_some()
        || path.stroke_gradient.is_some()
        || scale
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0 || *v > 1.0e6)
    {
        return None;
    }
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (a, b, _) in edges(path) {
        for p in [a, b] {
            if p.iter().any(|v| !v.is_finite() || v.abs() > 1.0e6) {
                return None;
            }
            for axis in 0..2 {
                bounds[axis] = bounds[axis].min(p[axis]);
                bounds[axis + 2] = bounds[axis + 2].max(p[axis]);
            }
        }
    }
    if !bounds.iter().all(|v| v.is_finite()) {
        return None;
    }
    for axis in 0..2 {
        let magnitude = bounds[axis].abs().max(bounds[axis + 2].abs()).max(1.0);
        let guard = magnitude * f32::EPSILON * 64.0 + scale[axis] * 2.0;
        bounds[axis] -= guard;
        bounds[axis + 2] += guard;
    }
    Some(bounds)
}
impl ScenePass<'_> {
    fn texture(&self) -> Result<SurfaceLease, GpuError> {
        self.gpu.acquire_surface_with_usage(
            self.size.output_resolution,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        )
    }
    fn pass(
        &mut self,
        mode: u32,
        inputs: (&SurfaceLease, &SurfaceLease),
        path: Option<&PathDraw>,
        opacity: f32,
        kind: MaskKind,
        output_transform: Option<OutputTransform>,
    ) -> Result<SurfaceLease, GpuError> {
        let (source, previous) = inputs;
        let output = self.texture()?;
        let edge_list = path.map(edges).unwrap_or_default();
        let mut edge_bytes = Vec::new();
        for &(a, b, stroke) in &edge_list {
            edge_bytes.extend(a.into_iter().chain(b).flat_map(f32::to_le_bytes));
            edge_bytes.extend(
                [u32::from(stroke), 0, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            edge_bytes.extend([0.0f32; 4].into_iter().flat_map(f32::to_le_bytes));
        }
        let primitives = path.map(stroke_primitives).unwrap_or_default();
        for primitive in &primitives {
            let (points, extra, kind) = match *primitive {
                StrokePrimitive::Triangle([a, b, c]) => {
                    ([a[0], a[1], b[0], b[1]], [c[0], c[1], 0.0, 0.0], 1u32)
                }
                StrokePrimitive::Circle(c, r) => ([c[0], c[1], 0.0, 0.0], [r, 0.0, 0.0, 0.0], 2u32),
            };
            edge_bytes.extend(points.into_iter().flat_map(f32::to_le_bytes));
            edge_bytes.extend([0, kind, 0, 0].into_iter().flat_map(u32::to_le_bytes));
            edge_bytes.extend(extra.into_iter().flat_map(f32::to_le_bytes));
        }
        if edge_bytes.is_empty() {
            edge_bytes.resize(48, 0);
        }
        if edge_bytes.len() as u64 > self.gpu.device.limits().max_storage_buffer_binding_size {
            return Err(GpuError::UnsupportedFeature(
                "path exceeds storage buffer limits",
            ));
        }
        let fill = path.and_then(|p| p.fill);
        let stroke = path.and_then(|p| p.stroke);
        let mut params = Vec::new();
        params.extend(
            [
                mode,
                (edge_list.len() + primitives.len()) as u32,
                u32::from(fill.is_some_and(|f| f.rule == FillRule::Evenodd)),
                u32::from(self.working == WorkingSpace::LinearRec2020),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let scale = self.size.pixel_scale();
        let bounds = path
            .filter(|_| self.coverage_bounds_enabled)
            .and_then(|path| coverage_bounds(path, scale));
        params.extend(
            [scale[0], scale[1], opacity, stroke.map_or(0.0, |s| s.width)]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        params.extend(
            fill.map_or([0.0; 4], |f| f.paint.rgba)
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        params.extend(
            stroke
                .map_or([0.0; 4], |s| s.paint.rgba)
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        params.extend(
            [
                fill.map_or(1, |f| space(f.paint.space)),
                stroke.map_or(1, |s| space(s.paint.space)),
                match kind {
                    MaskKind::Alpha => 0,
                    MaskKind::Luminance => 1,
                    MaskKind::AlphaInverted => 2,
                    MaskKind::LuminanceInverted => 3,
                },
                output_transform.map_or(1, |t| space(t.space)),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        params.extend(
            [
                u32::from(output_transform.is_some_and(|t| t.alpha == OutputAlpha::Premultiplied)),
                path.and_then(|p| p.stroke_geometry.as_ref())
                    .map_or(0, |g| match g.alignment {
                        kronello_model::StrokeAlignment::Center => 0,
                        kronello_model::StrokeAlignment::Inside => 1,
                        kronello_model::StrokeAlignment::Outside => 2,
                    }),
                path.and_then(|p| p.stroke_geometry.as_ref())
                    .map_or(0, |g| u32::from(g.fill_rule == FillRule::Evenodd)),
                u32::from(bounds.is_some()),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let mut stop_bytes = Vec::new();
        let mut stop_count = 0u32;
        for (gradient_index, g) in [
            path.and_then(|p| p.fill_gradient.as_ref()),
            path.and_then(|p| p.stroke_gradient.as_ref()),
        ]
        .into_iter()
        .enumerate()
        {
            let (kind, geometry, extra) = match g.map(|g| g.geometry) {
                Some(crate::GradientGeometry::Linear { start, end }) => {
                    (1u32, [start[0], start[1], end[0], end[1]], [0.0; 4])
                }
                Some(crate::GradientGeometry::Radial { center, radius }) => {
                    (2u32, [center[0], center[1], radius, 0.0], [0.0; 4])
                }
                Some(crate::GradientGeometry::FocalRadial {
                    center,
                    radius,
                    focal,
                    focal_radius,
                }) => (
                    3u32,
                    [center[0], center[1], radius, 0.0],
                    [focal[0], focal[1], focal_radius, 0.0],
                ),
                Some(crate::GradientGeometry::Conic {
                    center,
                    start_angle,
                    sweep_angle,
                }) => (
                    4u32,
                    [
                        center[0],
                        center[1],
                        start_angle.to_radians(),
                        sweep_angle.to_radians(),
                    ],
                    [0.0; 4],
                ),
                None => (0u32, [0.0; 4], [0.0; 4]),
            };
            let stops = g.map_or(&[][..], |g| g.stops.as_slice());
            let mode = g.map_or(0, |g| {
                let spread = match g.spread {
                    crate::GradientSpread::Pad => 0,
                    crate::GradientSpread::Repeat => 1,
                    crate::GradientSpread::Reflect => 2,
                };
                let interpolation = match g.interpolation {
                    crate::GradientInterpolation::WorkingLinearPremultiplied => 0,
                    crate::GradientInterpolation::WorkingLinearStraight => 1,
                    crate::GradientInterpolation::SrgbStraight => 2,
                    crate::GradientInterpolation::SrgbPremultiplied => 3,
                };
                spread | (interpolation << 2)
            });
            params.extend(
                [kind, stop_count, stops.len() as u32, mode]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            params.extend(geometry.into_iter().flat_map(f32::to_le_bytes));
            let extra = if gradient_index == 0 {
                bounds.unwrap_or(extra)
            } else {
                extra
            };
            params.extend(extra.into_iter().flat_map(f32::to_le_bytes));
            for row in g.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |g| g.transform) {
                params.extend(
                    [row[0], row[1], row[2], 0.0]
                        .into_iter()
                        .flat_map(f32::to_le_bytes),
                );
            }
            for stop in stops {
                stop_bytes.extend(stop.paint.rgba.into_iter().flat_map(f32::to_le_bytes));
                stop_bytes.extend(
                    [stop.offset, 0.0, 0.0, 0.0]
                        .into_iter()
                        .flat_map(f32::to_le_bytes),
                );
                stop_bytes.extend(
                    [space(stop.paint.space), 0, 0, 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes),
                );
            }
            stop_count += stops.len() as u32;
        }
        for row in path.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |p| p.paint_transform) {
            params.extend(
                [row[0], row[1], row[2], 0.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        if stop_bytes.is_empty() {
            stop_bytes.resize(48, 0);
        }
        let mapping = path
            .and_then(|p| p.stroke_geometry.as_ref())
            .map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |g| g.output_to_local);
        for row in mapping {
            params.extend(row.into_iter().chain([0.0]).flat_map(f32::to_le_bytes));
        }
        let stop_buffer = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("VEC-003 gradient stops"),
                contents: &stop_bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let uniform = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GPU-002 params"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let edge_buffer = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GPU-002 geometry"),
                contents: &edge_bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        self.stats.cpu_upload_control_bytes +=
            (params.len() + edge_bytes.len() + stop_bytes.len()) as u64;
        self.stats.cpu_upload_control_operations += 3;
        for buffer in [&stop_buffer, &uniform, &edge_buffer] {
            self.controls.push((
                buffer.clone(),
                self.gpu
                    .track_resource(crate::allocation::ResourceKind::Control, buffer.size()),
            ));
        }
        let views = [
            source.create_view(&Default::default()),
            previous.create_view(&Default::default()),
            output.create_view(&Default::default()),
        ];
        let bind = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("GPU-002 draw"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&views[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: edge_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: stop_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.validation.as_entire_binding(),
                    },
                ],
            });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                self.size.output_resolution[0].div_ceil(8),
                self.size.output_resolution[1].div_ceil(8),
                1,
            );
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.stats.gpu_compute_dispatches += 1;
        Ok(output)
    }
    fn effect_pass(
        &mut self,
        source: &SurfaceLease,
        original: &SurfaceLease,
        weights: &[f32],
        axis: u32,
        shadow: Option<([f32; 2], [f32; 4])>,
    ) -> Result<SurfaceLease, GpuError> {
        let config = [
            if shadow.is_some() {
                if axis == 3 { 3 } else { 1 }
            } else if axis == 2 {
                2
            } else {
                0
            },
            if axis == 2 {
                (weights.len() / 3) as u32
            } else {
                (weights.len() / 2) as u32
            },
            axis,
            0,
        ];
        let (offset, color) = shadow.unwrap_or(([0.0; 2], [0.0; 4]));
        let floats = [
            offset[0], offset[1], 0.0, 0.0, color[0], color[1], color[2], color[3],
        ];
        self.effect_pass_raw(source, original, config, floats, weights)
    }
    /// Uniform layout: config vec4<u32> then offset/color as two vec4<f32>.
    fn effect_pass_raw(
        &mut self,
        source: &SurfaceLease,
        original: &SurfaceLease,
        config: [u32; 4],
        floats: [f32; 8],
        weights: &[f32],
    ) -> Result<SurfaceLease, GpuError> {
        let output = self.texture()?;
        let mut params = Vec::new();
        params.extend(config.into_iter().flat_map(u32::to_le_bytes));
        params.extend(floats.into_iter().flat_map(f32::to_le_bytes));
        let weights: Vec<u8> = if weights.is_empty() {
            // The binding requires a nonempty storage buffer.
            vec![0; 4]
        } else {
            weights.iter().flat_map(|w| w.to_le_bytes()).collect()
        };
        let uniform = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("FX-001 parameters"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let weights_buffer =
            self.gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("FX-001 Gaussian kernel"),
                    contents: &weights,
                    usage: wgpu::BufferUsages::STORAGE,
                });
        self.stats.cpu_upload_control_bytes += (params.len() + weights.len()) as u64;
        self.stats.cpu_upload_control_operations += 2;
        for buffer in [&uniform, &weights_buffer] {
            self.controls.push((
                buffer.clone(),
                self.gpu
                    .track_resource(crate::allocation::ResourceKind::Control, buffer.size()),
            ));
        }
        let views = [
            source.create_view(&Default::default()),
            original.create_view(&Default::default()),
            output.create_view(&Default::default()),
        ];
        let bind = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("FX-001 effect"),
                layout: &self.effect_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&views[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: weights_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.validation.as_entire_binding(),
                    },
                ],
            });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.effect_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                self.size.output_resolution[0].div_ceil(8),
                self.size.output_resolution[1].div_ceil(8),
                1,
            );
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.stats.gpu_compute_dispatches += 1;
        Ok(output)
    }
    /// COLOR-002 pointwise pass: config.y selects the op and the floats/weights
    /// carry parameters; the source doubles as the original input.
    fn color_effect(
        &mut self,
        source: &SurfaceLease,
        effect: &PixelEffect,
    ) -> Result<SurfaceLease, GpuError> {
        let (op, floats, weights): (u32, [f32; 8], Vec<f32>) = match effect {
            PixelEffect::ColorExposure { exposure, offset } => (
                1,
                [*exposure, *offset, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                vec![],
            ),
            PixelEffect::ColorLevels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => (
                2,
                [
                    *in_black, *in_white, *gamma, *out_black, *out_white, 0.0, 0.0, 0.0,
                ],
                vec![],
            ),
            PixelEffect::ColorCurves { points } => {
                (3, [0.0; 8], points.iter().flatten().copied().collect())
            }
            PixelEffect::ColorHsl {
                hue_shift,
                saturation,
                lightness,
            } => (
                4,
                [*hue_shift, *saturation, *lightness, 0.0, 0.0, 0.0, 0.0, 0.0],
                vec![],
            ),
            _ => unreachable!("not a pointwise color effect"),
        };
        let count = match effect {
            PixelEffect::ColorCurves { points } => points.len() as u32,
            _ => 0,
        };
        self.effect_pass_raw(source, source, [4, op, count, 0], floats, &weights)
    }
    fn effect(
        &mut self,
        source: &SurfaceLease,
        effect: &PixelEffect,
    ) -> Result<SurfaceLease, GpuError> {
        if effect.is_pointwise_color() {
            return self.color_effect(source, effect);
        }
        let blurred = if let Some(covariance) = effect.covariance() {
            let taps = kronello_render::affine_gaussian_kernel(covariance).map_err(|_| {
                GpuError::UnsupportedFeature("affine Gaussian covariance or kernel budget")
            })?;
            let weights: Vec<f32> = taps
                .iter()
                .flat_map(|t| [t.offset[0] as f32, t.offset[1] as f32, t.weight])
                .collect();
            self.effect_pass(source, source, &weights, 2, None)?
        } else {
            let [sx, sy] = effect.sigma();
            let horizontal =
                self.effect_pass(source, source, &crate::effect::kernel(sx)?, 0, None)?;
            self.effect_pass(&horizontal, source, &crate::effect::kernel(sy)?, 1, None)?
        };
        if let Some((offset, _, _)) = effect.shadow() {
            self.effect_pass(
                &blurred,
                source,
                &[1.0],
                if effect.covariance().is_some() { 3 } else { 0 },
                Some((offset, crate::effect::shadow_color(effect, self.working))),
            )
        } else {
            Ok(blurred)
        }
    }
    fn validate(&mut self) -> Result<(), GpuError> {
        self.controls.push((
            self.validation.clone(),
            self.gpu.track_resource(
                crate::allocation::ResourceKind::Control,
                self.validation.size(),
            ),
        ));
        let _allocation = self
            .gpu
            .track_resource(crate::allocation::ResourceKind::Readback, 4);
        let readback = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU-002 validation status"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.validation, 0, &readback, 0, 4);
        self.gpu.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.gpu.wait()?;
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let failed = mapped[..4] != [0; 4];
        drop(mapped);
        readback.unmap();
        self.stats.gpu_readback_bytes += 4;
        self.stats.gpu_readback_operations += 1;
        self.stats.gpu_wait_operations += 1;
        if failed {
            return Err(GpuError::InvalidInput(
                "RGBA16F surface value outside finite representable range",
            ));
        }
        // Publish only after the entire graph's sticky status is known valid.
        // A later opaque draw must not hide an invalid cached intermediate.
        for (key, surface) in &self.pending_cache {
            self.gpu.retain_surface(*key, surface.clone());
        }
        Ok(())
    }
    fn node(
        &mut self,
        scene: &DrawScene,
        id: usize,
        cache: &mut [Option<SurfaceLease>],
    ) -> Result<SurfaceLease, GpuError> {
        let _node = self.gpu.track_node(id);
        if let Some(t) = &cache[id] {
            return Ok(t.clone());
        }
        if let Some(key) = self.semantic_keys.and_then(|keys| keys[id])
            && let Some(surface) = self.gpu.cached_surface(
                key,
                self.size.output_resolution,
                &mut self.stats,
                self.allow_disk,
            )?
        {
            cache[id] = Some(surface.clone());
            return Ok(surface);
        }
        let blank = self.blank.clone();
        let t = match &scene.nodes[id] {
            DrawNode::GpuRaster(image) => {
                image.validate_for(self.gpu, self.size.output_resolution, self.working)?;
                SurfaceLease::external(self.gpu, image)
            }
            DrawNode::Raster(pixels) => {
                if pixels.len()
                    != pixel_count(
                        self.size.output_resolution[0],
                        self.size.output_resolution[1],
                    )?
                {
                    return Err(GpuError::InvalidInput("raster input dimensions"));
                }
                let texture = self.gpu.acquire_surface_with_usage(
                    self.size.output_resolution,
                    wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC,
                )?;
                let bytes: Vec<_> = pixels
                    .iter()
                    .flat_map(|p| {
                        let mut p = p.map(half::f16::from_f32);
                        if p[3] == half::f16::ZERO {
                            p[..3].fill(half::f16::ZERO);
                        }
                        p.into_iter().flat_map(|v| v.to_bits().to_le_bytes())
                    })
                    .collect();
                self.gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: texture.texture(),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.size.output_resolution[0] * 8),
                        rows_per_image: Some(self.size.output_resolution[1]),
                    },
                    wgpu::Extent3d {
                        width: self.size.output_resolution[0],
                        height: self.size.output_resolution[1],
                        depth_or_array_layers: 1,
                    },
                );
                self.stats.cpu_upload_pixel_bytes += bytes.len() as u64;
                self.stats.cpu_upload_pixel_operations += 1;
                texture
            }
            DrawNode::Path(path) => {
                self.pass(0, (&blank, &blank), Some(path), 1.0, MaskKind::Alpha, None)?
            }
            DrawNode::Group { children, opacity } => {
                let texture = self.composite(scene, children, cache)?;
                self.pass(2, (&texture, &blank), None, *opacity, MaskKind::Alpha, None)?
            }
            DrawNode::Blend {
                source,
                backdrop,
                mode,
            } => {
                let source = self.node(scene, *source, cache)?;
                let backdrop = self.node(scene, *backdrop, cache)?;
                let operation = blend_operation(*mode);
                self.pass(
                    operation,
                    (&source, &backdrop),
                    None,
                    1.0,
                    MaskKind::Alpha,
                    None,
                )?
            }
            DrawNode::Effect { source, effect } => {
                let source = self.node(scene, *source, cache)?;
                self.effect(&source, effect)?
            }
            DrawNode::Masked {
                source,
                matte,
                kind,
            } => {
                let source = self.node(scene, *source, cache)?;
                let matte = self.node(scene, *matte, cache)?;
                self.pass(3, (&source, &matte), None, 1.0, *kind, None)?
            }
        };
        if let Some(key) = self.semantic_keys.and_then(|keys| keys[id]) {
            self.pending_cache.push((key, t.clone()));
        }
        cache[id] = Some(t.clone());
        Ok(t)
    }
    fn composite(
        &mut self,
        scene: &DrawScene,
        ids: &[usize],
        cache: &mut [Option<SurfaceLease>],
    ) -> Result<SurfaceLease, GpuError> {
        let mut current = self.blank.clone();
        for &id in ids {
            let source = self.node(scene, id, cache)?;
            current = self.pass(1, (&source, &current), None, 1.0, MaskKind::Alpha, None)?;
        }
        Ok(current)
    }
}
impl GpuContext {
    fn scene_pass(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
    ) -> Result<ScenePass<'_>, GpuError> {
        let scope = self.render_scope()?;
        size.validate()?;
        scene.validate()?;
        // A warm parent hit cannot hide a foreign resource in its inputs.
        for node in &scene.nodes {
            if let DrawNode::GpuRaster(image) = node {
                image.validate_for(self, size.output_resolution, working)?;
            }
        }
        // RGBA16F intermediate surfaces are retained for shared references. Bound
        // the conservative peak including root/child composite temporaries.
        check_scene_budget(size, scene, 8)?;
        let blank = self.acquire_surface_with_usage(
            size.output_resolution,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        )?;
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("GPU-002 coverage and compositing"),
                source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("GPU-002"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let effect_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("FX-001"),
                source: wgpu::ShaderSource::Wgsl(EFFECT_SHADER.into()),
            });
        let effect_pipeline =
            self.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("FX-001"),
                    layout: None,
                    module: &effect_shader,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Ok(ScenePass {
            _scope: scope,
            gpu: self,
            semantic_keys: None,
            allow_disk: false,
            pending_cache: vec![],
            controls: vec![],
            size,
            working,
            coverage_bounds_enabled: true,
            pipeline,
            effect_pipeline,
            blank,
            stats: TransferStats {
                cpu_upload_control_bytes: 4,
                cpu_upload_control_operations: 1,
                ..TransferStats::default()
            },
            validation: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("GPU-002 sticky surface validation"),
                    contents: &[0; 4],
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                }),
        })
    }
    /// Rasterize derived polylines and resolve nested isolated groups/mattes on
    /// the GPU. Intermediate surfaces stay GPU resident; one final image readback
    /// plus a four-byte validation status.
    pub fn render_scene(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
    ) -> Result<RenderOutput, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        let mut cache = vec![None; scene.nodes.len()];
        let texture = pass.composite(scene, &scene.roots, &mut cache)?;
        pass.validate()?;
        self.finish_render(size, texture.texture(), working, pass.stats)
    }
    /// Same scene pipeline followed by explicit external color/alpha conversion.
    pub fn render_scene_output(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        transform: OutputTransform,
    ) -> Result<ExternalFrame, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        let mut cache = vec![None; scene.nodes.len()];
        let texture = pass.composite(scene, &scene.roots, &mut cache)?;
        let blank = pass.blank.clone();
        let output = pass.pass(
            4,
            (&texture, &blank),
            None,
            1.0,
            MaskKind::Alpha,
            Some(transform),
        )?;
        pass.validate()?;
        let rgba16f = self.read_texture(output.texture(), 8, &mut pass.stats)?;
        // This boundary currently produces zero RGB at alpha zero for both
        // associations, so the existing finite/binary16 validator also applies.
        let pixels = decode_rgba16f(&rgba16f)?;
        Ok(ExternalFrame {
            width: size.output_resolution[0],
            height: size.output_resolution[1],
            transform,
            pixels,
            rgba16f,
            transfers: pass.stats,
        })
    }
    #[cfg(test)]
    pub(crate) fn render_scene_cached(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        keys: &[Option<kronello_render::RasterCacheKey>],
        allow_disk: bool,
        output: Option<OutputTransform>,
    ) -> Result<RenderOutput, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        pass.semantic_keys = Some(keys);
        pass.allow_disk = allow_disk;
        let mut cache = vec![None; scene.nodes.len()];
        let texture = pass.composite(scene, &scene.roots, &mut cache)?;
        let texture = if let Some(transform) = output {
            let blank = pass.blank.clone();
            pass.pass(
                4,
                (&texture, &blank),
                None,
                1.0,
                MaskKind::Alpha,
                Some(transform),
            )?
        } else {
            texture
        };
        pass.validate()?;
        if pass.allow_disk {
            for (key, surface) in &pass.pending_cache {
                self.persist_surface(*key, surface, &mut pass.stats)?;
            }
        }
        if output.is_some() {
            let rgba16f = self.read_texture(texture.texture(), 8, &mut pass.stats)?;
            let pixels = decode_rgba16f(&rgba16f)?;
            Ok(RenderOutput {
                width: size.output_resolution[0],
                height: size.output_resolution[1],
                working_space: working,
                design_extent: size.design_extent,
                pixels,
                rgba16f,
                transfers: pass.stats,
            })
        } else {
            self.finish_render(size, texture.texture(), working, pass.stats)
        }
    }
    /// Produce both final boundaries from one validated graph. No intermediate
    /// pixels leave the GPU, and no duplicate graph execution is performed.
    pub fn render_scene_pair(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        transform: OutputTransform,
    ) -> Result<SceneFramePair, GpuError> {
        self.render_scene_pair_cached(size, scene, working, None, false, transform)
    }
    pub(crate) fn render_scene_pair_cached(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        keys: Option<&[Option<kronello_render::RasterCacheKey>]>,
        allow_disk: bool,
        transform: OutputTransform,
    ) -> Result<SceneFramePair, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        pass.semantic_keys = keys;
        pass.allow_disk = allow_disk;
        let mut cache = vec![None; scene.nodes.len()];
        let linear = pass.composite(scene, &scene.roots, &mut cache)?;
        let blank = pass.blank.clone();
        let display = pass.pass(
            4,
            (&linear, &blank),
            None,
            1.0,
            MaskKind::Alpha,
            Some(transform),
        )?;
        pass.validate()?;
        if pass.allow_disk {
            for (key, surface) in &pass.pending_cache {
                self.persist_surface(*key, surface, &mut pass.stats)?;
            }
        }
        let linear = decode_rgba16f(&self.read_texture(linear.texture(), 8, &mut pass.stats)?)?;
        let display = decode_rgba16f(&self.read_texture(display.texture(), 8, &mut pass.stats)?)?;
        Ok(SceneFramePair {
            linear,
            display,
            transfers: pass.stats,
        })
    }
    pub(crate) fn render_scene_texture_cached(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        keys: &[Option<kronello_render::RasterCacheKey>],
        transform: OutputTransform,
    ) -> Result<wgpu::Texture, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        pass.semantic_keys = Some(keys);
        // Native preview remains resident: persistence readbacks are disabled.
        let mut cache = vec![None; scene.nodes.len()];
        let texture = pass.composite(scene, &scene.roots, &mut cache)?;
        let blank = pass.blank.clone();
        let output = pass.pass(
            4,
            (&texture, &blank),
            None,
            1.0,
            MaskKind::Alpha,
            Some(transform),
        )?;
        pass.validate()?;
        self.record_transfers(&pass.stats);
        Ok(output.detach())
    }
    /// GPU-resident display output for native surfaces. Only the four-byte
    /// shader validation status is read back; image pixels stay on the device.
    pub fn render_scene_texture(
        &self,
        size: RenderSize,
        scene: &DrawScene,
        working: WorkingSpace,
        transform: OutputTransform,
    ) -> Result<wgpu::Texture, GpuError> {
        let mut pass = self.scene_pass(size, scene, working)?;
        let mut cache = vec![None; scene.nodes.len()];
        let texture = pass.composite(scene, &scene.roots, &mut cache)?;
        let blank = pass.blank.clone();
        let output = pass.pass(
            4,
            (&texture, &blank),
            None,
            1.0,
            MaskKind::Alpha,
            Some(transform),
        )?;
        pass.validate()?;
        Ok(output.detach())
    }
}

#[cfg(test)]
mod coverage_bounds_tests {
    use super::*;
    fn path(offset: [f32; 2], alpha: f32) -> PathDraw {
        PathDraw {
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: vec![Contour {
                points: vec![
                    [offset[0], offset[1]],
                    [offset[0] + 9.25, offset[1] + 1.125],
                    [offset[0] + 7.75, offset[1] + 8.625],
                    [offset[0] - 1.25, offset[1] + 6.5],
                ],
                closed: true,
            }],
            fill: Some(Fill {
                paint: Paint {
                    rgba: [0.75, 0.25, 0.125, alpha],
                    space: InputSpace::LinearRec709,
                },
                rule: FillRule::Nonzero,
            }),
            stroke: None,
        }
    }
    fn draw(
        gpu: &GpuContext,
        scene: &DrawScene,
        enabled: bool,
        keys: Option<&[Option<kronello_render::RasterCacheKey>]>,
    ) -> Result<SceneFramePair, GpuError> {
        draw_scaled(
            gpu,
            scene,
            enabled,
            keys,
            RenderSize {
                output_resolution: [48, 32],
                design_extent: [24.0, 40.0],
            },
        )
    }
    fn draw_scaled(
        gpu: &GpuContext,
        scene: &DrawScene,
        enabled: bool,
        keys: Option<&[Option<kronello_render::RasterCacheKey>]>,
        size: RenderSize,
    ) -> Result<SceneFramePair, GpuError> {
        let mut pass = gpu.scene_pass(size, scene, WorkingSpace::LinearRec709)?;
        pass.coverage_bounds_enabled = enabled;
        pass.semantic_keys = keys;
        let mut cache = vec![None; scene.nodes.len()];
        let linear = pass.composite(scene, &scene.roots, &mut cache)?;
        let blank = pass.blank.clone();
        let display = pass.pass(
            4,
            (&linear, &blank),
            None,
            1.0,
            MaskKind::Alpha,
            Some(OutputTransform {
                space: InputSpace::Srgb,
                alpha: OutputAlpha::Straight,
            }),
        )?;
        pass.validate()?;
        Ok(SceneFramePair {
            linear: decode_rgba16f(&gpu.read_texture(linear.texture(), 8, &mut pass.stats)?)?,
            display: decode_rgba16f(&gpu.read_texture(display.texture(), 8, &mut pass.stats)?)?,
            transfers: pass.stats,
        })
    }
    #[test]
    fn coverage_bound_uncertain_inputs_retain_original_loop() {
        let mut p = path([0.0; 2], 1.0);
        assert!(coverage_bounds(&p, [1.0; 2]).is_some());
        for v in [f32::NAN, f32::INFINITY, 1.0e10] {
            p.contours[0].points[0][0] = v;
            assert!(coverage_bounds(&p, [1.0; 2]).is_none());
        }
        assert!(coverage_bounds(&path([0.0; 2], 1.0), [f32::NAN, 1.0]).is_none());
    }
    #[test]
    #[ignore = "requires an actual GPU adapter"]
    fn gpu_coverage_bounds_exact_fractional_clipping_stroke_gradient_pool() {
        let gpu = GpuContext::new().unwrap();
        for cold in [true, false] {
            if cold {
                gpu.configure_cache(GpuCacheConfig {
                    textures: kronello_render::CacheCapacity {
                        entries: 0,
                        bytes: 0,
                    },
                    pool: kronello_render::CacheCapacity {
                        entries: 0,
                        bytes: 0,
                    },
                    disk: None,
                })
                .unwrap();
            } else {
                gpu.configure_cache(GpuCacheConfig::default()).unwrap();
            }
            for offset in [
                [0.125, 0.375],
                [-4.375, -2.125],
                [42.125, 28.875],
                [100.0, 100.0],
            ] {
                for alpha in [0.0, 0.00000006, 0.5, 1.0] {
                    for variant in 0..3 {
                        let mut p = path(offset, alpha);
                        if variant == 1 {
                            p.stroke = Some(RoundStroke {
                                join: StrokeJoin::Miter,
                                cap: StrokeCap::Round,
                                miter_limit: 4.0,
                                paint: p.fill.unwrap().paint,
                                width: 1.75,
                            });
                        }
                        if variant == 2 {
                            p.fill_gradient = Some(Box::new(GradientPaint {
                                spread: GradientSpread::Reflect,
                                interpolation: GradientInterpolation::WorkingLinearStraight,
                                interpolation_version: 1,
                                transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                                geometry: GradientGeometry::Linear {
                                    start: [0.0; 2],
                                    end: [8.0, 2.0],
                                },
                                stops: vec![
                                    GradientStop {
                                        offset: 0.0,
                                        paint: p.fill.unwrap().paint,
                                    },
                                    GradientStop {
                                        offset: 1.0,
                                        paint: Paint {
                                            rgba: [0.25, 0.75, 0.125, alpha],
                                            space: InputSpace::LinearRec709,
                                        },
                                    },
                                ],
                            }));
                        }
                        assert_eq!(coverage_bounds(&p, [1.0; 2]).is_some(), variant == 0);
                        let scene = DrawScene {
                            nodes: vec![
                                DrawNode::Path(p),
                                DrawNode::Group {
                                    children: vec![0],
                                    opacity: 0.75,
                                },
                            ],
                            roots: vec![1],
                        };
                        let old = draw(&gpu, &scene, false, None).unwrap();
                        let new = draw(&gpu, &scene, true, None).unwrap();
                        for (a, b) in old
                            .linear
                            .iter()
                            .chain(&old.display)
                            .zip(new.linear.iter().chain(&new.display))
                        {
                            assert_eq!(
                                a.map(f32::to_bits),
                                b.map(f32::to_bits),
                                "offset={offset:?} alpha={alpha} variant={variant}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "requires an actual GPU adapter"]
    fn gpu_coverage_bounds_sticky_failure_and_cache_are_unchanged() {
        let gpu = GpuContext::new().unwrap();
        for opaque_parent in [false, true] {
            for enabled in [false, true] {
                gpu.configure_cache(GpuCacheConfig::default()).unwrap();
                let mut invalid = path([2.125, 1.375], 1.0);
                invalid.fill.as_mut().unwrap().paint = Paint {
                    rgba: [65504.0, 0.0, 0.0, 1.0],
                    space: InputSpace::LinearRec2020,
                };
                let mut scene = DrawScene {
                    nodes: vec![
                        DrawNode::Path(invalid),
                        DrawNode::Group {
                            children: vec![0],
                            opacity: 0.0,
                        },
                    ],
                    roots: vec![1],
                };
                if opaque_parent {
                    let mut opaque = path([0.0; 2], 1.0);
                    opaque.contours[0].points =
                        vec![[0.0, 0.0], [48.0, 0.0], [48.0, 32.0], [0.0, 32.0]];
                    scene.nodes.push(DrawNode::Path(opaque));
                    scene.roots.push(2);
                }
                let keys: Vec<_> = (0..scene.nodes.len())
                    .map(|i| {
                        Some(
                            kronello_render::RasterCacheKey::external_source(
                                &format!("coverage-invalid-{opaque_parent}-{i}"),
                                kronello_render::OutputRegion {
                                    origin: [0.0; 2],
                                    extent: [48.0, 32.0],
                                    pixels: [48, 32],
                                },
                                kronello_model::ColorSpace::LinearRec709,
                                &gpu.strict_namespace(),
                            )
                            .unwrap(),
                        )
                    })
                    .collect();
                for _ in 0..2 {
                    assert!(matches!(
                        draw(&gpu, &scene, enabled, Some(&keys)),
                        Err(GpuError::InvalidInput(
                            "RGBA16F surface value outside finite representable range"
                        ))
                    ));
                }
                assert_eq!(gpu.render_cache_stats().gpu_textures.inserts, 0);
            }
        }
    }
    #[test]
    #[ignore = "requires an actual GPU adapter"]
    fn gpu_coverage_bounds_valid_cache_hit_matches_legacy() {
        let gpu = GpuContext::new().unwrap();
        let scene = DrawScene {
            nodes: vec![
                DrawNode::Path(path([3.125, 2.375], 0.5)),
                DrawNode::Group {
                    children: vec![0],
                    opacity: 0.75,
                },
            ],
            roots: vec![1],
        };
        let keys: Vec<_> = (0..2)
            .map(|i| {
                Some(
                    kronello_render::RasterCacheKey::external_source(
                        &format!("coverage-valid-{i}"),
                        kronello_render::OutputRegion {
                            origin: [0.0; 2],
                            extent: [24.0, 40.0],
                            pixels: [48, 32],
                        },
                        kronello_model::ColorSpace::LinearRec709,
                        &gpu.strict_namespace(),
                    )
                    .unwrap(),
                )
            })
            .collect();
        let old = draw(&gpu, &scene, false, None).unwrap();
        for _ in 0..2 {
            let new = draw(&gpu, &scene, true, Some(&keys)).unwrap();
            for (a, b) in old
                .linear
                .iter()
                .chain(&old.display)
                .zip(new.linear.iter().chain(&new.display))
            {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
        }
        assert!(gpu.render_cache_stats().gpu_textures.hits > 0);
    }
    #[test]
    #[ignore = "requires an actual GPU adapter"]
    fn gpu_coverage_bounds_4k_scale_evenodd_implicit_close_and_negative_coordinates() {
        let gpu = GpuContext::new().unwrap();
        let scale = [64.0 / 3840.0, 32.0 / 2160.0];
        let size = RenderSize {
            output_resolution: [48, 32],
            design_extent: [48.0 * scale[0], 32.0 * scale[1]],
        };
        for offset in [[-0.375, -1.125], [5.125, 8.375]] {
            let mut p = path(offset, 0.5);
            p.fill.as_mut().unwrap().rule = FillRule::Evenodd;
            p.contours[0].closed = false;
            p.contours.push(Contour {
                points: vec![
                    [offset[0] + 2.25, offset[1] + 2.25],
                    [offset[0] + 5.25, offset[1] + 2.25],
                    [offset[0] + 5.25, offset[1] + 4.25],
                    [offset[0] + 2.25, offset[1] + 4.25],
                ],
                closed: false,
            });
            for c in &mut p.contours {
                for point in &mut c.points {
                    for axis in 0..2 {
                        point[axis] *= scale[axis];
                    }
                }
            }
            assert!(coverage_bounds(&p, scale).is_some());
            let scene = DrawScene {
                nodes: vec![DrawNode::Path(p)],
                roots: vec![0],
            };
            let old = draw_scaled(&gpu, &scene, false, None, size).unwrap();
            let new = draw_scaled(&gpu, &scene, true, None, size).unwrap();
            assert!(old.linear.iter().any(|p| p[3] > 0.0));
            for (a, b) in old
                .linear
                .iter()
                .chain(&old.display)
                .zip(new.linear.iter().chain(&new.display))
            {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
        }
    }
}
