mod eguisdl;
pub mod genesis;
pub mod smsgg;
pub mod snes;

use egui_wgpu_backend::ScreenDescriptor;

use sdl2::event::{Event, WindowEvent};

use egui::{Button, Response, Ui, Widget, WidgetText};
use sdl2::video::{Window, WindowBuildError};
use sdl2::VideoSubsystem;
use std::iter;
use std::time::SystemTime;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("Failed to create SDL2 window: {0}")]
    SdlWindowCreateFailed(#[from] WindowBuildError),
    #[error("Failed to create wgpu surface: {0}")]
    CreateSurfaceFailed(#[from] wgpu::CreateSurfaceError),
    #[error("Failed to obtain wgpu surface output texture: {0}")]
    SurfaceCurrentTexture(#[from] wgpu::SurfaceError),
    #[error("Failed to obtain wgpu adapter")]
    RequestAdapterFailed,
    #[error("Failed to obtain wgpu device: {0}")]
    RequestDeviceFailed(#[from] wgpu::RequestDeviceError),
    #[error("Error in egui wgpu backend: {0}")]
    WgpuBackendError(#[from] egui_wgpu_backend::BackendError),
}

pub type DebugRenderFn<Emulator> = dyn FnMut(
    &egui::Context,
    &Emulator,
    &wgpu::Device,
    &wgpu::Queue,
    &mut egui_wgpu_backend::RenderPass,
) -> Result<(), DebuggerError>;

pub struct DebuggerWindow<Emulator> {
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    platform: eguisdl::Platform,
    egui_pass: egui_wgpu_backend::RenderPass,
    start_time: SystemTime,
    render_fn: Box<DebugRenderFn<Emulator>>,
    // SAFETY: The window must be dropped after the surface
    window: Window,
    scale_factor: f32,
}

impl<Emulator> DebuggerWindow<Emulator> {
    pub fn new(
        video: &VideoSubsystem,
        render_fn: Box<DebugRenderFn<Emulator>>,
    ) -> Result<Self, DebuggerError> {
        let window = video.window("Memory Viewer", 800, 600).resizable().build()?;
        let (width, height) = window.size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());

        // SAFETY: The surface must not outlive the window
        let surface = unsafe { instance.create_surface(&window) }?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .ok_or(DebuggerError::RequestAdapterFailed)?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: "debugger_device".into(),
                features: wgpu::Features::default(),
                limits: wgpu::Limits::default(),
            },
            None,
        ))?;

        let surface_format = surface.get_capabilities(&adapter).formats[0];
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let scale_factor = determine_scale_factor(&window, video);
        log::info!("Guessed scale factor {scale_factor}");

        let platform = eguisdl::Platform::new(&window, scale_factor);
        let start_time = SystemTime::now();

        let egui_pass = egui_wgpu_backend::RenderPass::new(&device, surface_format, 1);

        Ok(Self {
            surface,
            surface_config,
            device,
            queue,
            platform,
            egui_pass,
            start_time,
            render_fn,
            window,
            scale_factor,
        })
    }

    pub fn update(&mut self, emulator: &Emulator) -> Result<(), DebuggerError> {
        self.platform.update_time(
            SystemTime::now().duration_since(self.start_time).unwrap_or_default().as_secs_f64(),
        );

        self.platform.begin_frame();
        let ctx = self.platform.context();

        (self.render_fn)(ctx, emulator, &self.device, &self.queue, &mut self.egui_pass)?;

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Outdated) => {
                log::warn!("Skipping debug frame because wgpu surface has changed");
                self.platform.end_frame();
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };
        let output_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let full_output = self.platform.end_frame();
        let paint_jobs = ctx.tessellate(full_output.shapes);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: "debugger_encoder".into(),
        });

        let screen_descriptor = ScreenDescriptor {
            physical_width: self.surface_config.width,
            physical_height: self.surface_config.height,
            scale_factor: self.scale_factor,
        };

        let tdelta = full_output.textures_delta;
        self.egui_pass.add_textures(&self.device, &self.queue, &tdelta)?;
        self.egui_pass.update_buffers(&self.device, &self.queue, &paint_jobs, &screen_descriptor);
        self.egui_pass.execute(
            &mut encoder,
            &output_view,
            &paint_jobs,
            &screen_descriptor,
            Some(wgpu::Color::BLACK),
        )?;

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        self.egui_pass.remove_textures(tdelta)?;

        Ok(())
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        match event {
            Event::Window { window_id, win_event, .. } if *window_id == self.window.id() => {
                match win_event {
                    WindowEvent::Resized(..) | WindowEvent::SizeChanged(..) => {
                        let (width, height) = self.window.size();
                        self.surface_config.width = width;
                        self.surface_config.height = height;
                        self.surface.configure(&self.device, &self.surface_config);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        self.platform.handle_event(event);
    }

    pub fn window_id(&self) -> u32 {
        self.window.id()
    }
}

fn determine_scale_factor(window: &Window, video: &VideoSubsystem) -> f32 {
    let scale_factor = window
        .display_index()
        .ok()
        .and_then(|idx| video.display_dpi(idx).ok())
        .and_then(|(_, hdpi, vdpi)| {
            // Set scale factor to DPI/96 if HDPI and VDPI are equal and non-zero
            let delta = (hdpi - vdpi).abs();
            (delta < 1e-3 && hdpi > 0.0).then(|| {
                let doubled_scale_factor = (hdpi / 96.0 * 2.0).round() as u32;
                doubled_scale_factor as f32 / 2.0
            })
        })
        .unwrap_or(1.0);

    // Arbitrary threshold; egui will panic if pixels_per_point is too high
    if (0.5..=10.0).contains(&scale_factor) { scale_factor } else { 1.0 }
}

fn screen_width(ctx: &egui::Context) -> f32 {
    let window_margin = ctx.style().spacing.window_margin;
    ctx.available_rect().width() - window_margin.left - window_margin.right
}

fn create_texture(
    label: &str,
    width: u32,
    height: u32,
    device: &wgpu::Device,
    rpass: &mut egui_wgpu_backend::RenderPass,
) -> (wgpu::Texture, egui::TextureId) {
    let wgpu_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texture_view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let egui_texture =
        rpass.egui_texture_from_wgpu_texture(device, &texture_view, wgpu::FilterMode::Nearest);

    (wgpu_texture, egui_texture)
}

struct SelectableButton<'a, T> {
    label: WidgetText,
    current_value: &'a mut T,
    alternative: T,
}

impl<'a, T> SelectableButton<'a, T> {
    fn new(label: impl Into<WidgetText>, current_value: &'a mut T, alternative: T) -> Self {
        Self { label: label.into(), current_value, alternative }
    }
}

impl<'a, T: Copy + PartialEq> Widget for SelectableButton<'a, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let response =
            Button::new(self.label).selected(*self.current_value == self.alternative).ui(ui);
        if response.clicked() {
            *self.current_value = self.alternative;
        }
        response
    }
}

fn write_textures(
    wgpu_texture: &wgpu::Texture,
    egui_texture: egui::TextureId,
    data: &[u8],
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rpass: &mut egui_wgpu_backend::RenderPass,
) -> Result<(), DebuggerError> {
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: wgpu_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(wgpu_texture.width() * 4),
            rows_per_image: None,
        },
        wgpu_texture.size(),
    );

    let texture_view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
    rpass.update_egui_texture_from_wgpu_texture(
        device,
        &texture_view,
        wgpu::FilterMode::Nearest,
        egui_texture,
    )?;

    Ok(())
}
