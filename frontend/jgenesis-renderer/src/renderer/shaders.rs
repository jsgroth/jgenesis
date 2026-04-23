use crate::config::{AntiDitherShader, PreprocessShader, PrescaleMode, RendererConfig, Scanlines};
use crate::renderer::{PipelineShader, Shaders};
use jgenesis_common::frontend::{ColorCorrection, DisplayArea, FiniteF64, FrameSize};
use std::sync::Arc;
use wgpu::util::DeviceExt;

const IDENTITY_VERTICES: u32 = 4;

const SRGB_TEX_VIEW_DESCRIPTOR: wgpu::TextureViewDescriptor<'static> =
    wgpu::TextureViewDescriptor {
        label: None,
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        dimension: None,
        usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    };

fn basic_render_pass<'encoder, 'label>(
    encoder: &'encoder mut wgpu::CommandEncoder,
    output: &wgpu::Texture,
    output_format: wgpu::TextureFormat,
    label: impl Into<wgpu::Label<'label>>,
) -> wgpu::RenderPass<'encoder> {
    let output_view = output.create_view(&wgpu::TextureViewDescriptor {
        format: Some(output_format),
        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
        ..wgpu::TextureViewDescriptor::default()
    });

    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: label.into(),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &output_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

pub struct ColorCorrectionShader {
    output: Arc<wgpu::Texture>,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl ColorCorrectionShader {
    pub fn create(
        correction: ColorCorrection,
        input: &wgpu::Texture,
        device: &wgpu::Device,
        shaders: &Shaders,
    ) -> Option<Self> {
        let (fs_main, screen_gamma) = match correction {
            ColorCorrection::GbcLcd { screen_gamma } => ("gbc_color_correction", screen_gamma),
            ColorCorrection::GbaLcd { screen_gamma } => ("gba_color_correction", screen_gamma),
            ColorCorrection::None => return None,
        };

        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: "color_correction_texture".into(),
            size: input.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "color_correction_pipeline".into(),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shaders.identity,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shaders.gb_color,
                entry_point: Some(fs_main),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let input_view = input.create_view(&SRGB_TEX_VIEW_DESCRIPTOR);
        let gamma_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "color_correction_gamma_buffer".into(),
            contents: bytemuck::cast_slice(&[f32::from(screen_gamma)]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "color_correction_bind_group".into(),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        gamma_buffer.as_entire_buffer_binding(),
                    ),
                },
            ],
        });

        Some(Self { output: Arc::new(output), bind_group, pipeline })
    }
}

impl PipelineShader for ColorCorrectionShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut render_pass = basic_render_pass(
            encoder,
            &self.output,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "color_correction_render_pass",
        );

        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_pipeline(&self.pipeline);

        render_pass.draw(0..IDENTITY_VERTICES, 0..1);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }
}

pub struct FrameBlendShader {
    previous_frame: Arc<wgpu::Texture>,
    input: Arc<wgpu::Texture>,
    output: Arc<wgpu::Texture>,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    skip_next_frame: bool,
}

impl FrameBlendShader {
    pub fn create(input: Arc<wgpu::Texture>, device: &wgpu::Device, shaders: &Shaders) -> Self {
        let previous_frame_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "blend_previous_frame_texture".into(),
            size: input.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "blend_output_texture".into(),
            size: input.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "blend_pipeline".into(),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shaders.identity,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shaders.frame_blend,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "blend_bind_group".into(),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &input.create_view(&SRGB_TEX_VIEW_DESCRIPTOR),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &previous_frame_texture.create_view(&SRGB_TEX_VIEW_DESCRIPTOR),
                    ),
                },
            ],
        });

        Self {
            previous_frame: Arc::new(previous_frame_texture),
            input,
            output: Arc::new(output_texture),
            bind_group,
            pipeline,
            skip_next_frame: true,
        }
    }
}

impl PipelineShader for FrameBlendShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.skip_next_frame {
            let mut render_pass = basic_render_pass(
                encoder,
                &self.output,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                "blend_render_pass",
            );

            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_pipeline(&self.pipeline);

            render_pass.draw(0..IDENTITY_VERTICES, 0..1);
        } else {
            encoder.copy_texture_to_texture(
                self.input.as_image_copy(),
                self.output.as_image_copy(),
                self.input.size(),
            );
        }
        self.skip_next_frame = false;

        encoder.copy_texture_to_texture(
            self.input.as_image_copy(),
            self.previous_frame.as_image_copy(),
            self.input.size(),
        );
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }

    fn reset_interframe_state(&mut self) {
        self.skip_next_frame = true;
    }
}

pub struct BlurShader {
    output: Arc<wgpu::Texture>,
    bind_groups: Vec<wgpu::BindGroup>,
    pipeline: wgpu::RenderPipeline,
}

impl BlurShader {
    pub fn create_horizontal_blur(
        preprocess_shader: PreprocessShader,
        device: &wgpu::Device,
        input_texture: &wgpu::Texture,
        shaders: &Shaders,
    ) -> Option<Self> {
        let fs_main = match preprocess_shader {
            PreprocessShader::HorizontalBlurTwoPixels => "hblur_2px",
            PreprocessShader::HorizontalBlurThreePixels => "hblur_3px",
            PreprocessShader::HorizontalBlurSnesAdaptive => "hblur_snes",
            _ => return None,
        };

        let width_scale_factor = match preprocess_shader {
            PreprocessShader::HorizontalBlurSnesAdaptive if input_texture.width() >= 512 => 1,
            PreprocessShader::HorizontalBlurSnesAdaptive => 2,
            _ => 1,
        };

        Some(Self::create(device, input_texture, shaders, fs_main, width_scale_factor))
    }

    pub fn create_anti_dither(
        anti_dither_shader: AntiDitherShader,
        device: &wgpu::Device,
        input_texture: &wgpu::Texture,
        shaders: &Shaders,
    ) -> Option<Self> {
        let fs_main = match anti_dither_shader {
            AntiDitherShader::Weak => "anti_dither_weak",
            AntiDitherShader::Strong => "anti_dither_strong",
            AntiDitherShader::None => return None,
        };

        Some(Self::create(device, input_texture, shaders, fs_main, 1))
    }

    pub fn create(
        device: &wgpu::Device,
        input_texture: &wgpu::Texture,
        shaders: &Shaders,
        fragment_entry_point: &str,
        width_scale_factor: u32,
    ) -> Self {
        let input_texture_view = input_texture.create_view(&SRGB_TEX_VIEW_DESCRIPTOR);

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "preprocess_output_texture".into(),
            size: wgpu::Extent3d {
                width: input_texture.width() * width_scale_factor,
                height: input_texture.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "hblur_pipeline".into(),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shaders.identity,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shaders.hblur,
                entry_point: Some(fragment_entry_point),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let texture_width_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "hblur_texture_width_buffer".into(),
            contents: bytemuck::cast_slice(&[input_texture.size().width]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "hblur_bind_group".into(),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        texture_width_buffer.as_entire_buffer_binding(),
                    ),
                },
            ],
        });

        Self { output: Arc::new(output_texture), bind_groups: vec![bind_group], pipeline }
    }
}

impl PipelineShader for BlurShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut render_pass = basic_render_pass(
            encoder,
            &self.output,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "preprocess_render_pass",
        );

        for (i, bind_group) in self.bind_groups.iter().enumerate() {
            render_pass.set_bind_group(i as u32, bind_group, &[]);
        }
        render_pass.set_pipeline(&self.pipeline);

        render_pass.draw(0..IDENTITY_VERTICES, 0..1);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }
}

pub struct PrescaleShader {
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    output: Arc<wgpu::Texture>,
}

impl PrescaleShader {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        renderer_config: RendererConfig,
        frame_size: FrameSize,
        display_area: DisplayArea,
        pixel_aspect_ratio: Option<FiniteF64>,
        input: &wgpu::Texture,
        device: &wgpu::Device,
        limits: &wgpu::Limits,
        shaders: &Shaders,
    ) -> Option<Self> {
        let (prescale_width, prescale_height) = determine_prescale_factors(
            renderer_config.prescale_mode,
            frame_size,
            pixel_aspect_ratio,
            display_area,
            input.size(),
            limits,
        );

        if prescale_width <= 1
            && prescale_height <= 1
            && renderer_config.scanlines == Scanlines::None
        {
            return None;
        }

        log::info!(
            "Creating prescale shader with width factor {prescale_width}x and height factor {prescale_height}x",
        );

        let scaled_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "scaled_texture".into(),
            size: wgpu::Extent3d {
                width: prescale_width * input.width(),
                height: prescale_height * input.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let (prescale_fs_main, scanline_multiplier) = match renderer_config.scanlines {
            Scanlines::None => ("basic_prescale", 1.0),
            Scanlines::SlightDim => ("scanlines", 0.75),
            Scanlines::Dim => ("scanlines", 0.5),
            Scanlines::VeryDim => ("scanlines", 0.25),
            Scanlines::Black => ("scanlines", 0.0),
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "prescale_pipeline".into(),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shaders.identity,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shaders.prescale,
                entry_point: Some(prescale_fs_main),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let prescale_params = [
            prescale_width,
            prescale_height,
            frame_size.height,
            scaled_texture.height(),
            (scanline_multiplier as f32).to_bits(),
        ];

        let prescale_factor_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "prescale_factor_buffer".into(),
            contents: bytemuck::cast_slice(&prescale_params),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });

        let input_view = input.create_view(&SRGB_TEX_VIEW_DESCRIPTOR);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "prescale_bind_group".into(),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        prescale_factor_buffer.as_entire_buffer_binding(),
                    ),
                },
            ],
        });

        Some(Self { bind_group, pipeline, output: Arc::new(scaled_texture) })
    }
}

fn determine_prescale_factors(
    mode: PrescaleMode,
    frame_size: FrameSize,
    pixel_aspect_ratio: Option<FiniteF64>,
    display_area: DisplayArea,
    input_size: wgpu::Extent3d,
    limits: &wgpu::Limits,
) -> (u32, u32) {
    let (target_width, target_height) = match mode {
        PrescaleMode::Auto => {
            let width = match pixel_aspect_ratio {
                Some(par) => {
                    let frame_aspect_ratio =
                        f64::from(frame_size.width) / f64::from(frame_size.height);
                    let screen_aspect_ratio = f64::from(par) * frame_aspect_ratio;
                    f64::from(display_area.height) * screen_aspect_ratio
                }
                None => f64::from(display_area.width),
            };
            let height = f64::from(display_area.height);
            (width, height)
        }
        PrescaleMode::Manual { width, height } => {
            let width = f64::from(width.get() * frame_size.width);
            let height = f64::from(height.get() * frame_size.height);
            (width, height)
        }
    };

    let width_ratio = (target_width / f64::from(input_size.width)) as u32;
    let height_ratio = (target_height / f64::from(input_size.height)) as u32;
    let prescale_width = clamp_prescale_factor(width_ratio, input_size.width, limits);
    let prescale_height = clamp_prescale_factor(height_ratio, input_size.height, limits);

    (prescale_width, prescale_height)
}

fn clamp_prescale_factor(prescale_factor: u32, input_dimension: u32, limits: &wgpu::Limits) -> u32 {
    let max_dimension = limits.max_texture_dimension_2d;
    let max_prescale_factor = max_dimension / input_dimension;

    if max_prescale_factor < prescale_factor {
        log::warn!(
            "Prescale factor {prescale_factor} is too high for frame dimension {input_dimension}; reducing to {max_prescale_factor}",
        );
    }

    prescale_factor.clamp(1, max_prescale_factor)
}

impl PipelineShader for PrescaleShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut prescale_pass = basic_render_pass(
            encoder,
            &self.output,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "prescale_render_pass",
        );

        prescale_pass.set_bind_group(0, &self.bind_group, &[]);
        prescale_pass.set_pipeline(&self.pipeline);

        prescale_pass.draw(0..IDENTITY_VERTICES, 0..1);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }
}

pub struct UpscaleShader {
    output: Arc<wgpu::Texture>,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    x_workgroups: u32,
    y_workgroups: u32,
}

impl UpscaleShader {
    pub fn create_xbrz(
        preprocess_shader: PreprocessShader,
        device: &wgpu::Device,
        shaders: &Shaders,
        input: &wgpu::Texture,
    ) -> Option<Self> {
        let scale_factor = match preprocess_shader {
            PreprocessShader::Xbrz2x => 2,
            PreprocessShader::Xbrz3x => 3,
            PreprocessShader::Xbrz4x => 4,
            PreprocessShader::Xbrz5x => 5,
            PreprocessShader::Xbrz6x => 6,
            _ => return None,
        };

        let shader = (&shaders.xbrz, None);
        let shader_constants = [("scale_factor", scale_factor.into())];

        Some(Self::create(device, shader, &shader_constants, input, scale_factor))
    }

    pub fn create_mmpx(device: &wgpu::Device, shaders: &Shaders, input: &wgpu::Texture) -> Self {
        Self::create(device, (&shaders.mmpx, None), &[], input, 2)
    }

    pub fn create(
        device: &wgpu::Device,
        (shader_module, shader_entry_point): (&wgpu::ShaderModule, Option<&str>),
        shader_constants: &[(&str, f64)],
        input: &wgpu::Texture,
        scale_factor: u32,
    ) -> Self {
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: "xbrz_texture".into(),
            size: wgpu::Extent3d {
                width: scale_factor * input.width(),
                height: scale_factor * input.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: "xbrz_pipeline".into(),
            layout: None,
            module: shader_module,
            entry_point: shader_entry_point,
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: shader_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "xbrz_bind_group".into(),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
            ],
        });

        let x_workgroups = input.width().div_ceil(16);
        let y_workgroups = input.height().div_ceil(16);

        Self { output: Arc::new(output), bind_group, pipeline, x_workgroups, y_workgroups }
    }
}

impl PipelineShader for UpscaleShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());

        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        compute_pass.set_pipeline(&self.pipeline);

        compute_pass.dispatch_workgroups(self.x_workgroups, self.y_workgroups, 1);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrescaleFactor;

    fn basic_auto_prescale_test(
        width: u32,
        height: u32,
        width_scale: u32,
        height_scale: u32,
    ) -> (u32, u32) {
        determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width, height },
            None,
            DisplayArea { width: width * width_scale, height: height * height_scale, x: 0, y: 0 },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        )
    }

    #[test]
    fn auto_prescale_square() {
        let (width, height) = basic_auto_prescale_test(320, 240, 4, 4);

        assert_eq!(width, 4);
        assert_eq!(height, 4);
    }

    #[test]
    fn auto_prescale_horizontal_rect() {
        let (width, height) = basic_auto_prescale_test(320, 240, 4, 2);

        assert_eq!(width, 4);
        assert_eq!(height, 2);
    }

    #[test]
    fn auto_prescale_vertical_rect() {
        let (width, height) = basic_auto_prescale_test(320, 240, 2, 4);

        assert_eq!(width, 2);
        assert_eq!(height, 4);
    }

    #[test]
    fn auto_prescale_squish_vertical() {
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width: 320, height: 480 },
            Some(FiniteF64::try_from(2.0).unwrap()),
            DisplayArea { width: 320 * 4, height: 240 * 4, x: 0, y: 0 },
            wgpu::Extent3d { width: 320, height: 480, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 4);
        assert_eq!(height, 2);
    }

    #[test]
    fn auto_prescale_squish_horizontal() {
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width: 512, height: 240 },
            Some(FiniteF64::try_from(0.5).unwrap()),
            DisplayArea { width: 256 * 4, height: 240 * 4, x: 0, y: 0 },
            wgpu::Extent3d { width: 512, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 2);
        assert_eq!(height, 4);
    }

    #[test]
    fn auto_prescale_scaled_input() {
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width: 320, height: 240 },
            None,
            DisplayArea { width: 320 * 4, height: 240 * 4, x: 0, y: 0 },
            wgpu::Extent3d { width: 320 * 2, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 2);
        assert_eq!(height, 4);
    }

    #[test]
    fn auto_prescale_round_down() {
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width: 320, height: 240 },
            None,
            DisplayArea { width: 320 * 11 / 4, height: 240 * 7 / 4, x: 0, y: 0 },
            wgpu::Extent3d { width: 320, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 2);
        assert_eq!(height, 1);
    }

    #[test]
    fn auto_prescale_pixel_aspect_ratio() {
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Auto,
            FrameSize { width: 320, height: 240 },
            Some(FiniteF64::try_from(0.9).unwrap()),
            DisplayArea { width: 320 * 2 * 9 / 10, height: 240 * 2, x: 0, y: 0 },
            wgpu::Extent3d { width: 320, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        // Sub-1 pixel aspect ratio should drop prescale factor
        assert_eq!(width, 1);
        assert_eq!(height, 2);
    }

    #[test]
    fn manual_prescale_basic() {
        let factor = PrescaleFactor::try_from(5).unwrap();
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Manual { width: factor, height: factor },
            FrameSize { width: 320, height: 240 },
            None,
            DisplayArea { width: 320 * 5, height: 240 * 5, x: 0, y: 0 },
            wgpu::Extent3d { width: 320, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 5);
        assert_eq!(height, 5);
    }

    #[test]
    fn manual_prescale_scaled_input() {
        let factor = PrescaleFactor::try_from(5).unwrap();
        let (width, height) = determine_prescale_factors(
            PrescaleMode::Manual { width: factor, height: factor },
            FrameSize { width: 320, height: 240 },
            None,
            DisplayArea { width: 320 * 5, height: 240 * 5, x: 0, y: 0 },
            wgpu::Extent3d { width: 320 * 2, height: 240, depth_or_array_layers: 1 },
            &wgpu::Limits::default(),
        );

        assert_eq!(width, 2);
        assert_eq!(height, 5);
    }
}
