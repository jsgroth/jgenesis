mod m68kdebug;
mod sh2debug;

use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::debug;
use crate::mainloop::debug::genesis::m68kdebug::{
    Genesis68kMemoryMap, M68kDebugWindowState, S32XMemoryMap, SegaCdMainMemoryMap,
    SegaCdSubMemoryMap,
};
use crate::mainloop::debug::genesis::sh2debug::Sh2DebugWindowState;
use crate::mainloop::debug::memviewer::MemoryViewerState;
use crate::mainloop::debug::{
    DebugRenderContext, DebuggerMainProcess, DebuggerRunnerProcess, memviewer,
};
use crate::mainloop::input::ThreadedInputPoller;
use crate::mainloop::render::ThreadedRenderer;
use crate::mainloop::runner::RunTillNextErr;
use crate::mainloop::save::FsSaveWriter;
use egui::panel::TopBottomSide;
use egui::scroll_area::ScrollBarVisibility;
use egui::{TopBottomPanel, UiKind, Vec2, Window};
use egui_extras::{Column, TableBuilder};
use genesis_config::GenesisInputs;
use genesis_core::GenesisEmulator;
use genesis_core::api::debug::{
    CopySpriteAttributesResult, GenesisDebugCommand, GenesisDebugState, GenesisDebugger,
    GenesisMemoryArea, SpriteAttributeEntry,
};
use genesis_core::vdp::ColorModifier;
use jgenesis_common::debug::{DebugMemoryView, DebugViewWithWriteHook, Endian};
use jgenesis_common::frontend::{Color, TickEffect};
use jgenesis_common::sync::{SharedVarReceiver, SharedVarSender};
use s32x_core::WhichCpu;
use s32x_core::api::Sega32XEmulator;
use s32x_core::api::debug::{
    S32XMemoryArea, Sega32XDebugCommand, Sega32XDebugState, Sega32XDebugger, Sega32XDebuggerHandle,
    Sh2BreakStatus,
};
use segacd_core::api::SegaCdEmulator;
use segacd_core::api::debug::{
    SegaCdDebugCommand, SegaCdDebugState, SegaCdDebugger, SegaCdMemoryArea,
};
use std::collections::HashMap;
use std::error::Error;
use std::hash::Hash;
use std::sync::mpsc::Sender;

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
                        .with_default_file_name(area.default_file_name().into())
                        .with_editable(),
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
    memory_edit_hook: Box<dyn FnMut(MemoryArea, usize, u8)>,
    cram: CramWindowState,
    vram: VramWindowState,
    h_scroll: HScrollWindowState,
    sprite_attributes: SpriteAttributesWindowState,
    s32x_palette: S32XPaletteRamState,
    m68k: M68kDebugWindowState,
    m68k_sub: M68kDebugWindowState,
    sh2_master: Sh2DebugWindowState,
    sh2_slave: Sh2DebugWindowState,
    vdp_registers_open: bool,
    s32x_system_registers_open: bool,
    s32x_vdp_registers_open: bool,
    s32x_pwm_registers_open: bool,
}

impl State {
    fn new(memory_edit_hook: Box<dyn FnMut(MemoryArea, usize, u8)>) -> Self {
        Self {
            memory_viewers: MemoryArea::new_states(),
            memory_edit_hook,
            cram: CramWindowState::new(),
            vram: VramWindowState::new(),
            h_scroll: HScrollWindowState::new(),
            sprite_attributes: SpriteAttributesWindowState::new(),
            s32x_palette: S32XPaletteRamState::new(),
            m68k: M68kDebugWindowState::new(),
            m68k_sub: M68kDebugWindowState::new_with_title("Sub 68000 Disassembly"),
            sh2_master: Sh2DebugWindowState::new(WhichCpu::Master),
            sh2_slave: Sh2DebugWindowState::new(WhichCpu::Slave),
            vdp_registers_open: false,
            s32x_system_registers_open: false,
            s32x_vdp_registers_open: false,
            s32x_pwm_registers_open: false,
        }
    }
}

pub(crate) enum GenesisBasedDebugState<'a> {
    Genesis(&'a mut GenesisDebugState),
    SegaCd(&'a mut SegaCdDebugState),
    Sega32X(&'a mut Sega32XDebugState, &'a Sender<Sega32XDebugCommand>, Sh2BreakStatus),
}

macro_rules! match_each_state_variant {
    ($self:expr, state => state.$method:ident($($param:tt)*)) => {
        match $self {
            Self::Genesis(state) => state.$method($($param)*),
            Self::SegaCd(state) => state.genesis().$method($($param)*),
            Self::Sega32X(state, ..) => state.genesis().$method($($param)*),
        }
    }
}

impl GenesisBasedDebugState<'_> {
    fn copy_cram(&mut self, out: &mut [Color], modifier: ColorModifier) {
        match_each_state_variant!(self, state => state.copy_cram(out, modifier));
    }

    fn copy_vram(&mut self, out: &mut [Color], palette: u8, row_len: usize) {
        match_each_state_variant!(self, state => state.copy_vram(out, palette, row_len));
    }

    fn dump_vdp_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        match_each_state_variant!(self, state => state.dump_vdp_registers(callback));
    }

    fn copy_h_scroll(&mut self, out: &mut [(u16, u16)]) {
        match_each_state_variant!(self, state => state.copy_h_scroll(out));
    }

    fn copy_sprite_attributes(
        &mut self,
        out: &mut [SpriteAttributeEntry],
    ) -> CopySpriteAttributesResult {
        match_each_state_variant!(self, state => state.copy_sprite_attributes(out))
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
            Self::Sega32X(..) => {
                matches!(memory_area, MemoryArea::Genesis(_) | MemoryArea::Sega32X(_))
            }
        }
    }

    fn debug_memory_view(
        &mut self,
        memory_area: MemoryArea,
    ) -> Option<Box<dyn DebugMemoryView + '_>> {
        match (self, memory_area) {
            (Self::Genesis(state), MemoryArea::Genesis(area)) => Some(state.memory_view(area)),
            (Self::SegaCd(state), MemoryArea::Genesis(area)) => {
                Some(state.genesis().memory_view(area))
            }
            (Self::SegaCd(state), MemoryArea::SegaCd(area)) => Some(state.scd_memory_view(area)),
            (Self::Sega32X(state, ..), MemoryArea::Genesis(area)) => {
                Some(state.genesis().memory_view(area))
            }
            (Self::Sega32X(state, ..), MemoryArea::Sega32X(area)) => {
                Some(state.s32x_memory_view(area))
            }
            (Self::Genesis(_), MemoryArea::SegaCd(_) | MemoryArea::Sega32X(_))
            | (Self::SegaCd(_), MemoryArea::Sega32X(_))
            | (Self::Sega32X(..), MemoryArea::SegaCd(_)) => None,
        }
    }
}

pub type GenesisDebugRenderFn = dyn FnMut(DebugRenderContext<'_>, &mut GenesisBasedDebugState<'_>);

pub(crate) fn render_fn(
    memory_edit_hook: Box<dyn FnMut(MemoryArea, usize, u8)>,
) -> Box<GenesisDebugRenderFn> {
    let mut state = State::new(memory_edit_hook);
    Box::new(move |ctx, emu_state| render(ctx, emu_state, &mut state))
}

fn render(
    ctx: DebugRenderContext<'_>,
    mut debug_state: &mut GenesisBasedDebugState<'_>,
    state: &mut State,
) {
    TopBottomPanel::new(TopBottomSide::Top, "gen_debug_top").show(ctx.egui_ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Memory Viewers", |ui| {
                for &memory_area in MemoryArea::ALL {
                    if !debug_state.has_memory(memory_area) {
                        continue;
                    }

                    if ui.button(memory_area.name()).clicked() {
                        if let Some(memviewer_state) = state.memory_viewers.get_mut(&memory_area) {
                            memviewer_state.open = true;
                        }
                        ui.close_kind(UiKind::Menu);
                    }
                }
            });

            ui.menu_button("Register Viewers", |ui| {
                if ui.button("VDP").clicked() {
                    state.vdp_registers_open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if matches!(debug_state, GenesisBasedDebugState::Sega32X(..)) {
                    if ui.button("32X System Registers").clicked() {
                        state.s32x_system_registers_open = true;
                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.button("32X VDP").clicked() {
                        state.s32x_vdp_registers_open = true;
                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.button("32X PWM").clicked() {
                        state.s32x_pwm_registers_open = true;
                        ui.close_kind(UiKind::Menu);
                    }
                }
            });

            ui.menu_button("Video Memory", |ui| {
                if ui.button("CRAM").clicked() {
                    state.cram.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("VRAM").clicked() {
                    state.vram.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("Sprite Attributes").clicked() {
                    state.sprite_attributes.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if ui.button("H Scroll Table").clicked() {
                    state.h_scroll.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if matches!(debug_state, GenesisBasedDebugState::Sega32X(..))
                    && ui.button("32X Palette RAM").clicked()
                {
                    state.s32x_palette.open = true;
                    ui.close_kind(UiKind::Menu);
                }
            });

            ui.menu_button("CPU Debuggers", |ui| {
                if ui.button("68000 Disassembly").clicked() {
                    state.m68k.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if matches!(debug_state, GenesisBasedDebugState::SegaCd(..))
                    && ui.button("Sub 68000 Disassembly").clicked()
                {
                    state.m68k_sub.open = true;
                    ui.close_kind(UiKind::Menu);
                }

                if matches!(debug_state, GenesisBasedDebugState::Sega32X(..)) {
                    if ui.button("SH-2 Master Disassembly").clicked() {
                        state.sh2_master.disassembly_open = true;
                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.button("SH-2 Master Breakpoints").clicked() {
                        state.sh2_master.breakpoints_open = true;
                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.button("SH-2 Slave Disassembly").clicked() {
                        state.sh2_slave.disassembly_open = true;
                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.button("SH-2 Slave Breakpoints").clicked() {
                        state.sh2_slave.breakpoints_open = true;
                        ui.close_kind(UiKind::Menu);
                    }
                }
            });
        });
    });

    render_memory_viewer_windows(
        ctx.egui_ctx,
        debug_state,
        &mut state.memory_viewers,
        &mut state.memory_edit_hook,
    );

    render_vdp_registers_window(ctx.egui_ctx, debug_state, &mut state.vdp_registers_open);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    render_cram_window(ctx.egui_ctx, screen_width, debug_state, &mut state.cram);
    render_vram_window(ctx.egui_ctx, screen_width, debug_state, &mut state.vram);
    render_h_scroll_window(ctx.egui_ctx, debug_state, &mut state.h_scroll);
    render_sprite_attributes_window(ctx.egui_ctx, debug_state, &mut state.sprite_attributes);

    match debug_state {
        GenesisBasedDebugState::Genesis(debug_state) => {
            if let Some(cartridge) = debug_state.cartridge() {
                let m68k = debug_state.m68k();
                let memory_map =
                    Genesis68kMemoryMap { cartridge, working_ram: debug_state.working_ram() };
                m68kdebug::render_disassembly_window(
                    ctx.egui_ctx,
                    m68k,
                    &memory_map,
                    &mut state.m68k,
                );
            }
        }
        GenesisBasedDebugState::SegaCd(debug_state) => {
            let memory_map = SegaCdMainMemoryMap::new(debug_state);
            let m68k = debug_state.genesis.m68k();
            m68kdebug::render_disassembly_window(ctx.egui_ctx, m68k, &memory_map, &mut state.m68k);

            let sub_memory_map = SegaCdSubMemoryMap::new(debug_state);
            let sub_cpu = debug_state.sub_cpu();
            m68kdebug::render_disassembly_window(
                ctx.egui_ctx,
                sub_cpu,
                &sub_memory_map,
                &mut state.m68k_sub,
            );
        }
        GenesisBasedDebugState::Sega32X(debug_state, ..) => {
            if let Some(memory_map) = S32XMemoryMap::new(debug_state) {
                let m68k = debug_state.genesis.m68k();
                m68kdebug::render_disassembly_window(
                    ctx.egui_ctx,
                    m68k,
                    &memory_map,
                    &mut state.m68k,
                );
            }
        }
    }

    if let GenesisBasedDebugState::Sega32X(debug_state, command_sender, break_status) =
        &mut debug_state
    {
        render_32x_palette_window(ctx.egui_ctx, debug_state, &mut state.s32x_palette);
        render_32x_system_registers_window(
            ctx.egui_ctx,
            debug_state,
            &mut state.s32x_system_registers_open,
        );
        render_32x_vdp_registers_window(
            ctx.egui_ctx,
            debug_state,
            &mut state.s32x_vdp_registers_open,
        );
        render_32x_pwm_registers_window(
            ctx.egui_ctx,
            debug_state,
            &mut state.s32x_pwm_registers_open,
        );

        sh2debug::render_disassembly_window(
            ctx.egui_ctx,
            debug_state,
            &mut state.sh2_master,
            command_sender,
            *break_status,
        );
        sh2debug::render_disassembly_window(
            ctx.egui_ctx,
            debug_state,
            &mut state.sh2_slave,
            command_sender,
            *break_status,
        );

        sh2debug::render_breakpoints_window(ctx.egui_ctx, &mut state.sh2_master, command_sender);
        sh2debug::render_breakpoints_window(ctx.egui_ctx, &mut state.sh2_slave, command_sender);
    }
}

fn render_memory_viewer_windows(
    egui_ctx: &egui::Context,
    emu_state: &mut GenesisBasedDebugState<'_>,
    memory_viewer_states: &mut HashMap<MemoryArea, MemoryViewerState>,
    memory_edit_hook: &mut dyn FnMut(MemoryArea, usize, u8),
) {
    for (&memory_area, state) in memory_viewer_states.iter_mut() {
        if let Some(memory) = emu_state.debug_memory_view(memory_area) {
            let mut memory = DebugViewWithWriteHook::new(
                memory,
                Box::new(|address, value| memory_edit_hook(memory_area, address, value)),
            );
            memviewer::render(egui_ctx, &mut memory, state);
        }
    }
}

fn render_cram_window(
    ctx: &egui::Context,
    screen_width: f32,
    emu_state: &mut GenesisBasedDebugState<'_>,
    state: &mut CramWindowState,
) {
    Window::new("CRAM").default_width(screen_width * 0.95).open(&mut state.open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.modifier, ColorModifier::None, "Normal");
            ui.radio_value(&mut state.modifier, ColorModifier::Shadow, "Shadowed");
            ui.radio_value(&mut state.modifier, ColorModifier::Highlight, "Highlighted");
        });

        emu_state.copy_cram(state.buffer.as_mut_slice(), state.modifier);

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
    emu_state: &mut GenesisBasedDebugState<'_>,
    state: &mut VramWindowState,
) {
    Window::new("VRAM").default_width(screen_width * 0.95).open(&mut state.open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Palette");

            for i in 0..4 {
                ui.radio_value(&mut state.palette, i, format!("{i}"));
            }
        });

        emu_state.copy_vram(state.buffer.as_mut_slice(), state.palette, 64);

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
    emu_state: &mut GenesisBasedDebugState<'_>,
    state: &mut HScrollWindowState,
) {
    Window::new("H Scroll Table").default_width(200.0).open(&mut state.open).show(ctx, |ui| {
        emu_state.copy_h_scroll(state.buffer.as_mut_slice());

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
    emu_state: &mut GenesisBasedDebugState<'_>,
    state: &mut SpriteAttributesWindowState,
) {
    Window::new("Sprite Attribute Table").open(&mut state.open).default_width(500.0).show(
        ctx,
        |ui| {
            let CopySpriteAttributesResult { sprite_table_len, top_left_x, top_left_y } =
                emu_state.copy_sprite_attributes(state.buffer.as_mut_slice());

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
    emu_state: &mut Sega32XDebugState,
    state: &mut S32XPaletteRamState,
) {
    Window::new("32X Palette RAM").open(&mut state.open).default_size([500.0, 550.0]).show(
        ctx,
        |ui| {
            emu_state.copy_palette(state.buffer.as_mut_slice());

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
    emu_state: &mut GenesisBasedDebugState<'_>,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "VDP Registers", open, |ui| {
        emu_state.dump_vdp_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_system_registers_window(
    ctx: &egui::Context,
    emu_state: &mut Sega32XDebugState,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X System Registers", open, |ui| {
        emu_state.dump_32x_system_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_vdp_registers_window(
    ctx: &egui::Context,
    emu_state: &mut Sega32XDebugState,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X VDP Registers", open, |ui| {
        emu_state.dump_32x_vdp_registers(debug::dump_registers_callback(ui));
    });
}

fn render_32x_pwm_registers_window(
    ctx: &egui::Context,
    emu_state: &mut Sega32XDebugState,
    open: &mut bool,
) {
    debug::render_registers_window(ctx, "32X PWM Registers", open, |ui| {
        emu_state.dump_pwm_registers(debug::dump_registers_callback(ui));
    });
}

struct GenesisDebugRunnerProcess {
    state_sender: SharedVarSender<GenesisDebugState>,
    debugger: GenesisDebugger,
}

impl DebuggerRunnerProcess<GenesisEmulator> for GenesisDebugRunnerProcess {
    fn run(
        &mut self,
        emulator: &mut GenesisEmulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.debugger.process_commands(&mut emulator.as_debug_view());
        self.state_sender.update(emulator.to_debug_state());

        Ok(())
    }
}

struct GenesisDebugMainProcess {
    state_receiver: SharedVarReceiver<GenesisDebugState>,
    render_fn: Box<GenesisDebugRenderFn>,
}

impl DebuggerMainProcess for GenesisDebugMainProcess {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let Some(state) = self.state_receiver.get() else { return Ok(()) };
        (self.render_fn)(ctx, &mut GenesisBasedDebugState::Genesis(state));

        Ok(())
    }
}

pub fn genesis_debug_fn()
-> (Box<dyn DebuggerRunnerProcess<GenesisEmulator>>, Box<dyn DebuggerMainProcess>) {
    let (state_sender, state_receiver) = jgenesis_common::sync::new_shared_var();
    let (debugger, command_sender) = GenesisDebugger::new();

    let memory_edit_hook = Box::new(move |memory_area, address, value| {
        if let MemoryArea::Genesis(memory_area) = memory_area {
            let _ =
                command_sender.send(GenesisDebugCommand::EditMemory(memory_area, address, value));
        }
    });

    let runner_process = GenesisDebugRunnerProcess { state_sender, debugger };
    let main_process =
        GenesisDebugMainProcess { state_receiver, render_fn: render_fn(memory_edit_hook) };

    (Box::new(runner_process), Box::new(main_process))
}

struct SegaCdDebugRunnerProcess {
    state_sender: SharedVarSender<SegaCdDebugState>,
    debugger: SegaCdDebugger,
}

impl DebuggerRunnerProcess<SegaCdEmulator> for SegaCdDebugRunnerProcess {
    fn run(
        &mut self,
        emulator: &mut SegaCdEmulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.debugger.process_commands(&mut emulator.as_debug_view());
        self.state_sender.update(emulator.to_debug_state());

        Ok(())
    }
}

struct SegaCdDebugMainProcess {
    state_receiver: SharedVarReceiver<SegaCdDebugState>,
    render_fn: Box<GenesisDebugRenderFn>,
}

impl DebuggerMainProcess for SegaCdDebugMainProcess {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let Some(state) = self.state_receiver.get() else { return Ok(()) };
        (self.render_fn)(ctx, &mut GenesisBasedDebugState::SegaCd(state));

        Ok(())
    }
}

pub fn sega_cd_debug_fn()
-> (Box<dyn DebuggerRunnerProcess<SegaCdEmulator>>, Box<dyn DebuggerMainProcess>) {
    let (state_sender, state_receiver) = jgenesis_common::sync::new_shared_var();
    let (debugger, command_sender) = SegaCdDebugger::new();

    let memory_edit_hook = Box::new(move |memory_area, address, value| match memory_area {
        MemoryArea::Genesis(memory_area) => {
            let _ = command_sender.send(SegaCdDebugCommand::EditGenesisMemory(
                memory_area,
                address,
                value,
            ));
        }
        MemoryArea::SegaCd(memory_area) => {
            let _ = command_sender.send(SegaCdDebugCommand::EditSegaCdMemory(
                memory_area,
                address,
                value,
            ));
        }
        MemoryArea::Sega32X(_) => {}
    });

    let runner_process = SegaCdDebugRunnerProcess { state_sender, debugger };
    let main_process =
        SegaCdDebugMainProcess { state_receiver, render_fn: render_fn(memory_edit_hook) };

    (Box::new(runner_process), Box::new(main_process))
}

struct Sega32XDebugRunnerProcess {
    state_sender: SharedVarSender<Sega32XDebugState>,
    debugger: Sega32XDebugger,
}

impl DebuggerRunnerProcess<Sega32XEmulator> for Sega32XDebugRunnerProcess {
    fn run(
        &mut self,
        emulator: &mut Sega32XEmulator,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.debugger.process_commands(&mut emulator.as_debug_view());
        self.state_sender.update(emulator.to_debug_state());

        Ok(())
    }

    fn run_emulator_till_next_frame(
        &mut self,
        emulator: &mut Sega32XEmulator,
        renderer: &mut ThreadedRenderer,
        audio_output: &mut SdlAudioOutput,
        input_poller: &mut ThreadedInputPoller<GenesisInputs>,
        save_writer: &mut FsSaveWriter,
    ) -> Result<(), RunTillNextErr<Sega32XEmulator>> {
        while emulator.debug_tick(
            renderer,
            audio_output,
            input_poller,
            save_writer,
            &mut self.debugger,
        )? != TickEffect::FrameRendered
        {}

        Ok(())
    }
}

struct Sega32XDebugMainProcess {
    debugger_handle: Sega32XDebuggerHandle,
    state_receiver: SharedVarReceiver<Sega32XDebugState>,
    render_fn: Box<GenesisDebugRenderFn>,
}

impl DebuggerMainProcess for Sega32XDebugMainProcess {
    fn run(
        &mut self,
        ctx: DebugRenderContext<'_>,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let break_status = self.debugger_handle.take_break_status();
        let Some(state) = self.state_receiver.get() else { return Ok(()) };
        (self.render_fn)(
            ctx,
            &mut GenesisBasedDebugState::Sega32X(
                state,
                &self.debugger_handle.command_sender,
                break_status,
            ),
        );

        Ok(())
    }
}

pub fn sega_32x_debug_fn()
-> (Box<dyn DebuggerRunnerProcess<Sega32XEmulator>>, Box<dyn DebuggerMainProcess>) {
    let (state_sender, state_receiver) = jgenesis_common::sync::new_shared_var();
    let (debugger, debugger_handle) = Sega32XDebugger::new(state_sender.clone());

    let memory_edit_hook = {
        let command_sender = debugger_handle.command_sender.clone();

        Box::new(move |memory_area, address, value| match memory_area {
            MemoryArea::Genesis(memory_area) => {
                let _ = command_sender.send(Sega32XDebugCommand::EditGenesisMemory(
                    memory_area,
                    address,
                    value,
                ));
            }
            MemoryArea::Sega32X(memory_area) => {
                let _ = command_sender.send(Sega32XDebugCommand::Edit32XMemory(
                    memory_area,
                    address,
                    value,
                ));
            }
            MemoryArea::SegaCd(_) => {}
        })
    };

    let runner_process = Sega32XDebugRunnerProcess { state_sender, debugger };
    let main_process = Sega32XDebugMainProcess {
        debugger_handle,
        state_receiver,
        render_fn: render_fn(memory_edit_hook),
    };

    (Box::new(runner_process), Box::new(main_process))
}
