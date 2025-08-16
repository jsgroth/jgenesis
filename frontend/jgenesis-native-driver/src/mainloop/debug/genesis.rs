use crate::mainloop::debug;
use crate::mainloop::debug::memviewer::MemoryViewerState;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, memviewer};
use egui::epaint::ImageDelta;
use egui::panel::TopBottomSide;
use egui::{
    Color32, ColorImage, ImageData, ScrollArea, TextureFilter, TextureOptions, TextureWrapMode,
    TopBottomPanel, Vec2, Window, menu,
};
use egui_extras::{Column, TableBuilder};
use genesis_core::GenesisEmulator;
use genesis_core::api::debug::GenesisMemoryArea;
use genesis_core::vdp::ColorModifier;
use jgenesis_common::debug::{DebugMemoryView, Endian};
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::MatchEachVariantMacro;
use s32x_core::api::Sega32XEmulator;
use s32x_core::api::debug::S32XMemoryArea;
use segacd_core::api::SegaCdEmulator;
use segacd_core::api::debug::SegaCdMemoryArea;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MemoryArea {
    Genesis(GenesisMemoryArea),
    SegaCd(SegaCdMemoryArea),
    Sega32X(S32XMemoryArea),
}

impl MemoryArea {
    const ALL: &'static [Self] = &[
        Self::Genesis(GenesisMemoryArea::CartridgeRom),
        Self::Genesis(GenesisMemoryArea::WorkingRam),
        Self::Genesis(GenesisMemoryArea::AudioRam),
        Self::Genesis(GenesisMemoryArea::Vram),
        Self::Genesis(GenesisMemoryArea::Cram),
        Self::Genesis(GenesisMemoryArea::Vsram),
        Self::SegaCd(SegaCdMemoryArea::BiosRom),
        Self::SegaCd(SegaCdMemoryArea::PrgRam),
        Self::SegaCd(SegaCdMemoryArea::WordRam),
        Self::SegaCd(SegaCdMemoryArea::PcmRam),
        Self::SegaCd(SegaCdMemoryArea::CdcRam),
        Self::Sega32X(S32XMemoryArea::Sdram),
        Self::Sega32X(S32XMemoryArea::MasterSh2Cache),
        Self::Sega32X(S32XMemoryArea::SlaveSh2Cache),
        Self::Sega32X(S32XMemoryArea::FrameBuffer0),
        Self::Sega32X(S32XMemoryArea::FrameBuffer1),
        Self::Sega32X(S32XMemoryArea::PaletteRam),
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Genesis(area) => match area {
                GenesisMemoryArea::CartridgeRom => "Cartridge ROM",
                GenesisMemoryArea::WorkingRam => "Working RAM",
                GenesisMemoryArea::AudioRam => "Audio RAM",
                GenesisMemoryArea::Vram => "VRAM",
                GenesisMemoryArea::Cram => "CRAM",
                GenesisMemoryArea::Vsram => "VSRAM",
            },
            Self::SegaCd(area) => match area {
                SegaCdMemoryArea::BiosRom => "BIOS ROM",
                SegaCdMemoryArea::PrgRam => "PRG RAM",
                SegaCdMemoryArea::WordRam => "Word RAM",
                SegaCdMemoryArea::PcmRam => "PCM Waveform RAM",
                SegaCdMemoryArea::CdcRam => "CDC Buffer RAM",
            },
            Self::Sega32X(area) => match area {
                S32XMemoryArea::Sdram => "32X SDRAM",
                S32XMemoryArea::MasterSh2Cache => "Master SH-2 Cache",
                S32XMemoryArea::SlaveSh2Cache => "Slave SH-2 Cache",
                S32XMemoryArea::FrameBuffer0 => "32X Frame Buffer 0",
                S32XMemoryArea::FrameBuffer1 => "32X Frame Buffer 1",
                S32XMemoryArea::PaletteRam => "32X Palette RAM",
            },
        }
    }

    fn new_states() -> HashMap<Self, MemoryViewerState> {
        Self::ALL
            .iter()
            .map(|&area| (area, MemoryViewerState::new(area.name(), Endian::Big)))
            .collect()
    }
}

struct CramWindowState {
    open: bool,
    modifier: ColorModifier,
    buffer: Box<[Color; 64]>,
    texture: Option<egui::TextureId>,
}

impl CramWindowState {
    fn new() -> Self {
        Self {
            open: true,
            modifier: ColorModifier::None,
            buffer: vec![Color::default(); 64].into_boxed_slice().try_into().unwrap(),
            texture: None,
        }
    }
}

struct VramWindowState {
    open: bool,
    palette: u8,
    buffer: Box<[Color; 2048 * 64]>,
    texture: Option<egui::TextureId>,
}

impl VramWindowState {
    fn new() -> Self {
        Self {
            open: true,
            palette: 0,
            buffer: vec![Color::default(); 2048 * 64].into_boxed_slice().try_into().unwrap(),
            texture: None,
        }
    }
}

struct S32XPaletteRamState {
    open: bool,
    buffer: Box<[Color; 256]>,
    texture: Option<egui::TextureId>,
}

impl S32XPaletteRamState {
    fn new() -> Self {
        Self {
            open: false,
            buffer: vec![Color::default(); 256].into_boxed_slice().try_into().unwrap(),
            texture: None,
        }
    }
}

struct State {
    memory_viewers: HashMap<MemoryArea, MemoryViewerState>,
    cram: CramWindowState,
    vram: VramWindowState,
    s32x_palette: S32XPaletteRamState,
    vdp_registers_open: bool,
    s32x_system_registers_open: bool,
    s32x_vdp_registers_open: bool,
    s32x_pwm_registers_open: bool,
}

impl State {
    fn new() -> Self {
        Self {
            memory_viewers: MemoryArea::new_states(),
            cram: CramWindowState::new(),
            vram: VramWindowState::new(),
            s32x_palette: S32XPaletteRamState::new(),
            vdp_registers_open: false,
            s32x_system_registers_open: false,
            s32x_vdp_registers_open: false,
            s32x_pwm_registers_open: false,
        }
    }
}

#[derive(MatchEachVariantMacro)]
pub(crate) enum GenesisBasedEmulator<'a> {
    Genesis(&'a mut GenesisEmulator),
    SegaCd(&'a mut SegaCdEmulator),
    Sega32X(&'a mut Sega32XEmulator),
}

impl GenesisBasedEmulator<'_> {
    fn copy_cram(&mut self, out: &mut [Color], modifier: ColorModifier) {
        match_each_variant!(self, emulator => emulator.debug().copy_cram(out, modifier));
    }

    fn copy_vram(&mut self, out: &mut [Color], palette: u8, row_len: usize) {
        match_each_variant!(self, emulator => emulator.debug().copy_vram(out, palette, row_len));
    }

    fn dump_vdp_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        match_each_variant!(self, emulator => emulator.debug().dump_vdp_registers(callback));
    }

    fn has_memory(&self, memory_area: MemoryArea) -> bool {
        match self {
            Self::Genesis(_) => matches!(memory_area, MemoryArea::Genesis(_)),
            Self::SegaCd(_) => match memory_area {
                MemoryArea::Genesis(GenesisMemoryArea::CartridgeRom) | MemoryArea::Sega32X(_) => {
                    false
                }
                MemoryArea::Genesis(_) | MemoryArea::SegaCd(_) => true,
            },
            Self::Sega32X(_) => {
                matches!(memory_area, MemoryArea::Genesis(_) | MemoryArea::Sega32X(_))
            }
        }
    }

    fn debug_memory_view(
        &mut self,
        memory_area: MemoryArea,
    ) -> Option<Box<dyn DebugMemoryView + '_>> {
        match (self, memory_area) {
            (Self::Genesis(emulator), MemoryArea::Genesis(area)) => {
                Some(emulator.debug().memory_view(area))
            }
            (Self::SegaCd(emulator), MemoryArea::Genesis(area)) => {
                Some(emulator.debug().genesis_memory_view(area))
            }
            (Self::SegaCd(emulator), MemoryArea::SegaCd(area)) => {
                Some(emulator.debug().scd_memory_view(area))
            }
            (Self::Sega32X(emulator), MemoryArea::Genesis(area)) => {
                Some(emulator.debug().genesis_memory_view(area))
            }
            (Self::Sega32X(emulator), MemoryArea::Sega32X(area)) => {
                Some(emulator.debug().s32x_memory_view(area))
            }
            (Self::Genesis(_), MemoryArea::SegaCd(_) | MemoryArea::Sega32X(_))
            | (Self::SegaCd(_), MemoryArea::Sega32X(_))
            | (Self::Sega32X(_), MemoryArea::SegaCd(_)) => None,
        }
    }
}

pub(crate) trait GenesisBase {
    fn as_enum(&mut self) -> GenesisBasedEmulator<'_>;
}

impl GenesisBase for GenesisEmulator {
    fn as_enum(&mut self) -> GenesisBasedEmulator<'_> {
        GenesisBasedEmulator::Genesis(self)
    }
}

impl GenesisBase for SegaCdEmulator {
    fn as_enum(&mut self) -> GenesisBasedEmulator<'_> {
        GenesisBasedEmulator::SegaCd(self)
    }
}

impl GenesisBase for Sega32XEmulator {
    fn as_enum(&mut self) -> GenesisBasedEmulator<'_> {
        GenesisBasedEmulator::Sega32X(self)
    }
}

pub(crate) fn render_fn<Emulator: GenesisBase>() -> Box<DebugRenderFn<Emulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render<Emulator: GenesisBase>(ctx: DebugRenderContext<'_, Emulator>, state: &mut State) {
    let mut emulator = ctx.emulator.as_enum();

    TopBottomPanel::new(TopBottomSide::Top, "gen_debug_top").show(ctx.egui_ctx, |ui| {
        menu::bar(ui, |ui| {
            ui.menu_button("Memory Viewers", |ui| {
                for &memory_area in MemoryArea::ALL {
                    if !emulator.has_memory(memory_area) {
                        continue;
                    }

                    if ui.button(memory_area.name()).clicked() {
                        if let Some(memviewer_state) = state.memory_viewers.get_mut(&memory_area) {
                            memviewer_state.open = true;
                        }
                        ui.close_menu();
                    }
                }
            });

            ui.menu_button("Register Viewers", |ui| {
                if ui.button("VDP").clicked() {
                    state.vdp_registers_open = true;
                    ui.close_menu();
                }

                if matches!(emulator, GenesisBasedEmulator::Sega32X(_)) {
                    if ui.button("32X System Registers").clicked() {
                        state.s32x_system_registers_open = true;
                        ui.close_menu();
                    }

                    if ui.button("32X VDP").clicked() {
                        state.s32x_vdp_registers_open = true;
                        ui.close_menu();
                    }

                    if ui.button("32X PWM").clicked() {
                        state.s32x_pwm_registers_open = true;
                        ui.close_menu();
                    }
                }
            });

            ui.menu_button("Video Memory", |ui| {
                if ui.button("CRAM").clicked() {
                    state.cram.open = true;
                    ui.close_menu();
                }

                if ui.button("VRAM").clicked() {
                    state.vram.open = true;
                    ui.close_menu();
                }

                if matches!(emulator, GenesisBasedEmulator::Sega32X(_))
                    && ui.button("32X Palette RAM").clicked()
                {
                    state.s32x_palette.open = true;
                    ui.close_menu();
                }
            });
        });
    });

    render_memory_viewer_windows(ctx.egui_ctx, &mut emulator, &mut state.memory_viewers);

    render_vdp_registers_window(ctx.egui_ctx, &mut emulator, &mut state.vdp_registers_open);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    render_cram_window(ctx.egui_ctx, screen_width, &mut emulator, &mut state.cram);
    render_vram_window(ctx.egui_ctx, screen_width, &mut emulator, &mut state.vram);

    if let GenesisBasedEmulator::Sega32X(emulator) = &mut emulator {
        render_32x_palette_window(ctx.egui_ctx, screen_width, emulator, &mut state.s32x_palette);
        render_32x_system_registers_window(
            ctx.egui_ctx,
            emulator,
            &mut state.s32x_system_registers_open,
        );
        render_32x_vdp_registers_window(ctx.egui_ctx, emulator, &mut state.s32x_vdp_registers_open);
        render_32x_pwm_registers_window(ctx.egui_ctx, emulator, &mut state.s32x_pwm_registers_open);
    }
}

fn render_memory_viewer_windows(
    egui_ctx: &egui::Context,
    emulator: &mut GenesisBasedEmulator<'_>,
    memory_viewer_states: &mut HashMap<MemoryArea, MemoryViewerState>,
) {
    for (&memory_area, state) in memory_viewer_states.iter_mut() {
        if let Some(mut memory) = emulator.debug_memory_view(memory_area) {
            memviewer::render(egui_ctx, memory.as_mut(), state);
        }
    }
}

fn render_cram_window(
    ctx: &egui::Context,
    screen_width: f32,
    emulator: &mut GenesisBasedEmulator<'_>,
    state: &mut CramWindowState,
) {
    Window::new("CRAM").default_width(screen_width * 0.95).open(&mut state.open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.modifier, ColorModifier::None, "Normal");
            ui.radio_value(&mut state.modifier, ColorModifier::Shadow, "Shadowed");
            ui.radio_value(&mut state.modifier, ColorModifier::Highlight, "Highlighted");
        });

        emulator.copy_cram(state.buffer.as_mut_slice(), state.modifier);

        let mut height = ui.available_width() * 0.25;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 4.0;

        let texture = update_texture(ctx, [16, 4], state.buffer.as_slice(), &mut state.texture);
        ui.image((texture, Vec2::new(width, height)));
    });
}

fn update_texture(
    ctx: &egui::Context,
    size: [usize; 2],
    image: &[Color],
    texture: &mut Option<egui::TextureId>,
) -> egui::TextureId {
    let tex_manager = ctx.tex_manager();

    match *texture {
        Some(texture) => {
            tex_manager.write().set(
                texture,
                ImageDelta {
                    image: ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(
                        size,
                        bytemuck::cast_slice(image),
                    ))),
                    options: TextureOptions {
                        magnification: TextureFilter::Nearest,
                        minification: TextureFilter::Nearest,
                        wrap_mode: TextureWrapMode::ClampToEdge,
                        mipmap_mode: None,
                    },
                    pos: None,
                },
            );

            texture
        }
        None => {
            let id = tex_manager.write().alloc(
                "cram_texture".into(),
                ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(
                    size,
                    bytemuck::cast_slice(image),
                ))),
                TextureOptions {
                    magnification: TextureFilter::Nearest,
                    minification: TextureFilter::Nearest,
                    wrap_mode: TextureWrapMode::ClampToEdge,
                    mipmap_mode: None,
                },
            );
            *texture = Some(id);

            id
        }
    }
}

fn render_vram_window(
    ctx: &egui::Context,
    screen_width: f32,
    emulator: &mut GenesisBasedEmulator<'_>,
    state: &mut VramWindowState,
) {
    Window::new("VRAM").default_width(screen_width * 0.95).open(&mut state.open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Palette");

            for i in 0..4 {
                ui.radio_value(&mut state.palette, i, format!("{i}"));
            }
        });

        emulator.copy_vram(state.buffer.as_mut_slice(), state.palette, 64);

        let mut height = ui.available_width() * 0.45;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 2.0;

        let texture =
            update_texture(ctx, [64 * 8, 32 * 8], state.buffer.as_slice(), &mut state.texture);
        ui.image((texture, Vec2::new(width, height)));
    });
}

fn render_32x_palette_window(
    ctx: &egui::Context,
    screen_width: f32,
    emulator: &mut Sega32XEmulator,
    state: &mut S32XPaletteRamState,
) {
    Window::new("32X Palette RAM")
        .open(&mut state.open)
        .default_width(screen_width * 0.6)
        .default_height(screen_width * 0.6)
        .show(ctx, |ui| {
            emulator.debug().copy_palette(state.buffer.as_mut_slice());

            let mut size = ui.available_width();
            if ui.available_height() < size {
                size = ui.available_height();
            }

            let texture =
                update_texture(ctx, [16, 16], state.buffer.as_slice(), &mut state.texture);
            ui.image((texture, Vec2::new(size, size)));
        });
}

fn render_registers_window(
    ctx: &egui::Context,
    window_title: &str,
    open: &mut bool,
    render_registers: impl FnOnce(&mut egui::Ui),
) {
    Window::new(window_title).open(open).show(ctx, |ui| {
        ScrollArea::vertical().show(ui, |ui| {
            let color = ui.visuals_mut().faint_bg_color;

            // Make stripes a little lighter
            ui.visuals_mut().faint_bg_color = Color32::from_rgba_premultiplied(
                color.r().saturating_add(10),
                color.g().saturating_add(10),
                color.b().saturating_add(10),
                color.a(),
            );

            render_registers(ui);
        });
    });
}

fn render_registers_table(ui: &mut egui::Ui, register: &str, values: &[(&str, &str)]) {
    TableBuilder::new(ui)
        .id_salt(register)
        .column(Column::exact(200.0))
        .column(Column::exact(150.0))
        .vscroll(false)
        .striped(true)
        .header(25.0, |mut header| {
            header.col(|ui| {
                ui.heading(register);
            });
        })
        .body(|mut body| {
            for &(field, value) in values {
                body.row(17.0, |mut row| {
                    row.col(|ui| {
                        ui.label(field);
                    });

                    row.col(|ui| {
                        ui.label(value);
                    });
                });
            }
        });
}

fn dump_registers_callback(ui: &mut egui::Ui) -> impl FnMut(&str, &[(&str, &str)]) {
    let mut first = true;

    move |register, values| {
        if !first {
            ui.separator();
        }
        first = false;

        render_registers_table(ui, register, values);
    }
}

fn render_vdp_registers_window(
    ctx: &egui::Context,
    emulator: &mut GenesisBasedEmulator<'_>,
    open: &mut bool,
) {
    render_registers_window(ctx, "VDP Registers", open, |ui| {
        emulator.dump_vdp_registers(dump_registers_callback(ui));
    });
}

fn render_32x_system_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    render_registers_window(ctx, "32X System Registers", open, |ui| {
        emulator.debug().dump_32x_system_registers(dump_registers_callback(ui));
    });
}

fn render_32x_vdp_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    render_registers_window(ctx, "32X VDP Registers", open, |ui| {
        emulator.debug().dump_32x_vdp_registers(dump_registers_callback(ui));
    });
}

fn render_32x_pwm_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    render_registers_window(ctx, "32X PWM Registers", open, |ui| {
        emulator.debug().dump_pwm_registers(dump_registers_callback(ui));
    });
}
