use egui::{Color32, FontId, RichText, TextEdit, Window};
use egui_extras::{Column, TableBuilder};
use jgenesis_common::frontend::ViewableMemory;
use std::array;

pub struct MemoryViewerState {
    pub open: bool,
    pub goto_text: String,
    pub goto_text_error: bool,
    pub goto_address: Option<u32>,
    pub edit_address: String,
    pub edit_value: String,
    pub edit_error: bool,
    pub search_value: String,
    pub search_address: String,
    pub search_invalid_value: bool,
    pub search_not_found: bool,
    pub search_invalid_address: bool,
    pub search_found_address: Option<u32>,
    pub search_len: Option<usize>,
}

impl MemoryViewerState {
    pub fn new() -> Self {
        Self {
            open: false,
            goto_text: String::new(),
            goto_text_error: false,
            goto_address: None,
            edit_address: String::new(),
            edit_value: String::new(),
            edit_error: false,
            search_value: String::new(),
            search_address: String::new(),
            search_invalid_value: false,
            search_not_found: false,
            search_invalid_address: false,
            search_found_address: None,
            search_len: None,
        }
    }
}

pub struct MemoryViewer<'a, Memory> {
    pub window_title: &'a str,
    pub memory: Memory,
    pub state: &'a mut MemoryViewerState,
}

impl<'a, Memory: ViewableMemory> MemoryViewer<'a, Memory> {
    pub fn new(window_title: &'a str, state: &'a mut MemoryViewerState, memory: Memory) -> Self {
        Self { window_title, memory, state }
    }

    pub fn show(self, ctx: &egui::Context) {
        let mut memory = self.memory;
        Window::new(self.window_title).open(&mut self.state.open).default_width(600.0).show(
            ctx,
            |ui| {
                let table_rows = memory.len() / 16 + usize::from(memory.len() % 16 != 0);
                let address_len = format!("{:X}", memory.len() - 1).len();

                egui::SidePanel::new(
                    egui::panel::Side::Right,
                    format!("{}_right_panel", self.window_title),
                )
                .resizable(false)
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            TextEdit::singleline(&mut self.state.goto_text).desired_width(60.0),
                        );
                        self.state.goto_text_error &= !response.changed();
                        let enter_pressed =
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        if ui.button("Go to address").clicked() || enter_pressed {
                            match u32::from_str_radix(&self.state.goto_text, 16) {
                                Ok(address) => {
                                    self.state.goto_address = Some(address);
                                }
                                Err(_) => {
                                    self.state.goto_text_error = true;
                                }
                            }
                        }
                    });

                    if self.state.goto_text_error {
                        ui.colored_label(Color32::RED, "Invalid address");
                    }

                    ui.add_space(20.0);
                    ui.group(|ui| {
                        ui.label("Edit memory");

                        ui.add_space(5.0);

                        let mut enter_pressed = false;

                        ui.horizontal(|ui| {
                            let response = ui.add(
                                TextEdit::singleline(&mut self.state.edit_address)
                                    .desired_width(60.0),
                            );
                            self.state.edit_error &= !response.changed();
                            enter_pressed |= response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));

                            ui.label("Address");
                        });

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            let response = ui.add(
                                TextEdit::singleline(&mut self.state.edit_value)
                                    .desired_width(30.0),
                            );
                            self.state.edit_error &= !response.changed();
                            enter_pressed |= response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));

                            ui.label("Value (8-bit)");
                        });

                        ui.add_space(5.0);

                        if ui.button("Apply write").clicked() || enter_pressed {
                            match (
                                u32::from_str_radix(&self.state.edit_address, 16),
                                u8::from_str_radix(&self.state.edit_value, 16),
                            ) {
                                (Ok(address), Ok(value)) => {
                                    log::info!(
                                        "Writing 0x{value:02X} to {}",
                                        fmt_address(address as usize, address_len)
                                    );
                                    memory.write(address, value);
                                    self.state.goto_address = Some(address);
                                }
                                _ => {
                                    self.state.edit_error = true;
                                }
                            }
                        }

                        if self.state.edit_error {
                            ui.colored_label(Color32::RED, "Invalid address/value");
                        }
                    });

                    ui.add_space(20.0);
                    ui.group(|ui| {
                        ui.label("Search");

                        ui.add_space(5.0);

                        let mut enter_pressed = false;

                        ui.horizontal(|ui| {
                            let response = ui.add(
                                TextEdit::singleline(&mut self.state.search_value)
                                    .desired_width(70.0),
                            );
                            if response.changed() {
                                self.state.search_invalid_value = false;
                            }
                            enter_pressed |= response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        });

                        ui.add_space(5.0);

                        let response = ui.add(
                            TextEdit::singleline(&mut self.state.search_address)
                                .desired_width(70.0),
                        );
                        if response.changed() {
                            self.state.search_invalid_address = false;
                        }
                        enter_pressed |=
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        ui.label("Start address (optional)");

                        ui.add_space(5.0);

                        if ui.button("Search").clicked() || enter_pressed {
                            self.state.search_not_found = false;
                            self.state.search_found_address = None;
                            self.state.search_len = None;

                            match parse_hex(&self.state.search_value) {
                                Some(bytes) => {
                                    let mut start = None;
                                    if !self.state.search_address.is_empty() {
                                        match u32::from_str_radix(&self.state.search_address, 16) {
                                            Ok(address) => {
                                                start = Some(address);
                                            }
                                            Err(_) => {
                                                self.state.search_invalid_address = true;
                                            }
                                        }
                                    }

                                    match memory.search(&bytes, start) {
                                        Some(address) => {
                                            self.state.search_found_address = Some(address);
                                            self.state.search_len = Some(bytes.len());
                                            self.state.goto_address = Some(address);
                                        }
                                        None => {
                                            self.state.search_not_found = true;
                                        }
                                    }
                                }
                                None => {
                                    self.state.search_invalid_value = true;
                                }
                            }
                        }
                    });

                    if let Some(address) = self.state.search_found_address {
                        ui.label(format!(
                            "Found at {}",
                            fmt_address(address as usize, address_len)
                        ));
                    }

                    if self.state.search_not_found {
                        ui.colored_label(Color32::RED, "Value not found");
                    }

                    if self.state.search_invalid_value {
                        ui.colored_label(Color32::RED, "Invalid search value");
                    }

                    if self.state.search_invalid_address {
                        ui.colored_label(Color32::RED, "Invalid start address");
                    }
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let mut builder = TableBuilder::new(ui)
                        .column(Column::auto().at_least(60.0))
                        .columns(Column::auto().at_least(35.0), 8);

                    if let Some(address) = self.state.goto_address.take() {
                        let row = (address / 16) as usize;
                        builder = builder.scroll_to_row(row, None);
                    }

                    builder.body(|body| {
                        body.rows(15.0, table_rows, |mut row| {
                            let address = row.index() * 16;
                            let data: [u8; 16] =
                                array::from_fn(|i| memory.read((address + i) as u32));

                            row.col(|ui| {
                                ui.label(
                                    RichText::new(fmt_address(address, address_len))
                                        .font(FontId::monospace(12.0)),
                                );
                            });

                            for (i, chunk) in data.chunks_exact(2).enumerate() {
                                if address + 2 * i < memory.len() {
                                    row.col(|ui| {
                                        let mut text = RichText::new(format!(
                                            "{:02X}{:02X}",
                                            chunk[0], chunk[1]
                                        ))
                                        .font(FontId::monospace(12.0));

                                        if let (Some(search_address), Some(len)) =
                                            (self.state.search_found_address, self.state.search_len)
                                        {
                                            let len = len + (search_address & 1) as usize;
                                            let search_address = (search_address & !1) as usize;
                                            if (search_address..search_address + len)
                                                .contains(&(address + 2 * i))
                                            {
                                                text = text.color(Color32::GREEN);
                                            }
                                        }

                                        ui.label(text);
                                    });
                                }
                            }
                        });
                    });
                });
            },
        );
    }
}

// TODO this could be more optimized
fn fmt_address(address: usize, len: usize) -> String {
    let mut s = format!("{address:X}");
    while s.len() < len {
        s.insert(0, '0');
    }

    s.insert(0, '$');

    s
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() {
        return None;
    }

    if s.len() % 2 != 0 {
        let mut s2 = String::with_capacity(s.len() + 1);
        s2.push('0');
        s2.push_str(s);
        return parse_hex(&s2);
    }

    let mut bytes = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks_exact(2) {
        let value = 16 * parse_hex_char(chunk[0])? + parse_hex_char(chunk[1])?;
        bytes.push(value);
    }

    Some(bytes)
}

fn parse_hex_char(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + (c - b'a')),
        b'A'..=b'F' => Some(10 + (c - b'A')),
        _ => None,
    }
}
