use crate::genesis::m68kdebug::M68kDebugMemoryMap;
use crate::genesis::widgets::BreakpointsWidget;
use egui::panel::{Side, TopBottomSide};
use egui::style::ScrollStyle;
use egui::{Align, CentralPanel, Grid, RichText, SidePanel, TextEdit, TopBottomPanel, Ui, Window};
use egui_extras::{Column, TableBuilder};
use genesis_core::api::debug::{GenesisDebugState, Z80BreakStatus, Z80Breakpoint};
use z80_emu::{DisassembledInstruction, MemoryAccess, Z80};

const DISASSEMBLY_WINDOW_TITLE: &str = "Z80 Disassembly";
const BREAKPOINTS_WINDOW_TITLE: &str = "Z80 Breakpoints";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Z80BreakCommand {
    Pause,
    Resume,
    Step,
}

pub struct Z80DebugWindowState {
    pub disassembly_open: bool,
    pub disassembly_address: u16,
    pub disassembly_end_address: Option<u16>,
    pub disassembly_addr_changed: bool,
    pub jump_to_address: String,
    pub disassembly_table_offset: f32,
    pub disassembly_table_height: f32,
    pub break_status_last_frame: Z80BreakStatus,
    pub breakpoints_open: bool,
    pub breakpoints: BreakpointsWidget<u16>,
}

impl Z80DebugWindowState {
    pub fn new() -> Self {
        Self {
            disassembly_open: false,
            disassembly_address: 0,
            disassembly_end_address: None,
            disassembly_addr_changed: false,
            jump_to_address: String::new(),
            disassembly_table_offset: 0.0,
            disassembly_table_height: 1.0,
            break_status_last_frame: Z80BreakStatus::default(),
            breakpoints_open: false,
            breakpoints: BreakpointsWidget::new("z80_breakpoints"),
        }
    }

    pub fn open_disassembly_window(&mut self, ctx: &egui::Context) {
        self.disassembly_open = true;
        crate::move_to_top(ctx, DISASSEMBLY_WINDOW_TITLE);
    }

    pub fn open_breakpoints_window(&mut self, ctx: &egui::Context) {
        self.breakpoints_open = true;
        crate::move_to_top(ctx, BREAKPOINTS_WINDOW_TITLE);
    }

    fn maybe_change_disassembly_address(&mut self, address: u16) {
        if self
            .disassembly_end_address
            .is_none_or(|end_addr| !(self.disassembly_address..end_addr).contains(&address))
        {
            self.change_disassembly_address(address);
        }
    }

    fn change_disassembly_address(&mut self, address: u16) {
        self.disassembly_address = address;
        self.disassembly_addr_changed = true;
    }
}

pub trait Z80MemoryMap {
    fn peek(&self, address: u16) -> Option<u8>;
}

pub struct GenesisZ80MemoryMap<'a, M68kMap> {
    pub audio_ram: &'a [u8],
    pub z80_memory_bank: u32,
    pub m68k_map: &'a M68kMap,
}

impl<'a, M68kMap> GenesisZ80MemoryMap<'a, M68kMap>
where
    M68kMap: M68kDebugMemoryMap,
{
    pub fn new(debug_state: &'a GenesisDebugState, m68k_map: &'a M68kMap) -> Self {
        Self {
            audio_ram: debug_state.audio_ram(),
            z80_memory_bank: debug_state.z80_bank_number(),
            m68k_map,
        }
    }
}

impl<M68kMap> Z80MemoryMap for GenesisZ80MemoryMap<'_, M68kMap>
where
    M68kMap: M68kDebugMemoryMap,
{
    fn peek(&self, address: u16) -> Option<u8> {
        match address {
            0x0000..=0x3FFF => Some(self.audio_ram[(address & 0x1FFF) as usize]),
            0x8000..=0xFFFF => {
                let m68k_addr = (self.z80_memory_bank << 15) | u32::from(address & 0x7FFF);
                self.m68k_map.peek(m68k_addr).map(|word| word.to_be_bytes()[(address & 1) as usize])
            }
            _ => None,
        }
    }
}

pub fn render_disassembly_window(
    ctx: &egui::Context,
    z80: &Z80,
    memory_map: impl Z80MemoryMap,
    state: &mut Z80DebugWindowState,
    break_status: Z80BreakStatus,
    handle_command: Option<impl FnMut(Z80BreakCommand)>,
) {
    if break_status.breaking && break_status != state.break_status_last_frame {
        let mut move_to_pc = break_status.pc;
        for previous_pc in break_status.previous_pcs {
            if previous_pc > move_to_pc || previous_pc < move_to_pc.saturating_sub(16) {
                break;
            }
            move_to_pc = previous_pc;
        }

        state.maybe_change_disassembly_address(move_to_pc);
        state.disassembly_open = true;
        crate::move_to_top(ctx, DISASSEMBLY_WINDOW_TITLE);
    }
    state.break_status_last_frame = break_status;

    let mut open = state.disassembly_open;
    Window::new(DISASSEMBLY_WINDOW_TITLE)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(crate::rand_window_pos())
        .default_width(700.0)
        .show(ctx, |ui| {
            if let Some(handle_command) = handle_command {
                render_disasm_top_panel(handle_command, ui);
            }

            render_disasm_right_panel(z80, state, ui);
            render_disasm_central_panel(z80, memory_map, state, break_status, ui);
        });
    state.disassembly_open = open;
}

fn render_disasm_top_panel(mut handle_command: impl FnMut(Z80BreakCommand), ui: &mut Ui) {
    TopBottomPanel::new(TopBottomSide::Top, "z80_top_panel").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Pause").clicked() {
                handle_command(Z80BreakCommand::Pause);
            }

            if ui.button("Resume").clicked() {
                handle_command(Z80BreakCommand::Resume);
            }

            if ui.button("Step").clicked() {
                handle_command(Z80BreakCommand::Step);
            }
        });

        ui.add_space(3.0);
    });
}

fn render_disasm_right_panel(z80: &Z80, state: &mut Z80DebugWindowState, ui: &mut Ui) {
    SidePanel::new(Side::Right, "z80_right_panel").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            let text_resp =
                ui.add(TextEdit::singleline(&mut state.jump_to_address).desired_width(40.0));
            let button_resp = ui.button("Jump to address");

            let should_jump = button_resp.clicked()
                || (text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if should_jump && let Ok(address) = u16::from_str_radix(&state.jump_to_address, 16) {
                state.change_disassembly_address(address);
            }
        });

        ui.add_space(3.0);

        if ui.button("Jump to PC").clicked() {
            state.change_disassembly_address(z80.pc());
        }

        ui.separator();

        let registers = z80.registers();
        Grid::new("z80_registers_grid").show(ui, |ui| {
            for ((label0, value0), (label1, value1)) in [
                (("A", registers.a), ("F", u8::from(registers.f))),
                (("B", registers.b), ("C", registers.c)),
                (("D", registers.d), ("E", registers.e)),
                (("H", registers.h), ("L", registers.l)),
            ] {
                ui.label(label0);
                ui.label(monospace_u8(value0));
                ui.label(label1);
                ui.label(monospace_u8(value1));
                ui.end_row();
            }

            ui.label("HL");
            ui.label(monospace_u16(u16::from_be_bytes([registers.h, registers.l])));
            ui.label("SP");
            ui.label(monospace_u16(registers.sp));
            ui.end_row();

            ui.label("IX");
            ui.label(monospace_u16(registers.ix));
            ui.label("IY");
            ui.label(monospace_u16(registers.iy));
            ui.end_row();

            ui.label("PC");
            ui.label(monospace_u16(registers.pc));
            ui.end_row();

            ui.label("IFF1");
            ui.label(monospace_bool(registers.iff1));
            ui.label("IFF2");
            ui.label(monospace_bool(registers.iff2));
            ui.end_row();

            ui.label("I");
            ui.label(monospace_u8(registers.i));
            ui.label("R");
            ui.label(monospace_u8(registers.r));
            ui.end_row();

            for ((label0, value0), (label1, value1)) in [
                (("A'", registers.ap), ("F'", u8::from(registers.fp))),
                (("B'", registers.bp), ("C'", registers.cp)),
                (("D'", registers.dp), ("E'", registers.ep)),
                (("H'", registers.hp), ("L'", registers.lp)),
            ] {
                ui.label(label0);
                ui.label(monospace_u8(value0));
                ui.label(label1);
                ui.label(monospace_u8(value1));
                ui.end_row();
            }
        });

        ui.add_space(3.0);

        ui.label(
            RichText::new(format!(
                "C={} N={} V={} H={} Z={} S={}",
                u8::from(registers.f.carry),
                u8::from(registers.f.subtract),
                u8::from(registers.f.overflow),
                u8::from(registers.f.half_carry),
                u8::from(registers.f.zero),
                u8::from(registers.f.sign),
            ))
            .monospace(),
        );
    });
}

fn render_disasm_central_panel(
    z80: &Z80,
    memory_map: impl Z80MemoryMap,
    state: &mut Z80DebugWindowState,
    break_status: Z80BreakStatus,
    ui: &mut Ui,
) {
    let ctx = ui.ctx().clone();

    CentralPanel::default().show_inside(ui, |ui| {
        ui.spacing_mut().scroll = ScrollStyle { bar_width: 10.0, ..ScrollStyle::solid() };

        let mut table_builder = TableBuilder::new(ui)
            .column(Column::auto().at_least(60.0))
            .column(Column::auto().at_least(150.0))
            .column(Column::remainder())
            .striped(true);

        if state.disassembly_addr_changed {
            state.disassembly_addr_changed = false;
            table_builder = table_builder.scroll_to_row(0, Some(Align::Min));
        } else if crate::window_on_top(&ctx, DISASSEMBLY_WINDOW_TITLE) {
            let keys = crate::scroll_keys_pressed(&ctx);
            if let Some(offset) = keys.relative_scroll_offset(state.disassembly_table_height) {
                table_builder =
                    table_builder.vertical_scroll_offset(state.disassembly_table_offset + offset);
            }
        }

        let z80_pc = if break_status.breaking { break_status.pc } else { z80.pc() };

        let scroll_output = table_builder.body(|mut body| {
            let mut pc = state.disassembly_address;
            let mut instruction = DisassembledInstruction::new();

            for _ in 0..100 {
                body.row(15.0, |mut row| {
                    if pc == z80_pc {
                        row.set_selected(true);
                    }

                    row.col(|ui| {
                        ui.label(monospace_u16(pc));
                    });

                    z80_emu::disassemble_into(&mut instruction, pc, || {
                        let byte = memory_map.peek(pc).unwrap_or(0xFF);
                        pc = pc.wrapping_add(1);
                        byte
                    });

                    row.col(|ui| {
                        ui.label(RichText::new(&instruction.text).monospace());
                    });

                    row.col(|ui| {
                        if let Some(memory_access) = instruction.memory_access {
                            let address = memory_access.resolve_address(z80);
                            let value =
                                memory_map.peek(address).map(|value| format!("0x{value:02X}"));

                            match (memory_access, value) {
                                (MemoryAccess::Absolute(..), Some(value)) => {
                                    ui.label(monospace_text(format!("; = {value}")));
                                }
                                (MemoryAccess::Absolute(..), None) => {}
                                (MemoryAccess::Indirect(..), Some(value)) => {
                                    ui.label(monospace_text(format!("; ${address:04X} = {value}")));
                                }
                                (MemoryAccess::Indirect(..), None) => {
                                    ui.label(monospace_text(format!("; ${address:04X}")));
                                }
                            }
                        }
                    });
                });
            }

            state.disassembly_end_address = Some(pc);
        });
        state.disassembly_table_offset = scroll_output.state.offset.y;
        state.disassembly_table_height = scroll_output.inner_rect.height();
    });
}

pub fn render_breakpoints_window(
    ctx: &egui::Context,
    state: &mut Z80DebugWindowState,
    update_breakpoints: impl FnOnce(Vec<Z80Breakpoint>),
) {
    let mut open = state.breakpoints_open;
    Window::new(BREAKPOINTS_WINDOW_TITLE)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(crate::rand_window_pos())
        .show(ctx, |ui| {
            state.breakpoints.render(ui, |breakpoints| {
                let z80_breakpoints = breakpoints
                    .iter()
                    .map(|breakpoint| Z80Breakpoint {
                        start_address: breakpoint.start_address,
                        end_address: breakpoint.end_address,
                        read: breakpoint.read,
                        write: breakpoint.write,
                        execute: breakpoint.execute,
                    })
                    .collect();
                update_breakpoints(z80_breakpoints);
            });
        });
    state.breakpoints_open = open;
}

fn monospace_text(value: impl Into<String>) -> RichText {
    RichText::new(value).monospace()
}

fn monospace_bool(value: bool) -> RichText {
    RichText::new(["0", "1"][usize::from(value)]).monospace()
}

fn monospace_u8(value: u8) -> RichText {
    RichText::new(format!("{value:02X}")).monospace()
}

fn monospace_u16(value: u16) -> RichText {
    RichText::new(format!("{value:04X}")).monospace()
}
