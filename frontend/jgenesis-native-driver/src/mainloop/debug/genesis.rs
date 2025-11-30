use crate::mainloop::debug;
use crate::mainloop::debug::memviewer::MemoryViewerState;
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn, memviewer};
use egui::panel::TopBottomSide;
use egui::scroll_area::ScrollBarVisibility;
use egui::{TopBottomPanel, Vec2, Window, menu};
use egui_extras::{Column, TableBuilder};
use genesis_core::GenesisEmulator;
use genesis_core::api::debug::{
    CopySpriteAttributesResult, GenesisMemoryArea, SpriteAttributeEntry,
};
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

    fn default_file_name(self) -> &'static str {
        match self {
            Self::Genesis(area) => match area {
                GenesisMemoryArea::CartridgeRom => "rom.bin",
                GenesisMemoryArea::WorkingRam => "wram.bin",
                GenesisMemoryArea::AudioRam => "audioram.bin",
                GenesisMemoryArea::Vram => "vram.bin",
                GenesisMemoryArea::Cram => "cram.bin",
                GenesisMemoryArea::Vsram => "vsram.bin",
            },
            Self::SegaCd(area) => match area {
                SegaCdMemoryArea::BiosRom => "bios.bin",
                SegaCdMemoryArea::PrgRam => "prgram.bin",
                SegaCdMemoryArea::WordRam => "wordram.bin",
                SegaCdMemoryArea::PcmRam => "pcmram.bin",
                SegaCdMemoryArea::CdcRam => "cdcram.bin",
            },
            Self::Sega32X(area) => match area {
                S32XMemoryArea::Sdram => "sdram.bin",
                S32XMemoryArea::MasterSh2Cache => "mcache.bin",
                S32XMemoryArea::SlaveSh2Cache => "scache.bin",
                S32XMemoryArea::FrameBuffer0 => "fb0.bin",
                S32XMemoryArea::FrameBuffer1 => "fb1.bin",
                S32XMemoryArea::PaletteRam => "paletteram.bin",
            },
        }
    }

    fn new_states() -> HashMap<Self, MemoryViewerState> {
        Self::ALL
            .iter()
            .map(|&area| {
                (
                    area,
                    MemoryViewerState::new(area.name(), Endian::Big)
                        .with_default_file_name(area.default_file_name().into()),
                )
            })
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

struct HScrollWindowState {
    open: bool,
    buffer: Box<[(u16, u16); 256]>,
}

impl HScrollWindowState {
    fn new() -> Self {
        Self { open: false, buffer: vec![(0, 0); 256].into_boxed_slice().try_into().unwrap() }
    }
}

struct SpriteAttributesWindowState {
    open: bool,
    buffer: Box<[SpriteAttributeEntry; 80]>,
    adjust_coordinates: bool,
}

impl SpriteAttributesWindowState {
    fn new() -> Self {
        Self {
            open: false,
            buffer: vec![SpriteAttributeEntry::default(); 80]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            adjust_coordinates: false,
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
    h_scroll: HScrollWindowState,
    sprite_attributes: SpriteAttributesWindowState,
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
            h_scroll: HScrollWindowState::new(),
            sprite_attributes: SpriteAttributesWindowState::new(),
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

    fn copy_h_scroll(&mut self, out: &mut [(u16, u16)]) {
        match_each_variant!(self, emulator => emulator.debug().copy_h_scroll(out));
    }

    fn copy_sprite_attributes(
        &mut self,
        out: &mut [SpriteAttributeEntry],
    ) -> CopySpriteAttributesResult {
        match_each_variant!(self, emulator => emulator.debug().copy_sprite_attributes(out))
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

                if ui.button("Sprite Attributes").clicked() {
                    state.sprite_attributes.open = true;
                    ui.close_menu();
                }

                if ui.button("H Scroll Table").clicked() {
                    state.h_scroll.open = true;
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
    render_h_scroll_window(ctx.egui_ctx, &mut emulator, &mut state.h_scroll);
    render_sprite_attributes_window(ctx.egui_ctx, &mut emulator, &mut state.sprite_attributes);

    if let GenesisBasedEmulator::Sega32X(emulator) = &mut emulator {
        render_32x_palette_window(ctx.egui_ctx, emulator, &mut state.s32x_palette);
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

        let texture =
            debug::update_egui_texture(ctx, [16, 4], state.buffer.as_slice(), &mut state.texture);
        ui.image((texture, Vec2::new(width, height)));
    });
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

        let texture = debug::update_egui_texture(
            ctx,
            [64 * 8, 32 * 8],
            state.buffer.as_slice(),
            &mut state.texture,
        );
        ui.image((texture, Vec2::new(width, height)));
    });
}

fn render_h_scroll_window(
    ctx: &egui::Context,
    emulator: &mut GenesisBasedEmulator<'_>,
    state: &mut HScrollWindowState,
) {
    Window::new("H Scroll Table").default_width(200.0).open(&mut state.open).show(ctx, |ui| {
        emulator.copy_h_scroll(state.buffer.as_mut_slice());

        debug::brighten_faint_bg_color(ui);

        TableBuilder::new(ui)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            .column(Column::auto().at_least(50.0))
            .columns(Column::auto(), 2)
            .column(Column::remainder())
            .striped(true)
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.heading("Line");
                });
                header.col(|ui| {
                    ui.heading("Plane A");
                });
                header.col(|ui| {
                    ui.heading("Plane B");
                });
                header.col(|_ui| {});
            })
            .body(|body| {
                body.rows(18.0, 256, |mut row| {
                    let line = row.index();
                    let (h_scroll_a, h_scroll_b) = state.buffer[line];

                    row.col(|ui| {
                        ui.label(line.to_string());
                    });
                    row.col(|ui| {
                        ui.label(h_scroll_a.to_string());
                    });
                    row.col(|ui| {
                        ui.label(h_scroll_b.to_string());
                    });
                    row.col(|_ui| {});
                });
            });
    });
}

fn render_sprite_attributes_window(
    ctx: &egui::Context,
    emulator: &mut GenesisBasedEmulator<'_>,
    state: &mut SpriteAttributesWindowState,
) {
    Window::new("Sprite Attribute Table").open(&mut state.open).default_width(500.0).show(
        ctx,
        |ui| {
            let CopySpriteAttributesResult { sprite_table_len, top_left_x, top_left_y } =
                emulator.copy_sprite_attributes(state.buffer.as_mut_slice());

            ui.checkbox(&mut state.adjust_coordinates, "Shift coordinates to top-left of screen");

            let (x_offset, y_offset) = if state.adjust_coordinates {
                (-i32::from(top_left_x), -i32::from(top_left_y))
            } else {
                (0, 0)
            };

            debug::brighten_faint_bg_color(ui);

            TableBuilder::new(ui)
                .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                .columns(Column::auto().at_least(50.0), 11)
                .column(Column::remainder())
                .striped(true)
                .header(20.0, |mut header| {
                    for heading in [
                        "Index",
                        "Tile",
                        "X",
                        "Y",
                        "Cell Size",
                        "Palette",
                        "Priority",
                        "H Flip",
                        "V Flip",
                        "Link",
                    ] {
                        header.col(|ui| {
                            ui.heading(heading);
                        });
                    }
                    header.col(|_ui| {});
                })
                .body(|body| {
                    body.rows(18.0, sprite_table_len as usize, |mut row| {
                        let idx = row.index();
                        let sprite = state.buffer[idx];

                        for value in [
                            idx.to_string(),
                            sprite.tile_number.to_string(),
                            (i32::from(sprite.x) + x_offset).to_string(),
                            (i32::from(sprite.y) + y_offset).to_string(),
                            format!("{}x{}", sprite.h_cells, sprite.v_cells),
                            sprite.palette.to_string(),
                            u8::from(sprite.priority).to_string(),
                            u8::from(sprite.h_flip).to_string(),
                            u8::from(sprite.v_flip).to_string(),
                            sprite.link.to_string(),
                        ] {
                            row.col(|ui| {
                                ui.label(value);
                            });
                        }
                        row.col(|_ui| {});
                    });
                });
        },
    );
}

fn render_32x_palette_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    state: &mut S32XPaletteRamState,
) {
    Window::new("32X Palette RAM").open(&mut state.open).default_size([500.0, 550.0]).show(
        ctx,
        |ui| {
            emulator.debug().copy_palette(state.buffer.as_mut_slice());

            let mut size = ui.available_width();
            if ui.available_height() < size {
                size = ui.available_height();
            }

            let texture = debug::update_egui_texture(
                ctx,
                [16, 16],
                state.buffer.as_slice(),
                &mut state.texture,
            );
            ui.image((texture, Vec2::new(size, size)));
        },
    );
}

fn render_vdp_registers_window(
    ctx: &egui::Context,
    emulator: &mut GenesisBasedEmulator<'_>,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "VDP Registers", open, |ui| {
        emulator.dump_vdp_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_system_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X System Registers", open, |ui| {
        emulator.debug().dump_32x_system_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_vdp_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X VDP Registers", open, |ui| {
        emulator.debug().dump_32x_vdp_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_pwm_registers_window(
    ctx: &egui::Context,
    emulator: &mut Sega32XEmulator,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X PWM Registers", open, |ui| {
        emulator.debug().dump_pwm_registers(debug::dump_registers_callback(ui));
    });
}
