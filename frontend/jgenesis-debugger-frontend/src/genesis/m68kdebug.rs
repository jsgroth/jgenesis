use crate::genesis::widgets::{BreakpointsWidget, U24};
use egui::panel::{Side, TopBottomSide};
use egui::style::ScrollStyle;
use egui::{Align, CentralPanel, Grid, RichText, SidePanel, TextEdit, TopBottomPanel, Ui, Window};
use egui_extras::{Column, TableBuilder};
use genesis_core::api::debug::{GenesisDebugState, M68000BreakStatus, M68000Breakpoint};
use genesis_core::cartridge::Cartridge;
use jgenesis_common::num::{GetBit, U16Ext};
use m68000_emu::disassemble::{DisassembledInstruction, MemoryAccess, MemoryReadType};
use m68000_emu::{M68000, OpSize};
use s32x_core::api::debug::Sega32XDebugState;
use segacd_core::WordRam;
use segacd_core::api::debug::SegaCdDebugState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M68kBreakCommand {
    Pause,
    Resume,
    Step,
}

pub struct M68kDebugWindowState {
    pub disassembly_window_title: String,
    pub breakpoints_window_title: String,
    pub disassembly_open: bool,
    pub breakpoints_open: bool,
    pub disassemble_start: u32,
    pub disassemble_end_addr: Option<u32>,
    pub disassemble_jump_addr: String,
    pub disassemble_reset_table: bool,
    pub disassemble_table_offset: f32,
    pub disassemble_table_height: f32,
    pub break_status_last_frame: M68000BreakStatus,
    pub breakpoints: BreakpointsWidget<U24>,
}

impl M68kDebugWindowState {
    pub fn new_default_titles() -> Self {
        Self::new_with_titles("68000 Disassembly", "68000 Breakpoints")
    }

    pub fn new_with_titles(
        disassembly_window_title: impl Into<String>,
        breakpoints_window_title: impl Into<String>,
    ) -> Self {
        let disassembly_window_title = disassembly_window_title.into();
        let breakpoints_window_title = breakpoints_window_title.into();
        let breakpoints = BreakpointsWidget::new(&breakpoints_window_title);

        Self {
            disassembly_window_title,
            breakpoints_window_title,
            disassembly_open: false,
            breakpoints_open: false,
            disassemble_start: 0,
            disassemble_end_addr: None,
            disassemble_jump_addr: String::new(),
            disassemble_reset_table: false,
            disassemble_table_offset: 0.0,
            disassemble_table_height: 1.0,
            break_status_last_frame: M68000BreakStatus::default(),
            breakpoints,
        }
    }

    pub fn open_disassembly_window(&mut self, ctx: &egui::Context) {
        self.disassembly_open = true;
        crate::move_to_top(ctx, &self.disassembly_window_title);
    }

    pub fn open_breakpoints_window(&mut self, ctx: &egui::Context) {
        self.breakpoints_open = true;
        crate::move_to_top(ctx, &self.breakpoints_window_title);
    }

    fn maybe_move_disassembly_table(&mut self, address: u32) {
        if self
            .disassemble_end_addr
            .is_none_or(|end_addr| !(self.disassemble_start..end_addr).contains(&address))
        {
            self.move_disassembly_table(address);
        }
    }

    fn move_disassembly_table(&mut self, address: u32) {
        self.disassemble_start = address;
        self.disassemble_reset_table = true;
    }
}

pub trait M68kDebugMemoryMap {
    fn peek(&self, address: u32) -> Option<u16>;

    fn extra_info(&self) -> Option<(&'static str, String)> {
        None
    }
}

fn read_u16(memory: &[u8], address: usize) -> u16 {
    u16::from_be_bytes([memory[address], memory[address + 1]])
}

pub struct Genesis68kMemoryMap<'a, Medium> {
    pub medium: Medium,
    pub working_ram: &'a [u16],
    pub audio_ram: &'a [u8],
}

impl<Medium: M68kDebugMemoryMap> M68kDebugMemoryMap for Genesis68kMemoryMap<'_, Medium> {
    fn peek(&self, address: u32) -> Option<u16> {
        let address = address & 0xFFFFFF;

        match address {
            0xA00000..=0xA03FFF | 0xA08000..=0xA0BFFF => {
                let byte = self.audio_ram[(address & 0x1FFF) as usize];
                Some(u16::from_le_bytes([byte, byte]))
            }
            0xE00000..=0xFFFFFF => Some(self.working_ram[((address & 0xFFFF) >> 1) as usize]),
            _ => self.medium.peek(address),
        }
    }

    fn extra_info(&self) -> Option<(&'static str, String)> {
        self.medium.extra_info()
    }
}

pub struct CartridgeMemoryMap<'a> {
    pub cartridge: &'a Cartridge,
}

impl M68kDebugMemoryMap for CartridgeMemoryMap<'_> {
    fn peek(&self, address: u32) -> Option<u16> {
        let address = address & 0xFFFFFF;

        match address {
            0x000000..=0x7FFFFF => Some(self.cartridge.peek_word(address)),
            _ => None,
        }
    }
}

pub fn new_genesis_memory_map(
    debug_state: &GenesisDebugState,
) -> Option<Genesis68kMemoryMap<'_, CartridgeMemoryMap<'_>>> {
    Some(Genesis68kMemoryMap {
        medium: CartridgeMemoryMap { cartridge: debug_state.cartridge()? },
        working_ram: debug_state.working_ram(),
        audio_ram: debug_state.audio_ram(),
    })
}

pub struct SegaCdMainMemoryMap<'a> {
    pub bios_rom: &'a [u8],
    pub prg_ram: &'a [u8],
    pub word_ram: &'a WordRam,
    pub prg_ram_base_addr: usize,
}

impl<'a> SegaCdMainMemoryMap<'a> {
    pub fn new(debug_state: &'a SegaCdDebugState) -> Self {
        Self {
            bios_rom: debug_state.bios_rom(),
            prg_ram: debug_state.prg_ram(),
            word_ram: debug_state.word_ram(),
            prg_ram_base_addr: usize::from(debug_state.main_cpu_prg_ram_bank()) << 17,
        }
    }
}

impl M68kDebugMemoryMap for SegaCdMainMemoryMap<'_> {
    fn peek(&self, address: u32) -> Option<u16> {
        let address = address & 0xFFFFFF & !1;

        let word = match address {
            0x000000..=0x1FFFFF => {
                let relative_addr = (address & 0x1FFFF) as usize;
                if address & 0x20000 == 0 {
                    read_u16(self.bios_rom, relative_addr)
                } else {
                    let prg_ram_addr = self.prg_ram_base_addr | relative_addr;
                    read_u16(self.prg_ram, prg_ram_addr)
                }
            }
            0x200000..=0x3FFFFF => {
                let msb = self.word_ram.main_cpu_read_ram(address);
                let lsb = self.word_ram.main_cpu_read_ram(address + 1);
                u16::from_be_bytes([msb, lsb])
            }
            _ => return None,
        };

        Some(word)
    }

    fn extra_info(&self) -> Option<(&'static str, String)> {
        Some((
            "PRG RAM Bank",
            format!(
                "{} ({:05X}-{:05X})",
                self.prg_ram_base_addr >> 17,
                self.prg_ram_base_addr,
                self.prg_ram_base_addr | 0x1FFFF
            ),
        ))
    }
}

pub fn new_scd_main_memory_map(
    debug_state: &SegaCdDebugState,
) -> Genesis68kMemoryMap<'_, SegaCdMainMemoryMap<'_>> {
    Genesis68kMemoryMap {
        medium: SegaCdMainMemoryMap::new(debug_state),
        working_ram: debug_state.genesis.working_ram(),
        audio_ram: debug_state.genesis.audio_ram(),
    }
}

pub struct SegaCdSubMemoryMap<'a> {
    prg_ram: &'a [u8],
    word_ram: &'a WordRam,
}

impl<'a> SegaCdSubMemoryMap<'a> {
    pub fn new(debug_state: &'a SegaCdDebugState) -> Self {
        Self { prg_ram: debug_state.prg_ram(), word_ram: debug_state.word_ram() }
    }
}

impl M68kDebugMemoryMap for SegaCdSubMemoryMap<'_> {
    fn peek(&self, address: u32) -> Option<u16> {
        let address = address & 0x0FFFFF;

        let word = match address {
            0x00000..=0x7FFFF => read_u16(self.prg_ram, address as usize),
            0x80000..=0xBFFFF => {
                let msb = self.word_ram.sub_cpu_peek_ram(address);
                let lsb = self.word_ram.sub_cpu_peek_ram(address + 1);
                u16::from_be_bytes([msb, lsb])
            }
            _ => return None,
        };

        Some(word)
    }
}

pub struct S32X68kMemoryMap<'a> {
    pub cartridge: &'a Cartridge,
    pub banked_rom_base_addr: u32,
}

impl<'a> S32X68kMemoryMap<'a> {
    pub fn new(debug_state: &'a Sega32XDebugState) -> Option<Self> {
        let cartridge = debug_state.genesis.cartridge()?;

        Some(Self { cartridge, banked_rom_base_addr: u32::from(debug_state.m68k_rom_bank()) << 20 })
    }
}

impl M68kDebugMemoryMap for S32X68kMemoryMap<'_> {
    fn peek(&self, address: u32) -> Option<u16> {
        let address = address & 0xFFFFFF;

        let word = match address {
            0x000000..=0x3FFFFF => self.cartridge.peek_word(address),
            0x880000..=0x8FFFFF => self.cartridge.peek_word(address & 0x7FFFF),
            0x900000..=0x9FFFFF => {
                let address = self.banked_rom_base_addr | (address & 0xFFFFF);
                self.cartridge.peek_word(address)
            }
            _ => return None,
        };

        Some(word)
    }

    fn extra_info(&self) -> Option<(&'static str, String)> {
        let banked_rom_range = format!(
            "{} ({:06X}-{:06X})",
            self.banked_rom_base_addr >> 20,
            self.banked_rom_base_addr,
            self.banked_rom_base_addr | 0xFFFFF
        );
        Some(("32X ROM Bank", banked_rom_range))
    }
}

pub fn new_32x_memory_map(
    debug_state: &Sega32XDebugState,
) -> Option<Genesis68kMemoryMap<'_, S32X68kMemoryMap<'_>>> {
    Some(Genesis68kMemoryMap {
        medium: S32X68kMemoryMap::new(debug_state)?,
        working_ram: debug_state.genesis.working_ram(),
        audio_ram: debug_state.genesis.audio_ram(),
    })
}

pub fn render_disassembly_window(
    ctx: &egui::Context,
    m68k: &M68000,
    memory_map: impl M68kDebugMemoryMap,
    state: &mut M68kDebugWindowState,
    break_status: M68000BreakStatus,
    handle_command: Option<impl FnMut(M68kBreakCommand)>,
) {
    if break_status.breaking && break_status != state.break_status_last_frame {
        let mut move_to_pc = break_status.pc;
        for previous_pc in break_status.previous_pcs {
            if previous_pc > move_to_pc || previous_pc < move_to_pc.saturating_sub(16) {
                break;
            }
            move_to_pc = previous_pc;
        }

        state.maybe_move_disassembly_table(move_to_pc);
        state.disassembly_open = true;
        crate::move_to_top(ctx, &state.disassembly_window_title);
    }
    state.break_status_last_frame = break_status;

    // Prevent window from spawning partially offscreen due to large default width
    let default_pos = [50.0, crate::rand_window_pos()[1]];

    let mut open = state.disassembly_open;
    Window::new(&state.disassembly_window_title)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(default_pos)
        .default_width(750.0)
        .show(ctx, |ui| {
            if let Some(handle_command) = handle_command {
                render_disasm_top_panel(state, handle_command, ui);
            }

            render_disasm_right_panel(m68k, &memory_map, state, ui);
            render_disasm_central_panel(m68k, &memory_map, state, break_status, ui);
        });
    state.disassembly_open = open;
}

fn render_disasm_top_panel(
    state: &mut M68kDebugWindowState,
    mut handle_command: impl FnMut(M68kBreakCommand),
    ui: &mut Ui,
) {
    TopBottomPanel::new(
        TopBottomSide::Top,
        format!("{}_top_panel", state.disassembly_window_title),
    )
    .show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Pause").clicked() {
                handle_command(M68kBreakCommand::Pause);
            }

            if ui.button("Resume").clicked() {
                handle_command(M68kBreakCommand::Resume);
            }

            if ui.button("Step").clicked() {
                handle_command(M68kBreakCommand::Step);
            }
        });

        ui.add_space(5.0);
    });
}

fn render_disasm_right_panel(
    m68k: &M68000,
    memory_map: &impl M68kDebugMemoryMap,
    state: &mut M68kDebugWindowState,
    ui: &mut Ui,
) {
    SidePanel::new(Side::Right, format!("{}_right_panel", state.disassembly_window_title))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let text_resp = ui.add(
                    TextEdit::singleline(&mut state.disassemble_jump_addr).desired_width(60.0),
                );
                let button_resp = ui.button("Jump to address");

                let should_jump = button_resp.clicked()
                    || (text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if should_jump
                    && let Ok(address) = u32::from_str_radix(&state.disassemble_jump_addr, 16)
                {
                    state.move_disassembly_table(address & 0xFFFFFF & !1);
                }
            });

            ui.add_space(3.0);

            if ui.button("Jump to PC").clicked() {
                state.move_disassembly_table(m68k.pc());
            }

            ui.separator();

            let data_registers = m68k.data_registers();
            let address_registers = m68k.address_registers();
            let status_register = m68k.status_register();

            Grid::new(format!("{}_registers", state.disassembly_window_title)).show(ui, |ui| {
                for i in 0..7 {
                    ui.label(format!("D{i}"));
                    ui.label(monospace_u32(data_registers[i]));

                    ui.label(format!("A{i}"));
                    ui.label(monospace_u32(address_registers[i]));

                    ui.end_row();
                }

                ui.label("D7");
                ui.label(monospace_u32(data_registers[7]));

                ui.label("A7");
                ui.label(monospace_u32(m68k.stack_pointer()));

                ui.end_row();

                ui.label("SSP");
                ui.label(monospace_u32(m68k.supervisor_stack_pointer()));

                ui.label("USP");
                ui.label(monospace_u32(m68k.user_stack_pointer()));

                ui.end_row();

                ui.label("SR");
                ui.label(monospace_u16(status_register));

                ui.label("PC");
                ui.label(monospace_u32(m68k.pc()));

                ui.end_row();
            });

            ui.horizontal(|ui| {
                ui.label("CCR");

                ui.add_space(20.0);

                let carry: u8 = status_register.bit(0).into();
                let overflow: u8 = status_register.bit(1).into();
                let zero: u8 = status_register.bit(2).into();
                let negative: u8 = status_register.bit(3).into();
                let extend: u8 = status_register.bit(4).into();
                ui.label(
                    RichText::new(format!(
                        "C={carry} V={overflow} Z={zero} N={negative} X={extend}"
                    ))
                    .monospace(),
                );
            });

            if let Some((label, text)) = memory_map.extra_info() {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add_space(5.0);
                    ui.label(RichText::new(text).monospace());
                });
            }
        });
}

fn render_disasm_central_panel(
    m68k: &M68000,
    memory_map: &impl M68kDebugMemoryMap,
    state: &mut M68kDebugWindowState,
    break_status: M68000BreakStatus,
    ui: &mut Ui,
) {
    let ctx = ui.ctx().clone();

    CentralPanel::default().show_inside(ui, |ui| {
        ui.spacing_mut().scroll = ScrollStyle { bar_width: 10.0, ..ScrollStyle::solid() };

        let mut table_builder = TableBuilder::new(ui)
            .column(Column::auto().at_least(60.0))
            .column(Column::auto().at_least(250.0))
            .column(Column::remainder())
            .striped(true);

        if state.disassemble_reset_table {
            state.disassemble_reset_table = false;
            table_builder = table_builder.scroll_to_row(0, Some(Align::Min));
        } else if crate::window_on_top(&ctx, &state.disassembly_window_title) {
            let keys = crate::scroll_keys_pressed(&ctx);
            if let Some(offset) = keys.relative_scroll_offset(state.disassemble_table_height) {
                table_builder =
                    table_builder.vertical_scroll_offset(state.disassemble_table_offset + offset);
            }
        }

        let m68k_pc = if break_status.breaking { break_status.pc } else { m68k.pc() };

        let scroll_output = table_builder.body(|mut body| {
            let mut pc = state.disassemble_start;
            let mut instruction = DisassembledInstruction::new();

            for _ in 0..100 {
                body.row(15.0, |mut row| {
                    if pc == m68k_pc {
                        row.set_selected(true);
                    }

                    row.col(|ui| {
                        ui.label(monospace_u24(pc));
                    });

                    m68000_emu::disassemble::disassemble_into(&mut instruction, pc, || {
                        let word = memory_map.peek(pc).unwrap_or(0xFFFF);
                        pc = (pc + 2) & 0xFFFFFF;
                        word
                    });

                    row.col(|ui| {
                        ui.label(RichText::new(&instruction.text).monospace());
                    });

                    row.col(|ui| {
                        if let Some(memory_access) =
                            instruction.memory_read.or(instruction.memory_write)
                        {
                            render_memory_access_col(
                                memory_access,
                                m68k,
                                memory_map,
                                &instruction,
                                ui,
                            );
                        }
                    });
                });
            }

            state.disassemble_end_addr = Some(pc);
        });
        state.disassemble_table_offset = scroll_output.state.offset.y;
        state.disassemble_table_height = scroll_output.inner_rect.height();
    });
}

fn render_memory_access_col(
    memory_read: MemoryAccess,
    m68k: &M68000,
    memory_map: &impl M68kDebugMemoryMap,
    instruction: &DisassembledInstruction,
    ui: &mut Ui,
) {
    let (mut address, size) = memory_read.resolve_address(m68k);
    address &= 0xFFFFFF;

    let value = match size {
        OpSize::Byte => memory_map
            .peek(address)
            .map(|word| if !address.bit(0) { word.msb().into() } else { word.lsb().into() }),
        OpSize::Word => memory_map.peek(address).map(u32::from),
        OpSize::LongWord => {
            let high = memory_map.peek(address);
            let low = memory_map.peek(address.wrapping_add(2));
            match (high, low) {
                (Some(high), Some(low)) => Some(u32::from(low) | (u32::from(high) << 16)),
                (Some(high), None) => Some(u32::from(high) << 16),
                (None, Some(low)) => Some(low.into()),
                (None, None) => None,
            }
        }
    };

    let value_str = value.map(|value| match size {
        OpSize::Byte => format!("0x{value:02X}"),
        OpSize::Word => format!("0x{value:04X}"),
        OpSize::LongWord => format!("0x{value:08X}"),
    });

    match (memory_read, instruction.memory_read_type, value_str) {
        (MemoryAccess::Absolute { .. }, MemoryReadType::Read, Some(value_str)) => {
            ui.label(monospace_str(format!("; = {value_str}")));
        }
        (MemoryAccess::Absolute { .. }, ..) => {}
        (_, MemoryReadType::Read, Some(value_str)) => {
            ui.label(monospace_str(format!("; ${address:06X} = {value_str}")));
        }
        (_, MemoryReadType::Read, None) | (_, MemoryReadType::EffectiveAddress, _) => {
            ui.label(monospace_str(format!("; ${address:06X}")));
        }
    }
}

pub fn render_breakpoints_window(
    ctx: &egui::Context,
    state: &mut M68kDebugWindowState,
    mut update_breakpoints: impl FnMut(Vec<M68000Breakpoint>),
) {
    let mut open = state.breakpoints_open;
    Window::new(&state.breakpoints_window_title)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(crate::rand_window_pos())
        .show(ctx, |ui| {
            state.breakpoints.render(ui, |breakpoints| {
                let m68k_breakpoints = breakpoints
                    .iter()
                    .map(|breakpoint| M68000Breakpoint {
                        start_address: breakpoint.start_address.get(),
                        end_address: breakpoint.end_address.get(),
                        read: breakpoint.read,
                        write: breakpoint.write,
                        execute: breakpoint.execute,
                    })
                    .collect();

                update_breakpoints(m68k_breakpoints);
            });
        });
    state.breakpoints_open = open;
}

fn monospace_u16(value: u16) -> RichText {
    RichText::new(format!("{value:04X}")).monospace()
}

fn monospace_u24(value: u32) -> RichText {
    RichText::new(format!("{:06X}", value & 0xFFFFFF)).monospace()
}

fn monospace_u32(value: u32) -> RichText {
    RichText::new(format!("{value:08X}")).monospace()
}

fn monospace_str(s: impl Into<String>) -> RichText {
    RichText::new(s).monospace()
}
