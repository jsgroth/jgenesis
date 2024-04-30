use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, SelectableButton};
use egui::{CentralPanel, Grid, ScrollArea, Vec2};
use gb_core::api::{BackgroundTileMap, GameBoyEmulator};
use jgenesis_common::frontend::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Background,
    Sprites,
    Palettes,
}

#[derive(Debug)]
struct State {
    tab: Tab,
    background_tile_map: BackgroundTileMap,
    background_buffer: Vec<Color>,
    background_texture: Option<(wgpu::Texture, egui::TextureId)>,
    sprites_buffer: Vec<Color>,
    sprites_texture: Option<(wgpu::Texture, egui::TextureId)>,
    sprites_double_height_texture: Option<(wgpu::Texture, egui::TextureId)>,
    bg_palettes_texture: Option<(wgpu::Texture, egui::TextureId)>,
    obj_palettes_texture: Option<(wgpu::Texture, egui::TextureId)>,
}

impl State {
    fn new() -> Self {
        Self {
            tab: Tab::default(),
            background_tile_map: BackgroundTileMap::default(),
            background_buffer: vec![Color::default(); 256 * 256],
            background_texture: None,
            sprites_buffer: vec![Color::default(); 40 * 8 * 16],
            sprites_texture: None,
            sprites_double_height_texture: None,
            bg_palettes_texture: None,
            obj_palettes_texture: None,
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<GameBoyEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render(mut ctx: DebugRenderContext<'_, GameBoyEmulator>, state: &mut State) {
    update_background_texture(&mut ctx, state);
    update_sprite_texture(&mut ctx, state);
    update_palettes_texture(&mut ctx, state);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    CentralPanel::default().show(ctx.egui_ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(SelectableButton::new("Background", &mut state.tab, Tab::Background));
            ui.add(SelectableButton::new("Sprites", &mut state.tab, Tab::Sprites));
            ui.add(SelectableButton::new("Palettes", &mut state.tab, Tab::Palettes));
        });

        ui.add_space(15.0);

        match state.tab {
            Tab::Background => {
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

                ui.add_space(10.0);

                ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let egui_texture = state.background_texture.as_ref().unwrap().1;
                        ui.image((
                            egui_texture,
                            Vec2::new(screen_width * 0.95, screen_width * 0.95),
                        ))
                    });
                });
            }
            Tab::Sprites => {
                if ctx.emulator.is_using_double_height_sprites() {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let egui_texture =
                                state.sprites_double_height_texture.as_ref().unwrap().1;
                            ui.image((
                                egui_texture,
                                Vec2::new(screen_width * 0.6, screen_width * 0.6 * 80.0 / 64.0),
                            ));
                        });
                    });
                } else {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let egui_texture = state.sprites_texture.as_ref().unwrap().1;
                            ui.image((
                                egui_texture,
                                Vec2::new(screen_width * 0.95, screen_width * 0.95 * 40.0 / 64.0),
                            ));
                        });
                    });
                }
            }
            Tab::Palettes => {
                let bg_texture = state.bg_palettes_texture.as_ref().unwrap().1;
                let obj_texture = state.obj_palettes_texture.as_ref().unwrap().1;

                Grid::new("debug_gb_palettes_grid").show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Background");
                    });
                    ui.vertical_centered(|ui| {
                        ui.heading("Sprites");
                    });
                    ui.end_row();

                    ui.image((bg_texture, Vec2::new(screen_width * 0.3, screen_width * 0.3 * 2.0)));
                    ui.image((
                        obj_texture,
                        Vec2::new(screen_width * 0.3, screen_width * 0.3 * 2.0),
                    ));
                    ui.end_row();
                });
            }
        }
    });
}

fn update_background_texture(ctx: &mut DebugRenderContext<'_, GameBoyEmulator>, state: &mut State) {
    if state.tab == Tab::Background {
        ctx.emulator.copy_background(state.background_tile_map, &mut state.background_buffer);
    }

    if state.background_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_gb_bg", 256, 256, ctx.device, ctx.renderer);
        state.background_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.background_texture.as_ref().unwrap();
    let egui_texture = *egui_texture;

    debug::write_textures(
        wgpu_texture,
        egui_texture,
        bytemuck::cast_slice(&state.background_buffer),
        ctx,
    );
}

fn update_sprite_texture(ctx: &mut DebugRenderContext<'_, GameBoyEmulator>, state: &mut State) {
    if state.tab == Tab::Sprites {
        ctx.emulator.copy_sprites(&mut state.sprites_buffer);
    }

    if state.sprites_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_gb_sprites", 8 * 8, 5 * 8, ctx.device, ctx.renderer);
        state.sprites_texture = Some((wgpu_texture, egui_texture));
    }

    if state.sprites_double_height_texture.is_none() {
        let (wgpu_texture, egui_texture) = debug::create_texture(
            "debug_gb_sprites_2x_height",
            8 * 8,
            2 * 5 * 8,
            ctx.device,
            ctx.renderer,
        );
        state.sprites_double_height_texture = Some((wgpu_texture, egui_texture));
    }

    if ctx.emulator.is_using_double_height_sprites() {
        let (wgpu_texture, egui_texture) = state.sprites_double_height_texture.as_ref().unwrap();
        let egui_texture = *egui_texture;

        debug::write_textures(
            wgpu_texture,
            egui_texture,
            bytemuck::cast_slice(&state.sprites_buffer),
            ctx,
        );
    } else {
        let (wgpu_texture, egui_texture) = state.sprites_texture.as_ref().unwrap();
        let egui_texture = *egui_texture;

        debug::write_textures(
            wgpu_texture,
            egui_texture,
            bytemuck::cast_slice(&state.sprites_buffer[..40 * 64]),
            ctx,
        );
    }
}

fn update_palettes_texture(ctx: &mut DebugRenderContext<'_, GameBoyEmulator>, state: &mut State) {
    let mut palettes_buffer = [Color::default(); 64];

    if state.tab == Tab::Palettes {
        ctx.emulator.copy_palettes(&mut palettes_buffer);
    }

    if state.bg_palettes_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_gb_bg_palettes", 4, 8, ctx.device, ctx.renderer);
        state.bg_palettes_texture = Some((wgpu_texture, egui_texture));
    }

    if state.obj_palettes_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_gb_obj_palettes", 4, 8, ctx.device, ctx.renderer);
        state.obj_palettes_texture = Some((wgpu_texture, egui_texture));
    }

    let mut bg_palettes_buffer = [Color::TRANSPARENT; 32];
    let mut obj_palettes_buffer = [Color::TRANSPARENT; 32];
    if ctx.emulator.is_cgb_mode() {
        bg_palettes_buffer.copy_from_slice(&palettes_buffer[..32]);
        obj_palettes_buffer.copy_from_slice(&palettes_buffer[32..]);
    } else {
        bg_palettes_buffer[..4].copy_from_slice(&palettes_buffer[..4]);
        obj_palettes_buffer[..8].copy_from_slice(&palettes_buffer[4..12]);
    }

    let (bg_wgpu_texture, bg_egui_texture) = state.bg_palettes_texture.as_ref().unwrap();
    debug::write_textures(
        bg_wgpu_texture,
        *bg_egui_texture,
        bytemuck::cast_slice(&bg_palettes_buffer),
        ctx,
    );

    let (obj_wgpu_texture, obj_egui_texture) = state.obj_palettes_texture.as_ref().unwrap();
    debug::write_textures(
        obj_wgpu_texture,
        *obj_egui_texture,
        bytemuck::cast_slice(&obj_palettes_buffer),
        ctx,
    );
}
