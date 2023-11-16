use crate::config::{PreprocessShader, RendererConfig, Scanlines, WgpuBackend};
use jgenesis_common::frontend::{Color, FrameSize, PixelAspectRatio, Renderer};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use std::{cmp, iter, mem};
use thiserror::Error;
use wgpu::util::DeviceExt;

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
            array_stride: mem::size_of::<Vertex>() as u64,
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
            Self::HorizontalBlurSnesAdaptive if frame_width == 256 => 2,
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
        identity_shader: &wgpu::ShaderModule,
    ) -> Self {
        match preprocess_shader {
            PreprocessShader::None => Self::None(input_texture),
            PreprocessShader::HorizontalBlurTwoPixels
            | PreprocessShader::HorizontalBlurThreePixels
            | PreprocessShader::HorizontalBlurSnesAdaptive
            | PreprocessShader::AntiDitherWeak
            | PreprocessShader::AntiDitherStrong => create_horizontal_blur_pipeline(
                preprocess_shader,
                device,
                input_texture,
                identity_shader,
            ),
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
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: None,
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
    identity_shader: &wgpu::ShaderModule,
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

    let hblur_shader = device.create_shader_module(wgpu::include_wgsl!("hblur.wgsl"));
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
        vertex: wgpu::VertexState { module: identity_shader, entry_point: "vs_main", buffers: &[] },
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
            module: &hblur_shader,
            entry_point: fs_main,
            targets: &[Some(wgpu::ColorTargetState {
                format: output_texture.format(),
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
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
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    scaled_texture: wgpu::Texture,
    vertex_buffer: wgpu::Buffer,
    preprocess_pipeline: PreprocessPipeline,
    prescale_bind_group: wgpu::BindGroup,
    prescale_pipeline: wgpu::RenderPipeline,
    render_bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
}

impl RenderingPipeline {
    fn create(
        device: &wgpu::Device,
        window_size: (u32, u32),
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

        let prescale_factor = renderer_config.prescale_factor.get();

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

        let vertices = compute_vertices(
            window_size,
            frame_size,
            pixel_aspect_ratio,
            renderer_config.force_integer_height_scaling,
        );
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "vertex_buffer".into(),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        });

        let identity_shader = device.create_shader_module(wgpu::include_wgsl!("identity.wgsl"));
        let preprocess_pipeline = PreprocessPipeline::create(
            renderer_config.preprocess_shader,
            device,
            input_texture,
            &identity_shader,
        );

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

        let preprocess_output_texture = preprocess_pipeline.output_texture();
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

        let prescale_shader = device.create_shader_module(wgpu::include_wgsl!("prescale.wgsl"));
        let prescale_fs_main = match renderer_config.scanlines {
            Scanlines::None => "basic_prescale",
            Scanlines::Dim => "dim_scanlines",
            Scanlines::Black => "black_scanlines",
        };
        let prescale_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "prescale_pipeline".into(),
            layout: Some(&prescale_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &identity_shader,
                entry_point: "vs_main",
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
                module: &prescale_shader,
                entry_point: prescale_fs_main,
                targets: &[Some(wgpu::ColorTargetState {
                    format: scaled_texture.format(),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
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

        let render_shader = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "render_pipeline".into(),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: "vs_main",
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
                module: &render_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });

        Self {
            frame_size,
            pixel_aspect_ratio,
            scaled_texture,
            vertex_buffer,
            preprocess_pipeline,
            prescale_bind_group,
            prescale_pipeline,
            render_bind_group,
            render_pipeline,
        }
    }

    fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: &wgpu::Surface,
        frame_buffer: &[Color],
    ) -> Result<(), RendererError> {
        let output = surface.get_current_texture()?;
        let output_texture_view =
            output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let input_texture = self.preprocess_pipeline.input_texture();
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(frame_buffer),
            wgpu::ImageDataLayout {
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
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            prescale_pass.set_bind_group(0, &self.prescale_bind_group, &[]);
            prescale_pass.set_pipeline(&self.prescale_pipeline);

            prescale_pass.draw(0..VERTICES.len() as u32, 0..1);
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "render_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

            render_pass.draw(0..VERTICES.len() as u32, 0..1);
        }

        queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

fn compute_vertices(
    (window_width, window_height): (u32, u32),
    frame_size: FrameSize,
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    force_integer_height_scaling: bool,
) -> Vec<Vertex> {
    let Some(pixel_aspect_ratio) = pixel_aspect_ratio else {
        return VERTICES.into();
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

    log::info!("Display area: width={screen_width}, height={screen_height}, left={x}, top={y}");

    VERTICES
        .into_iter()
        .map(|vertex| Vertex {
            position: [
                scale_vertex_position(vertex.position[0], window_width, screen_width, x),
                scale_vertex_position(vertex.position[1], window_height, screen_height, y),
            ],
            texture_coords: vertex.texture_coords,
        })
        .collect()
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
    #[error("Error creating wgpu surface: {0}")]
    WgpuCreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("Error requesting wgpu device: {0}")]
    WgpuRequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("Error getting handle to wgpu output surface: {0}")]
    WgpuSurface(#[from] wgpu::SurfaceError),
    #[error("Failed to obtain wgpu adapter")]
    NoWgpuAdapter,
    #[error(
        "wgpu adapter does not support present mode {desired:?}; supported modes are {available:?}"
    )]
    UnsupportedPresentMode { desired: wgpu::PresentMode, available: Vec<wgpu::PresentMode> },
}

pub type WindowSizeFn<Window> = fn(&Window) -> (u32, u32);

pub struct WgpuRenderer<Window> {
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture_format: wgpu::TextureFormat,
    renderer_config: RendererConfig,
    pipeline: Option<RenderingPipeline>,
    frame_count: u64,
    speed_multiplier: u64,
    // SAFETY: The surface must not outlive the window it was created from, thus the window must be
    // declared after the surface
    window: Window,
    window_size_fn: WindowSizeFn<Window>,
}

impl<Window: HasRawDisplayHandle + HasRawWindowHandle> WgpuRenderer<Window> {
    /// Construct a wgpu renderer from the given window and config.
    ///
    /// # Errors
    ///
    /// This function will return any errors encountered while initializing wgpu.
    pub async fn new(
        window: Window,
        window_size_fn: WindowSizeFn<Window>,
        config: RendererConfig,
    ) -> Result<Self, RendererError> {
        let backends = match config.wgpu_backend {
            WgpuBackend::Auto => wgpu::Backends::PRIMARY,
            WgpuBackend::Vulkan => wgpu::Backends::VULKAN,
            WgpuBackend::DirectX12 => wgpu::Backends::DX12,
            WgpuBackend::OpenGl => wgpu::Backends::GL,
        };

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            dx12_shader_compiler: wgpu::Dx12Compiler::default(),
        });

        // SAFETY: The surface must not outlive the window it was created from
        let surface = unsafe { instance.create_surface(&window) }?;

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
                    features: wgpu::Features::empty(),
                    limits: if config.use_webgl2_limits {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
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

        let (window_width, window_height) = window_size_fn(&window);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_width,
            height: window_height,
            present_mode,
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let texture_format = if surface_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        Ok(Self {
            surface,
            surface_config,
            device,
            queue,
            texture_format,
            renderer_config: config,
            pipeline: None,
            frame_count: 0,
            speed_multiplier: 1,
            window,
            window_size_fn,
        })
    }
}

impl<Window> WgpuRenderer<Window> {
    pub fn reload_config(&mut self, config: RendererConfig) {
        self.renderer_config = config;
        self.surface_config.present_mode = config.vsync_mode.to_wgpu_present_mode();
        self.surface.configure(&self.device, &self.surface_config);

        // Force render pipeline to be recreated on the next render_frame() call
        self.pipeline = None;
    }

    pub fn handle_resize(&mut self) {
        let (window_width, window_height) = (self.window_size_fn)(&self.window);
        self.surface_config.width = window_width;
        self.surface_config.height = window_height;
        self.surface.configure(&self.device, &self.surface_config);

        // Force render pipeline to be recreated on the next render_frame() call
        self.pipeline = None;
    }

    fn ensure_pipeline(
        &mut self,
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<PixelAspectRatio>,
    ) {
        if self.pipeline.is_none()
            || self.pipeline.as_ref().is_some_and(|pipeline| {
                pipeline.frame_size != frame_size
                    || pipeline.pixel_aspect_ratio != pixel_aspect_ratio
            })
        {
            log::info!(
                "Creating render pipeline for frame size {frame_size:?} and pixel aspect ratio {pixel_aspect_ratio:?}"
            );

            let window_size = (self.window_size_fn)(&self.window);
            self.pipeline = Some(RenderingPipeline::create(
                &self.device,
                window_size,
                frame_size,
                pixel_aspect_ratio,
                self.texture_format,
                &self.surface_config,
                self.renderer_config,
            ));
        }
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

        self.ensure_pipeline(frame_size, pixel_aspect_ratio);
        self.pipeline.as_ref().unwrap().render(
            &self.device,
            &self.queue,
            &self.surface,
            frame_buffer,
        )?;

        Ok(())
    }
}
