use crate::{DebugRenderContext, DebugRenderFn};
use egui::{CentralPanel, MenuBar, Panel, UiKind, Window};
use jgenesis_common::frontend::Color;
use smsgg_core::SmsGgEmulator;

const VRAM_WINDOW_TITLE: &str = "VRAM";
const CRAM_WINDOW_TITLE: &str = "CRAM";
const REGISTERS_WINDOW_TITLE: &str = "VDP Registers";

const CRAM_TEXTURE_SIZE: [usize; 2] = [16, 2];
const VRAM_TEXTURE_SIZE: [usize; 2] = [32 * 8, 16 * 8];

struct State {
    vram_open: bool,
    cram_open: bool,
    registers_open: bool,
    vram_palette: u8,
    cram_texture: Option<egui::TextureId>,
    vram_texture: Option<egui::TextureId>,
    cram_buffer: Box<[Color; 32]>,
    vram_buffer: Box<[Color; 512 * 64]>,
}

impl State {
    fn new() -> Self {
        Self {
            vram_open: true,
            cram_open: true,
            registers_open: false,
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

fn render(ctx: DebugRenderContext<'_>, emulator: &mut SmsGgEmulator, state: &mut State) {
    Panel::top("debug_top_panel").show_inside(ctx.egui_ui, |ui| {
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Video Memory", |ui| {
                if ui.button("VRAM").clicked() {
                    state.vram_open = true;
                    crate::move_to_top(ui.ctx(), VRAM_WINDOW_TITLE);
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("CRAM").clicked() {
                    state.cram_open = true;
                    crate::move_to_top(ui.ctx(), CRAM_WINDOW_TITLE);
                    ui.close_kind(UiKind::Menu);
                }
            });

            ui.menu_button("Register Viewers", |ui| {
                if ui.button("VDP Registers").clicked() {
                    state.registers_open = true;
                    crate::move_to_top(ui.ctx(), REGISTERS_WINDOW_TITLE);
                    ui.close_kind(UiKind::Menu);
                }
            });
        });
    });

    Window::new(CRAM_WINDOW_TITLE)
        .open(&mut state.cram_open)
        .default_pos([10.0, 25.0])
        .default_size([850.0, 125.0])
        .show(ctx.egui_ui, |ui| {
            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_cram(state.cram_buffer.as_mut());

                let cram_texture = crate::update_egui_texture(
                    ui.ctx(),
                    CRAM_TEXTURE_SIZE,
                    state.cram_buffer.as_ref(),
                    &mut state.cram_texture,
                );
                let image_size = crate::clamp_image_size(ui.max_rect().size(), CRAM_TEXTURE_SIZE);
                ui.image((cram_texture, image_size));
            });
        });

    Window::new(VRAM_WINDOW_TITLE).open(&mut state.vram_open).default_size([850.0, 450.0]).show(
        ctx.egui_ui,
        |ui| {
            Panel::top("vram_panel_top").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Palette");

                    ui.radio_value(&mut state.vram_palette, 0, "0");
                    ui.radio_value(&mut state.vram_palette, 1, "1");
                });
            });

            CentralPanel::default().show_inside(ui, |ui| {
                emulator.copy_vram(
                    state.vram_buffer.as_mut(),
                    state.vram_palette,
                    VRAM_TEXTURE_SIZE[0] / 8,
                );

                let vram_texture = crate::update_egui_texture(
                    ui.ctx(),
                    VRAM_TEXTURE_SIZE,
                    state.vram_buffer.as_ref(),
                    &mut state.vram_texture,
                );
                let image_size = crate::clamp_image_size(ui.max_rect().size(), VRAM_TEXTURE_SIZE);
                ui.image((vram_texture, image_size));
            });
        },
    );

    crate::render_registers_window(
        ctx.egui_ui.ctx(),
        REGISTERS_WINDOW_TITLE,
        &mut state.registers_open,
        |ui| {
            emulator.dump_vdp_registers(|register, fields| {
                crate::render_registers_table(ui, &format!("Register #{register}"), fields);
                ui.separator();
            });
        },
    );
}
