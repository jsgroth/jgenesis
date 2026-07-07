use egui::{OrderedViewportIdMap, Ui, ViewportCommand, ViewportId, ViewportOutput};
use egui_wgpu::ScreenDescriptor;
use sdl3::VideoSubsystem;
use sdl3::event::{Event, WindowEvent};
use sdl3::video::{Window, WindowBuildError};
use std::cmp::Ordering;
use std::iter;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use thiserror::Error;

pub struct FrameContext<'frame> {
    pub device: &'frame wgpu::Device,
    pub queue: &'frame wgpu::Queue,
    pub renderer: &'frame mut egui_wgpu::Renderer,
}

#[derive(Debug, Clone)]
pub struct FrameOptions {
    pub window_width: u32,
    pub window_height: u32,
    pub scale_factor: Option<f32>,
    pub resizable: bool,
    pub text_input: bool,
    pub egui_theme: egui::ThemePreference,
    pub install_egui_image_loaders: bool,
    pub wgpu_backends: wgpu::Backends,
    pub wgpu_power_preference: wgpu::PowerPreference,
    pub wgpu_surface_usages: wgpu::TextureUsages,
    pub wgpu_present_mode: wgpu::PresentMode,
}

impl Default for FrameOptions {
    fn default() -> Self {
        Self {
            window_width: 800,
            window_height: 600,
            scale_factor: None,
            resizable: true,
            text_input: true,
            egui_theme: egui::ThemePreference::System,
            install_egui_image_loaders: false,
            wgpu_backends: wgpu::Backends::PRIMARY,
            wgpu_power_preference: wgpu::PowerPreference::None,
            wgpu_surface_usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
            wgpu_present_mode: wgpu::PresentMode::AutoNoVsync,
        }
    }
}

#[derive(Debug, Error)]
pub enum FrameCreateError {
    #[error("Error creating SDL3 window: {0}")]
    SdlWindow(#[from] WindowBuildError),
    #[error("Error obtaining window/display handle: {0}")]
    WindowHandle(#[from] raw_window_handle::HandleError),
    #[error("Error creating wgpu surface: {0}")]
    WgpuCreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("Error obtaining wgpu adapter: {0}")]
    WgpuRequestAdapter(#[from] wgpu::RequestAdapterError),
    #[error("Error obtaining wgpu device and queue: {0}")]
    WgpuRequestDevice(#[from] wgpu::RequestDeviceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRunEffect {
    None,
    Closed,
}

#[derive(Debug, Error)]
pub enum FrameRunError {
    #[error("wgpu surface was lost or failed validation")]
    WgpuSurfaceLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NextRepaint {
    delay: Duration,
    pass_number: u64,
}

impl NextRepaint {
    #[must_use]
    fn min(self, other: Self) -> Self {
        // Prefer higher pass numbers first, then lower delay
        let ordering =
            self.pass_number.cmp(&other.pass_number).reverse().then(self.delay.cmp(&other.delay));

        match ordering {
            Ordering::Greater => other,
            Ordering::Less | Ordering::Equal => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportEffect {
    Close,
}

pub struct Frame {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    platform: crate::Platform,
    egui_renderer: egui_wgpu::Renderer,
    start_time: SystemTime,
    last_repaint: SystemTime,
    next_repaint: Arc<Mutex<NextRepaint>>,
    paint_count: u64,
    closed: bool,
    // SAFETY: The window must be dropped after the surface
    window: Window,
}

impl Frame {
    #[allow(clippy::missing_errors_doc)]
    pub fn new(
        window_title: &str,
        video: &VideoSubsystem,
        options: FrameOptions,
    ) -> Result<Self, FrameCreateError> {
        let scale_factor =
            options.scale_factor.or_else(|| try_get_primary_display_scale(video)).unwrap_or(1.0);
        let window_width = (options.window_width as f32 * scale_factor).round() as u32;
        let window_height = (options.window_height as f32 * scale_factor).round() as u32;

        let mut window_builder = video.window(window_title, window_width, window_height);

        window_builder.metal_view();

        if options.resizable {
            window_builder.resizable();
        }

        let mut window = window_builder.build()?;
        window.raise();

        let (width, height) = window.size();

        if options.text_input {
            video.text_input().start(&window);
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: options.wgpu_backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // SAFETY: The surface must not outlive the window
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_display_and_window(
                &window, &window,
            )?)
        }?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: options.wgpu_power_preference,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let surface_capabilities = surface.get_capabilities(&adapter);

        // egui prefers non-sRGB-aware surface formats
        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|&format| !format.is_srgb())
            .unwrap_or(surface_capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: options.wgpu_surface_usages,
            format: surface_format,
            width,
            height,
            present_mode: options.wgpu_present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let window_scale = window.display_scale();

        log::info!(
            "Rendering window '{window_title}' with surface format {surface_format:?} and scale factor {window_scale}"
        );

        let platform = crate::Platform::new(&window, window_scale);
        platform.context().set_theme(options.egui_theme);

        if options.install_egui_image_loaders {
            egui_extras::install_image_loaders(platform.context());
        }

        let start_time = SystemTime::now();
        let next_repaint =
            Arc::new(Mutex::new(NextRepaint { delay: Duration::ZERO, pass_number: 0 }));

        platform.context().set_request_repaint_callback({
            let next_repaint = Arc::clone(&next_repaint);
            move |info| {
                let mut next_repaint = next_repaint.lock().unwrap();
                *next_repaint = next_repaint.min(NextRepaint {
                    delay: info.delay,
                    pass_number: info.current_cumulative_pass_nr,
                });
            }
        });

        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        Ok(Self {
            surface,
            surface_config,
            device,
            queue,
            platform,
            egui_renderer,
            start_time,
            last_repaint: start_time,
            next_repaint,
            paint_count: 0,
            closed: false,
            window,
        })
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn run(
        &mut self,
        mut render_fn: impl FnMut(&mut Ui, FrameContext<'_>),
    ) -> Result<FrameRunEffect, FrameRunError> {
        // Painting at least 3 times before waiting seems to avoid a black screen at window open on some platforms
        // Maybe some sort of triple buffering in the graphics driver
        const MIN_PAINTS_BEFORE_WAIT: u64 = 3;

        if self.closed {
            return Ok(FrameRunEffect::Closed);
        }

        let now = SystemTime::now();
        let since_last_repaint = now.duration_since(self.last_repaint).unwrap_or_default();

        {
            let mut next_repaint = self.next_repaint.lock().unwrap();
            let needs_repaint = since_last_repaint >= next_repaint.delay;

            if self.paint_count >= MIN_PAINTS_BEFORE_WAIT && !needs_repaint {
                return Ok(FrameRunEffect::None);
            }

            self.paint_count += 1;

            if next_repaint.pass_number < self.platform.context().cumulative_pass_nr() {
                next_repaint.delay = Duration::MAX;
            }
        }

        let egui_input = self
            .platform
            .take_raw_input(now.duration_since(self.start_time).unwrap_or_default().as_secs_f64());

        let full_output = self.platform.context().run_ui(egui_input, |ui| {
            let frame_ctx = FrameContext {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.egui_renderer,
            };
            render_fn(ui, frame_ctx);
        });
        self.platform.handle_egui_output(&full_output.platform_output);

        if let Some(effect) = self.handle_viewport_output(&full_output.viewport_output) {
            return match effect {
                ViewportEffect::Close => {
                    self.closed = true;
                    Ok(FrameRunEffect::Closed)
                }
            };
        }

        let mut suboptimal_surface = false;
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output) => output,
            wgpu::CurrentSurfaceTexture::Suboptimal(output) => {
                suboptimal_surface = true;
                output
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("Skipping frame because wgpu surface is outdated");
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(FrameRunEffect::None);
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::warn!("Skipping frame because wgpu surface timed out");
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(FrameRunEffect::None);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                log::debug!("Skipping frame because wgpu surface is occluded");
                return Ok(FrameRunEffect::None);
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                return Err(FrameRunError::WgpuSurfaceLost);
            }
        };
        let output_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let paint_jobs =
            self.platform.context().tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: "egui_render_pass".into(),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });

            // egui-wgpu requires a RenderPass with static lifetime
            let mut render_pass = render_pass.forget_lifetime();

            self.egui_renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        if suboptimal_surface {
            self.surface.configure(&self.device, &self.surface_config);
        }

        self.last_repaint = now;

        Ok(FrameRunEffect::None)
    }

    pub fn update_egui_theme(&mut self, theme_preference: egui::ThemePreference) {
        self.platform.context().set_theme(theme_preference);
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        if self.closed {
            return;
        }

        match event {
            Event::Quit { .. } => {
                self.closed = true;
                return;
            }
            Event::Window { window_id, win_event, .. } if *window_id == self.window.id() => {
                match win_event {
                    WindowEvent::CloseRequested => {
                        self.closed = true;
                        return;
                    }
                    WindowEvent::PixelSizeChanged(..) | WindowEvent::Resized(..) => {
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

        if self.platform.has_pending_input_event() {
            *self.next_repaint.lock().unwrap() = NextRepaint {
                delay: Duration::ZERO,
                pass_number: self.platform.context().cumulative_pass_nr(),
            };
        }
    }

    fn handle_viewport_output(
        &mut self,
        viewport_output: &OrderedViewportIdMap<ViewportOutput>,
    ) -> Option<ViewportEffect> {
        let Some(output) = viewport_output.get(&ViewportId::ROOT) else {
            return Some(ViewportEffect::Close);
        };

        for command in &output.commands {
            match command {
                ViewportCommand::Close => {
                    return Some(ViewportEffect::Close);
                }
                ViewportCommand::Focus => {
                    self.window.raise();
                }
                ViewportCommand::RequestCut => {
                    todo!()
                }
                ViewportCommand::RequestCopy => {
                    todo!()
                }
                ViewportCommand::RequestPaste => {
                    todo!()
                }
                _ => {}
            }
        }

        None
    }

    pub fn window_id(&self) -> u32 {
        self.window.id()
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        self.platform.context()
    }
}

fn try_get_primary_display_scale(video: &VideoSubsystem) -> Option<f32> {
    video.get_primary_display().ok().and_then(|display| display.get_content_scale().ok())
}
