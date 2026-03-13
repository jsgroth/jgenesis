use egui::panel::Side;
use egui::{Align, FontFamily, Grid, RichText, TextEdit, Window};
use egui_extras::{Column, TableBuilder};
use genesis_core::api::debug::GenesisMemoryArea;
use s32x_core::WhichCpu;
use s32x_core::api::debug::Sega32XDebugState;
use sh2_emu::Sh2;
use std::ops::Range;

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

                debug_state.sdram.get((address & 0x01FFFFFF) as usize).copied().unwrap_or(0)
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

pub struct DisassemblyWindowState {
    pub which: WhichCpu,
    pub open: bool,
    pub area: DisassemblyArea,
    pub scroll_to_row: Option<usize>,
}

impl DisassemblyWindowState {
    pub fn new(which: WhichCpu) -> Self {
        Self {
            which,
            open: false,
            area: DisassemblyArea::Sdram { cached: true },
            scroll_to_row: None,
        }
    }
}

pub fn render_disassembly_window(
    ctx: &egui::Context,
    debug_state: &mut Sega32XDebugState,
    window_state: &mut DisassemblyWindowState,
) {
    let (sh2, window_title) = match window_state.which {
        WhichCpu::Master => (debug_state.sh2_master.clone(), "Master SH-2 Disassembly"),
        WhichCpu::Slave => (debug_state.sh2_slave.clone(), "Slave SH-2 Disassembly"),
    };

    Window::new(window_title)
        .open(&mut window_state.open)
        .resizable([true, true])
        .default_width(650.0)
        .show(ctx, |ui| {
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
                        ui.radio_value(&mut window_state.area, value, label);
                    }

                    if ui.button("Jump to PC").clicked() {
                        let pc = sh2.pc();
                        if let Some(area) = DisassemblyArea::from_address(pc) {
                            window_state.area = area;
                            window_state.scroll_to_row =
                                Some(((pc as usize) - area.address_range().start) / 2);
                        }
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
                let disassembly_area = window_state.area;
                let address_range = disassembly_area.address_range();

                let mut table_builder = TableBuilder::new(ui)
                    .striped(true)
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(40.0))
                    .column(Column::remainder());

                if let Some(scroll_to_row) = window_state.scroll_to_row.take() {
                    table_builder = table_builder.scroll_to_row(scroll_to_row, Some(Align::Min));
                }

                table_builder.body(|body| {
                    body.rows(15.0, (address_range.end - address_range.start) / 2, |mut row| {
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
        });
}

fn monospace_u16(value: u16) -> RichText {
    RichText::new(format!("{value:04X}")).family(FontFamily::Monospace)
}

fn monospace_u32(value: u32) -> RichText {
    RichText::new(format!("{value:08X}")).family(FontFamily::Monospace)
}
