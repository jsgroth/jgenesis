use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn};
use egui::{Grid, Pos2, ScrollArea, Vec2, Window};
use jgenesis_common::frontend::Color;
use smsgg_core::SmsGgEmulator;

struct State {
    vram_palette: u8,
    cram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    cram_buffer: Box<[Color; 32]>,
    vram_buffer: Box<[Color; 512 * 64]>,
}

impl State {
    fn new() -> Self {
        Self {
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
    Box::new(move |ctx, emulator| render(ctx, emulator, &mut state))
}

fn render(mut ctx: DebugRenderContext<'_>, emulator: &mut SmsGgEmulator, state: &mut State) {
    update_cram_texture(&mut ctx, emulator, state);
    update_vram_texture(&mut ctx, emulator, state);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    Window::new("CRAM").default_width(screen_width * 0.95).show(ctx.egui_ctx, |ui| {
        let mut height = ui.available_width() * 0.125;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 8.0;

        let cram_texture = state.cram_texture.as_ref().unwrap().1;
        ui.image((cram_texture, Vec2::new(width, height)));
    });

    Window::new("VRAM").default_width(screen_width * 0.95).show(ctx.egui_ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Palette");

            ui.radio_value(&mut state.vram_palette, 0, "0");
            ui.radio_value(&mut state.vram_palette, 1, "1");
        });

        ui.add_space(5.0);

        let mut height = ui.available_width() * 0.5;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 2.0;

        let vram_texture = state.vram_texture.as_ref().unwrap().1;
        ui.image((vram_texture, Vec2::new(width, height)));
    });

    Window::new("VDP Registers").default_open(false).default_pos(Pos2::new(5.0, 5.0)).show(
        ctx.egui_ctx,
        |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                Grid::new("smsgg_vdp_registers").num_columns(2).show(ui, |ui| {
                    emulator.dump_vdp_registers(|register, fields| {
                        ui.heading(format!("Register #{register}"));
                        ui.end_row();

                        for &(name, value) in fields {
                            ui.label(format!("  {name}:"));
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                });
            });
        },
    );
}

fn update_cram_texture(
    ctx: &mut DebugRenderContext<'_>,
    emulator: &mut SmsGgEmulator,
    state: &mut State,
) {
    emulator.copy_cram(state.cram_buffer.as_mut());

    if state.cram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_smsgg_cram", 16, 2, ctx.device, ctx.renderer);
        state.cram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.cram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.cram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_texture(
    ctx: &mut DebugRenderContext<'_>,
    emulator: &mut SmsGgEmulator,
    state: &mut State,
) {
    emulator.copy_vram(state.vram_buffer.as_mut(), state.vram_palette, 32);

    if state.vram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_smsgg_vram", 32 * 8, 16 * 8, ctx.device, ctx.renderer);
        state.vram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}
