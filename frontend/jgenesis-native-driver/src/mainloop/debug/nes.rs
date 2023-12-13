use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, DebuggerError, SelectableButton};
use egui::{CentralPanel, ScrollArea, Vec2};
use jgenesis_common::frontend::Color;
use nes_core::api::{NesEmulator, PatternTable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Nametables,
    Oam,
    PaletteRam,
}

#[derive(Debug)]
struct State {
    tab: Tab,
    nametables_pattern_table: PatternTable,
    nametables_buffer: Vec<Color>,
    nametables_texture: Option<(wgpu::Texture, egui::TextureId)>,
    oam_pattern_table: PatternTable,
    oam_buffer: Vec<Color>,
    oam_texture: Option<(wgpu::Texture, egui::TextureId)>,
    oam_double_height_texture: Option<(wgpu::Texture, egui::TextureId)>,
    palette_ram_texture: Option<(wgpu::Texture, egui::TextureId)>,
}

impl State {
    fn new() -> Self {
        Self {
            tab: Tab::default(),
            nametables_pattern_table: PatternTable::Zero,
            nametables_buffer: vec![Color::default(); 4 * 256 * 240],
            nametables_texture: None,
            oam_pattern_table: PatternTable::One,
            oam_buffer: vec![Color::default(); 2 * 64 * 8 * 8],
            oam_texture: None,
            oam_double_height_texture: None,
            palette_ram_texture: None,
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<NesEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render(
    mut ctx: DebugRenderContext<'_, NesEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    update_nametables_texture(&mut ctx, state)?;
    update_oam_texture(&mut ctx, state)?;
    update_palette_ram_texture(&mut ctx, state)?;

    let screen_width = debug::screen_width(ctx.egui_ctx);

    CentralPanel::default().show(ctx.egui_ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(SelectableButton::new("Nametables", &mut state.tab, Tab::Nametables));
            ui.add(SelectableButton::new("OAM", &mut state.tab, Tab::Oam));
            ui.add(SelectableButton::new("Palette RAM", &mut state.tab, Tab::PaletteRam));
        });

        ui.add_space(15.0);

        match state.tab {
            Tab::Nametables => {
                ui.horizontal(|ui| {
                    ui.label("Pattern table:");

                    ui.radio_value(
                        &mut state.nametables_pattern_table,
                        PatternTable::Zero,
                        "$0000",
                    );
                    ui.radio_value(&mut state.nametables_pattern_table, PatternTable::One, "$1000");
                });

                ui.add_space(10.0);

                ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let egui_texture = state.nametables_texture.as_ref().unwrap().1;
                        ui.image((
                            egui_texture,
                            Vec2::new(screen_width * 0.95, screen_width * 0.95),
                        ));
                    });
                });
            }
            Tab::Oam => {
                ui.horizontal(|ui| {
                    ui.set_enabled(!ctx.emulator.using_double_height_sprites());

                    ui.label("Pattern table:");

                    ui.radio_value(&mut state.oam_pattern_table, PatternTable::Zero, "$0000");
                    ui.radio_value(&mut state.oam_pattern_table, PatternTable::One, "$1000");
                });

                ui.add_space(10.0);

                ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        if ctx.emulator.using_double_height_sprites() {
                            let egui_texture = state.oam_double_height_texture.as_ref().unwrap().1;
                            ui.image((
                                egui_texture,
                                Vec2::new(screen_width * 0.325, screen_width * 0.65),
                            ));
                        } else {
                            let egui_texture = state.oam_texture.as_ref().unwrap().1;
                            ui.image((
                                egui_texture,
                                Vec2::new(screen_width * 0.65, screen_width * 0.65),
                            ));
                        }
                    });
                });
            }
            Tab::PaletteRam => {
                ui.vertical_centered(|ui| {
                    let egui_texture = state.palette_ram_texture.as_ref().unwrap().1;
                    ui.image((egui_texture, Vec2::new(screen_width * 0.325, screen_width * 0.65)));
                });
            }
        }
    });

    Ok(())
}

fn update_nametables_texture(
    ctx: &mut DebugRenderContext<'_, NesEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    if state.tab == Tab::Nametables {
        ctx.emulator.copy_nametables(state.nametables_pattern_table, &mut state.nametables_buffer);
    }

    if state.nametables_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_nes_nametables", 2 * 256, 2 * 240, ctx.device, ctx.rpass);
        state.nametables_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.nametables_texture.as_ref().unwrap();
    let egui_texture = *egui_texture;

    debug::write_textures(
        wgpu_texture,
        egui_texture,
        bytemuck::cast_slice(&state.nametables_buffer),
        ctx,
    )
}

fn update_oam_texture(
    ctx: &mut DebugRenderContext<'_, NesEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    if state.tab == Tab::Oam {
        ctx.emulator.copy_oam(state.oam_pattern_table, &mut state.oam_buffer);
    }

    if state.oam_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_nes_oam", 8 * 8, 8 * 8, ctx.device, ctx.rpass);
        state.oam_texture = Some((wgpu_texture, egui_texture));
    }

    if state.oam_double_height_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_nes_oam_2x", 8 * 8, 2 * 8 * 8, ctx.device, ctx.rpass);
        state.oam_double_height_texture = Some((wgpu_texture, egui_texture));
    }

    if ctx.emulator.using_double_height_sprites() {
        let (wgpu_texture, egui_texture) = state.oam_double_height_texture.as_ref().unwrap();
        let egui_texture = *egui_texture;

        debug::write_textures(
            wgpu_texture,
            egui_texture,
            bytemuck::cast_slice(&state.oam_buffer),
            ctx,
        )
    } else {
        let (wgpu_texture, egui_texture) = state.oam_texture.as_ref().unwrap();
        let egui_texture = *egui_texture;

        debug::write_textures(
            wgpu_texture,
            egui_texture,
            bytemuck::cast_slice(&state.oam_buffer[..64 * 64]),
            ctx,
        )
    }
}

fn update_palette_ram_texture(
    ctx: &mut DebugRenderContext<'_, NesEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    let mut colors = [Color::default(); 32];
    ctx.emulator.copy_palette_ram(&mut colors);

    if state.palette_ram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_nes_palette_ram", 4, 8, ctx.device, ctx.rpass);
        state.palette_ram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.palette_ram_texture.as_ref().unwrap();
    let egui_texture = *egui_texture;

    debug::write_textures(wgpu_texture, egui_texture, bytemuck::cast_slice(&colors), ctx)
}
