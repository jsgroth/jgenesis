use crate::config::RendererConfig;
use crate::renderer::{PipelineShader, Shaders, WindowSize};
use jgenesis_common::frontend::{Color, FiniteF64, FrameSize, RenderFrameOptions};
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RendererCaptureOutput {
    #[default]
    ConsoleNative,
    Emulator,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureFrameInfo {
    pub size: (u32, u32),
    pub pixel_aspect_ratio: Option<FiniteF64>,
    pub fps: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum CaptureAbortReason {
    FrameSizeChanged { from: (u32, u32), to: (u32, u32) },
    FrameRateChanged { from: f64, to: f64 },
    UnexpectedTextureFormat(wgpu::TextureFormat),
}

impl Display for CaptureAbortReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::FrameSizeChanged {
                from: (from_width, from_height),
                to: (to_width, to_height),
            } => write!(
                f,
                "frame size changed from {from_width}x{from_height} to {to_width}x{to_height}"
            ),
            Self::FrameRateChanged { from, to } => {
                write!(f, "frame rate changed from {from} FPS to {to} FPS")
            }
            Self::UnexpectedTextureFormat(format) => {
                write!(f, "unexpected input texture format {format:?} (this is probably a bug)")
            }
        }
    }
}

#[derive(Debug)]
pub enum ReadCapturedFrameError {
    NoFrameReady,
    NotCapturing,
    CaptureAborted(CaptureAbortReason),
}

pub trait ReadCapturedFrameCallback {
    fn copy_buffer(&mut self, buffer: &[Color], row_stride: usize);
}

impl<F> ReadCapturedFrameCallback for F
where
    F: FnMut(&[Color], usize),
{
    fn copy_buffer(&mut self, buffer: &[Color], row_stride: usize) {
        self(buffer, row_stride);
    }
}

struct CaptureFlags {
    capturing: AtomicBool,
    captured: AtomicBool,
}

impl CaptureFlags {
    fn new() -> Self {
        Self { capturing: AtomicBool::new(false), captured: AtomicBool::new(false) }
    }

    fn clear(&self) {
        self.capturing.store(false, Ordering::Release);
        self.captured.store(false, Ordering::Release);
    }
}

pub struct CaptureBuffers {
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    padded_width: u32,
}

impl CaptureBuffers {
    fn copy_texture_to_buffer(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * self.padded_width),
                    rows_per_image: Some(self.texture.height()),
                },
            },
            self.texture.size(),
        );
    }
}

pub struct CaptureState {
    output: RendererCaptureOutput,
    flags: Arc<CaptureFlags>,
    buffers: Arc<CaptureBuffers>,
    aborted: Option<CaptureAbortReason>,
}

impl CaptureState {
    pub fn create(
        device: &wgpu::Device,
        capture_output: RendererCaptureOutput,
        frame_size: FrameSize,
        window_size: WindowSize,
    ) -> Self {
        let (texture_width, texture_height) = match capture_output {
            RendererCaptureOutput::ConsoleNative => (frame_size.width, frame_size.height),
            RendererCaptureOutput::Emulator => (window_size.width, window_size.height),
        };

        let texture_usage = match capture_output {
            RendererCaptureOutput::ConsoleNative => {
                wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC
            }
            RendererCaptureOutput::Emulator => {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            }
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "capture_texture".into(),
            size: wgpu::Extent3d {
                width: texture_width,
                height: texture_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: texture_usage,
            view_formats: &[],
        });

        // wgpu requires that texture-to-buffer copies use a row stride that is a multiple of 256 bytes
        let padded_width = texture_width.div_ceil(64) * 64;

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: "capture_buffer".into(),
            size: (4 * padded_width * texture_height).into(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            output: capture_output,
            flags: Arc::new(CaptureFlags::new()),
            buffers: Arc::new(CaptureBuffers { texture, buffer, padded_width }),
            aborted: None,
        }
    }

    pub fn read_captured_frame(
        &self,
        device: &wgpu::Device,
        callback: &mut impl ReadCapturedFrameCallback,
    ) -> Result<(), ReadCapturedFrameError> {
        self.poll_device_if_capturing(device);
        if !self.flags.captured.load(Ordering::Acquire) {
            return Err(ReadCapturedFrameError::NoFrameReady);
        }

        let buffer = self.buffers.buffer.get_mapped_range(..);

        callback
            .copy_buffer(bytemuck::cast_slice(buffer.as_ref()), self.buffers.padded_width as usize);

        drop(buffer);
        self.flags.clear();
        self.buffers.buffer.unmap();

        Ok(())
    }

    pub fn poll_device_if_capturing(&self, device: &wgpu::Device) {
        if self.flags.capturing.load(Ordering::Acquire)
            && !self.flags.captured.load(Ordering::Acquire)
        {
            let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        }
    }

    pub fn map_buffer_async(&self) {
        let flags = Arc::clone(&self.flags);
        flags.capturing.store(true, Ordering::Release);
        flags.captured.store(false, Ordering::Release);

        self.buffers.buffer.slice(..).map_async(wgpu::MapMode::Read, move |map_result| {
            match map_result {
                Ok(()) => {
                    flags.captured.store(true, Ordering::Release);
                }
                Err(err) => {
                    log::debug!("Error mapping video capture buffer for CPU read: {err}");
                    flags.clear();
                }
            }
        });
    }

    pub fn reset_capturing_state(&self, device: &wgpu::Device) {
        self.poll_device_if_capturing(device);

        if self.flags.captured.load(Ordering::Acquire) {
            // Captured frame was not consumed
            self.buffers.buffer.unmap();
        }

        self.flags.clear();
    }

    pub fn output(&self) -> RendererCaptureOutput {
        self.output
    }

    pub fn buffers(&self) -> &Arc<CaptureBuffers> {
        &self.buffers
    }

    pub fn abort(&mut self, reason: CaptureAbortReason) {
        self.aborted = Some(reason);
    }

    pub fn aborted(&self) -> Option<CaptureAbortReason> {
        self.aborted
    }
}

pub struct ConsoleCapturingShader {
    input_texture: Arc<wgpu::Texture>,
    buffers: Arc<CaptureBuffers>,
}

impl ConsoleCapturingShader {
    pub fn new(
        input_texture: &Arc<wgpu::Texture>,
        buffers: &Arc<CaptureBuffers>,
    ) -> Result<Self, CaptureAbortReason> {
        if input_texture.width() != buffers.texture.width()
            || input_texture.height() != buffers.texture.height()
        {
            return Err(CaptureAbortReason::FrameSizeChanged {
                from: (buffers.texture.width(), buffers.texture.height()),
                to: (input_texture.width(), input_texture.height()),
            });
        }

        if !matches!(
            input_texture.format(),
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        ) {
            // This is really a bug if it ever happens, but don't crash the application if it does
            return Err(CaptureAbortReason::UnexpectedTextureFormat(input_texture.format()));
        }

        Ok(Self { input_texture: Arc::clone(input_texture), buffers: Arc::clone(buffers) })
    }
}

impl PipelineShader for ConsoleCapturingShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_texture(
            self.input_texture.as_image_copy(),
            self.buffers.texture.as_image_copy(),
            self.input_texture.size(),
        );

        self.buffers.copy_texture_to_buffer(encoder);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.input_texture
    }
}

pub struct EmulatorCapturingShader {
    input_texture: Arc<wgpu::Texture>,
    buffers: Arc<CaptureBuffers>,
    multisample_texture: Option<wgpu::Texture>,
    bind_group: Arc<wgpu::BindGroup>,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
}

pub struct EmulatorCapturingShaderArgs<'a> {
    pub input_texture: Arc<wgpu::Texture>,
    pub buffers: Arc<CaptureBuffers>,
    pub render_bind_group: Arc<wgpu::BindGroup>,
    pub render_pipeline_layout: &'a wgpu::PipelineLayout,
    pub frame_size: FrameSize,
    pub config: &'a RendererConfig,
    pub options: RenderFrameOptions,
    pub multisample: bool,
}

impl EmulatorCapturingShader {
    pub fn new(
        device: &wgpu::Device,
        shaders: &Shaders,
        EmulatorCapturingShaderArgs {
            input_texture,
            buffers,
            render_bind_group,
            render_pipeline_layout,
            frame_size,
            config,
            options,
            multisample,
        }: EmulatorCapturingShaderArgs<'_>,
    ) -> Self {
        let multisample_texture = multisample.then(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: "capture_multisample_texture".into(),
                size: buffers.texture.size(),
                mip_level_count: 1,
                sample_count: 4,
                dimension: wgpu::TextureDimension::D2,
                format: buffers.texture.format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });

        let pipeline = super::create_render_pipeline(
            device,
            shaders,
            render_pipeline_layout,
            buffers.texture.format(),
            multisample,
        );

        let window_size = WindowSize {
            width: buffers.texture.width(),
            height: buffers.texture.height(),
            pixel_density: 1.0,
        };
        let display_area = super::determine_display_area(
            window_size,
            frame_size,
            options.pixel_aspect_ratio,
            config.force_integer_height_scaling,
        );

        let vertex_buffer =
            super::create_render_vertex_buffer(device, config, options, window_size, display_area);

        Self {
            input_texture,
            buffers,
            multisample_texture,
            bind_group: render_bind_group,
            pipeline,
            vertex_buffer,
        }
    }
}

impl PipelineShader for EmulatorCapturingShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let capture_tex_view =
            self.buffers.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (attachment_view, resolve_target) = match &self.multisample_texture {
            Some(multisample_texture) => {
                let multisample_view =
                    multisample_texture.create_view(&wgpu::TextureViewDescriptor::default());
                (multisample_view, Some(capture_tex_view))
            }
            None => (capture_tex_view, None),
        };

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "capture_render_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &attachment_view,
                    depth_slice: None,
                    resolve_target: resolve_target.as_ref(),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });

            render_pass.set_bind_group(0, &*self.bind_group, &[]);
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

            render_pass.draw(0..super::VERTICES.len() as u32, 0..1);
        }

        self.buffers.copy_texture_to_buffer(encoder);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.input_texture
    }
}
