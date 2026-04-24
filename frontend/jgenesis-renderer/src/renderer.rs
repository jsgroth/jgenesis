mod ntsc;
mod shaders;

use crate::config;
use crate::config::{FrameRotation, PreprocessShader, RendererConfig};
use crate::renderer::ntsc::{NtscShader, NtscShaderVariant};
use crate::renderer::shaders::{
    BlurShader, ColorCorrectionShader, FrameBlendShader, PrescaleShader, UpscaleShader,
};
#[cfg(feature = "ttf")]
use crate::ttf;
use jgenesis_common::frontend::{
    Color, DisplayArea, DisplayInfo, FiniteF64, FrameSize, RenderFrameOptions,
    RenderFrameOptionsHashable, Renderer,
};
use jgenesis_common::timeutils;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::{cmp, iter};
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

trait PipelineShader {
    #[allow(unused_variables)]
    fn prepare(&mut self, device: &wgpu::Device, options: RenderFrameOptions) {}

    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder);

    fn output_texture(&self) -> &Arc<wgpu::Texture>;

    fn reset_interframe_state(&mut self) {}
}

impl FrameRotation {
    fn rotate_frame_size_and_aspect_ratio(
        self,
        size: FrameSize,
        pixel_aspect_ratio: Option<FiniteF64>,
    ) -> (FrameSize, Option<FiniteF64>) {
        match self {
            Self::None | Self::OneEighty => (size, pixel_aspect_ratio),
            Self::Clockwise | Self::Counterclockwise => {
                let rotated_size = FrameSize { width: size.height, height: size.width };
                let rotated_par =
                    pixel_aspect_ratio.and_then(|par| FiniteF64::try_from(1.0 / par.get()).ok());

                (rotated_size, rotated_par)
            }
        }
    }

    fn rotate_display_area_size(self, area: DisplayArea) -> (u32, u32) {
        match self {
            Self::None | Self::OneEighty => (area.width, area.height),
            Self::Clockwise | Self::Counterclockwise => (area.height, area.width),
        }
    }

    fn rotate_texture_coords(self, [x, y]: [f32; 2]) -> [f32; 2] {
        // Rotation is reversed because input coordinates are position in the rotated frame, and
        // return value should be position in the original frame
        match self {
            Self::None => [x, y],
            Self::Clockwise => [y, 1.0 - x],
            Self::OneEighty => [1.0 - x, 1.0 - y],
            Self::Counterclockwise => [1.0 - y, x],
        }
    }
}

struct RenderingPipeline {
    frame_size: FrameSize,
    display_area: DisplayArea,
    rotation: FrameRotation,
    input_texture: Arc<wgpu::Texture>,
    shader_pipeline: Vec<Box<dyn PipelineShader>>,
    vertex_buffer: wgpu::Buffer,
    render_bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
    multisample_output: Option<wgpu::Texture>,
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
        options: RenderFrameOptions,
        surface_config: &wgpu::SurfaceConfiguration,
        renderer_config: RendererConfig,
    ) -> Self {
        fn current_output_texture(
            pipeline: &[Box<dyn PipelineShader>],
            input: &Arc<wgpu::Texture>,
        ) -> Arc<wgpu::Texture> {
            Arc::clone(pipeline.last().map_or(input, |shader| shader.output_texture()))
        }

        let input_texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
            label: "input_texture".into(),
            size: wgpu::Extent3d {
                width: frame_size.width,
                height: frame_size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        }));

        // For 90/270 degree rotations, compute display area based on swapped frame width/height and
        // inverted aspect ratio
        let (rotated_frame_size, rotated_aspect_ratio) = renderer_config
            .frame_rotation
            .rotate_frame_size_and_aspect_ratio(frame_size, options.pixel_aspect_ratio);
        let display_area = determine_display_area(
            window_size.width,
            window_size.height,
            rotated_frame_size,
            rotated_aspect_ratio,
            renderer_config.force_integer_height_scaling,
        );

        // Pipeline shaders (all optional):
        //   1. Color correction
        //   2. Anti-dither
        //   3. NTSC composite / Upscaling / Horizontal blur
        //   4. Frame blending
        //   5. Prescale / Scanlines
        let mut shader_pipeline: Vec<Box<dyn PipelineShader>> = Vec::new();

        macro_rules! current_output_texture {
            () => {
                current_output_texture(&shader_pipeline, &input_texture)
            };
        }

        // GBC/GBA color correction
        if let Some(color_correction_shader) = ColorCorrectionShader::create(
            options.color_correction,
            &current_output_texture!(),
            device,
            shaders,
        ) {
            log::debug!("Adding color correction shader");
            shader_pipeline.push(Box::new(color_correction_shader));
        }

        // Anti-dither
        if !renderer_config.preprocess_shader.exclude_anti_dither()
            && let Some(anti_dither_shader) = BlurShader::create_anti_dither(
                renderer_config.anti_dither_shader,
                device,
                &current_output_texture!(),
                shaders,
            )
        {
            log::debug!("Adding anti-dither shader");
            shader_pipeline.push(Box::new(anti_dither_shader));
        }

        // NTSC composite
        if renderer_config.preprocess_shader == PreprocessShader::NtscComposite
            && let Some(params) = options.composite_params
        {
            log::debug!("Adding NTSC composite shader");

            let variant = if options.emulate_nes_ntsc_output {
                NtscShaderVariant::NesPpu
            } else {
                NtscShaderVariant::Rgb
            };
            shader_pipeline.push(Box::new(NtscShader::create(
                device,
                shaders,
                &current_output_texture!(),
                params,
                renderer_config.ntsc_config,
                variant,
            )));
        }

        // Horizontal blur
        if let Some(blur_shader) = BlurShader::create_horizontal_blur(
            renderer_config.preprocess_shader,
            device,
            &current_output_texture!(),
            shaders,
        ) {
            log::debug!("Adding blur shader");
            shader_pipeline.push(Box::new(blur_shader));
        }

        // xBRZ upscaling
        if let Some(xbrz_shader) = UpscaleShader::create_xbrz(
            renderer_config.preprocess_shader,
            device,
            shaders,
            &current_output_texture!(),
        ) {
            log::debug!("Adding xBRZ shader");
            shader_pipeline.push(Box::new(xbrz_shader));
        }

        // MMPX upscaling
        if renderer_config.preprocess_shader == PreprocessShader::Mmpx {
            log::debug!("Adding MMPX shader");
            shader_pipeline.push(Box::new(UpscaleShader::create_mmpx(
                device,
                shaders,
                &current_output_texture!(),
            )));
        }

        // Frame blending
        if options.frame_blending {
            log::debug!("Adding frame blending shader");
            shader_pipeline.push(Box::new(FrameBlendShader::create(
                current_output_texture!(),
                device,
                shaders,
            )));
        }

        // Prescaling / Scanlines
        if let Some(prescale_shader) = PrescaleShader::create(
            renderer_config,
            frame_size,
            display_area,
            options.pixel_aspect_ratio,
            &current_output_texture!(),
            device,
            limits,
            shaders,
        ) {
            log::debug!("Adding prescale/scanlines shader");
            shader_pipeline.push(Box::new(prescale_shader));
        }

        let render_input_texture = current_output_texture!();
        let render_input_view = render_input_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            ..wgpu::TextureViewDescriptor::default()
        });

        // Use multisampled rendering only when the final frame texture is at least twice as large
        // as the display area in at least 1 dimension; otherwise it's just a waste of compute
        let multisample = renderer_config.supersample_minification
            && (render_input_texture.width() > 2 * display_area.width
                || render_input_texture.height() > 2 * display_area.height);

        let multisample_output = multisample.then(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: "multisample_output_texture".into(),
                size: wgpu::Extent3d {
                    width: surface_config.width,
                    height: surface_config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 4,
                dimension: wgpu::TextureDimension::D2,
                format: surface_config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });

        // If surface format is not sRGB-aware, fragment shader needs to perform gamma encoding
        // since the frame texture view is always sRGB-aware
        let encode_to_srgb = !surface_config.format.is_srgb();

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "render_pipeline".into(),
            layout: None,
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
                count: if multisample { 4 } else { 1 },
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shaders.render,
                entry_point: None,
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("encode_to_srgb", encode_to_srgb.into())],
                    ..wgpu::PipelineCompilationOptions::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let filter_mode = renderer_config.filter_mode.to_wgpu_filter_mode();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: "sampler".into(),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: filter_mode,
            min_filter: if renderer_config.supersample_minification {
                wgpu::FilterMode::Linear
            } else {
                filter_mode
            },
            ..wgpu::SamplerDescriptor::default()
        });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "render_bind_group".into(),
            layout: &render_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&render_input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let mut vertices = match options.pixel_aspect_ratio {
            Some(_) => compute_vertices(window_size.width, window_size.height, display_area),
            None => VERTICES.into(),
        };

        apply_frame_rotation(&mut vertices, renderer_config.frame_rotation);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "vertex_buffer".into(),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        });

        Self {
            frame_size,
            display_area,
            rotation: renderer_config.frame_rotation,
            input_texture,
            shader_pipeline,
            vertex_buffer,
            render_bind_group,
            render_pipeline,
            multisample_output,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: &wgpu::Surface<'_>,
        frame_buffer: &[Color],
        options: RenderFrameOptions,
        #[cfg(feature = "ttf")] surface_config: &wgpu::SurfaceConfiguration,
        #[cfg(feature = "ttf")] modal_renderer: &mut ttf::ModalRenderer,
        frame_time_tracker: &mut FrameTimeTracker,
    ) -> Result<RenderResult, RendererError> {
        let output = surface.get_current_texture()?;

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.input_texture,
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
            self.input_texture.size(),
        );

        for shader in &mut self.shader_pipeline {
            shader.prepare(device, options);
        }

        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: "encoder".into() });

        for shader in &mut self.shader_pipeline {
            shader.draw(&mut encoder);
        }

        #[cfg(feature = "ttf")]
        let ttf_multisampled = match self.multisample_output {
            Some(_) => ttf::Multisampled::Yes,
            None => ttf::Multisampled::No,
        };

        #[cfg(feature = "ttf")]
        let modal_vertex_buffer = modal_renderer.prepare_modals(
            device,
            queue,
            ttf_multisampled,
            surface_config.width,
            surface_config.height,
        )?;

        let surface_output_view =
            output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (output_view, resolve_target) = match &self.multisample_output {
            Some(multisample_output) => (
                multisample_output.create_view(&wgpu::TextureViewDescriptor::default()),
                Some(surface_output_view),
            ),
            None => (surface_output_view, None),
        };

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "surface_render_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: resolve_target.as_ref(),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });

            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

            render_pass.draw(0..VERTICES.len() as u32, 0..1);

            #[cfg(feature = "ttf")]
            if let Some(modal_vertex_buffer) = &modal_vertex_buffer {
                modal_renderer.render(ttf_multisampled, modal_vertex_buffer, &mut render_pass)?;
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
    pixel_aspect_ratio: Option<FiniteF64>,
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

fn apply_frame_rotation(vertices: &mut Vec<Vertex>, rotation: FrameRotation) {
    // Rotate frame by rotating the texture coordinates of each vertex
    for vertex in vertices {
        vertex.texture_coords = rotation.rotate_texture_coords(vertex.texture_coords);
    }
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error(
        "Frame buffer of len {buffer_len} is too small for specified frame size of {frame_width}x{frame_height}"
    )]
    FrameBufferTooSmall { frame_width: u32, frame_height: u32, buffer_len: usize },
    #[error("Invalid target fps value, must be finite and positive: {0}")]
    InvalidTargetFps(f64),
    #[error("Error creating surface from window: {0}")]
    WindowHandleError(#[from] HandleError),
    #[error("Error creating wgpu surface: {0}")]
    WgpuCreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("Error requesting wgpu device: {0}")]
    WgpuRequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("Error getting handle to wgpu output surface: {0}")]
    WgpuSurface(#[from] wgpu::SurfaceError),
    #[error("Failed to obtain wgpu adapter")]
    WgpuRequestAdapter(#[from] wgpu::RequestAdapterError),
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

struct Shaders {
    render: wgpu::ShaderModule,
    prescale: wgpu::ShaderModule,
    identity: wgpu::ShaderModule,
    hblur: wgpu::ShaderModule,
    frame_blend: wgpu::ShaderModule,
    gb_color: wgpu::ShaderModule,
    ntsc: wgpu::ShaderModule,
    xbrz: wgpu::ShaderModule,
    mmpx: wgpu::ShaderModule,
}

impl Shaders {
    fn create(device: &wgpu::Device) -> Self {
        let render = device.create_shader_module(wgpu::include_wgsl!("wgsl/render.wgsl"));
        let prescale = device.create_shader_module(wgpu::include_wgsl!("wgsl/prescale.wgsl"));
        let identity = device.create_shader_module(wgpu::include_wgsl!("wgsl/identity.wgsl"));
        let hblur = device.create_shader_module(wgpu::include_wgsl!("wgsl/hblur.wgsl"));
        let frame_blend = device.create_shader_module(wgpu::include_wgsl!("wgsl/frameblend.wgsl"));
        let gb_color = device.create_shader_module(wgpu::include_wgsl!("wgsl/gb_color.wgsl"));
        let ntsc = device.create_shader_module(wgpu::include_wgsl!("wgsl/ntsc.wgsl"));
        let xbrz = device.create_shader_module(wgpu::include_wgsl!("wgsl/xbrz.wgsl"));
        let mmpx = device.create_shader_module(wgpu::include_wgsl!("wgsl/mmpx.wgsl"));

        Self { render, prescale, identity, hblur, frame_blend, gb_color, ntsc, xbrz, mmpx }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    frame_size: FrameSize,
    options: RenderFrameOptionsHashable,
}

impl PipelineKey {
    fn new(frame_size: FrameSize, options: RenderFrameOptions) -> Self {
        Self { frame_size, options: options.to_hashable() }
    }
}

struct RenderingPipelines {
    pipelines: HashMap<PipelineKey, RenderingPipeline>,
    last_display_info: Option<DisplayInfo>,
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
        options: RenderFrameOptions,
        create_fn: impl FnOnce() -> RenderingPipeline,
    ) -> &mut RenderingPipeline {
        let pipeline =
            self.pipelines.entry(PipelineKey::new(frame_size, options)).or_insert_with(create_fn);

        self.last_display_info = Some(DisplayInfo {
            frame_size,
            display_area: pipeline.display_area,
            rotation: pipeline.rotation.into(),
        });

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

    fn set_target_fps(&mut self, fps: f64) {
        self.frame_interval_nanos = (1_000_000_000.0_f64 / fps).round() as u128;
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
        let backends = config.wgpu_backend.to_wgpu();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions {
                dx12: config::dx12_backend_options(),
                gl: wgpu::GlBackendOptions::default(),
                noop: wgpu::NoopBackendOptions::default(),
            },
        });

        // SAFETY: The surface must not outlive the window it was created from
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)
        }?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: config.wgpu_power_preference.to_wgpu(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let adapter_info = adapter.get_info();
        log::info!(
            "Obtained wgpu adapter with backend {:?}: {}",
            adapter_info.backend,
            adapter_info.name
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: "device".into(),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_capabilities = surface.get_capabilities(&adapter);

        let present_mode = config.vsync_mode.to_wgpu_present_mode();
        if !surface_capabilities.present_modes.contains(&present_mode) {
            return Err(RendererError::UnsupportedPresentMode {
                desired: present_mode,
                available: surface_capabilities.present_modes.clone(),
            });
        }

        // On Windows, using the Vulkan backend with an AMD GPU can seemingly cause incorrect colors
        // when rendering to a surface with an sRGB-aware texture format; prefer non-sRGB-aware for
        // Windows+Vulkan
        //
        // Possibly related: https://github.com/gfx-rs/wgpu/issues/8354
        let prefer_srgb_format = cfg_select! {
            target_os = "windows" => adapter_info.backend != wgpu::Backend::Vulkan,
            _ => true,
        };

        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb() == prefer_srgb_format)
            .unwrap_or_else(|| {
                log::warn!("wgpu adapter does not support any surface formats with is_srgb={prefer_srgb_format}; defaulting to first format in this list: {:?}", surface_capabilities.formats);
                surface_capabilities.formats[0]
            });

        log::info!("Configuring wgpu surface with texture format {surface_format:?}");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width,
            height: window_size.height,
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

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
        let prev_surface_config = self.surface_config.clone();

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

        // Firefox Nightly on Linux crashes if Surface::configure() is called with an unchanged config
        if prev_surface_config != self.surface_config {
            self.surface.configure(&self.device, &self.surface_config);
        }

        // Force render pipeline to be recreated on the next render_frame() call
        self.pipelines.clear();
    }

    pub fn handle_resize(&mut self, size: WindowSize) {
        if self.window_size == size {
            // No change
            return;
        }

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

    pub fn config(&self) -> &RendererConfig {
        &self.renderer_config
    }

    /// Obtain the last rendered frame size and the current display area within the window.
    ///
    /// May return None if rendering config was just changed or initialized and a frame has not yet been rendered with
    /// the new config.
    #[must_use]
    pub fn current_display_info(&self) -> Option<DisplayInfo> {
        self.pipelines.last_display_info
    }

    pub fn reset_interframe_state(&mut self) {
        for pipeline in self.pipelines.pipelines.values_mut() {
            for shader in &mut pipeline.shader_pipeline {
                shader.reset_interframe_state();
            }
        }
    }

    #[cfg(feature = "ttf")]
    pub fn add_modal(&mut self, text: String, duration: std::time::Duration) {
        self.add_or_update_modal(None, text, duration);
    }

    #[cfg(feature = "ttf")]
    pub fn add_or_update_modal(
        &mut self,
        id: Option<Cow<'static, str>>,
        text: String,
        duration: std::time::Duration,
    ) {
        self.modal_renderer.add_or_update_modal(id, text, duration);
    }

    pub fn reload(&mut self) {
        self.reload_config(self.renderer_config);
    }
}

impl<Window> Renderer for WgpuRenderer<Window> {
    type Err = RendererError;

    fn render_frame(
        &mut self,
        frame_buffer: &[Color],
        frame_size: FrameSize,
        target_fps: f64,
        options: RenderFrameOptions,
    ) -> Result<(), Self::Err> {
        if frame_size.len() > frame_buffer.len() as u32 {
            return Err(RendererError::FrameBufferTooSmall {
                frame_width: frame_size.width,
                frame_height: frame_size.height,
                buffer_len: frame_buffer.len(),
            });
        }

        if !target_fps.is_finite() || target_fps <= 0.0 {
            return Err(RendererError::InvalidTargetFps(target_fps));
        }

        self.frame_count += 1;
        if !self.frame_count.is_multiple_of(self.speed_multiplier) {
            return Ok(());
        }

        self.frame_time_tracker.set_target_fps(target_fps);

        let pipeline = self.pipelines.get_or_insert(frame_size, options, || {
            log::info!(
                "Creating render pipeline for frame size {frame_size:?} and pixel aspect ratio {}",
                pixel_aspect_ratio_display(options.pixel_aspect_ratio)
            );

            RenderingPipeline::create(
                &self.device,
                &self.device_limits,
                &self.shaders,
                self.window_size,
                frame_size,
                options,
                &self.surface_config,
                self.renderer_config,
            )
        });

        match pipeline.render(
            &self.device,
            &self.queue,
            &self.surface,
            frame_buffer,
            options,
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

fn pixel_aspect_ratio_display(par: Option<FiniteF64>) -> Cow<'static, str> {
    par.map_or("None".into(), |par| par.to_string().into())
}
