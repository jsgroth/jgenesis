use crate::{DebugRenderContext, DebugRenderFn};
use egui::{CentralPanel, MenuBar, Panel, UiKind, Window};
use gb_core::api::{BackgroundTileMap, GameBoyEmulator};
use jgenesis_common::frontend::Color;

const BACKGROUND_WINDOW_TITLE: &str = "Background";
const SPRITES_WINDOW_TITLE: &str = "Sprites";
const BG_PALETTES_WINDOW_TITLE: &str = "BG Palettes";
const SPRITE_PALETTES_WINDOW_TITLE: &str = "Sprite Palettes";

const BG_TEXTURE_SIZE: [usize; 2] = [256, 256];
const SPRITES_TEXTURE_SIZE: [usize; 2] = [8 * 8, 5 * 8];
const DOUBLE_HEIGHT_SPRITES_TEXTURE_SIZE: [usize; 2] =
    [SPRITES_TEXTURE_SIZE[0], 2 * SPRITES_TEXTURE_SIZE[1]];
const PALETTE_TEXTURE_SIZE: [usize; 2] = [4, 8];

#[derive(Debug)]
struct State {
    background_open: bool,
    background_tile_map: BackgroundTileMap,
    background_buffer: Vec<Color>,
    background_texture: Option<egui::TextureId>,
    sprites_open: bool,
    sprites_buffer: Vec<Color>,
    sprites_texture: Option<egui::TextureId>,
    sprites_double_height_texture: Option<egui::TextureId>,
    bg_palettes_open: bool,
    bg_palettes_texture: Option<egui::TextureId>,
    obj_palettes_open: bool,
    obj_palettes_texture: Option<egui::TextureId>,
}

impl State {
    fn new() -> Self {
        Self {
            background_open: true,
            background_tile_map: BackgroundTileMap::default(),
            background_buffer: vec![Color::default(); 256 * 256],
            background_texture: None,
            sprites_open: true,
            sprites_buffer: vec![Color::default(); 40 * 8 * 16],
            sprites_texture: None,
            sprites_double_height_texture: None,
            bg_palettes_open: true,
            bg_palettes_texture: None,
            obj_palettes_open: true,
            obj_palettes_texture: None,
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<GameBoyEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx, emulator| render(ctx, emulator, &mut state))
}

fn render(ctx: DebugRenderContext<'_>, emulator: &mut GameBoyEmulator, state: &mut State) {
    Panel::top("debug_menu").show_inside(ctx.egui_ui, |ui| {
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Video Memory", |ui| {
                for (label, open, title) in [
                    ("Background", &mut state.background_open, BACKGROUND_WINDOW_TITLE),
                    ("Sprites", &mut state.sprites_open, SPRITES_WINDOW_TITLE),
                    ("BG Palettes", &mut state.bg_palettes_open, BG_PALETTES_WINDOW_TITLE),
                    ("Sprite Palettes", &mut state.obj_palettes_open, SPRITE_PALETTES_WINDOW_TITLE),
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

    render_background_window(ctx.egui_ui.ctx(), emulator, state);
    render_sprites_window(ctx.egui_ui.ctx(), emulator, state);
    render_palette_windows(ctx.egui_ui.ctx(), emulator, state);
}

fn render_background_window(
    ctx: &egui::Context,
    emulator: &mut GameBoyEmulator,
    state: &mut State,
) {
    Window::new(BACKGROUND_WINDOW_TITLE)
        .open(&mut state.background_open)
        .default_pos([10.0, 25.0])
        .default_size([350.0, 375.0])
        .show(ctx, |ui| {
            Panel::top("bg_top_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Tile map:");

                    ui.radio_value(
                        &mut state.background_tile_map,
                        BackgroundTileMap::Zero,
                        "$9800-$9BFF",
                    );
                    ui.radio_value(
                        &mut state.background_tile_map,
                        BackgroundTileMap::One,
                        "$9C00-$9FFF",
                    );
                });
            });

            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_background(state.background_tile_map, &mut state.background_buffer);
                let texture = crate::update_egui_texture(
                    ctx,
                    BG_TEXTURE_SIZE,
                    &state.background_buffer,
                    &mut state.background_texture,
                );

                let image_size = crate::clamp_image_size(ui.max_rect().size(), BG_TEXTURE_SIZE);
                ui.image((texture, image_size));
            });
        });
}

fn render_sprites_window(ctx: &egui::Context, emulator: &mut GameBoyEmulator, state: &mut State) {
    Window::new(SPRITES_WINDOW_TITLE)
        .open(&mut state.sprites_open)
        .default_pos([400.0, 25.0])
        .default_size([350.0, 375.0])
        .show(ctx, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_sprites(&mut state.sprites_buffer);

                let (texture, texture_size) = if emulator.is_using_double_height_sprites() {
                    let texture = crate::update_egui_texture(
                        ctx,
                        DOUBLE_HEIGHT_SPRITES_TEXTURE_SIZE,
                        &state.sprites_buffer,
                        &mut state.sprites_double_height_texture,
                    );
                    (texture, DOUBLE_HEIGHT_SPRITES_TEXTURE_SIZE)
                } else {
                    let texture = crate::update_egui_texture(
                        ctx,
                        SPRITES_TEXTURE_SIZE,
                        &state.sprites_buffer[..SPRITES_TEXTURE_SIZE[0] * SPRITES_TEXTURE_SIZE[1]],
                        &mut state.sprites_texture,
                    );
                    (texture, SPRITES_TEXTURE_SIZE)
                };

                let image_size = crate::clamp_image_size(ui.max_rect().size(), texture_size);
                ui.image((texture, image_size));
            });
        });
}

fn render_palette_windows(ctx: &egui::Context, emulator: &mut GameBoyEmulator, state: &mut State) {
    if !state.bg_palettes_open && !state.obj_palettes_open {
        return;
    }

    let mut palettes_buffer = [Color::default(); 64];
    emulator.copy_palettes(&mut palettes_buffer);

    let mut bg_palettes_buffer = [Color::TRANSPARENT; 32];
    let mut obj_palettes_buffer = [Color::TRANSPARENT; 32];
    if emulator.is_cgb_mode() {
        bg_palettes_buffer.copy_from_slice(&palettes_buffer[..32]);
        obj_palettes_buffer.copy_from_slice(&palettes_buffer[32..]);
    } else {
        bg_palettes_buffer[..4].copy_from_slice(&palettes_buffer[..4]);
        obj_palettes_buffer[..8].copy_from_slice(&palettes_buffer[4..12]);
    }

    Window::new(BG_PALETTES_WINDOW_TITLE)
        .open(&mut state.bg_palettes_open)
        .default_pos([10.0, 460.0])
        .default_size([150.0, 275.0])
        .show(ctx, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                let texture = crate::update_egui_texture(
                    ctx,
                    PALETTE_TEXTURE_SIZE,
                    &bg_palettes_buffer,
                    &mut state.bg_palettes_texture,
                );
                let image_size =
                    crate::clamp_image_size(ui.max_rect().size(), PALETTE_TEXTURE_SIZE);
                ui.image((texture, image_size));
            });
        });

    Window::new(SPRITE_PALETTES_WINDOW_TITLE)
        .open(&mut state.obj_palettes_open)
        .default_pos([400.0, 460.0])
        .default_size([150.0, 275.0])
        .show(ctx, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                let texture = crate::update_egui_texture(
                    ctx,
                    PALETTE_TEXTURE_SIZE,
                    &obj_palettes_buffer,
                    &mut state.obj_palettes_texture,
                );
                let image_size =
                    crate::clamp_image_size(ui.max_rect().size(), PALETTE_TEXTURE_SIZE);
                ui.image((texture, image_size));
            });
        });
}
