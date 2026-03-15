use egui::panel::{Side, TopBottomSide};
use egui::{Align, FontFamily, Grid, RichText, TextEdit, Window};
use egui_extras::{Column, TableBuilder};
use genesis_core::api::debug::GenesisMemoryArea;
use s32x_core::WhichCpu;
use s32x_core::api::debug::{
    Sega32XDebugCommand, Sega32XDebugState, Sh2BreakStatus, Sh2Breakpoint,
};
use sh2_emu::Sh2;
use std::ops::Range;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisassemblyArea {
    CartridgeRom { cached: bool },
    Sdram { cached: bool },
    Cache,
}

impl DisassemblyArea {
    fn address_range(self) -> Range<usize> {
        match self {
            Self::CartridgeRom { cached } => {
                let start_address = 0x02000000 | (usize::from(!cached) << 29);
                start_address..start_address + 0x400000
            }
            Self::Sdram { cached } => {
                let start_address = 0x06000000 | (usize::from(!cached) << 29);
                start_address..start_address + 0x40000
            }
            Self::Cache => 0xC0000000..0xC0001000,
        }
    }

    fn from_address(address: u32) -> Option<Self> {
        match address {
            0x02000000..=0x023FFFFF => Some(Self::CartridgeRom { cached: true }),
            0x22000000..=0x223FFFFF => Some(Self::CartridgeRom { cached: false }),
            0x06000000..=0x0603FFFF => Some(Self::Sdram { cached: true }),
            0x26000000..=0x2603FFFF => Some(Self::Sdram { cached: false }),
            0xC0000000..=0xC0000FFF => Some(Self::Cache),
            _ => None,
        }
    }

    fn read_address(self, address: u32, cpu: &Sh2, debug_state: &mut Sega32XDebugState) -> u16 {
        match self {
            Self::Sdram { cached } => {
                if cached && let Some(word) = cpu.peek_cache(address) {
                    return word;
                }

                debug_state.sdram.get(((address & 0x01FFFFFF) >> 1) as usize).copied().unwrap_or(0)
            }
            Self::CartridgeRom { cached } => {
                if cached && let Some(word) = cpu.peek_cache(address) {
                    return word;
                }

                let cartridge_addr = (address & 0x3FFFFF & !1) as usize;
                let rom_view = debug_state.genesis.memory_view(GenesisMemoryArea::CartridgeRom);
                let msb = rom_view.read(cartridge_addr);
                let lsb = rom_view.read(cartridge_addr + 1);
                u16::from_be_bytes([msb, lsb])
            }
            Self::Cache => cpu.peek_data_array(address),
        }
    }
}

pub struct Sh2DebugWindowState {
    pub which: WhichCpu,
    pub disassembly_open: bool,
    pub breakpoints_open: bool,
    pub disassembly_area: DisassemblyArea,
    pub disasm_scroll_to_row: Option<usize>,
    pub breakpoints: Vec<Sh2Breakpoint>,
    pub breakpoint_start_addr: String,
    pub breakpoint_end_addr: String,
    pub breakpoint_read: bool,
    pub breakpoint_write: bool,
    pub breakpoint_exec: bool,
}

impl Sh2DebugWindowState {
    pub fn new(which: WhichCpu) -> Self {
        Self {
            which,
            disassembly_open: false,
            breakpoints_open: false,
            disassembly_area: DisassemblyArea::Sdram { cached: true },
            disasm_scroll_to_row: None,
            breakpoints: Vec::new(),
            breakpoint_start_addr: String::new(),
            breakpoint_end_addr: String::new(),
            breakpoint_read: false,
            breakpoint_write: false,
            breakpoint_exec: false,
        }
    }

    fn try_jump_to_address(&mut self, address: u32) {
        let Some(area) = DisassemblyArea::from_address(address) else { return };

        self.disassembly_area = area;
        self.disasm_scroll_to_row = Some(((address as usize) - area.address_range().start) / 2);
    }
}

trait WhichExt {
    fn disassembly_window_title(self) -> &'static str;

    fn breakpoints_window_title(self) -> &'static str;
}

impl WhichExt for WhichCpu {
    fn disassembly_window_title(self) -> &'static str {
        match self {
            WhichCpu::Master => "Master SH-2 Disassembly",
            WhichCpu::Slave => "Slave SH-2 Disassembly",
        }
    }

    fn breakpoints_window_title(self) -> &'static str {
        match self {
            WhichCpu::Master => "Master SH-2 Breakpoints",
            WhichCpu::Slave => "Slave SH-2 Breakpoints",
        }
    }
}

pub fn render_disassembly_window(
    ctx: &egui::Context,
    debug_state: &mut Sega32XDebugState,
    window_state: &mut Sh2DebugWindowState,
    command_sender: &Sender<Sega32XDebugCommand>,
    break_status: Sh2BreakStatus<bool>,
) {
    let sh2 = match window_state.which {
        WhichCpu::Master => debug_state.sh2_master.clone(),
        WhichCpu::Slave => debug_state.sh2_slave.clone(),
    };

    let window_title = window_state.which.disassembly_window_title();

    if break_status.get(window_state.which) {
        window_state.try_jump_to_address(sh2.pc());
        window_state.disassembly_open = true;
        // TODO focus window?
    }

    let mut open = window_state.disassembly_open;
    Window::new(window_title).open(&mut open).resizable([true, true]).default_width(650.0).show(
        ctx,
        |ui| {
            egui::TopBottomPanel::new(TopBottomSide::Top, format!("{window_title}_top_panel"))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Pause").clicked() {
                            let _ = command_sender
                                .send(Sega32XDebugCommand::BreakPause(window_state.which));
                        }

                        if ui.button("Resume").clicked() {
                            let _ = command_sender.send(Sega32XDebugCommand::BreakResume);
                        }

                        if ui.button("Step").clicked() {
                            let _ = command_sender
                                .send(Sega32XDebugCommand::BreakStep(window_state.which));
                        }
                    });

                    ui.add_space(3.0);
                });

            egui::SidePanel::new(Side::Right, format!("{window_title}_left_panel"))
                .min_width(300.0)
                .show_inside(ui, |ui| {
                    ui.heading("Disassembly Area");

                    for (value, label) in [
                        (DisassemblyArea::Sdram { cached: true }, "SDRAM (Cached)"),
                        (DisassemblyArea::Sdram { cached: false }, "SDRAM (Uncached)"),
                        (DisassemblyArea::CartridgeRom { cached: true }, "ROM (Cached)"),
                        (DisassemblyArea::CartridgeRom { cached: false }, "ROM (Uncached)"),
                        (DisassemblyArea::Cache, "CPU Cache"),
                    ] {
                        ui.radio_value(&mut window_state.disassembly_area, value, label);
                    }

                    if ui.button("Jump to PC").clicked() {
                        window_state.try_jump_to_address(sh2.pc());
                    }

                    ui.separator();

                    let registers = sh2.registers();
                    Grid::new(format!("{window_title}_reg_grid")).num_columns(4).show(ui, |ui| {
                        for i in 0..8 {
                            for r in [i, i + 8] {
                                ui.label(format!("R{r}"));
                                ui.label(monospace_u32(registers.gpr[r]));
                                ui.label("");
                            }
                            ui.end_row();
                        }

                        ui.label("SR");
                        ui.label(monospace_u32(registers.sr.into()));
                        ui.label("");
                        ui.label("VBR");
                        ui.label(monospace_u32(registers.vbr));
                        ui.end_row();

                        ui.label("GBR");
                        ui.label(monospace_u32(registers.gbr));
                        ui.label("");
                        ui.label("PR");
                        ui.label(monospace_u32(registers.pr));
                        ui.end_row();

                        ui.label("MACH");
                        ui.label(monospace_u32(registers.mach));
                        ui.label("");
                        ui.label("MACL");
                        ui.label(monospace_u32(registers.macl));
                        ui.end_row();

                        ui.label("PC");
                        ui.label(monospace_u32(registers.pc));
                        ui.end_row();
                    });
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                let disassembly_area = window_state.disassembly_area;
                let address_range = disassembly_area.address_range();

                let mut table_builder = TableBuilder::new(ui)
                    .striped(true)
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(40.0))
                    .column(Column::remainder());

                if let Some(scroll_to_row) = window_state.disasm_scroll_to_row.take() {
                    table_builder = table_builder.scroll_to_row(scroll_to_row, Some(Align::Min));
                }

                let sh2_pc = sh2.pc() as usize;
                let pc_row_index =
                    address_range.contains(&sh2_pc).then(|| (sh2_pc - address_range.start) / 2);

                table_builder.body(|body| {
                    body.rows(15.0, (address_range.end - address_range.start) / 2, |mut row| {
                        row.set_selected(pc_row_index == Some(row.index()));

                        let address = (address_range.start + 2 * row.index()) as u32;

                        row.col(|ui| {
                            ui.label(monospace_u32(address));
                        });

                        let opcode = disassembly_area.read_address(address, &sh2, debug_state);

                        row.col(|ui| {
                            ui.label(monospace_u16(opcode));
                        });

                        row.col(|ui| {
                            ui.label(
                                RichText::new(sh2_emu::disassemble(opcode).to_ascii_lowercase())
                                    .family(FontFamily::Monospace),
                            );
                        });
                    });
                });
            });
        },
    );
    window_state.disassembly_open = open;
}

pub fn render_breakpoints_window(
    ctx: &egui::Context,
    window_state: &mut Sh2DebugWindowState,
    command_sender: &Sender<Sega32XDebugCommand>,
) {
    let window_title = window_state.which.breakpoints_window_title();

    let initial_breakpoints = window_state.breakpoints.clone();

    let mut open = window_state.breakpoints_open;
    Window::new(window_title).open(&mut open).resizable([true, true]).show(ctx, |ui| {
        if !window_state.breakpoints.is_empty() {
            Grid::new(format!("{window_title}_breakpoints")).show(ui, |ui| {
                ui.heading("Addresses");
                ui.heading("R");
                ui.heading("W");
                ui.heading("X");
                ui.label("");
                ui.end_row();

                let mut remove_idx: Option<usize> = None;
                for (i, breakpoint) in window_state.breakpoints.iter_mut().enumerate() {
                    if breakpoint.start_address == breakpoint.end_address {
                        ui.label(
                            RichText::new(format!("${:08X}", breakpoint.start_address))
                                .family(FontFamily::Monospace),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "${:08X}-${:08X}",
                                breakpoint.start_address, breakpoint.end_address
                            ))
                            .family(FontFamily::Monospace),
                        );
                    }

                    for value in
                        [&mut breakpoint.read, &mut breakpoint.write, &mut breakpoint.execute]
                    {
                        ui.checkbox(value, "");
                    }

                    if ui.button("Remove").clicked() {
                        remove_idx = Some(i);
                    }

                    ui.end_row();
                }

                if let Some(remove_idx) = remove_idx
                    && remove_idx < window_state.breakpoints.len()
                {
                    window_state.breakpoints.remove(remove_idx);
                }
            });

            ui.separator();
        }

        ui.heading("Add Breakpoint");
        ui.horizontal(|ui| {
            ui.label("$");
            if ui
                .add(
                    TextEdit::singleline(&mut window_state.breakpoint_start_addr)
                        .desired_width(80.0),
                )
                .lost_focus()
                && window_state.breakpoint_end_addr.is_empty()
            {
                window_state.breakpoint_end_addr.clone_from(&window_state.breakpoint_start_addr);
            }
            ui.label("-");
            ui.label("$");
            ui.add(TextEdit::singleline(&mut window_state.breakpoint_end_addr).desired_width(80.0));

            ui.checkbox(&mut window_state.breakpoint_read, "Read");
            ui.checkbox(&mut window_state.breakpoint_write, "Write");
            ui.checkbox(&mut window_state.breakpoint_exec, "Execute");
        });

        if ui.button("Add").clicked()
            && let Ok(start_address) = u32::from_str_radix(&window_state.breakpoint_start_addr, 16)
            && let Ok(end_address) = u32::from_str_radix(&window_state.breakpoint_end_addr, 16)
            && end_address >= start_address
        {
            window_state.breakpoints.push(Sh2Breakpoint {
                start_address,
                end_address,
                read: window_state.breakpoint_read,
                write: window_state.breakpoint_write,
                execute: window_state.breakpoint_exec,
            });

            window_state.breakpoint_start_addr.clear();
            window_state.breakpoint_end_addr.clear();
        }
    });
    window_state.breakpoints_open = open;

    if initial_breakpoints != window_state.breakpoints {
        let _ = command_sender.send(Sega32XDebugCommand::UpdateBreakpoints(
            window_state.which,
            window_state.breakpoints.clone(),
        ));
    }
}

fn monospace_u16(value: u16) -> RichText {
    RichText::new(format!("{value:04X}")).family(FontFamily::Monospace)
}

fn monospace_u32(value: u32) -> RichText {
    RichText::new(format!("{value:08X}")).family(FontFamily::Monospace)
}
