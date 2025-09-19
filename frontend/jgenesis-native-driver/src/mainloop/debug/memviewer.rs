use egui::panel::Side;
use egui::scroll_area::ScrollBarVisibility;
use egui::{Align, CentralPanel, Color32, Context, FontId, RichText, SidePanel, TextEdit, Window};
use egui_extras::{Column, TableBuilder};
use jgenesis_common::debug::{DebugMemoryView, Endian};
use std::fmt::Write;
use std::{array, cmp};

const MONOSPACE: FontId = FontId::monospace(12.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryViewerColumns {
    Byte,
    Word,
    Longword,
}

impl MemoryViewerColumns {
    fn num_columns(self) -> usize {
        match self {
            Self::Byte => 16,
            Self::Word => 8,
            Self::Longword => 4,
        }
    }

    fn column_width(self) -> f32 {
        match self {
            Self::Byte => 15.0,
            Self::Word => 35.0,
            Self::Longword => 80.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryViewerState {
    pub window_title: String,
    pub endian: Endian,
    pub columns: MemoryViewerColumns,
    pub open: bool,
    pub goto_text: String,
    pub goto_address: Option<usize>,
    pub set_address_text: String,
    pub value_text: String,
    pub set_invalid: bool,
}

impl MemoryViewerState {
    pub fn new(name: &str, endian: Endian) -> Self {
        Self {
            window_title: format!("{name} Viewer"),
            endian,
            columns: MemoryViewerColumns::Word,
            open: false,
            goto_text: String::new(),
            goto_address: None,
            set_address_text: String::new(),
            value_text: String::new(),
            set_invalid: false,
        }
    }
}

pub fn render(ctx: &Context, memory: &mut dyn DebugMemoryView, state: &mut MemoryViewerState) {
    let memory_len = memory.len();
    if memory_len == 0 {
        return;
    }

    let mut open = state.open;

    Window::new(&state.window_title).open(&mut open).default_width(600.0).show(ctx, |ui| {
        let table_rows = memory_len / 16 + usize::from(!memory_len.is_multiple_of(16));
        let address_len = format!("{:X}", memory_len - 1).len();

        SidePanel::new(Side::Right, format!("{}_right", state.window_title))
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.heading("Go to address");

                ui.horizontal(|ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut state.goto_text)
                            .desired_width(70.0)
                            .font(MONOSPACE),
                    );
                    let enter_pressed =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    let goto_address = ui.button("Go").clicked() || enter_pressed;

                    if goto_address && let Ok(address) = usize::from_str_radix(&state.goto_text, 16)
                    {
                        state.goto_address = Some(address);
                    }
                });

                ui.add_space(20.0);

                ui.heading("Edit memory");

                ui.horizontal(|ui| {
                    ui.label("Address");

                    ui.add(
                        TextEdit::singleline(&mut state.set_address_text)
                            .desired_width(70.0)
                            .font(MONOSPACE),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Value");

                    ui.add(
                        TextEdit::singleline(&mut state.value_text)
                            .desired_width(70.0)
                            .font(MONOSPACE),
                    );
                });

                if ui.button("Set byte (8-bit)").clicked() {
                    try_set_byte(memory, state);
                }

                if ui.button("Set word (16-bit)").clicked() {
                    try_set_word(memory, state);
                }

                if ui.button("Set longword (32-bit)").clicked() {
                    try_set_longword(memory, state);
                }

                if state.set_invalid {
                    ui.colored_label(Color32::RED, "Invalid address or value");
                }

                ui.add_space(20.0);

                ui.heading("Column size");
                ui.radio_value(&mut state.columns, MemoryViewerColumns::Byte, "Byte (8-bit)");
                ui.radio_value(&mut state.columns, MemoryViewerColumns::Word, "Word (16-bit)");
                ui.radio_value(
                    &mut state.columns,
                    MemoryViewerColumns::Longword,
                    "Longword (32-bit)",
                );

                ui.add_space(20.0);

                ui.heading("Endianness");
                ui.radio_value(&mut state.endian, Endian::Big, "Big-endian");
                ui.radio_value(&mut state.endian, Endian::Little, "Little-endian");
            });

        CentralPanel::default().show_inside(ui, |ui| {
            let num_columns = state.columns.num_columns();
            let column_width = state.columns.column_width();

            let mut builder = TableBuilder::new(ui)
                .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                .column(Column::auto().at_least(60.0))
                .columns(Column::auto().at_least(column_width), num_columns)
                .column(Column::remainder().at_least(10.0));

            if let Some(address) = state.goto_address.take() {
                builder = builder
                    .scroll_to_row(cmp::min(table_rows - 1, address / 16), Some(Align::Center));
            }

            builder.body(|body| {
                body.rows(15.0, table_rows, |mut row| {
                    let address = 16 * row.index();
                    let data: [_; 16] = array::from_fn(|i| memory.read(address + i));

                    row.col(|ui| {
                        ui.label(RichText::new(fmt_address(address, address_len)).font(MONOSPACE));
                    });

                    match state.columns {
                        MemoryViewerColumns::Byte => {
                            for (i, byte) in data.into_iter().enumerate() {
                                if address + i < memory_len {
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(format!("{byte:02X}")).font(MONOSPACE),
                                        );
                                    });
                                }
                            }
                        }
                        MemoryViewerColumns::Word => {
                            let to_bytes = match state.endian {
                                Endian::Big => u16::from_be_bytes,
                                Endian::Little => u16::from_le_bytes,
                            };

                            for (i, chunk) in data.chunks_exact(2).enumerate() {
                                if address + 2 * i + 1 < memory_len {
                                    let word = to_bytes(chunk.try_into().unwrap());

                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(format!("{word:04X}")).font(MONOSPACE),
                                        );
                                    });
                                }
                            }
                        }
                        MemoryViewerColumns::Longword => {
                            let to_bytes = match state.endian {
                                Endian::Big => u32::from_be_bytes,
                                Endian::Little => u32::from_le_bytes,
                            };

                            for (i, chunk) in data.chunks_exact(4).enumerate() {
                                if address + 4 * i + 3 < memory_len {
                                    let longword = to_bytes(chunk.try_into().unwrap());

                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(format!("{longword:08X}"))
                                                .font(MONOSPACE),
                                        );
                                    });
                                }
                            }
                        }
                    }

                    // Hack to make scroll bar not overlap the rightmost data column
                    row.col(|_ui| {});
                });
            });
        });
    });

    state.open = open;
}

fn fmt_address(address: usize, len: usize) -> String {
    let mut s = String::with_capacity(len);
    let _ = write!(s, "{address:X}");
    while s.len() < len {
        s.insert(0, '0');
    }

    s
}

fn try_set_byte(memory: &mut dyn DebugMemoryView, state: &mut MemoryViewerState) {
    let Ok(address) = usize::from_str_radix(&state.set_address_text, 16) else {
        state.set_invalid = true;
        return;
    };

    let Ok(byte) = u8::from_str_radix(&state.value_text, 16) else {
        state.set_invalid = true;
        return;
    };

    memory.write(address, byte);

    state.goto_address = Some(address);
    state.set_invalid = false;
}

fn try_set_word(memory: &mut dyn DebugMemoryView, state: &mut MemoryViewerState) {
    let Ok(address) = usize::from_str_radix(&state.set_address_text, 16) else {
        state.set_invalid = true;
        return;
    };

    let Ok(word) = u16::from_str_radix(&state.value_text, 16) else {
        state.set_invalid = true;
        return;
    };

    let [first, second] = match state.endian {
        Endian::Big => word.to_be_bytes(),
        Endian::Little => word.to_le_bytes(),
    };

    memory.write(address & !1, first);
    memory.write(address | 1, second);

    state.goto_address = Some(address);
    state.set_invalid = false;
}

fn try_set_longword(memory: &mut dyn DebugMemoryView, state: &mut MemoryViewerState) {
    let Ok(address) = usize::from_str_radix(&state.set_address_text, 16) else {
        state.set_invalid = true;
        return;
    };

    let Ok(longword) = u32::from_str_radix(&state.value_text, 16) else {
        state.set_invalid = true;
        return;
    };

    let bytes = match state.endian {
        Endian::Big => longword.to_be_bytes(),
        Endian::Little => longword.to_le_bytes(),
    };

    for (i, byte) in bytes.into_iter().enumerate() {
        memory.write((address & !3) | i, byte);
    }

    state.goto_address = Some(address);
    state.set_invalid = false;
}
