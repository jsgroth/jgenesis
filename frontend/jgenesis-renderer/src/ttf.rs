use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer,
};
use jgenesis_common::timeutils;
use std::mem;
use std::time::Duration;
use wgpu::util::DeviceExt;

const FONT_SIZE: f32 = 30.0;
const LINE_HEIGHT: f32 = 60.0;
const BORDER_OFFSET: f32 = 20.0;
const BOX_OFFSET: f32 = 7.5;

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
struct Vertex {
    position: [f32; 2],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<Self>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &Self::ATTRIBUTES,
    };
}

pub struct Modal {
    text: String,
    expiry_nanos: u128,
}

pub struct ModalRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: glyphon::Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    buffers: Vec<Buffer>,
    modals: Vec<Modal>,
    bg_pipeline: wgpu::RenderPipeline,
}

impl ModalRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let glyphon_cache = glyphon::Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &glyphon_cache, surface_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        let shader = device.create_shader_module(wgpu::include_wgsl!("modal.wgsl"));
        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: "modal_bg_pipeline".into(),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::LAYOUT],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
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
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let viewport = glyphon::Viewport::new(device, &glyphon_cache);

        Self {
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            buffers: Vec::with_capacity(10),
            modals: Vec::with_capacity(10),
            bg_pipeline,
        }
    }

    pub fn add_modal(&mut self, text: String, duration: Duration) {
        let expiry_nanos = timeutils::current_time_nanos() + duration.as_nanos();
        self.modals.push(Modal { text, expiry_nanos });
    }

    pub fn prepare_modals(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Option<wgpu::Buffer>, glyphon::PrepareError> {
        let now_nanos = timeutils::current_time_nanos();
        self.modals.retain(|modal| modal.expiry_nanos > now_nanos);

        if self.modals.is_empty() {
            return Ok(None);
        }

        while self.buffers.len() < self.modals.len() {
            self.buffers
                .push(Buffer::new(&mut self.font_system, Metrics::new(FONT_SIZE, LINE_HEIGHT)));
        }

        let mut vertices = Vec::with_capacity(self.modals.len());
        let mut text_areas = Vec::with_capacity(self.modals.len());
        let mut line_top = BORDER_OFFSET;
        for (modal, buffer) in self.modals.iter().zip(self.buffers.iter_mut()) {
            buffer.set_size(&mut self.font_system, Some(width as f32), Some(height as f32));
            buffer.set_text(
                &mut self.font_system,
                &modal.text,
                Attrs::new().family(Family::Monospace),
                Shaping::Basic,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);

            text_areas.push(TextArea {
                buffer,
                left: BORDER_OFFSET,
                top: line_top,
                scale: 1.0,
                bounds: TextBounds { left: 0, top: 0, right: width as i32, bottom: height as i32 },
                default_color: glyphon::Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });

            let box_vertices =
                determine_box_positions(buffer, line_top, width as f32, height as f32);
            vertices.extend([
                box_vertices[0],
                box_vertices[1],
                box_vertices[2],
                box_vertices[1],
                box_vertices[2],
                box_vertices[3],
            ]);

            line_top += LINE_HEIGHT + BORDER_OFFSET;
        }

        self.viewport.update(queue, Resolution { width, height });

        self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        )?;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: "modal_bg_vertex_buffer".into(),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Ok(Some(vertex_buffer))
    }

    pub fn render<'rpass>(
        &'rpass self,
        vertex_buffer: &'rpass wgpu::Buffer,
        render_pass: &mut wgpu::RenderPass<'rpass>,
    ) -> Result<(), glyphon::RenderError> {
        if self.modals.is_empty() {
            return Ok(());
        }

        render_pass.set_pipeline(&self.bg_pipeline);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

        let vertex_count = 6 * self.modals.len() as u32;
        render_pass.draw(0..vertex_count, 0..1);

        self.text_renderer.render(&self.atlas, &self.viewport, render_pass)
    }
}

fn determine_box_positions(buffer: &Buffer, line_top: f32, width: f32, height: f32) -> [Vertex; 4] {
    let text_line = &buffer.lines[0].layout_opt().as_ref().unwrap()[0];
    let text_width = text_line.w;
    let max_ascent = text_line.max_ascent;
    let max_descent = text_line.max_descent;

    let line_left = BORDER_OFFSET;

    let center_offset = (LINE_HEIGHT - max_ascent - max_descent) / 2.0;
    let line_v_center = line_top + max_ascent + center_offset;

    let unnormalized = [
        Vertex { position: [line_left - BOX_OFFSET, line_v_center - max_ascent - BOX_OFFSET] },
        Vertex { position: [line_left - BOX_OFFSET, line_v_center + max_descent + BOX_OFFSET] },
        Vertex {
            position: [
                line_left + text_width + BOX_OFFSET,
                line_v_center - max_ascent - BOX_OFFSET,
            ],
        },
        Vertex {
            position: [
                line_left + text_width + BOX_OFFSET,
                line_v_center + max_descent + BOX_OFFSET,
            ],
        },
    ];

    let half_width = 0.5 * width;
    let half_height = 0.5 * height;
    unnormalized.map(|v| Vertex {
        position: [
            (v.position[0] - half_width) / half_width,
            -(v.position[1] - half_height) / half_height,
        ],
    })
}
