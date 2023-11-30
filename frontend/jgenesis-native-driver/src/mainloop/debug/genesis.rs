use crate::mainloop::debug;
use crate::mainloop::debug::{DebugRenderFn, DebuggerError, SelectableButton};
use egui::{CentralPanel, ScrollArea, Vec2};
use genesis_core::GenesisEmulator;
use jgenesis_common::frontend::Color;
use segacd_core::api::SegaCdEmulator;

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
    cram_buffer: Box<[Color; 64]>,
    vram_buffer: Box<[Color; 2048 * 64]>,
}

impl State {
    fn new() -> Self {
        Self {
            tab: Tab::default(),
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
}

impl GenesisBase for GenesisEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        GenesisEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        GenesisEmulator::copy_vram(self, out, palette, row_len);
    }
}

impl GenesisBase for SegaCdEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        SegaCdEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        SegaCdEmulator::copy_vram(self, out, palette, row_len);
    }
}

pub(crate) fn render_fn<Emulator: GenesisBase>() -> Box<DebugRenderFn<Emulator>> {
    let mut state = State::new();
    Box::new(move |ctx, emulator, device, queue, rpass| {
        render(ctx, emulator, device, queue, rpass, &mut state)
    })
}

fn render<Emulator: GenesisBase>(
    ctx: &egui::Context,
    emulator: &Emulator,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rpass: &mut egui_wgpu_backend::RenderPass,
    state: &mut State,
) -> Result<(), DebuggerError> {
    update_cram_texture(emulator, device, queue, rpass, state)?;
    update_vram_texture(emulator, device, queue, rpass, state)?;

    let screen_width = debug::screen_width(ctx);

    CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add(SelectableButton::new("VRAM", &mut state.tab, Tab::Vram));
            ui.add(SelectableButton::new("CRAM", &mut state.tab, Tab::Cram));
        });

        ui.add_space(15.0);

        match state.tab {
            Tab::Cram => {
                let egui_texture = state.cram_texture.as_ref().unwrap().1;
                ui.image((egui_texture, Vec2::new(screen_width, screen_width * 0.25)));
            }
            Tab::Vram => {
                ui.horizontal(|ui| {
                    ui.label("Palette:");

                    for palette in 0..4 {
                        ui.radio_value(&mut state.vram_palette, palette, format!("{palette}"));
                    }
                });

                ui.add_space(15.0);

                ScrollArea::vertical().show(ui, |ui| {
                    let egui_texture = state.vram_texture.as_ref().unwrap().1;
                    ui.image((egui_texture, Vec2::new(screen_width, screen_width * 0.5)));
                });
            }
        }
    });

    Ok(())
}

fn update_cram_texture<Emulator: GenesisBase>(
    emulator: &Emulator,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rpass: &mut egui_wgpu_backend::RenderPass,
    state: &mut State,
) -> Result<(), DebuggerError> {
    emulator.copy_cram(state.cram_buffer.as_mut());

    if state.cram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_cram", 16, 4, device, rpass);
        state.cram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.cram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.cram_buffer.as_ref()),
        device,
        queue,
        rpass,
    )
}

fn update_vram_texture<Emulator: GenesisBase>(
    emulator: &Emulator,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rpass: &mut egui_wgpu_backend::RenderPass,
    state: &mut State,
) -> Result<(), DebuggerError> {
    emulator.copy_vram(state.vram_buffer.as_mut(), state.vram_palette, 64);

    if state.vram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_vram", 64 * 8, 32 * 8, device, rpass);
        state.vram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        device,
        queue,
        rpass,
    )
}
