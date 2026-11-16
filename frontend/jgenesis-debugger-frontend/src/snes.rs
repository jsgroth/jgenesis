use crate::{DebugRenderContext, DebugRenderFn};
use egui::{CentralPanel, Grid, MenuBar, Panel, UiKind, Window};
use jgenesis_common::frontend::Color;
use snes_core::api::SnesEmulator;

const VRAM_WINDOW_TITLE: &str = "VRAM";
const CGRAM_WINDOW_TITLE: &str = "CGRAM";

const VRAM_2BPP_TEXTURE_SIZE: [usize; 2] = [64 * 8, 64 * 8];
const VRAM_4BPP_TEXTURE_SIZE: [usize; 2] = [64 * 8, 32 * 8];
const VRAM_8BPP_TEXTURE_SIZE: [usize; 2] = [32 * 8, 32 * 8];
const VRAM_MODE_7_TEXTURE_SIZE: [usize; 2] = [16 * 8, 16 * 8];
const CGRAM_TEXTURE_SIZE: [usize; 2] = [16, 16];

const VRAM_2BPP_BUFFER_LEN: usize = VRAM_2BPP_TEXTURE_SIZE[0] * VRAM_2BPP_TEXTURE_SIZE[1];
const VRAM_4BPP_BUFFER_LEN: usize = VRAM_4BPP_TEXTURE_SIZE[0] * VRAM_4BPP_TEXTURE_SIZE[1];
const VRAM_8BPP_BUFFER_LEN: usize = VRAM_8BPP_TEXTURE_SIZE[0] * VRAM_8BPP_TEXTURE_SIZE[1];
const VRAM_MODE_7_BUFFER_LEN: usize = VRAM_MODE_7_TEXTURE_SIZE[0] * VRAM_MODE_7_TEXTURE_SIZE[1];
const MAX_VRAM_BUFFER_LEN: usize = VRAM_2BPP_BUFFER_LEN;
const CGRAM_BUFFER_LEN: usize = CGRAM_TEXTURE_SIZE[0] * CGRAM_TEXTURE_SIZE[1];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum VramMode {
    TwoBpp,
    #[default]
    FourBpp,
    EightBpp,
    Mode7,
}

struct State {
    vram_open: bool,
    cgram_open: bool,
    vram_mode: VramMode,
    vram_palette: u8,
    cgram_texture: Option<egui::TextureId>,
    cgram_buffer: Box<[Color; CGRAM_BUFFER_LEN]>,
    vram_2bpp_texture: Option<egui::TextureId>,
    vram_4bpp_texture: Option<egui::TextureId>,
    vram_8bpp_texture: Option<egui::TextureId>,
    vram_mode7_texture: Option<egui::TextureId>,
    vram_buffer: Box<[Color; MAX_VRAM_BUFFER_LEN]>,
}

impl State {
    fn new() -> Self {
        Self {
            vram_open: true,
            cgram_open: true,
            vram_mode: VramMode::default(),
            vram_palette: 0,
            cgram_texture: None,
            cgram_buffer: vec![Color::default(); CGRAM_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            vram_2bpp_texture: None,
            vram_4bpp_texture: None,
            vram_8bpp_texture: None,
            vram_mode7_texture: None,
            vram_buffer: vec![Color::default(); MAX_VRAM_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<SnesEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx, emulator| render(ctx, emulator, &mut state))
}

fn render(ctx: DebugRenderContext<'_>, emulator: &mut SnesEmulator, state: &mut State) {
    Panel::top("debug_top_panel").show_inside(ctx.egui_ui, |ui| {
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Video Memory", |ui| {
                if ui.button("VRAM").clicked() {
                    state.vram_open = true;
                    crate::move_to_top(ui.ctx(), VRAM_WINDOW_TITLE);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("CRAM").clicked() {
                    state.cgram_open = true;
                    crate::move_to_top(ui.ctx(), CGRAM_WINDOW_TITLE);
                    ui.close_kind(UiKind::Menu);
                }
            });
        });
    });

    render_cgram_window(ctx.egui_ui.ctx(), emulator, state);
    render_vram_window(ctx.egui_ui.ctx(), emulator, state);
}

fn render_cgram_window(ctx: &egui::Context, emulator: &mut SnesEmulator, state: &mut State) {
    Window::new(CGRAM_WINDOW_TITLE)
        .open(&mut state.cgram_open)
        .default_pos([550.0, 25.0])
        .default_size([350.0, 360.0])
        .show(ctx, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_cgram(state.cgram_buffer.as_mut());

                ui.vertical_centered(|ui| {
                    let texture = crate::update_egui_texture(
                        ui.ctx(),
                        CGRAM_TEXTURE_SIZE,
                        state.cgram_buffer.as_ref(),
                        &mut state.cgram_texture,
                    );
                    let image_size =
                        crate::clamp_image_size(ui.max_rect().size(), CGRAM_TEXTURE_SIZE);
                    ui.image((texture, image_size));
                });
            });
        });
}

fn render_vram_window(ctx: &egui::Context, emulator: &mut SnesEmulator, state: &mut State) {
    Window::new(VRAM_WINDOW_TITLE)
        .open(&mut state.vram_open)
        .default_pos([10.0, 25.0])
        .default_size([525.0, 700.0])
        .show(ctx, |ui| {
            Panel::top("vram_top_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mode:");

                    for (value, label) in [
                        (VramMode::TwoBpp, "2bpp"),
                        (VramMode::FourBpp, "4bpp"),
                        (VramMode::EightBpp, "8bpp"),
                        (VramMode::Mode7, "Mode 7"),
                    ] {
                        ui.radio_value(&mut state.vram_mode, value, label);
                    }
                });

                ui.add_space(5.0);

                let paletted_mode = matches!(state.vram_mode, VramMode::TwoBpp | VramMode::FourBpp);
                ui.add_enabled_ui(paletted_mode, |ui| {
                    Grid::new("vram_palette_grid").num_columns(9).show(ui, |ui| {
                        ui.label("Palette:");

                        for i in 0..8 {
                            ui.radio_value(&mut state.vram_palette, i, format!("BG{i}"));
                        }

                        ui.end_row();

                        ui.label("");

                        for i in 0..8 {
                            ui.radio_value(&mut state.vram_palette, i + 8, format!("OBJ{i}"));
                        }

                        ui.end_row();
                    });
                });
            });

            CentralPanel::default().show_inside(ui, |ui| {
                let (texture, texture_size) = match state.vram_mode {
                    VramMode::TwoBpp => {
                        emulator.copy_vram_2bpp(
                            state.vram_buffer.as_mut(),
                            state.vram_palette,
                            VRAM_2BPP_TEXTURE_SIZE[0] / 8,
                        );

                        let texture = crate::update_egui_texture(
                            ui.ctx(),
                            VRAM_2BPP_TEXTURE_SIZE,
                            &state.vram_buffer[..VRAM_2BPP_BUFFER_LEN],
                            &mut state.vram_2bpp_texture,
                        );
                        (texture, VRAM_2BPP_TEXTURE_SIZE)
                    }
                    VramMode::FourBpp => {
                        emulator.copy_vram_4bpp(
                            state.vram_buffer.as_mut(),
                            state.vram_palette,
                            VRAM_4BPP_TEXTURE_SIZE[0] / 8,
                        );

                        let texture = crate::update_egui_texture(
                            ui.ctx(),
                            VRAM_4BPP_TEXTURE_SIZE,
                            &state.vram_buffer[..VRAM_4BPP_BUFFER_LEN],
                            &mut state.vram_4bpp_texture,
                        );
                        (texture, VRAM_4BPP_TEXTURE_SIZE)
                    }
                    VramMode::EightBpp => {
                        emulator.copy_vram_8bpp(
                            state.vram_buffer.as_mut(),
                            VRAM_8BPP_TEXTURE_SIZE[0] / 8,
                        );

                        let texture = crate::update_egui_texture(
                            ui.ctx(),
                            VRAM_8BPP_TEXTURE_SIZE,
                            &state.vram_buffer[..VRAM_8BPP_BUFFER_LEN],
                            &mut state.vram_8bpp_texture,
                        );
                        (texture, VRAM_8BPP_TEXTURE_SIZE)
                    }
                    VramMode::Mode7 => {
                        emulator.copy_vram_mode7(
                            state.vram_buffer.as_mut(),
                            VRAM_MODE_7_TEXTURE_SIZE[0] / 8,
                        );

                        let texture = crate::update_egui_texture(
                            ui.ctx(),
                            VRAM_MODE_7_TEXTURE_SIZE,
                            &state.vram_buffer[..VRAM_MODE_7_BUFFER_LEN],
                            &mut state.vram_mode7_texture,
                        );
                        (texture, VRAM_MODE_7_TEXTURE_SIZE)
                    }
                };

                let image_size = crate::clamp_image_size(ui.max_rect().size(), texture_size);
                ui.image((texture, image_size));
            });
        });
}
