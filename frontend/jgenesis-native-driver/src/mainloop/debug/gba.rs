use crate::mainloop::debug;
use crate::mainloop::debug::memviewer::MemoryViewerState;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, memviewer};
use egui::panel::TopBottomSide;
use egui::{TopBottomPanel, UiKind, Vec2, Window};
use gba_core::api::GameBoyAdvanceEmulator;
use gba_core::api::debug::GbaMemoryArea;
use jgenesis_common::debug::Endian;
use jgenesis_common::frontend::Color;
use std::collections::HashMap;

struct PaletteWindowState {
    open: bool,
    offset: usize,
    texture: Option<egui::TextureId>,
}

impl PaletteWindowState {
    fn new_bg() -> Self {
        Self { open: true, offset: 0, texture: None }
    }

    fn new_obj() -> Self {
        Self { open: true, offset: 256, texture: None }
    }
}

struct State {
    memory_viewer_states: HashMap<GbaMemoryArea, MemoryViewerState>,
    bg_palette: PaletteWindowState,
    obj_palette: PaletteWindowState,
    palette_buffer: Box<[Color; 512]>,
}

impl State {
    fn new() -> Self {
        let memory_viewer_states = GbaMemoryArea::ALL
            .into_iter()
            .map(|area| (area, MemoryViewerState::new(area.name(), Endian::Little)))
            .collect();

        Self {
            memory_viewer_states,
            bg_palette: PaletteWindowState::new_bg(),
            obj_palette: PaletteWindowState::new_obj(),
            palette_buffer: vec![Color::default(); 512].into_boxed_slice().try_into().unwrap(),
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<GameBoyAdvanceEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render(ctx: DebugRenderContext<'_, GameBoyAdvanceEmulator>, state: &mut State) {
    TopBottomPanel::new(TopBottomSide::Top, "gba_debug_top").show(ctx.egui_ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Memory Viewers", |ui| {
                for area in GbaMemoryArea::ALL {
                    if ui.button(area.name()).clicked()
                        && let Some(memviewer_state) = state.memory_viewer_states.get_mut(&area)
                    {
                        memviewer_state.open = true;
                        ui.close_kind(UiKind::Menu);
                    }
                }
            });

            ui.menu_button("Video Memory", |ui| {
                if ui.button("BG Palettes").clicked() {
                    state.bg_palette.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Sprite Palettes").clicked() {
                    state.obj_palette.open = true;
                    ui.close_kind(UiKind::Menu);
                }
            });
        });
    });

    for area in GbaMemoryArea::ALL {
        let Some(state) = state.memory_viewer_states.get_mut(&area) else { continue };

        let mut memory = ctx.emulator.debug().memory_view(area);
        memviewer::render(ctx.egui_ctx, memory.as_mut(), state);
    }

    ctx.emulator.debug().copy_palette_ram(state.palette_buffer.as_mut_slice());

    render_palette_window(
        ctx.egui_ctx,
        "BG Palettes",
        state.palette_buffer.as_slice(),
        &mut state.bg_palette,
    );
    render_palette_window(
        ctx.egui_ctx,
        "Sprite Palettes",
        state.palette_buffer.as_slice(),
        &mut state.obj_palette,
    );
}

fn render_palette_window(
    ctx: &egui::Context,
    title: &str,
    buffer: &[Color],
    state: &mut PaletteWindowState,
) {
    Window::new(title).open(&mut state.open).default_size([350.0, 400.0]).show(ctx, |ui| {
        let texture = debug::update_egui_texture(
            ctx,
            [16, 16],
            &buffer[state.offset..state.offset + 256],
            &mut state.texture,
        );

        let mut size = ui.available_width();
        if ui.available_height() < size {
            size = ui.available_height();
        }

        ui.image((texture, Vec2::new(size, size)));
    });
}
