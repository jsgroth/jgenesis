use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn};
use egui::{Grid, Pos2, ScrollArea, Vec2, Window};
use genesis_core::GenesisEmulator;
use jgenesis_common::frontend::Color;
use s32x_core::api::Sega32XEmulator;
use segacd_core::api::SegaCdEmulator;

struct State {
    vram_palette: u8,
    cram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    cram_buffer: Box<[Color; 64]>,
    vram_buffer: Box<[Color; 2048 * 64]>,
}

impl State {
    fn new() -> Self {
        Self {
            vram_palette: 0,
            cram_texture: None,
            vram_texture: None,
            cram_buffer: vec![Color::default(); 64].into_boxed_slice().try_into().unwrap(),
            vram_buffer: vec![Color::default(); 2048 * 64].into_boxed_slice().try_into().unwrap(),
        }
    }
}

pub(crate) trait GenesisBase {
    fn copy_cram(&self, out: &mut [Color]);

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize);

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)]));
}

impl GenesisBase for GenesisEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        GenesisEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        GenesisEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        GenesisEmulator::dump_vdp_registers(self, callback);
    }
}

impl GenesisBase for SegaCdEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        SegaCdEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        SegaCdEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        SegaCdEmulator::dump_vdp_registers(self, callback);
    }
}

impl GenesisBase for Sega32XEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        Sega32XEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        Sega32XEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        Sega32XEmulator::dump_vdp_registers(self, callback);
    }
}

pub(crate) fn render_fn<Emulator: GenesisBase>() -> Box<DebugRenderFn<Emulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render<Emulator: GenesisBase>(mut ctx: DebugRenderContext<'_, Emulator>, state: &mut State) {
    update_cram_texture(&mut ctx, state);
    update_vram_texture(&mut ctx, state);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    render_cram_window(ctx.egui_ctx, state.cram_texture.as_ref().unwrap().1, screen_width);

    render_vram_window(
        ctx.egui_ctx,
        &mut state.vram_palette,
        state.vram_texture.as_ref().unwrap().1,
        screen_width,
    );

    render_vdp_registers_window(ctx.egui_ctx, ctx.emulator);

    // CentralPanel::default().show(ctx.egui_ctx, |ui| {
    //     ui.horizontal(|ui| {
    //         ui.add(SelectableButton::new("VRAM", &mut state.tab, Tab::Vram));
    //         ui.add(SelectableButton::new("CRAM", &mut state.tab, Tab::Cram));
    //     });
    //
    //     ui.add_space(15.0);
    //
    //     match state.tab {
    //         Tab::Cram => {
    //             let egui_texture = state.cram_texture.as_ref().unwrap().1;
    //             ui.image((egui_texture, Vec2::new(screen_width, screen_width * 0.25)));
    //         }
    //         Tab::Vram => {
    //             ui.horizontal(|ui| {
    //                 ui.label("Palette:");
    //
    //                 for palette in 0..4 {
    //                     ui.radio_value(&mut state.vram_palette, palette, format!("{palette}"));
    //                 }
    //             });
    //
    //             ui.add_space(15.0);
    //
    //             ScrollArea::vertical().show(ui, |ui| {
    //                 let egui_texture = state.vram_texture.as_ref().unwrap().1;
    //                 ui.image((egui_texture, Vec2::new(screen_width, screen_width * 0.5)));
    //             });
    //         }
    //     }
    // });
}

fn render_cram_window(ctx: &egui::Context, cram_texture: egui::TextureId, screen_width: f32) {
    Window::new("CRAM").default_width(screen_width * 0.95).show(ctx, |ui| {
        let mut height = ui.available_width() * 0.25;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 4.0;

        ui.image((cram_texture, Vec2::new(width, height)));
    });
}

fn render_vram_window(
    ctx: &egui::Context,
    palette: &mut u8,
    vram_texture: egui::TextureId,
    screen_width: f32,
) {
    Window::new("VRAM").default_width(screen_width * 0.95).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Palette");

            for i in 0..4 {
                ui.radio_value(palette, i, format!("{i}"));
            }
        });

        let mut height = ui.available_width() * 0.5;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 2.0;

        ui.image((vram_texture, Vec2::new(width, height)));
    });
}

fn render_vdp_registers_window(ctx: &egui::Context, emulator: &impl GenesisBase) {
    Window::new("VDP Registers").default_open(false).default_pos(Pos2::new(5.0, 5.0)).show(
        ctx,
        |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                Grid::new("genesis_vdp_registers").num_columns(2).show(ui, |ui| {
                    emulator.dump_vdp_registers(|register, values| {
                        ui.heading(register);
                        ui.end_row();

                        for &(field, value) in values {
                            ui.label(format!("  {field}:"));
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                });
            });
        },
    );
}

fn update_cram_texture<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
) {
    ctx.emulator.copy_cram(state.cram_buffer.as_mut());

    if state.cram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_cram", 16, 4, ctx.device, ctx.renderer);
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

fn update_vram_texture<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
) {
    ctx.emulator.copy_vram(state.vram_buffer.as_mut(), state.vram_palette, 64);

    if state.vram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_vram", 64 * 8, 32 * 8, ctx.device, ctx.renderer);
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
