use crate::memviewer::MemoryViewerState;
use crate::{DebugRenderContext, DebugRenderFn, memviewer};
use egui::{Context, Grid, Panel, Vec2, Window};
use jgenesis_common::debug::Endian;
use jgenesis_common::frontend::Color;
use pce_core::api::PcEngineEmulator;
use pce_core::api::debug::PceMemoryArea;
use std::collections::HashMap;

const VRAM_TEXTURE_WIDTH: usize = 64;
const VRAM_TEXTURE_HEIGHT: usize = 32;

struct State {
    memory_viewer_states: HashMap<PceMemoryArea, MemoryViewerState>,
    palettes_buffer: Vec<Color>,
    bg_palettes_texture: Option<egui::TextureId>,
    sprite_palettes_texture: Option<egui::TextureId>,
    vram_palette: u16,
    vram_buffer_2d: Box<[[Color; 64]; 2048]>,
    vram_buffer_1d: Vec<Color>,
    vram_texture: Option<egui::TextureId>,
}

impl State {
    fn new() -> Self {
        Self {
            memory_viewer_states: PceMemoryArea::ALL
                .into_iter()
                .map(|memory_area| {
                    (memory_area, MemoryViewerState::new(memory_area.name(), Endian::Little))
                })
                .collect(),
            palettes_buffer: vec![Color::default(); 512],
            bg_palettes_texture: None,
            sprite_palettes_texture: None,
            vram_palette: 0,
            vram_buffer_2d: vec![[Color::default(); 64]; 2048]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            vram_buffer_1d: vec![Color::default(); 64 * 2048],
            vram_texture: None,
        }
    }
}

pub fn render_fn() -> Box<DebugRenderFn<PcEngineEmulator>> {
    let mut state = State::new();
    Box::new(move |ctx, emulator| render(ctx, emulator, &mut state))
}

fn render(ctx: DebugRenderContext<'_>, emulator: &mut PcEngineEmulator, state: &mut State) {
    Panel::top("pce_debug_top_panel").show_inside(ctx.egui_ui, |ui| {
        ui.menu_button("Memory Viewers", |ui| {
            for memory_area in PceMemoryArea::ALL {
                if emulator.debug_memory_view(memory_area).is_none() {
                    continue;
                }

                if ui.button(memory_area.name()).clicked()
                    && let Some(memviewer_state) = state.memory_viewer_states.get_mut(&memory_area)
                {
                    memviewer_state.open_window(ui);
                }
            }
        });
    });

    for (&memory_area, memviewer_state) in &mut state.memory_viewer_states {
        if let Some(mut memory_view) = emulator.debug_memory_view(memory_area) {
            memviewer::render(ctx.egui_ui, &mut *memory_view, memviewer_state);
        }
    }

    update_palette_textures(ctx.egui_ui, emulator, state);

    Window::new("BG Palettes").resizable(true).default_width(400.0).show(ctx.egui_ui, |ui| {
        let size = clamp_palette_image_size(ui.available_size());

        if let Some(bg_palettes_texture) = state.bg_palettes_texture {
            ui.image((bg_palettes_texture, size));
        }
    });

    Window::new("Sprite Palettes").resizable(true).default_width(400.0).show(ctx.egui_ui, |ui| {
        let size = clamp_palette_image_size(ui.available_size());

        if let Some(sprite_palettes_texture) = state.sprite_palettes_texture {
            ui.image((sprite_palettes_texture, size));
        }
    });

    Window::new("VRAM").resizable(true).show(ctx.egui_ui, |ui| {
        update_vram_texture(ui, emulator, state);

        ui.spacing_mut().item_spacing = [2.0, 3.0].into();

        Grid::new("pce_vram_grid").show(ui, |ui| {
            ui.label("BG palette:");

            for palette in 0..16 {
                ui.radio_value(&mut state.vram_palette, palette, palette.to_string());
            }

            ui.end_row();

            ui.label("Sprite palette:");

            for palette in 0..16 {
                ui.radio_value(&mut state.vram_palette, palette | (1 << 4), palette.to_string());
            }

            ui.end_row();
        });

        if let Some(vram_texture) = state.vram_texture {
            let mut size = ui.available_size();
            if size.y > size.x * 0.5 {
                size.y = size.x * 0.5;
            }
            if size.x > size.y * 2.0 {
                size.x = size.y * 2.0;
            }

            ui.image((vram_texture, size));
        }
    });
}

fn clamp_palette_image_size(mut size: Vec2) -> Vec2 {
    if size.y > size.x {
        size.y = size.x;
    }

    if size.x > size.y {
        size.x = size.y;
    }

    size
}

fn update_palette_textures(ctx: &Context, emulator: &mut PcEngineEmulator, state: &mut State) {
    emulator.dump_palettes(&mut state.palettes_buffer);

    crate::update_egui_texture(
        ctx,
        [16, 16],
        &state.palettes_buffer[..256],
        &mut state.bg_palettes_texture,
    );

    crate::update_egui_texture(
        ctx,
        [16, 16],
        &state.palettes_buffer[256..],
        &mut state.sprite_palettes_texture,
    );
}

fn update_vram_texture(ctx: &Context, emulator: &mut PcEngineEmulator, state: &mut State) {
    const WIDTH: usize = VRAM_TEXTURE_WIDTH;

    emulator.dump_vram(state.vram_palette, state.vram_buffer_2d.as_mut_slice());

    // Copy from 2D buffer to a 1D buffer, 64x32 tiles
    for (tile_number, tile) in state.vram_buffer_2d.iter().enumerate() {
        let tile_base_addr = (tile_number / WIDTH) * WIDTH * 8 * 8 + (tile_number % WIDTH) * 8;

        for tile_row in 0..8 {
            for tile_col in 0..8 {
                state.vram_buffer_1d[tile_base_addr + tile_row * WIDTH * 8 + tile_col] =
                    tile[8 * tile_row + tile_col];
            }
        }
    }

    crate::update_egui_texture(
        ctx,
        [8 * VRAM_TEXTURE_WIDTH, 8 * VRAM_TEXTURE_HEIGHT],
        &state.vram_buffer_1d,
        &mut state.vram_texture,
    );
}
