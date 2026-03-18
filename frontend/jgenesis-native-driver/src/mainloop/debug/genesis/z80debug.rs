use egui::panel::Side;
use egui::{Align, CentralPanel, Grid, RichText, SidePanel, TextEdit, Window};
use egui_extras::{Column, TableBuilder};
use z80_emu::{DisassembledInstruction, Z80};

pub struct Z80DebugWindowState {
    pub open: bool,
    pub disassembly_address: u16,
    pub disassembly_addr_changed: bool,
    pub jump_to_address: String,
}

impl Z80DebugWindowState {
    pub fn new() -> Self {
        Self {
            open: false,
            disassembly_address: 0,
            disassembly_addr_changed: false,
            jump_to_address: String::new(),
        }
    }

    fn change_disassembly_address(&mut self, address: u16) {
        self.disassembly_address = address;
        self.disassembly_addr_changed = true;
    }
}

pub trait Z80MemoryMap {
    fn peek(&self, address: u16) -> u8;
}

pub struct GenesisZ80MemoryMap<'a> {
    pub audio_ram: &'a [u8],
}

impl<'a> GenesisZ80MemoryMap<'a> {
    pub fn new(audio_ram: &'a [u8]) -> Self {
        Self { audio_ram }
    }
}

impl Z80MemoryMap for GenesisZ80MemoryMap<'_> {
    fn peek(&self, address: u16) -> u8 {
        self.audio_ram.get(address as usize).copied().unwrap_or(0xFF)
    }
}

pub fn render_disassembly_window(
    ctx: &egui::Context,
    z80: &Z80,
    memory_map: impl Z80MemoryMap,
    state: &mut Z80DebugWindowState,
) {
    let mut open = state.open;
    Window::new("Z80 Disassembly")
        .open(&mut open)
        .resizable([true, true])
        .default_width(650.0)
        .show(ctx, |ui| {
            SidePanel::new(Side::Right, "z80_right_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let text_resp = ui
                        .add(TextEdit::singleline(&mut state.jump_to_address).desired_width(40.0));
                    let button_resp = ui.button("Jump to address");

                    let should_jump = button_resp.clicked()
                        || (text_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if should_jump
                        && let Ok(address) = u16::from_str_radix(&state.jump_to_address, 16)
                    {
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

            CentralPanel::default().show_inside(ui, |ui| {
                let mut table_builder = TableBuilder::new(ui)
                    .column(Column::auto().at_least(60.0))
                    .column(Column::remainder())
                    .striped(true);

                if state.disassembly_addr_changed {
                    state.disassembly_addr_changed = false;
                    table_builder = table_builder.scroll_to_row(0, Some(Align::Min));
                }

                table_builder.body(|mut body| {
                    let mut pc = state.disassembly_address;
                    let mut instruction = DisassembledInstruction::new();

                    for _ in 0..100 {
                        body.row(15.0, |mut row| {
                            if pc == z80.pc() {
                                row.set_selected(true);
                            }

                            row.col(|ui| {
                                ui.label(monospace_u16(pc));
                            });

                            z80_emu::disassemble_into(&mut instruction, pc, || {
                                let byte = memory_map.peek(pc);
                                pc = pc.wrapping_add(1);
                                byte
                            });

                            row.col(|ui| {
                                ui.label(RichText::new(&instruction.text).monospace());
                            });
                        });
                    }
                });
            });
        });
    state.open = open;
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
