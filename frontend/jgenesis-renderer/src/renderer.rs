use crate::config::{PreprocessShader, PrescaleMode, RendererConfig, Scanlines, WgpuBackend};
use cfg_if::cfg_if;
use jgenesis_common::frontend::{Color, DisplayArea, FrameSize, PixelAspectRatio, Renderer};
use jgenesis_common::timeutils;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Debug;
use std::{cmp, iter};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[cfg(feature = "ttf")]
use crate::ttf;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    texture_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

    fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

const VERTICES: [Vertex; 4] = [
    Vertex { position: [-1.0, -1.0], texture_coords: [0.0, 1.0] },
    Vertex { position: [1.0, -1.0], texture_coords: [1.0, 1.0] },
    Vertex { position: [-1.0, 1.0], texture_coords: [0.0, 0.0] },
    Vertex { position: [1.0, 1.0], texture_coords: [1.0, 0.0] },
];

trait PreprocessShaderExt {
    fn width_scale_factor(self, frame_width: u32) -> u32;
}

impl PreprocessShaderExt for PreprocessShader {
    fn width_scale_factor(self, frame_width: u32) -> u32 {
        match self {
            Self::HorizontalBlurSnesAdaptive if frame_width >= 512 => 1,
            Self::HorizontalBlurSnesAdaptive => 2,
            _ => 1,
        }
    }
}

enum PreprocessPipeline {
    None(wgpu::Texture),
    PreprocessShader {
        input: wgpu::Texture,
        output: wgpu::Texture,
        bind_groups: Vec<wgpu::BindGroup>,
        pipeline: wgpu::RenderPipeline,
    },
}

impl PreprocessPipeline {
    fn create(
        preprocess_shader: PreprocessShader,
        device: &wgpu::Device,
        input_texture: wgpu::Texture,
        shaders: &Shaders,
    ) -> Self {
        match preprocess_shader {
            PreprocessShader::None => Self::None(input_texture),
            PreprocessShader::HorizontalBlurTwoPixels
            | PreprocessShader::HorizontalBlurThreePixels
            | PreprocessShader::HorizontalBlurSnesAdaptive
            | PreprocessShader::AntiDitherWeak
            | PreprocessShader::AntiDitherStrong => {
                create_horizontal_blur_pipeline(preprocess_shader, device, input_texture, shaders)
            }
        }
    }

    fn input_texture(&self) -> &wgpu::Texture {
        match self {
            Self::None(texture) => texture,
            Self::PreprocessShader { input, .. } => input,
        }
    }

    fn output_texture(&self) -> &wgpu::Texture {
        match self {
            Self::None(texture) => texture,
            Self::PreprocessShader { output, .. } => output,
        }
    }

    fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        match self {
            Self::None(..) => {}
            Self::PreprocessShader { output, bind_groups, pipeline, .. } => {
                let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: "preprocess_render_pass".into(),
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
                });

                for (i, bind_group) in bind_groups.iter().enumerate() {
                    render_pass.set_bind_group(i as u32, bind_group, &[]);
                }
                render_pass.set_pipeline(pipeline);

                render_pass.draw(0..VERTICES.len() as u32, 0..1);
            }
        }
    }
}

fn create_horizontal_blur_pipeline(
    preprocess_shader: PreprocessShader,
    device: &wgpu::Device,
    input_texture: wgpu::Texture,
    shaders: &Shaders,
) -> PreprocessPipeline {
    let input_texture_view = input_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let width_scale_factor = preprocess_shader.width_scale_factor(input_texture.width());
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
        format: input_texture.format(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: "hblur_bind_group_layout".into(),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let texture_width_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: "hblur_texture_width_buffer".into(),
        contents: bytemuck::cast_slice(&padded_u32(input_texture.size().width)),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: "hblur_bind_group".into(),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &texture_width_buffer,
                    offset: 0,
                    size: None,
                }),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: "hblur_pipeline_layout".into(),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let fs_main = match preprocess_shader {
        PreprocessShader::HorizontalBlurTwoPixels => "hblur_2px",
        PreprocessShader::HorizontalBlurThreePixels => "hblur_3px",
        PreprocessShader::HorizontalBlurSnesAdaptive => "hblur_snes",
        PreprocessShader::AntiDitherWeak => "anti_dither_weak",
        PreprocessShader::AntiDitherStrong => "anti_dither_strong",
        PreprocessShader::None => panic!("Not a horizontal blur shader: {preprocess_shader:?}"),
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: "hblur_pipeline".into(),
        layout: Some(&pipeline_layout),
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
            entry_point: Some(fs_main),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_texture.format(),
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });

    PreprocessPipeline::PreprocessShader {
        input: input_texture,
        output: output_texture,
        bind_groups: vec![bind_group],
        pipeline,
    }
}

// WebGL requires all uniforms to be padded to a multiple of 16 bytes
fn padded_u32(value: u32) -> [u32; 4] {
    [value, 0, 0, 0]
}

struct RenderingPipeline {
    frame_size: FrameSize,
    display_area: DisplayArea,
    scaled_texture: wgpu::Texture,
    vertex_buffer: wgpu::Buffer,
    preprocess_pipeline: PreprocessPipeline,
    prescale_bind_group: wgpu::BindGroup,
    prescale_pipeline: wgpu::RenderPipeline,
    render_bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderResult {
    None,
    SuboptimalSurface,
}

impl RenderingPipeline {
    #[allow(clippy::too_many_arguments)]
    fn create(
        device: &wgpu::Device,
        limits: &wgpu::Limits,
        shaders: &Shaders,
        window_size: WindowSize,
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<PixelAspectRatio>,
        texture_format: wgpu::TextureFormat,
        surface_config: &wgpu::SurfaceConfiguration,
        renderer_config: RendererConfig,
    ) -> Self {
        let input_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "input_texture".into(),
            size: wgpu::Extent3d {
                width: frame_size.width,
                height: frame_size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let display_area = determine_display_area(
            window_size.width,
            window_size.height,
            frame_size,
            pixel_aspect_ratio,
            renderer_config.force_integer_height_scaling,
        );

        let filter_mode = renderer_config.filter_mode.to_wgpu_filter_mode();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: "sampler".into(),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: filter_mode,
            min_filter: filter_mode,
            mipmap_filter: filter_mode,
            ..wgpu::SamplerDescriptor::default()
        });

        let vertices = match pixel_aspect_ratio {
            Some(_) => compute_vertices(window_size.width, window_size.height, display_area),
            None => VERTICES.into(),
        };
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "vertex_buffer".into(),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        });

        let preprocess_pipeline = PreprocessPipeline::create(
            renderer_config.preprocess_shader,
            device,
            input_texture,
            shaders,
        );
        let preprocess_output_texture = preprocess_pipeline.output_texture();

        let prescale_factor = match renderer_config.prescale_mode {
            PrescaleMode::Auto => {
                let width_ratio = (f64::from(display_area.width)
                    / f64::from(frame_size.width)
                    / f64::from(pixel_aspect_ratio.unwrap_or(PixelAspectRatio::SQUARE)))
                .floor() as u32;
                let height_ratio = display_area.height / frame_size.height;
                let prescale_factor = cmp::max(1, cmp::max(width_ratio, height_ratio));

                log::info!(
                    "Auto-prescale setting prescale factor to {prescale_factor}x (measured width scale {width_ratio} and height_scale {height_ratio})"
                );

                prescale_factor
            }
            PrescaleMode::Manual(factor) => factor.get(),
        };
        let prescale_factor =
            clamp_prescale_factor(prescale_factor, preprocess_output_texture, limits);

        let prescale_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: "prescale_bind_group_layout".into(),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let preprocess_output_view =
            preprocess_output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let prescale_factor_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "prescale_factor_buffer".into(),
            contents: bytemuck::cast_slice(&padded_u32(prescale_factor)),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });

        let prescale_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "prescale_bind_group".into(),
            layout: &prescale_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&preprocess_output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &prescale_factor_buffer,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        });

        let prescale_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: "prescale_pipeline_layout".into(),
                bind_group_layouts: &[&prescale_bind_group_layout],
                push_constant_ranges: &[],
            });

        let scaled_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "scaled_texture".into(),
            size: wgpu::Extent3d {
                width: prescale_factor * preprocess_output_texture.width(),
                height: prescale_factor * preprocess_output_texture.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let scaled_texture_view =
            scaled_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let prescale_fs_main = match renderer_config.scanlines {
            Scanlines::None => "basic_prescale",
            Scanlines::Dim => "dim_scanlines",
            Scanlines::Black => "black_scanlines",
        };
        let prescale_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "prescale_pipeline".into(),
            layout: Some(&prescale_pipeline_layout),
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
                    format: scaled_texture.format(),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: "render_bind_group_layout".into(),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "render_bind_group".into(),
            layout: &render_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scaled_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: "render_pipeline_layout".into(),
                bind_group_layouts: &[&render_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "render_pipeline".into(),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shaders.render,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::buffer_layout()],
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
                module: &shaders.render,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self {
            frame_size,
            display_area,
            scaled_texture,
            vertex_buffer,
            preprocess_pipeline,
            prescale_bind_group,
            prescale_pipeline,
            render_bind_group,
            render_pipeline,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: &wgpu::Surface<'_>,
        frame_buffer: &[Color],
        #[cfg(feature = "ttf")] surface_config: &wgpu::SurfaceConfiguration,
        #[cfg(feature = "ttf")] modal_renderer: &mut ttf::ModalRenderer,
        frame_time_tracker: &mut FrameTimeTracker,
    ) -> Result<RenderResult, RendererError> {
        let output = surface.get_current_texture()?;
        let output_texture_view =
            output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let input_texture = self.preprocess_pipeline.input_texture();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(frame_buffer),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.frame_size.width * 4),
                rows_per_image: Some(self.frame_size.height),
            },
            input_texture.size(),
        );

        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: "encoder".into() });

        self.preprocess_pipeline.draw(&mut encoder);

        let scaled_texture_view =
            self.scaled_texture.create_view(&wgpu::TextureViewDescriptor::default());

        {
            let mut prescale_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "prescale_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scaled_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            prescale_pass.set_bind_group(0, &self.prescale_bind_group, &[]);
            prescale_pass.set_pipeline(&self.prescale_pipeline);

            prescale_pass.draw(0..VERTICES.len() as u32, 0..1);
        }

        #[cfg(feature = "ttf")]
        let modal_vertex_buffer = modal_renderer.prepare_modals(
            device,
            queue,
            surface_config.width,
            surface_config.height,
        )?;

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "render_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

            render_pass.draw(0..VERTICES.len() as u32, 0..1);

            #[cfg(feature = "ttf")]
            if let Some(modal_vertex_buffer) = &modal_vertex_buffer {
                modal_renderer.render(modal_vertex_buffer, &mut render_pass)?;
            }
        }

        queue.submit(iter::once(encoder.finish()));

        let render_result =
            if output.suboptimal { RenderResult::SuboptimalSurface } else { RenderResult::None };

        frame_time_tracker.sync();
        output.present();

        Ok(render_result)
    }
}

fn clamp_prescale_factor(
    prescale_factor: u32,
    preprocess_output_texture: &wgpu::Texture,
    limits: &wgpu::Limits,
) -> u32 {
    let max_dimension = limits.max_texture_dimension_2d;
    let max_prescale_factor = cmp::min(
        max_dimension / preprocess_output_texture.width(),
        max_dimension / preprocess_output_texture.height(),
    );

    if max_prescale_factor < prescale_factor {
        log::warn!(
            "Prescale factor {prescale_factor} is too high for frame size {}x{}; reducing to {max_prescale_factor}",
            preprocess_output_texture.width(),
            preprocess_output_texture.height()
        );
    }

    cmp::min(max_prescale_factor, prescale_factor)
}

fn compute_vertices(
    window_width: u32,
    window_height: u32,
    display_area: DisplayArea,
) -> Vec<Vertex> {
    log::info!(
        "Display area: width={}, height={}, left={}, top={}",
        display_area.width,
        display_area.height,
        display_area.x,
        display_area.y
    );

    VERTICES
        .into_iter()
        .map(|vertex| Vertex {
            position: [
                scale_vertex_position(
                    vertex.position[0],
                    window_width,
                    display_area.width,
                    display_area.x,
                ),
                scale_vertex_position(
                    vertex.position[1],
                    window_height,
                    display_area.height,
                    display_area.y,
                ),
            ],
            texture_coords: vertex.texture_coords,
        })
        .collect()
}

fn determine_display_area(
    window_width: u32,
    window_height: u32,
    frame_size: FrameSize,
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    force_integer_height_scaling: bool,
) -> DisplayArea {
    let Some(pixel_aspect_ratio) = pixel_aspect_ratio else {
        return DisplayArea { width: window_width, height: window_height, x: 0, y: 0 };
    };

    let pixel_aspect_ratio: f64 = pixel_aspect_ratio.into();

    let frame_aspect_ratio = f64::from(frame_size.width) / f64::from(frame_size.height);
    let screen_aspect_ratio = pixel_aspect_ratio * frame_aspect_ratio;

    let screen_width =
        cmp::min(window_width, (f64::from(window_height) * screen_aspect_ratio).round() as u32);
    let screen_height =
        cmp::min(window_height, (f64::from(screen_width) / screen_aspect_ratio).round() as u32);

    // Apply integer height scaling
    let (screen_width, screen_height) =
        if force_integer_height_scaling && screen_height >= frame_size.height {
            let scale_factor = screen_height / frame_size.height;
            let scaled_height = scale_factor * frame_size.height;
            let scaled_width = (f64::from(scaled_height) * screen_aspect_ratio).round() as u32;
            (scaled_width, scaled_height)
        } else {
            (screen_width, screen_height)
        };

    let x = (window_width - screen_width) / 2;
    let y = (window_height - screen_height) / 2;

    DisplayArea { width: screen_width, height: screen_height, x, y }
}

fn scale_vertex_position(
    position: f32,
    window_dimension: u32,
    screen_dimension: u32,
    offset: u32,
) -> f32 {
    let position = if position.is_sign_positive() {
        f64::from(screen_dimension + offset) / f64::from(window_dimension) * 2.0 - 1.0
    } else {
        f64::from(offset) / f64::from(window_dimension) * 2.0 - 1.0
    };
    position as f32
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("Error creating surface from window: {0}")]
    WindowHandleError(#[from] HandleError),
    #[error("Error creating wgpu surface: {0}")]
    WgpuCreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("Error requesting wgpu device: {0}")]
    WgpuRequestDevice(#[source] Box<dyn Error + Send + Sync + 'static>),
    #[error("Error getting handle to wgpu output surface: {0}")]
    WgpuSurface(#[from] wgpu::SurfaceError),
    #[error("Failed to obtain wgpu adapter")]
    NoWgpuAdapter,
    #[error(
        "wgpu adapter does not support present mode {desired:?}; supported modes are {available:?}"
    )]
    UnsupportedPresentMode { desired: wgpu::PresentMode, available: Vec<wgpu::PresentMode> },
    #[cfg(feature = "ttf")]
    #[error("Error preparing text to render: {0}")]
    GlyphonPrepare(#[from] glyphon::PrepareError),
    #[cfg(feature = "ttf")]
    #[error("Error rendering text: {0}")]
    GlyphonRender(#[from] glyphon::RenderError),
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
struct RequestDeviceErrorWrapper(String);

#[cfg(target_arch = "wasm32")]
impl std::fmt::Display for RequestDeviceErrorWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(target_arch = "wasm32")]
impl std::error::Error for RequestDeviceErrorWrapper {}

impl From<wgpu::RequestDeviceError> for RendererError {
    fn from(value: wgpu::RequestDeviceError) -> Self {
        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                // On web, wgpu::RequestDeviceError contains a JsValue which is not Send+Sync.
                // Serialize the error to a String, which is not ideal but keeps the error type
                // Send+Sync
                Self::WgpuRequestDevice(Box::new(RequestDeviceErrorWrapper(value.to_string())))
            } else {
                Self::WgpuRequestDevice(Box::new(value))
            }
        }
    }
}

struct Shaders {
    render: wgpu::ShaderModule,
    prescale: wgpu::ShaderModule,
    identity: wgpu::ShaderModule,
    hblur: wgpu::ShaderModule,
}

impl Shaders {
    fn create(device: &wgpu::Device) -> Self {
        let render = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));
        let prescale = device.create_shader_module(wgpu::include_wgsl!("prescale.wgsl"));
        let identity = device.create_shader_module(wgpu::include_wgsl!("identity.wgsl"));
        let hblur = device.create_shader_module(wgpu::include_wgsl!("hblur.wgsl"));

        Self { render, prescale, identity, hblur }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    frame_size: FrameSize,
    pixel_aspect_ratio_bits: u64,
}

impl PipelineKey {
    fn new(frame_size: FrameSize, pixel_aspect_ratio: Option<PixelAspectRatio>) -> Self {
        Self {
            frame_size,
            pixel_aspect_ratio_bits: pixel_aspect_ratio
                .map_or(f64::NAN.to_bits(), |par| f64::from(par).to_bits()),
        }
    }
}

struct RenderingPipelines {
    pipelines: HashMap<PipelineKey, RenderingPipeline>,
    last_display_info: Option<(FrameSize, DisplayArea)>,
}

impl RenderingPipelines {
    fn new() -> Self {
        Self { pipelines: HashMap::new(), last_display_info: None }
    }

    fn clear(&mut self) {
        self.pipelines.clear();
        self.last_display_info = None;
    }

    fn get_or_insert(
        &mut self,
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<PixelAspectRatio>,
        create_fn: impl FnOnce() -> RenderingPipeline,
    ) -> &RenderingPipeline {
        let pipeline = self
            .pipelines
            .entry(PipelineKey::new(frame_size, pixel_aspect_ratio))
            .or_insert_with(create_fn);

        self.last_display_info = Some((frame_size, pipeline.display_area));

        pipeline
    }
}

#[derive(Debug, Clone)]
struct FrameTimeTracker {
    sync_enabled: bool,
    last_frame_time_nanos: u128,
    frame_interval_nanos: u128,
}

impl FrameTimeTracker {
    fn new(sync_enabled: bool) -> Self {
        Self {
            sync_enabled,
            last_frame_time_nanos: timeutils::current_time_nanos(),
            frame_interval_nanos: (1_000_000_000.0_f64 / 60.0).round() as u128,
        }
    }

    fn sync(&mut self) {
        if !self.sync_enabled {
            return;
        }

        let next_frame_time = self.last_frame_time_nanos + self.frame_interval_nanos;
        let now = timeutils::sleep_until(next_frame_time);
        self.last_frame_time_nanos += self.frame_interval_nanos;

        if now > self.last_frame_time_nanos
            && (now - self.last_frame_time_nanos) > 5 * self.frame_interval_nanos
        {
            log::warn!("Frame time sync is more than 5 frames behind; catching up frame time");
            self.last_frame_time_nanos = now;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

pub struct WgpuRenderer<Window> {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_capabilities: wgpu::SurfaceCapabilities,
    device: wgpu::Device,
    device_limits: wgpu::Limits,
    queue: wgpu::Queue,
    shaders: Shaders,
    texture_format: wgpu::TextureFormat,
    renderer_config: RendererConfig,
    pipelines: RenderingPipelines,
    #[cfg(feature = "ttf")]
    modal_renderer: ttf::ModalRenderer,
    frame_count: u64,
    speed_multiplier: u64,
    frame_time_tracker: FrameTimeTracker,
    // SAFETY: The surface must not outlive the window it was created from, thus the window must be
    // declared after the surface
    window: Window,
    window_size: WindowSize,
}

impl<Window: HasDisplayHandle + HasWindowHandle> WgpuRenderer<Window> {
    /// Construct a wgpu renderer from the given window and config.
    ///
    /// # Errors
    ///
    /// This function will return any errors encountered while initializing wgpu.
    pub async fn new(
        window: Window,
        window_size: WindowSize,
        config: RendererConfig,
    ) -> Result<Self, RendererError> {
        let backends = match config.wgpu_backend {
            WgpuBackend::Auto => wgpu::Backends::PRIMARY,
            WgpuBackend::Vulkan => wgpu::Backends::VULKAN,
            WgpuBackend::DirectX12 => wgpu::Backends::DX12,
            WgpuBackend::OpenGl => wgpu::Backends::GL,
        };

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                        dxc_path: "dxcompiler.dll".into(),
                        dxil_path: "dxil.dll".into(),
                    },
                },
                gl: wgpu::GlBackendOptions::default(),
            },
        });

        // SAFETY: The surface must not outlive the window it was created from
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)
        }?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RendererError::NoWgpuAdapter)?;

        log::info!("Obtained wgpu adapter with backend {:?}", adapter.get_info().backend);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: "device".into(),
                    required_features: wgpu::Features::empty(),
                    required_limits: if config.use_webgl2_limits {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await?;

        let surface_capabilities = surface.get_capabilities(&adapter);

        let present_mode = config.vsync_mode.to_wgpu_present_mode();
        if !surface_capabilities.present_modes.contains(&present_mode) {
            return Err(RendererError::UnsupportedPresentMode {
                desired: present_mode,
                available: surface_capabilities.present_modes.clone(),
            });
        }

        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or_else(|| {
                log::warn!("wgpu adapter does not support any sRGB texture formats; defaulting to first format in this list: {:?}", surface_capabilities.formats);
                surface_capabilities.formats[0]
            });

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width,
            height: window_size.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let texture_format = if surface_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        let device_limits = device.limits();
        let shaders = Shaders::create(&device);

        #[cfg(feature = "ttf")]
        let modal_renderer = ttf::ModalRenderer::new(&device, &queue, surface_format);

        Ok(Self {
            surface,
            surface_config,
            surface_capabilities,
            device,
            device_limits,
            queue,
            shaders,
            texture_format,
            renderer_config: config,
            pipelines: RenderingPipelines::new(),
            #[cfg(feature = "ttf")]
            modal_renderer,
            frame_count: 0,
            speed_multiplier: 1,
            frame_time_tracker: FrameTimeTracker::new(config.frame_time_sync),
            window,
            window_size,
        })
    }
}

impl<Window> WgpuRenderer<Window> {
    pub fn reload_config(&mut self, mut config: RendererConfig) {
        let present_mode = config.vsync_mode.to_wgpu_present_mode();
        if self.surface_capabilities.present_modes.contains(&present_mode) {
            self.surface_config.present_mode = present_mode;
        } else {
            log::error!(
                "wgpu adapter does not support requested present mode '{present_mode:?}' for VSync mode '{:?}'; leaving VSync mode set to '{:?}'",
                config.vsync_mode,
                self.renderer_config.vsync_mode
            );
            config.vsync_mode = self.renderer_config.vsync_mode;
        }

        if !self.frame_time_tracker.sync_enabled && config.frame_time_sync {
            // Reset last frame time if frame time sync was just enabled
            self.frame_time_tracker.last_frame_time_nanos = timeutils::current_time_nanos();
        }
        self.frame_time_tracker.sync_enabled = config.frame_time_sync;

        self.renderer_config = config;
        self.surface.configure(&self.device, &self.surface_config);

        // Force render pipeline to be recreated on the next render_frame() call
        self.pipelines.clear();
    }

    pub fn handle_resize(&mut self, size: WindowSize) {
        self.window_size = size;

        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);

        // Force render pipeline to be recreated on the next render_frame() call
        self.pipelines.clear();
    }

    /// Obtain a shared reference to the window.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Obtain a mutable reference to the window.
    ///
    /// # Safety
    ///
    /// You must not reassign the window. You can freely mutate it and call any methods
    /// that require `&mut self`, but you must not do anything that will deallocate the existing
    /// window.
    pub unsafe fn window_mut(&mut self) -> &mut Window {
        &mut self.window
    }

    /// Set the speed multiplier. For a multiplier of N, only 1 out of every N frames will be rendered.
    ///
    /// # Panics
    ///
    /// This method will panic if `speed_multiplier` is 0.
    pub fn set_speed_multiplier(&mut self, speed_multiplier: u64) {
        assert_ne!(speed_multiplier, 0, "speed multiplier must be non-zero");
        self.speed_multiplier = speed_multiplier;
    }

    /// Set the target framerate to use for frame time sync (if enabled).
    ///
    /// # Panics
    ///
    /// This method will panic if `fps` is infinite, NaN, or 0.
    pub fn set_target_fps(&mut self, fps: f64) {
        assert!(fps.is_finite() && fps != 0.0);

        self.frame_time_tracker.frame_interval_nanos = (1_000_000_000.0_f64 / fps).round() as u128;

        log::debug!(
            "Set frame time interval to {}ns for target framerate {fps} FPS",
            self.frame_time_tracker.frame_interval_nanos
        );
    }

    /// Obtain the last rendered frame size and the current display area within the window.
    ///
    /// May return None if rendering config was just changed or initialized and a frame has not yet been rendered with
    /// the new config.
    #[must_use]
    pub fn current_display_info(&self) -> Option<(FrameSize, DisplayArea)> {
        self.pipelines.last_display_info
    }

    #[cfg(feature = "ttf")]
    pub fn add_modal(&mut self, text: String, duration: std::time::Duration) {
        self.modal_renderer.add_modal(text, duration);
    }
}

impl<Window> Renderer for WgpuRenderer<Window> {
    type Err = RendererError;

    fn render_frame(
        &mut self,
        frame_buffer: &[Color],
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<PixelAspectRatio>,
    ) -> Result<(), Self::Err> {
        self.frame_count += 1;
        if self.frame_count % self.speed_multiplier != 0 {
            return Ok(());
        }

        let pipeline = self.pipelines.get_or_insert(frame_size, pixel_aspect_ratio, || {
            log::info!("Creating render pipeline for frame size {frame_size:?} and pixel aspect ratio {pixel_aspect_ratio:?}");

            RenderingPipeline::create(
                &self.device,
                &self.device_limits,
                &self.shaders,
                self.window_size,
                frame_size,
                pixel_aspect_ratio,
                self.texture_format,
                &self.surface_config,
                self.renderer_config,
            )
        });

        match pipeline.render(
            &self.device,
            &self.queue,
            &self.surface,
            frame_buffer,
            #[cfg(feature = "ttf")]
            &self.surface_config,
            #[cfg(feature = "ttf")]
            &mut self.modal_renderer,
            &mut self.frame_time_tracker,
        ) {
            Ok(RenderResult::None) => {}
            Ok(RenderResult::SuboptimalSurface) => {
                log::debug!("Reconfiguring surface because graphics API reported it as suboptimal");
                self.surface.configure(&self.device, &self.surface_config);
            }
            Err(RendererError::WgpuSurface(wgpu::SurfaceError::Outdated)) => {
                // This can sometimes happen on Windows with the Vulkan backend while the window is minimized
                log::warn!(
                    "Skipping frame because wgpu surface has changed and swap chain is outdated"
                );
                self.surface.configure(&self.device, &self.surface_config);
            }
            Err(RendererError::WgpuSurface(wgpu::SurfaceError::Timeout)) => {
                log::warn!("Skipping frame because wgpu surface timed out");
                self.surface.configure(&self.device, &self.surface_config);
            }
            Err(err) => return Err(err),
        }

        Ok(())
    }
}
