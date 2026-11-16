use crate::{DebugRenderContext, DebugRenderFn};
use egui::{CentralPanel, MenuBar, Panel, UiKind, Window};
use jgenesis_common::frontend::Color;
use nes_core::api::{NesEmulator, PatternTable};

const NAMETABLES_WINDOW_TITLE: &str = "Nametables";
const OAM_WINDOW_TITLE: &str = "Sprites";
const PALETTES_WINDOW_TITLE: &str = "Palettes";

const NAMETABLES_TEXTURE_SIZE: [usize; 2] = [2 * 256, 2 * 240];
const OAM_TEXTURE_SIZE: [usize; 2] = [8 * 8, 8 * 8];
const DOUBLE_HEIGHT_OAM_TEXTURE_SIZE: [usize; 2] = [OAM_TEXTURE_SIZE[0], 2 * OAM_TEXTURE_SIZE[1]];
const PALETTE_TEXTURE_SIZE: [usize; 2] = [4, 8];

#[derive(Debug)]
struct State {
    nametables_open: bool,
    nametables_pattern_table: PatternTable,
    nametables_buffer: Vec<Color>,
    nametables_texture: Option<egui::TextureId>,
    oam_open: bool,
    oam_pattern_table: PatternTable,
    oam_buffer: Vec<Color>,
    oam_texture: Option<egui::TextureId>,
    oam_double_height_texture: Option<egui::TextureId>,
    palettes_open: bool,
    palette_ram_texture: Option<egui::TextureId>,
}

impl State {
    fn new() -> Self {
        Self {
            nametables_open: true,
            nametables_pattern_table: PatternTable::Zero,
            nametables_buffer: vec![Color::default(); 4 * 256 * 240],
            nametables_texture: None,
            oam_open: true,
            oam_pattern_table: PatternTable::One,
            oam_buffer: vec![Color::default(); 2 * 64 * 8 * 8],
            oam_texture: None,
            oam_double_height_texture: None,
            palettes_open: true,
            palette_ram_texture: None,
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<NesEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx, emulator| render(ctx, emulator, &mut state))
}

fn render(ctx: DebugRenderContext<'_>, emulator: &mut NesEmulator, state: &mut State) {
    Panel::top("debug_top_panel").show_inside(ctx.egui_ui, |ui| {
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Video Memory", |ui| {
                for (label, open, title) in [
                    ("Nametables", &mut state.nametables_open, NAMETABLES_WINDOW_TITLE),
                    ("Sprites", &mut state.oam_open, OAM_WINDOW_TITLE),
                    ("Palettes", &mut state.palettes_open, PALETTES_WINDOW_TITLE),
                ] {
                    if ui.button(label).clicked() {
                        *open = true;
                        crate::move_to_top(ui.ctx(), title);
                        ui.close_kind(UiKind::Menu);
                    }
                }
            });
        });
    });

    render_nametables_window(ctx.egui_ui.ctx(), emulator, state);
    render_oam_window(ctx.egui_ui.ctx(), emulator, state);
    render_palette_window(ctx.egui_ui.ctx(), emulator, state);
}

fn render_nametables_window(ctx: &egui::Context, emulator: &mut NesEmulator, state: &mut State) {
    Window::new(NAMETABLES_WINDOW_TITLE)
        .open(&mut state.nametables_open)
        .default_pos([10.0, 25.0])
        .default_size([300.0, 325.0])
        .show(ctx, |ui| {
            Panel::top("nametables_top_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Pattern table:");

                    ui.radio_value(
                        &mut state.nametables_pattern_table,
                        PatternTable::Zero,
                        "$0000",
                    );
                    ui.radio_value(&mut state.nametables_pattern_table, PatternTable::One, "$1000");
                });
            });

            CentralPanel::default().show_inside(ui, |ui| {
                emulator
                    .copy_nametables(state.nametables_pattern_table, &mut state.nametables_buffer);

                let texture = crate::update_egui_texture(
                    ctx,
                    NAMETABLES_TEXTURE_SIZE,
                    &state.nametables_buffer,
                    &mut state.nametables_texture,
                );
                let image_size =
                    crate::clamp_image_size(ui.max_rect().size(), NAMETABLES_TEXTURE_SIZE);
                ui.image((texture, image_size));
            });
        });
}

fn render_oam_window(ctx: &egui::Context, emulator: &mut NesEmulator, state: &mut State) {
    Window::new(OAM_WINDOW_TITLE)
        .open(&mut state.oam_open)
        .default_pos([350.0, 25.0])
        .default_size([300.0, 325.0])
        .show(ctx, |ui| {
            Panel::top("oam_top_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!emulator.using_double_height_sprites(), |ui| {
                        ui.label("Pattern table:");

                        ui.radio_value(&mut state.oam_pattern_table, PatternTable::Zero, "$0000");
                        ui.radio_value(&mut state.oam_pattern_table, PatternTable::One, "$1000");
                    });
                });
            });

            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_oam(state.oam_pattern_table, &mut state.oam_buffer);

                let (texture, texture_size) = if emulator.using_double_height_sprites() {
                    let texture = crate::update_egui_texture(
                        ctx,
                        DOUBLE_HEIGHT_OAM_TEXTURE_SIZE,
                        &state.oam_buffer,
                        &mut state.oam_double_height_texture,
                    );
                    (texture, DOUBLE_HEIGHT_OAM_TEXTURE_SIZE)
                } else {
                    let texture = crate::update_egui_texture(
                        ctx,
                        OAM_TEXTURE_SIZE,
                        &state.oam_buffer[..OAM_TEXTURE_SIZE[0] * OAM_TEXTURE_SIZE[1]],
                        &mut state.oam_texture,
                    );
                    (texture, OAM_TEXTURE_SIZE)
                };
                let image_size = crate::clamp_image_size(ui.max_rect().size(), texture_size);
                ui.image((texture, image_size));
            });
        });
}

fn render_palette_window(ctx: &egui::Context, emulator: &mut NesEmulator, state: &mut State) {
    Window::new(PALETTES_WINDOW_TITLE)
        .open(&mut state.palettes_open)
        .default_pos([700.0, 25.0])
        .default_size([200.0, 400.0])
        .show(ctx, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                let mut colors = [Color::default(); 32];
                emulator.copy_palette_ram(&mut colors);

                let texture = crate::update_egui_texture(
                    ctx,
                    PALETTE_TEXTURE_SIZE,
                    &colors,
                    &mut state.palette_ram_texture,
                );
                let image_size =
                    crate::clamp_image_size(ui.max_rect().size(), PALETTE_TEXTURE_SIZE);
                ui.image((texture, image_size));
            });
        });
}
