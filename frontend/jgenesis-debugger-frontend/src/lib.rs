pub mod gb;
pub mod gba;
pub mod genesis;
mod memviewer;
pub mod nes;
#[cfg(feature = "pce")]
pub mod pce;
mod process;
pub mod smsgg;
pub mod snes;

use sdl3::event::{Event, WindowEvent};

use egui::epaint::ImageDelta;
use egui::{
    Button, Color32, ColorImage, Id, ImageData, LayerId, Order, Response, ScrollArea,
    TextureFilter, TextureOptions, TextureWrapMode, ThemePreference, Ui, Widget, WidgetText,
};
use egui_extras::{Column, TableBuilder};
use egui_wgpu::ScreenDescriptor;
use jgenesis_common::frontend::Color;
use jgenesis_native_config::EguiTheme;
use jgenesis_renderer::config::RendererConfig;
use sdl3::VideoSubsystem;
use sdl3::video::{Window, WindowBuildError};
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use std::time::SystemTime;
use std::{array, iter};
use thiserror::Error;

pub use process::{
    DebugFn, DebugRenderFn, DebuggerMainProcess, DebuggerRunnerProcess, clone_debug_fn,
    null_debug_fn, partial_clone_debug_fn,
};

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("Failed to create surface from window handle: {0}")]
    WindowHandleError(#[from] wgpu::rwh::HandleError),
    #[error("Failed to create SDL3 window: {0}")]
    SdlWindowCreateFailed(#[from] WindowBuildError),
    #[error("Failed to obtain wgpu adapter: {0}")]
    RequestAdapterFailed(#[from] wgpu::RequestAdapterError),
    #[error("Failed to create wgpu surface: {0}")]
    CreateSurfaceFailed(#[from] wgpu::CreateSurfaceError),
    #[error("Failed to obtain wgpu surface output texture: {0}")]
    SurfaceCurrentTexture(#[from] wgpu::SurfaceError),
    #[error("Failed to obtain wgpu device: {0}")]
    RequestDeviceFailed(#[from] wgpu::RequestDeviceError),
}

pub struct DebugRenderContext<'a> {
    egui_ctx: &'a egui::Context,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    renderer: &'a mut egui_wgpu::Renderer,
}

pub struct DebuggerWindow {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    platform: egui_sdl3_platform::Platform,
    egui_renderer: egui_wgpu::Renderer,
    start_time: SystemTime,
    debugger_process: Box<dyn DebuggerMainProcess>,
    // SAFETY: The window must be dropped after the surface
    window: Window,
}

impl DebuggerWindow {
    /// # Errors
    ///
    /// Propagates any errors encountered while initializing the window or the wgpu renderer.
    pub fn new(
        video: &VideoSubsystem,
        scale_factor: Option<f32>,
        egui_theme: EguiTheme,
        render_config: &RendererConfig,
        debugger_process: Box<dyn DebuggerMainProcess>,
    ) -> Result<Self, DebuggerError> {
        let scale_factor =
            scale_factor.or_else(|| try_get_primary_display_scale(video)).unwrap_or(1.0);
        let window_width = (900.0 * scale_factor).round() as u32;
        let window_height = (790.0 * scale_factor).round() as u32;

        let window = video
            .window("Memory Viewer", window_width, window_height)
            .resizable()
            .metal_view()
            .build()?;
        let (width, height) = window.size();

        video.text_input().start(&window);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: render_config.wgpu_backend.to_wgpu(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions {
                dx12: jgenesis_renderer::config::dx12_backend_options(),
                gl: wgpu::GlBackendOptions::default(),
                noop: wgpu::NoopBackendOptions::default(),
            },
        });

        // SAFETY: The surface must not outlive the window
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)
        }?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: render_config.wgpu_power_preference.to_wgpu(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: "debugger_device".into(),
                required_features: wgpu::Features::default(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            }))?;

        let surface_capabilities = surface.get_capabilities(&adapter);

        // egui prefers non-sRGB-aware surface formats
        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|&format| !format.is_srgb())
            .unwrap_or(surface_capabilities.formats[0]);
        log::info!("Rendering debugger window with surface format {surface_format:?}");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let window_scale = window.display_scale();
        log::info!("Window scale factor {window_scale}");

        let platform = egui_sdl3_platform::Platform::new(&window, window_scale);
        platform.context().set_theme(egui_theme_preference(egui_theme));
        egui_extras::install_image_loaders(platform.context());

        let start_time = SystemTime::now();

        let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

        Ok(Self {
            surface,
            surface_config,
            device,
            queue,
            platform,
            egui_renderer,
            start_time,
            debugger_process,
            window,
        })
    }

    /// Update internal state and render the debugger frontend.
    ///
    /// # Errors
    ///
    /// Propagates any errors encountered while rendering.
    pub fn update(&mut self) -> Result<(), DebuggerError> {
        let egui_input = self.platform.take_raw_input(
            SystemTime::now().duration_since(self.start_time).unwrap_or_default().as_secs_f64(),
        );

        let full_output = self.platform.context().run(egui_input, |ctx| {
            if let Err(err) = self.debugger_process.run(DebugRenderContext {
                egui_ctx: ctx,
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.egui_renderer,
            }) {
                log::error!("Error updating debugger window: {err}");
            }
        });
        self.platform.handle_egui_output(&full_output.platform_output);

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Outdated) => {
                log::warn!("Skipping debug frame because wgpu surface has changed");
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => {
                log::warn!("Skipping debug frame because wgpu surface timed out");
                return Ok(());
            }
            Err(err) => return Err(err.into()),
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

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: "debugger_encoder".into(),
        });

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

            // egui-wgpu requires a RenderPass with static lifetime
            let mut render_pass = render_pass.forget_lifetime();

            self.egui_renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        Ok(())
    }

    pub fn update_egui_theme(&mut self, egui_theme: EguiTheme) {
        self.platform.context().set_theme(egui_theme_preference(egui_theme));
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        match event {
            Event::Window {
                window_id,
                win_event: WindowEvent::Resized(..) | WindowEvent::PixelSizeChanged(..),
                ..
            } if *window_id == self.window.id() => {
                let (width, height) = self.window.size();
                self.surface_config.width = width;
                self.surface_config.height = height;
                self.surface.configure(&self.device, &self.surface_config);
            }
            _ => {}
        }

        self.platform.handle_event(event);
    }

    pub fn window_id(&self) -> u32 {
        self.window.id()
    }
}

fn try_get_primary_display_scale(video: &VideoSubsystem) -> Option<f32> {
    video.get_primary_display().ok().and_then(|display| display.get_content_scale().ok())
}

fn screen_width(ctx: &egui::Context) -> f32 {
    let window_margin = ctx.style().spacing.window_margin;
    ctx.available_rect().width() - f32::from(window_margin.left) - f32::from(window_margin.right)
}

fn egui_theme_preference(egui_theme: EguiTheme) -> ThemePreference {
    match egui_theme {
        EguiTheme::SystemDefault => ThemePreference::System,
        EguiTheme::Dark => ThemePreference::Dark,
        EguiTheme::Light => ThemePreference::Light,
    }
}

fn create_texture(
    label: &str,
    width: u32,
    height: u32,
    device: &wgpu::Device,
    renderer: &mut egui_wgpu::Renderer,
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
        renderer.register_native_texture(device, &texture_view, wgpu::FilterMode::Nearest);

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

impl<T: Copy + PartialEq> Widget for SelectableButton<'_, T> {
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
    ctx: &mut DebugRenderContext<'_>,
) {
    ctx.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: wgpu_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(wgpu_texture.width() * 4),
            rows_per_image: None,
        },
        wgpu_texture.size(),
    );

    let texture_view = wgpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
    ctx.renderer.update_egui_texture_from_wgpu_texture(
        ctx.device,
        &texture_view,
        wgpu::FilterMode::Nearest,
        egui_texture,
    );
}

fn update_egui_texture(
    ctx: &egui::Context,
    size: [usize; 2],
    image: &[Color],
    texture: &mut Option<egui::TextureId>,
) -> egui::TextureId {
    let tex_manager = ctx.tex_manager();

    match *texture {
        Some(texture) => {
            tex_manager.write().set(
                texture,
                ImageDelta {
                    image: ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(
                        size,
                        bytemuck::cast_slice(image),
                    ))),
                    options: TextureOptions {
                        magnification: TextureFilter::Nearest,
                        minification: TextureFilter::Nearest,
                        wrap_mode: TextureWrapMode::ClampToEdge,
                        mipmap_mode: None,
                    },
                    pos: None,
                },
            );

            texture
        }
        None => {
            let id = tex_manager.write().alloc(
                "cram_texture".into(),
                ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(
                    size,
                    bytemuck::cast_slice(image),
                ))),
                TextureOptions {
                    magnification: TextureFilter::Nearest,
                    minification: TextureFilter::Nearest,
                    wrap_mode: TextureWrapMode::ClampToEdge,
                    mipmap_mode: None,
                },
            );
            *texture = Some(id);

            id
        }
    }
}

fn render_registers_window(
    ctx: &egui::Context,
    window_title: &str,
    open: &mut bool,
    render_registers: impl FnOnce(&mut Ui),
) {
    egui::Window::new(window_title)
        .open(open)
        .constrain(false)
        .default_pos(rand_window_pos())
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                brighten_faint_bg_color(ui);

                render_registers(ui);
            });
        });
}

fn brighten_faint_bg_color(ui: &mut Ui) {
    // Make stripes a little lighter
    let color = ui.visuals_mut().faint_bg_color;
    ui.visuals_mut().faint_bg_color = Color32::from_rgba_premultiplied(
        color.r().saturating_add(5),
        color.g().saturating_add(5),
        color.b().saturating_add(5),
        color.a(),
    );
}

fn render_registers_table(ui: &mut Ui, register: &str, values: &[(&str, &str)]) {
    TableBuilder::new(ui)
        .id_salt(register)
        .column(Column::exact(200.0))
        .column(Column::exact(150.0))
        .vscroll(false)
        .striped(true)
        .header(25.0, |mut header| {
            header.col(|ui| {
                ui.heading(register);
            });
        })
        .body(|mut body| {
            for &(field, value) in values {
                body.row(17.0, |mut row| {
                    row.col(|ui| {
                        ui.label(field);
                    });

                    row.col(|ui| {
                        ui.label(value);
                    });
                });
            }
        });
}

fn dump_registers_callback(ui: &mut Ui) -> impl FnMut(&str, &[(&str, &str)]) {
    let mut first = true;

    move |register, values| {
        if !first {
            ui.separator();
        }
        first = false;

        render_registers_table(ui, register, values);
    }
}

// When an egui Window is created with constrain=false, letting egui decide the default position can
// cause the window to spawn offscreen. Instead, for windows that aren't initially open, spawn them
// in a random position near-ish the top-left corner of the screen
fn rand_window_pos() -> [f32; 2] {
    array::from_fn(|_| 100.0 + rand::random_range(-50.0..=50.0))
}

fn move_to_top(ctx: &egui::Context, id: impl Hash) {
    ctx.move_to_top(LayerId::new(Order::Middle, Id::new(id)));
}

fn window_on_top(ctx: &egui::Context, id: impl Hash) -> bool {
    ctx.top_layer_id() == Some(LayerId::new(Order::Middle, Id::new(id)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollKeys {
    up: bool,
    down: bool,
    page_up: bool,
    page_down: bool,
}

impl ScrollKeys {
    fn relative_scroll_offset(self, visible_height: f32) -> Option<f32> {
        const ASSUMED_ROW_HEIGHT: f32 = 15.0;

        if self.page_up && !self.page_down {
            Some(-visible_height * 0.9)
        } else if !self.page_up && self.page_down {
            Some(visible_height * 0.9)
        } else if self.up && !self.down {
            Some(-ASSUMED_ROW_HEIGHT)
        } else if !self.up && self.down {
            Some(ASSUMED_ROW_HEIGHT)
        } else {
            None
        }
    }
}

fn scroll_keys_pressed(ctx: &egui::Context) -> ScrollKeys {
    let [up, down, page_up, page_down] = ctx.input(|i| {
        [egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::PageUp, egui::Key::PageDown]
            .map(|key| i.key_pressed(key))
    });

    ScrollKeys { up, down, page_up, page_down }
}

fn highlight_color(theme: egui::Theme) -> Color32 {
    match theme {
        egui::Theme::Dark => Color32::GREEN,
        egui::Theme::Light => Color32::BLUE,
    }
}

fn non_selectable_label(text: impl Into<WidgetText>) -> egui::Label {
    egui::Label::new(text).selectable(false)
}

fn normalize_position(pos: egui::Pos2, interact_rect: egui::Rect) -> egui::Vec2 {
    (pos - interact_rect.min) / interact_rect.size()
}

struct AddressSet<T>(HashSet<T>);

impl<T: Copy + Eq + Hash> AddressSet<T> {
    fn new() -> Self {
        Self(HashSet::new())
    }

    fn contains(&self, value: T) -> bool {
        self.0.contains(&value)
    }

    fn handle_click(&mut self, value: T, modifiers: egui::Modifiers) {
        let existed = self.0.contains(&value);

        if !modifiers.ctrl {
            self.0.clear();
        }

        if !existed {
            self.0.insert(value);
        } else {
            self.0.remove(&value);
        }
    }
}
