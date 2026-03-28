use crate::genesis::widgets::BreakpointsWidget;
use crate::{AddressSet, non_selectable_label};
use egui::panel::{Side, TopBottomSide};
use egui::style::ScrollStyle;
use egui::{Align, Grid, RichText, TextEdit, Ui, Window};
use egui_extras::{Column, TableBuilder};
use s32x_core::WhichCpu;
use s32x_core::api::debug::{
    Sega32XDebugCommand, Sega32XDebugState, Sh2BreakStatus, Sh2Breakpoint,
};
use sh2_emu::{DisassembledInstruction, MemoryAccessSize, ReadType, Sh2};
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

    fn read_address(self, address: u32, cpu: &Sh2, debug_state: &Sega32XDebugState) -> u16 {
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

                let Some(cartridge) = debug_state.genesis.cartridge() else { return 0 };
                let cartridge_addr = address & 0x3FFFFF & !1;
                cartridge.peek_word(cartridge_addr)
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
    pub disassembly_address: String,
    pub disassembly_scroll_row: Option<usize>,
    pub disassembly_table_offset: f32,
    pub disassembly_table_height: f32,
    pub disassembly_selected_pcs: AddressSet<u32>,
    pub break_status_last_frame: Sh2BreakStatus,
    pub breakpoints: BreakpointsWidget<u32>,
}

impl Sh2DebugWindowState {
    pub fn new(which: WhichCpu) -> Self {
        Self {
            which,
            disassembly_open: false,
            breakpoints_open: false,
            disassembly_area: DisassemblyArea::Sdram { cached: true },
            disassembly_address: String::new(),
            disassembly_scroll_row: None,
            disassembly_table_offset: 0.0,
            disassembly_table_height: 1.0,
            disassembly_selected_pcs: AddressSet::new(),
            break_status_last_frame: Sh2BreakStatus::default(),
            breakpoints: BreakpointsWidget::new(format!("{which:?}_breakpoints")),
        }
    }

    pub fn open_disassembly_window(&mut self, ctx: &egui::Context) {
        self.disassembly_open = true;
        crate::move_to_top(ctx, self.which.disassembly_window_title());
    }

    pub fn open_breakpoints_window(&mut self, ctx: &egui::Context) {
        self.breakpoints_open = true;
        crate::move_to_top(ctx, self.which.breakpoints_window_title());
    }

    fn try_jump_to_address(&mut self, address: u32) {
        let Some(area) = DisassemblyArea::from_address(address) else { return };

        self.disassembly_area = area;
        self.disassembly_scroll_row = Some(((address as usize) - area.address_range().start) / 2);
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
    break_status: Sh2BreakStatus,
) {
    let window_title = window_state.which.disassembly_window_title();

    if break_status.breaking && break_status != window_state.break_status_last_frame {
        window_state.try_jump_to_address(break_status.pc);
        window_state.disassembly_open = true;
        crate::move_to_top(ctx, window_title);
    }
    window_state.break_status_last_frame = break_status;

    let sh2 = match window_state.which {
        WhichCpu::Master => debug_state.sh2_master.clone(),
        WhichCpu::Slave => debug_state.sh2_slave.clone(),
    };

    let default_pos = [50.0, crate::rand_window_pos()[1]];

    let mut open = window_state.disassembly_open;
    Window::new(window_title)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(default_pos)
        .default_size([800.0, 550.0])
        .show(ctx, |ui| {
            render_disasm_top_panel(window_state, command_sender, window_title, ui);
            render_disasm_right_panel(&sh2, window_state, window_title, ui);
            render_disasm_central_panel(&sh2, debug_state, window_state, break_status, ui);
        });
    window_state.disassembly_open = open;
}

fn render_disasm_top_panel(
    window_state: &mut Sh2DebugWindowState,
    command_sender: &Sender<Sega32XDebugCommand>,
    window_title: &str,
    ui: &mut Ui,
) {
    egui::TopBottomPanel::new(TopBottomSide::Top, format!("{window_title}_top_panel")).show_inside(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                if ui.button("Pause").clicked() {
                    let _ =
                        command_sender.send(Sega32XDebugCommand::BreakPauseSh2(window_state.which));
                }

                if ui.button("Resume").clicked() {
                    let _ = command_sender.send(Sega32XDebugCommand::BreakResume);
                }

                if ui.button("Step").clicked() {
                    let _ =
                        command_sender.send(Sega32XDebugCommand::BreakStepSh2(window_state.which));
                }
            });

            ui.add_space(3.0);
        },
    );
}

fn render_disasm_right_panel(
    sh2: &Sh2,
    window_state: &mut Sh2DebugWindowState,
    window_title: &str,
    ui: &mut Ui,
) {
    egui::SidePanel::new(Side::Right, format!("{window_title}_left_panel"))
        .min_width(250.0)
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

            ui.horizontal(|ui| {
                let text_resp = ui.add(
                    TextEdit::singleline(&mut window_state.disassembly_address).desired_width(80.0),
                );
                let button_resp = ui.button("Jump to address");

                let should_jump = button_resp.clicked()
                    || (text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if should_jump
                    && let Ok(address) = u32::from_str_radix(&window_state.disassembly_address, 16)
                {
                    window_state.try_jump_to_address(address);
                }
            });

            ui.add_space(3.0);

            if ui.button("Jump to PC").clicked() {
                window_state.try_jump_to_address(sh2.pc());
            }

            ui.separator();

            let registers = sh2.registers();
            Grid::new(format!("{window_title}_reg_grid")).num_columns(4).spacing([10.0, 3.0]).show(
                ui,
                |ui| {
                    for i in 0..8 {
                        for r in [i, i + 8] {
                            ui.label(format!("R{r}"));
                            ui.label(monospace_u32(registers.gpr[r]));
                        }
                        ui.end_row();
                    }

                    ui.label("SR");
                    ui.label(monospace_u32(registers.sr.into()));
                    ui.label("VBR");
                    ui.label(monospace_u32(registers.vbr));
                    ui.end_row();

                    ui.label("GBR");
                    ui.label(monospace_u32(registers.gbr));
                    ui.label("PR");
                    ui.label(monospace_u32(registers.pr));
                    ui.end_row();

                    ui.label("MACH");
                    ui.label(monospace_u32(registers.mach));
                    ui.label("MACL");
                    ui.label(monospace_u32(registers.macl));
                    ui.end_row();

                    ui.label("PC");
                    ui.label(monospace_u32(registers.pc));
                    ui.end_row();
                },
            );

            ui.label(
                RichText::new(format!(
                    "T={} S={} Q={} M={}",
                    u8::from(registers.sr.t),
                    u8::from(registers.sr.s),
                    u8::from(registers.sr.q),
                    u8::from(registers.sr.m)
                ))
                .monospace(),
            );
        });
}

fn render_disasm_central_panel(
    sh2: &Sh2,
    debug_state: &mut Sega32XDebugState,
    state: &mut Sh2DebugWindowState,
    break_status: Sh2BreakStatus,
    ui: &mut Ui,
) {
    let ctx = ui.ctx().clone();

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let disassembly_area = state.disassembly_area;
        let address_range = disassembly_area.address_range();

        ui.spacing_mut().scroll = ScrollStyle { bar_width: 10.0, ..ScrollStyle::solid() };

        let mut table_builder = TableBuilder::new(ui)
            .striped(true)
            .column(Column::auto().at_least(10.0))
            .column(Column::auto().at_least(80.0))
            .column(Column::auto().at_least(150.0))
            .column(Column::remainder())
            .sense(egui::Sense::click());

        if let Some(scroll_to_row) = state.disassembly_scroll_row.take() {
            table_builder = table_builder.scroll_to_row(scroll_to_row, Some(Align::Center));
        } else if crate::window_on_top(&ctx, state.which.disassembly_window_title()) {
            let keys = crate::scroll_keys_pressed(&ctx);
            if let Some(offset) = keys.relative_scroll_offset(state.disassembly_table_height) {
                table_builder =
                    table_builder.vertical_scroll_offset(state.disassembly_table_offset + offset);
            }
        }

        let sh2_pc = (if break_status.breaking { break_status.pc } else { sh2.pc() }) as usize;
        let pc_row_index =
            address_range.contains(&sh2_pc).then(|| (sh2_pc - address_range.start) / 2);

        let mut disassembled = DisassembledInstruction::new();

        let highlight_color = crate::highlight_color(ctx.theme());

        let scroll_output = table_builder.body(|body| {
            body.rows(15.0, (address_range.end - address_range.start) / 2, |mut row| {
                let is_pc_row = pc_row_index == Some(row.index());
                let address = (address_range.start + 2 * row.index()) as u32;

                row.set_selected(state.disassembly_selected_pcs.contains(address));

                row.col(|ui| {
                    if is_pc_row {
                        ui.add(non_selectable_label(
                            RichText::new("→").monospace().color(highlight_color),
                        ));
                    }
                });

                row.col(|ui| {
                    let mut text = monospace_u32(address);
                    if is_pc_row {
                        text = text.color(highlight_color);
                    }

                    ui.add(non_selectable_label(text));
                });

                let opcode = disassembly_area.read_address(address, sh2, debug_state);
                sh2_emu::disassemble_into(address, opcode, &mut disassembled);

                row.col(|ui| {
                    let mut text = RichText::new(&disassembled.text).monospace();
                    if is_pc_row {
                        text = text.color(highlight_color);
                    }

                    ui.add(non_selectable_label(text));
                });

                row.col(|ui| {
                    let memory_access = disassembled.memory_read.or(disassembled.memory_write);
                    let Some((memory_access, size)) = memory_access else { return };

                    let address = memory_access.resolve_address(size, sh2);

                    let value = DisassemblyArea::from_address(address).map(|area| {
                        let word = area.read_address(address, sh2, debug_state);
                        match size {
                            MemoryAccessSize::Byte => {
                                let byte = word.to_be_bytes()[(address & 1) as usize];
                                format!("0x{byte:02X}")
                            }
                            MemoryAccessSize::Word => format!("0x{word:04X}"),
                            MemoryAccessSize::Longword => {
                                let low =
                                    area.read_address(address.wrapping_add(2), sh2, debug_state);
                                let longword = u32::from(low) | (u32::from(word) << 16);
                                format!("0x{longword:08X}")
                            }
                        }
                    });

                    let text = match (disassembled.memory_read_type, value) {
                        (ReadType::Load, Some(value)) => {
                            RichText::new(format!("; ${address:08X} = {value}")).monospace()
                        }
                        (ReadType::Load, None) | (ReadType::EffectiveAddress, _) => {
                            RichText::new(format!("; ${address:08X}")).monospace()
                        }
                    };
                    ui.add(non_selectable_label(text));
                });

                if row.response().clicked() {
                    state.disassembly_selected_pcs.toggle(address);
                }
            });
        });
        state.disassembly_table_offset = scroll_output.state.offset.y;
        state.disassembly_table_height = scroll_output.inner_rect.height();
    });
}

pub fn render_breakpoints_window(
    ctx: &egui::Context,
    window_state: &mut Sh2DebugWindowState,
    command_sender: &Sender<Sega32XDebugCommand>,
) {
    let window_title = window_state.which.breakpoints_window_title();

    let mut open = window_state.breakpoints_open;
    Window::new(window_title)
        .open(&mut open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(crate::rand_window_pos())
        .show(ctx, |ui| {
            window_state.breakpoints.render(ui, |breakpoints| {
                let sh2_breakpoints = breakpoints
                    .iter()
                    .map(|breakpoint| Sh2Breakpoint {
                        start_address: breakpoint.start_address,
                        end_address: breakpoint.end_address,
                        read: breakpoint.read,
                        write: breakpoint.write,
                        execute: breakpoint.execute,
                    })
                    .collect();

                let _ = command_sender.send(Sega32XDebugCommand::UpdateSh2Breakpoints(
                    window_state.which,
                    sh2_breakpoints,
                ));
            });
        });
    window_state.breakpoints_open = open;
}

fn monospace_u32(value: u32) -> RichText {
    RichText::new(format!("{value:08X}")).monospace()
}
