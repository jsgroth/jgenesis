use egui::{FontFamily, Grid, RichText, TextEdit, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenericBreakpoint<T> {
    pub start_address: T,
    pub end_address: T,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone)]
pub struct BreakpointsWidget<T> {
    id: String,
    breakpoints: Vec<GenericBreakpoint<T>>,
    start_address: String,
    end_address: String,
    read: bool,
    write: bool,
    execute: bool,
}

impl<T> BreakpointsWidget<T> {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            breakpoints: Vec::new(),
            start_address: String::new(),
            end_address: String::new(),
            read: false,
            write: false,
            execute: false,
        }
    }
}

pub trait BreakpointAddress: Sized + Copy + Eq + Ord {
    fn from_hex_str(value: &str) -> Option<Self>;

    fn format_hex(self) -> String;
}

impl BreakpointAddress for u16 {
    fn from_hex_str(value: &str) -> Option<Self> {
        Self::from_str_radix(value, 16).ok()
    }

    fn format_hex(self) -> String {
        format!("{self:04X}")
    }
}

impl BreakpointAddress for u32 {
    fn from_hex_str(value: &str) -> Option<Self> {
        Self::from_str_radix(value, 16).ok()
    }

    fn format_hex(self) -> String {
        format!("{self:08X}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct U24(u32);

impl U24 {
    pub fn new(value: u32) -> Self {
        Self(value & 0xFFFFFF)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl BreakpointAddress for U24 {
    fn from_hex_str(value: &str) -> Option<Self> {
        u32::from_hex_str(value).map(Self::new)
    }

    fn format_hex(self) -> String {
        format!("{:06X}", self.0)
    }
}

impl<T> BreakpointsWidget<T>
where
    T: BreakpointAddress,
{
    pub fn render(
        &mut self,
        ui: &mut Ui,
        update_breakpoints: impl FnOnce(&[GenericBreakpoint<T>]),
    ) {
        let initial_breakpoints = self.breakpoints.clone();

        if !self.breakpoints.is_empty() {
            Grid::new(format!("{}_breakpoints", self.id)).show(ui, |ui| {
                ui.heading("Addresses");
                ui.heading("R");
                ui.heading("W");
                ui.heading("X");
                ui.label("");
                ui.end_row();

                let mut remove_idx: Option<usize> = None;
                for (i, breakpoint) in self.breakpoints.iter_mut().enumerate() {
                    if breakpoint.start_address == breakpoint.end_address {
                        ui.label(
                            RichText::new(format!("${}", breakpoint.start_address.format_hex()))
                                .family(FontFamily::Monospace),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "${}-${}",
                                breakpoint.start_address.format_hex(),
                                breakpoint.end_address.format_hex()
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
                    && remove_idx < self.breakpoints.len()
                {
                    self.breakpoints.remove(remove_idx);
                }
            });

            ui.separator();
        }

        let mut enter_pressed = false;

        ui.heading("Add Breakpoint");
        ui.horizontal(|ui| {
            ui.label("$");
            let start_resp =
                ui.add(TextEdit::singleline(&mut self.start_address).desired_width(80.0));
            ui.label("-");
            ui.label("$");
            let end_resp = ui.add(TextEdit::singleline(&mut self.end_address).desired_width(80.0));

            ui.checkbox(&mut self.read, "Read");
            ui.checkbox(&mut self.write, "Write");
            ui.checkbox(&mut self.execute, "Execute");

            enter_pressed = (start_resp.lost_focus() || end_resp.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });

        let button_resp = ui.button("Add");
        if (button_resp.clicked() || enter_pressed)
            && let Some(start_address) = T::from_hex_str(&self.start_address)
        {
            if self.end_address.is_empty() {
                self.breakpoints.push(GenericBreakpoint {
                    start_address,
                    end_address: start_address,
                    read: self.read,
                    write: self.write,
                    execute: self.execute,
                });

                self.start_address.clear();
            } else if let Some(end_address) = T::from_hex_str(&self.end_address)
                && end_address >= start_address
            {
                self.breakpoints.push(GenericBreakpoint {
                    start_address,
                    end_address,
                    read: self.read,
                    write: self.write,
                    execute: self.execute,
                });

                self.start_address.clear();
                self.end_address.clear();
            }
        }

        if initial_breakpoints != self.breakpoints {
            update_breakpoints(&self.breakpoints);
        }
    }
}
