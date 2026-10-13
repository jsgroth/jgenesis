use crate::renderer::PipelineShader;
use jgenesis_common::frontend::Color;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    flags: Arc<CaptureFlags>,
    buffers: Arc<CaptureBuffers>,
    aborted: Option<CaptureAbortReason>,
}

impl CaptureState {
    pub fn create(device: &wgpu::Device, size: wgpu::Extent3d, usage: wgpu::TextureUsages) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: "capture_texture".into(),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage,
            view_formats: &[],
        });

        // wgpu requires that texture-to-buffer copies use a row stride that is a multiple of 256 bytes
        let padded_width = size.width.div_ceil(64) * 64;

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: "capture_buffer".into(),
            size: (4 * padded_width * size.height).into(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
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

        let buffer = match self.buffers.buffer.get_mapped_range(..) {
            Ok(buffer) => buffer,
            Err(err) => {
                log::error!("Error mapping capture buffer for read: {err}");
                return Err(ReadCapturedFrameError::NoFrameReady);
            }
        };

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

// TODO post-shader capture
pub struct CapturingShader {
    input_texture: Arc<wgpu::Texture>,
    buffers: Arc<CaptureBuffers>,
}

impl CapturingShader {
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

impl PipelineShader for CapturingShader {
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
