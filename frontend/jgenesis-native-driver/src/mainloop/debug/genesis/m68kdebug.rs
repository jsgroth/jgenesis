use crate::mainloop::debug::genesis::widgets::{BreakpointsWidget, U24};
use egui::panel::{Side, TopBottomSide};
use egui::{
    Align, CentralPanel, Grid, Id, LayerId, Order, RichText, SidePanel, TextEdit, TopBottomPanel,
    Window,
};
use egui_extras::{Column, TableBuilder};
use genesis_core::api::debug::{M68000BreakStatus, M68000Breakpoint};
use genesis_core::cartridge::Cartridge;
use jgenesis_common::num::GetBit;
use m68000_emu::M68000;
use m68000_emu::disassemble::DisassembledInstruction;
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
    pub window_title: String,
    pub disassembly_open: bool,
    pub breakpoints_open: bool,
    pub disassemble_start: u32,
    pub disassemble_end_addr: Option<u32>,
    pub disassemble_jump_addr: String,
    pub disassemble_reset_table: bool,
    pub break_status_last_frame: M68000BreakStatus,
    pub breakpoints: BreakpointsWidget<U24>,
}

impl M68kDebugWindowState {
    pub fn new() -> Self {
        Self::new_with_title("68000 Disassembly")
    }

    pub fn new_with_title(window_title: impl Into<String>) -> Self {
        let window_title = window_title.into();
        let breakpoints_id = format!("{window_title}_breakpoints");

        Self {
            window_title,
            disassembly_open: false,
            breakpoints_open: false,
            disassemble_start: 0,
            disassemble_end_addr: None,
            disassemble_jump_addr: String::new(),
            disassemble_reset_table: false,
            break_status_last_frame: M68000BreakStatus::default(),
            breakpoints: BreakpointsWidget::new(breakpoints_id),
        }
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
    fn peek(&self, address: u32) -> u16;

    fn extra_info(&self) -> Option<(&'static str, String)> {
        None
    }
}

fn read_u16(memory: &[u8], address: usize) -> u16 {
    u16::from_be_bytes([memory[address], memory[address + 1]])
}

pub struct Genesis68kMemoryMap<'a> {
    pub cartridge: &'a Cartridge,
    pub working_ram: &'a [u16],
}

impl M68kDebugMemoryMap for Genesis68kMemoryMap<'_> {
    fn peek(&self, address: u32) -> u16 {
        let address = address & 0xFFFFFF;

        match address {
            0x000000..=0x7FFFFF => self.cartridge.peek_word(address),
            0xE00000..=0xFFFFFF => self.working_ram[((address & 0xFFFF) >> 1) as usize],
            _ => 0xFFFF,
        }
    }
}

pub struct SegaCdMainMemoryMap<'a> {
    pub bios_rom: &'a [u8],
    pub prg_ram: &'a [u8],
    pub word_ram: &'a WordRam,
    pub working_ram: &'a [u16],
    pub prg_ram_base_addr: usize,
}

impl<'a> SegaCdMainMemoryMap<'a> {
    pub fn new(debug_state: &'a SegaCdDebugState) -> Self {
        Self {
            bios_rom: debug_state.bios_rom(),
            prg_ram: debug_state.prg_ram(),
            word_ram: debug_state.word_ram(),
            working_ram: debug_state.genesis.working_ram(),
            prg_ram_base_addr: usize::from(debug_state.main_cpu_prg_ram_bank()) << 17,
        }
    }
}

impl M68kDebugMemoryMap for SegaCdMainMemoryMap<'_> {
    fn peek(&self, address: u32) -> u16 {
        let address = address & 0xFFFFFF & !1;

        match address {
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
            0xE00000..=0xFFFFFF => self.working_ram[((address & 0xFFFF) >> 1) as usize],
            _ => 0xFFFF,
        }
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
    fn peek(&self, address: u32) -> u16 {
        let address = address & 0x0FFFFF;

        match address {
            0x00000..=0x7FFFF => read_u16(self.prg_ram, address as usize),
            0x80000..=0xBFFFF => {
                let msb = self.word_ram.sub_cpu_peek_ram(address);
                let lsb = self.word_ram.sub_cpu_peek_ram(address + 1);
                u16::from_be_bytes([msb, lsb])
            }
            _ => 0xFFFF,
        }
    }
}

pub struct S32XMemoryMap<'a> {
    pub cartridge: &'a Cartridge,
    pub working_ram: &'a [u16],
    pub banked_rom_base_addr: u32,
}

impl<'a> S32XMemoryMap<'a> {
    pub fn new(debug_state: &'a Sega32XDebugState) -> Option<Self> {
        let cartridge = debug_state.genesis.cartridge()?;

        Some(Self {
            cartridge,
            working_ram: debug_state.genesis.working_ram(),
            banked_rom_base_addr: u32::from(debug_state.m68k_rom_bank()) << 20,
        })
    }
}

impl M68kDebugMemoryMap for S32XMemoryMap<'_> {
    fn peek(&self, address: u32) -> u16 {
        let address = address & 0xFFFFFF;

        match address {
            0x000000..=0x3FFFFF => self.cartridge.peek_word(address),
            0x880000..=0x8FFFFF => self.cartridge.peek_word(address & 0x7FFFF),
            0x900000..=0x9FFFFF => {
                let address = self.banked_rom_base_addr | (address & 0xFFFFF);
                self.cartridge.peek_word(address)
            }
            0xE00000..=0xFFFFFF => self.working_ram[((address & 0xFFFF) >> 1) as usize],
            _ => 0xFFFF,
        }
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

pub fn render_disassembly_window<MemoryMap: M68kDebugMemoryMap>(
    ctx: &egui::Context,
    m68k: &M68000,
    memory_map: &MemoryMap,
    state: &mut M68kDebugWindowState,
    break_status: M68000BreakStatus,
    handle_command: Option<&mut dyn FnMut(M68kBreakCommand)>,
) {
    if break_status.breaking && break_status != state.break_status_last_frame {
        state.maybe_move_disassembly_table(break_status.pc);
        state.disassembly_open = true;
        ctx.move_to_top(LayerId::new(Order::Middle, Id::new(&state.window_title)));
    }
    state.break_status_last_frame = break_status;

    let mut open = state.disassembly_open;
    Window::new(&state.window_title)
        .open(&mut open)
        .resizable([true, true])
        .default_width(650.0)
        .show(ctx, |ui| {
            if let Some(handle_command) = handle_command {
                TopBottomPanel::new(
                    TopBottomSide::Top,
                    format!("{}_top_panel", state.window_title),
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

            SidePanel::new(Side::Right, format!("{}_right_panel", state.window_title)).show_inside(
                ui,
                |ui| {
                    ui.horizontal(|ui| {
                        let text_resp = ui.add(
                            TextEdit::singleline(&mut state.disassemble_jump_addr)
                                .desired_width(60.0),
                        );
                        let button_resp = ui.button("Jump to address");

                        let should_jump = button_resp.clicked()
                            || (text_resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if should_jump
                            && let Ok(address) =
                                u32::from_str_radix(&state.disassemble_jump_addr, 16)
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
                    let supervisor = status_register.bit(13);

                    Grid::new(format!("{}_registers", state.window_title)).show(ui, |ui| {
                        for i in 0..7 {
                            ui.label(format!("D{i}"));
                            ui.label(monospace_u32(data_registers[i]));

                            ui.label(format!("A{i}"));
                            ui.label(monospace_u32(address_registers[i]));

                            ui.end_row();
                        }

                        ui.label("D7");
                        ui.label(monospace_u32(data_registers[7]));

                        let a7 = if supervisor {
                            m68k.supervisor_stack_pointer()
                        } else {
                            m68k.user_stack_pointer()
                        };
                        ui.label("A7");
                        ui.label(monospace_u32(a7));

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
                },
            );

            CentralPanel::default().show_inside(ui, |ui| {
                let mut table_builder = TableBuilder::new(ui)
                    .column(Column::auto().at_least(60.0))
                    .column(Column::remainder())
                    .striped(true);

                if state.disassemble_reset_table {
                    state.disassemble_reset_table = false;
                    table_builder = table_builder.scroll_to_row(0, Some(Align::Min));
                }

                let m68k_pc = if break_status.breaking { break_status.pc } else { m68k.pc() };

                table_builder.body(|mut body| {
                    let mut pc = state.disassemble_start;
                    let mut disassembled_instruction = DisassembledInstruction::new();

                    for _ in 0..100 {
                        body.row(15.0, |mut row| {
                            if pc == m68k_pc {
                                row.set_selected(true);
                            }

                            row.col(|ui| {
                                ui.label(monospace_u24(pc));
                            });

                            m68000_emu::disassemble::disassemble_into(
                                &mut disassembled_instruction,
                                pc,
                                || {
                                    let word = memory_map.peek(pc);
                                    pc = (pc + 2) & 0xFFFFFF;
                                    word
                                },
                            );

                            row.col(|ui| {
                                ui.label(RichText::new(&disassembled_instruction.text).monospace());
                            });
                        });
                    }

                    state.disassemble_end_addr = Some(pc);
                });
            });
        });
    state.disassembly_open = open;
}

pub fn render_breakpoints_window(
    ctx: &egui::Context,
    state: &mut M68kDebugWindowState,
    mut update_breakpoints: impl FnMut(Vec<M68000Breakpoint>),
) {
    let mut open = state.breakpoints_open;
    Window::new("68000 Breakpoints").open(&mut open).resizable([true, true]).show(ctx, |ui| {
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
