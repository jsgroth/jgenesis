use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, DebuggerError, SelectableButton};
use egui::{CentralPanel, ScrollArea, Vec2};
use jgenesis_common::frontend::Color;
use smsgg_core::SmsGgEmulator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    Cram,
    #[default]
    Vram,
}

struct State {
    tab: Tab,
    vram_palette: u8,
    cram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    cram_buffer: Box<[Color; 32]>,
    vram_buffer: Box<[Color; 512 * 64]>,
}

impl State {
    fn new() -> Self {
        Self {
            tab: Tab::default(),
            vram_palette: 0,
            cram_texture: None,
            vram_texture: None,
            cram_buffer: vec![Color::default(); 32].into_boxed_slice().try_into().unwrap(),
            vram_buffer: vec![Color::default(); 512 * 64].into_boxed_slice().try_into().unwrap(),
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<SmsGgEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render(
    mut ctx: DebugRenderContext<'_, SmsGgEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    update_cram_texture(&mut ctx, state)?;
    update_vram_texture(&mut ctx, state)?;

    let screen_width = debug::screen_width(ctx.egui_ctx);

    CentralPanel::default().show(ctx.egui_ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(SelectableButton::new("VRAM", &mut state.tab, Tab::Vram));
            ui.add(SelectableButton::new("CRAM", &mut state.tab, Tab::Cram));
        });

        ui.add_space(15.0);

        match state.tab {
            Tab::Cram => {
                let cram_texture = state.cram_texture.as_ref().unwrap().1;
                ui.image((cram_texture, Vec2::new(screen_width, screen_width * 0.125)));
            }
            Tab::Vram => {
                ui.horizontal(|ui| {
                    ui.label("Palette");

                    ui.radio_value(&mut state.vram_palette, 0, "0");
                    ui.radio_value(&mut state.vram_palette, 1, "1");
                });

                ui.add_space(15.0);

                ScrollArea::vertical().show(ui, |ui| {
                    let vram_texture = state.vram_texture.as_ref().unwrap().1;
                    ui.image((vram_texture, Vec2::new(screen_width, screen_width * 0.5)));
                });
            }
        }
    });

    Ok(())
}

fn update_cram_texture(
    ctx: &mut DebugRenderContext<'_, SmsGgEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    ctx.emulator.copy_cram(state.cram_buffer.as_mut());

    if state.cram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_smsgg_cram", 16, 2, ctx.device, ctx.rpass);
        state.cram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.cram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.cram_buffer.as_ref()),
        ctx,
    )
}

fn update_vram_texture(
    ctx: &mut DebugRenderContext<'_, SmsGgEmulator>,
    state: &mut State,
) -> Result<(), DebuggerError> {
    ctx.emulator.copy_vram(state.vram_buffer.as_mut(), state.vram_palette, 32);

    if state.vram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_smsgg_vram", 32 * 8, 16 * 8, ctx.device, ctx.rpass);
        state.vram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    )
}
