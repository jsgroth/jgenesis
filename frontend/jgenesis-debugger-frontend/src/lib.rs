pub mod gb;
pub mod gba;
pub mod genesis;
mod memviewer;
pub mod nes;
pub mod pce;
mod process;
pub mod smsgg;
pub mod snes;

use egui::epaint::ImageDelta;
use egui::{
    Button, Color32, ColorImage, Id, ImageData, LayerId, Order, Response, ScrollArea,
    TextureFilter, TextureOptions, TextureWrapMode, ThemePreference, Ui, Widget, WidgetText,
};
use egui_extras::{Column, TableBuilder};
use egui_sdl3_wgpu::{FrameCreateError, FrameOptions, FrameRunEffect, FrameRunError};
use jgenesis_common::frontend::Color;
use jgenesis_native_config::EguiTheme;
use jgenesis_renderer::config::RendererConfig;
pub use process::{
    DebugFn, DebugRenderFn, DebuggerMainProcess, DebuggerRunnerProcess, clone_debug_fn,
    null_debug_fn, partial_clone_debug_fn,
};
use sdl3::VideoSubsystem;
use sdl3::event::Event;
use std::array;
use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("Error creating egui/SDL3/wgpu frame: {0}")]
    FrameCreate(#[from] FrameCreateError),
    #[error("Error rendering debugger window: {0}")]
    FrameRun(#[from] FrameRunError),
}

pub struct DebugRenderContext<'a> {
    egui_ui: &'a mut Ui,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    renderer: &'a mut egui_wgpu::Renderer,
}

pub struct DebuggerWindow {
    frame: egui_sdl3_wgpu::Frame,
    debugger_process: Box<dyn DebuggerMainProcess>,
}

impl DebuggerWindow {
    /// # Errors
    ///
    /// Propagates any errors encountered while initializing the window or the wgpu renderer.
    pub fn new(
        video: &VideoSubsystem,
        egui_theme: EguiTheme,
        render_config: &RendererConfig,
        debugger_process: Box<dyn DebuggerMainProcess>,
    ) -> Result<Self, DebuggerError> {
        let options = FrameOptions {
            window_width: 925,
            window_height: 790,
            egui_theme: egui_theme_preference(egui_theme),
            install_egui_image_loaders: true,
            wgpu_backends: render_config.wgpu_backend.to_wgpu(),
            wgpu_power_preference: render_config.wgpu_power_preference.to_wgpu(),
            ..FrameOptions::default()
        };

        let frame = egui_sdl3_wgpu::Frame::new("Memory Viewer", video, options)?;

        Ok(Self { frame, debugger_process })
    }

    /// Update internal state and render the debugger frontend.
    ///
    /// # Errors
    ///
    /// Propagates any errors encountered while rendering.
    pub fn update(&mut self) -> Result<FrameRunEffect, DebuggerError> {
        let effect = self.frame.run(|ui, ctx| {
            let debug_ctx = DebugRenderContext {
                egui_ui: ui,
                device: ctx.device,
                queue: ctx.queue,
                renderer: ctx.renderer,
            };
            if let Err(err) = self.debugger_process.run(debug_ctx) {
                log::error!("Error rendering debugger window: {err}");
            }

            // Ensure window updates at 60 FPS minimum; displayed graphics can change as the
            // emulator runs
            ui.request_repaint_after(Duration::from_micros(16666));
        })?;

        Ok(effect)
    }

    pub fn update_egui_theme(&mut self, egui_theme: EguiTheme) {
        self.frame.update_egui_theme(egui_theme_preference(egui_theme));
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        self.frame.handle_sdl_event(event);
    }

    pub fn window_id(&self) -> u32 {
        self.frame.window_id()
    }
}

fn screen_width(ctx: &egui::Context) -> f32 {
    let window_margin = ctx.global_style().spacing.window_margin;
    ctx.content_rect().width() - f32::from(window_margin.left) - f32::from(window_margin.right)
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
