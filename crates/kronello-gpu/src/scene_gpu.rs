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
struct ScenePass<'a> {
    gpu: &'a GpuContext,
    size: RenderSize,
    working: WorkingSpace,
    pipeline: wgpu::ComputePipeline,
    effect_pipeline: wgpu::ComputePipeline,
    blank: wgpu::Texture,
    stats: TransferStats,
    validation: wgpu::Buffer,
}
fn space(space: InputSpace) -> u32 {
    match space {
        InputSpace::Srgb => 0,
        InputSpace::LinearRec709 => 1,
        InputSpace::LinearRec2020 => 2,
    }
}
impl ScenePass<'_> {
    fn texture(&self) -> Result<wgpu::Texture, GpuError> {
        self.gpu.texture(
            self.size.output_resolution[0],
            self.size.output_resolution[1],
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        )
    }
    fn pass(
        &mut self,
        mode: u32,
        inputs: (&wgpu::Texture, &wgpu::Texture),
        path: Option<&PathDraw>,
        opacity: f32,
        kind: MaskKind,
        output_transform: Option<OutputTransform>,
    ) -> Result<wgpu::Texture, GpuError> {
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
                u32::from(kind == MaskKind::Luminance),
                output_transform.map_or(1, |t| space(t.space)),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        params.extend(
            [
                u32::from(output_transform.is_some_and(|t| t.alpha == OutputAlpha::Premultiplied)),
                0,
                0,
                0,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let mut stop_bytes = Vec::new();
        let mut stop_count = 0u32;
        for g in [
            path.and_then(|p| p.fill_gradient.as_ref()),
            path.and_then(|p| p.stroke_gradient.as_ref()),
        ] {
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
        Ok(output)
    }
    fn effect_pass(
        &mut self,
        source: &wgpu::Texture,
        original: &wgpu::Texture,
        weights: &[f32],
        axis: u32,
        shadow: Option<([f32; 2], [f32; 4])>,
    ) -> Result<wgpu::Texture, GpuError> {
        let output = self.texture()?;
        let mut params = Vec::new();
        params.extend(
            [
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
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let (offset, color) = shadow.unwrap_or(([0.0; 2], [0.0; 4]));
        params.extend(
            [offset[0], offset[1], 0.0, 0.0]
                .into_iter()
                .chain(color)
                .flat_map(f32::to_le_bytes),
        );
        let weights: Vec<u8> = weights.iter().flat_map(|w| w.to_le_bytes()).collect();
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
        Ok(output)
    }
    fn effect(
        &mut self,
        source: &wgpu::Texture,
        effect: &PixelEffect,
    ) -> Result<wgpu::Texture, GpuError> {
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
        if failed {
            return Err(GpuError::InvalidInput(
                "RGBA16F surface value outside finite representable range",
            ));
        }
        Ok(())
    }
    fn node(
        &mut self,
        scene: &DrawScene,
        id: usize,
        cache: &mut [Option<wgpu::Texture>],
    ) -> Result<wgpu::Texture, GpuError> {
        if let Some(t) = &cache[id] {
            return Ok(t.clone());
        }
        let blank = self.blank.clone();
        let t = match &scene.nodes[id] {
            DrawNode::Raster(pixels) => {
                if pixels.len()
                    != pixel_count(
                        self.size.output_resolution[0],
                        self.size.output_resolution[1],
                    )?
                {
                    return Err(GpuError::InvalidInput("raster input dimensions"));
                }
                let texture = self.gpu.texture(
                    self.size.output_resolution[0],
                    self.size.output_resolution[1],
                    wgpu::TextureFormat::Rgba16Float,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
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
                        texture: &texture,
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
        cache[id] = Some(t.clone());
        Ok(t)
    }
    fn composite(
        &mut self,
        scene: &DrawScene,
        ids: &[usize],
        cache: &mut [Option<wgpu::Texture>],
    ) -> Result<wgpu::Texture, GpuError> {
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
        size.validate()?;
        scene.validate()?;
        // RGBA16F intermediate surfaces are retained for shared references. Bound
        // the conservative peak including root/child composite temporaries.
        check_scene_budget(size, scene, 8)?;
        let blank = self.texture(
            size.output_resolution[0],
            size.output_resolution[1],
            wgpu::TextureFormat::Rgba16Float,
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
            gpu: self,
            size,
            working,
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
        self.finish_render(size, &texture, working, pass.stats)
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
        let rgba16f = self.read_texture(&output, 8, &mut pass.stats)?;
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
        Ok(output)
    }
}
