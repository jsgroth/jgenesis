use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, SelectableButton};
use egui::{CentralPanel, ScrollArea, Vec2};
use jgenesis_common::frontend::Color;
use snes_core::api::SnesEmulator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    Cgram,
    #[default]
    Vram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum VramMode {
    TwoBpp,
    #[default]
    FourBpp,
    EightBpp,
    Mode7,
}

const CGRAM_BUFFER_LEN: usize = 256;
const VRAM_BUFFER_LEN: usize = 256 * 1024;

struct State {
    tab: Tab,
    vram_mode: VramMode,
    vram_palette: u8,
    cgram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    cgram_buffer: Box<[Color; CGRAM_BUFFER_LEN]>,
    vram_2bpp_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_4bpp_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_8bpp_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_mode7_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_buffer: Box<[Color; VRAM_BUFFER_LEN]>,
}

impl State {
    fn new() -> Self {
        Self {
            tab: Tab::default(),
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
            vram_buffer: vec![Color::default(); VRAM_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<SnesEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render(mut ctx: DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    update_cgram_texture(&mut ctx, state);
    update_vram_texture(&mut ctx, state);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    CentralPanel::default().show(ctx.egui_ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(SelectableButton::new("VRAM", &mut state.tab, Tab::Vram));
            ui.add(SelectableButton::new("CGRAM", &mut state.tab, Tab::Cgram));
        });

        ui.add_space(15.0);

        match state.tab {
            Tab::Cgram => {
                ui.vertical_centered(|ui| {
                    let egui_texture = state.cgram_texture.as_ref().unwrap().1;
                    ui.image((egui_texture, Vec2::new(screen_width * 0.65, screen_width * 0.65)));
                });
            }
            Tab::Vram => {
                let original_vram_mode = state.vram_mode;

                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    ui.add(SelectableButton::new("2bpp", &mut state.vram_mode, VramMode::TwoBpp));
                    ui.add(SelectableButton::new("4bpp", &mut state.vram_mode, VramMode::FourBpp));
                    ui.add(SelectableButton::new("8bpp", &mut state.vram_mode, VramMode::EightBpp));
                    ui.add(SelectableButton::new("Mode 7", &mut state.vram_mode, VramMode::Mode7));
                });

                ui.add_space(5.0);

                ui.add_enabled_ui(
                    matches!(state.vram_mode, VramMode::TwoBpp | VramMode::FourBpp),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Palette:");

                            for bg_palette in 0..8 {
                                ui.add(SelectableButton::new(
                                    format!("BG{bg_palette}"),
                                    &mut state.vram_palette,
                                    bg_palette,
                                ));
                            }

                            for obj_palette in 0..8 {
                                ui.add_enabled(
                                    state.vram_mode == VramMode::FourBpp,
                                    SelectableButton::new(
                                        format!("OBJ{obj_palette}"),
                                        &mut state.vram_palette,
                                        obj_palette + 8,
                                    ),
                                );
                            }
                        });
                    },
                );

                ui.add_space(10.0);

                ScrollArea::vertical().show(ui, |ui| match original_vram_mode {
                    VramMode::TwoBpp => {
                        let egui_texture = state.vram_2bpp_texture.as_ref().unwrap().1;
                        ui.image((egui_texture, Vec2::new(screen_width, screen_width)));
                    }
                    VramMode::FourBpp => {
                        let egui_texture = state.vram_4bpp_texture.as_ref().unwrap().1;
                        ui.image((egui_texture, Vec2::new(screen_width, screen_width * 0.5)));
                    }
                    VramMode::EightBpp => {
                        let egui_texture = state.vram_8bpp_texture.as_ref().unwrap().1;
                        ui.image((egui_texture, Vec2::new(screen_width, screen_width)));
                    }
                    VramMode::Mode7 => {
                        let egui_texture = state.vram_mode7_texture.as_ref().unwrap().1;
                        ui.image((egui_texture, Vec2::new(screen_width, screen_width)));
                    }
                });
            }
        }
    });
}

fn update_cgram_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    ctx.emulator.copy_cgram(state.cgram_buffer.as_mut());

    if state.cgram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_snes_cgram", 16, 16, ctx.device, ctx.renderer);
        state.cgram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.cgram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.cgram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    match state.vram_mode {
        VramMode::TwoBpp => update_vram_2bpp_texture(ctx, state),
        VramMode::FourBpp => update_vram_4bpp_texture(ctx, state),
        VramMode::EightBpp => update_vram_8bpp_texture(ctx, state),
        VramMode::Mode7 => update_vram_mode7_texture(ctx, state),
    }
}

fn update_vram_2bpp_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    ctx.emulator.copy_vram_2bpp(state.vram_buffer.as_mut(), state.vram_palette, 64);

    if state.vram_2bpp_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_snes_vram_2bpp", 64 * 8, 64 * 8, ctx.device, ctx.renderer);
        state.vram_2bpp_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_2bpp_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_4bpp_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    ctx.emulator.copy_vram_4bpp(state.vram_buffer.as_mut(), state.vram_palette, 64);

    if state.vram_4bpp_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_snes_vram_4bpp", 64 * 8, 32 * 8, ctx.device, ctx.renderer);
        state.vram_4bpp_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_4bpp_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_8bpp_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    ctx.emulator.copy_vram_8bpp(state.vram_buffer.as_mut(), 32);

    if state.vram_8bpp_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_snes_vram_8bpp", 32 * 8, 32 * 8, ctx.device, ctx.renderer);
        state.vram_8bpp_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_8bpp_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_mode7_texture(ctx: &mut DebugRenderContext<'_, SnesEmulator>, state: &mut State) {
    ctx.emulator.copy_vram_mode7(state.vram_buffer.as_mut(), 16);

    if state.vram_mode7_texture.is_none() {
        let (wgpu_texture, egui_texture) = debug::create_texture(
            "debug_snes_vram_mode7",
            16 * 8,
            16 * 8,
            ctx.device,
            ctx.renderer,
        );
        state.vram_mode7_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_mode7_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}
