use egui::{FontFamily, Grid, RichText, TextEdit, Ui, Window};

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
    breakpoints_changed: bool,
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
            breakpoints_changed: false,
            start_address: String::new(),
            end_address: String::new(),
            read: true,
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
    pub fn show_window_and_update(
        &mut self,
        ctx: &egui::Context,
        window_title: impl Into<egui::WidgetText>,
        window_open: &mut bool,
        update_breakpoints: impl FnOnce(&[GenericBreakpoint<T>]),
    ) {
        Window::new(window_title)
            .open(window_open)
            .constrain(false)
            .resizable([true, true])
            .default_pos(crate::rand_window_pos())
            .show(ctx, |ui| {
                self.render(ui);
            });

        if self.breakpoints_changed {
            self.breakpoints_changed = false;
            update_breakpoints(&self.breakpoints);
        }
    }

    fn render(&mut self, ui: &mut Ui) {
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

                macro_rules! check_all_button {
                    ($label:literal, $field:ident = $value:literal) => {
                        if ui.button($label).clicked() {
                            for breakpoint in &mut self.breakpoints {
                                breakpoint.$field = $value;
                            }
                        }
                    };
                }

                ui.label("");
                check_all_button!("Check all", read = true);
                check_all_button!("Check all", write = true);
                check_all_button!("Check all", execute = true);
                ui.end_row();

                ui.label("");
                check_all_button!("Uncheck all", read = false);
                check_all_button!("Uncheck all", write = false);
                check_all_button!("Uncheck all", execute = false);
                ui.end_row();
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

        self.breakpoints_changed |= initial_breakpoints != self.breakpoints;
    }

    pub fn has_execute_breakpoint(&self, address: T) -> bool {
        self.breakpoints.iter().any(|breakpoint| {
            breakpoint.execute
                && breakpoint.start_address == address
                && breakpoint.end_address == address
        })
    }

    pub fn toggle_execute_breakpoint(&mut self, address: T) {
        let mut value: Option<bool> = None;
        let mut to_remove = Vec::new();
        for (i, breakpoint) in self.breakpoints.iter_mut().enumerate() {
            if breakpoint.start_address != address || breakpoint.end_address != address {
                continue;
            }

            let value = *value.get_or_insert(!breakpoint.execute);
            breakpoint.execute = value;

            if !breakpoint.read && !breakpoint.write && !breakpoint.execute {
                to_remove.push(i);
            }
        }

        if value.is_none() {
            self.breakpoints.push(GenericBreakpoint {
                start_address: address,
                end_address: address,
                read: false,
                write: false,
                execute: true,
            });
        }

        for remove_idx in to_remove.into_iter().rev() {
            if remove_idx < self.breakpoints.len() {
                self.breakpoints.remove(remove_idx);
            }
        }

        self.breakpoints_changed = true;
    }

    // Render a widget that can be clicked on to toggle execute breakpoints at a specific address.
    //
    // Draws a red circle if there exists a breakpoint at the specified address, otherwise draws
    // an empty circle whenever the widget is hovered over.
    pub fn render_clickable_widget(
        &mut self,
        address: T,
        interact_id: impl Into<egui::Id>,
        ui: &mut Ui,
    ) {
        let max_rect = ui.max_rect();

        let interact_resp = ui.interact(max_rect, interact_id.into(), egui::Sense::click());

        let has_execute_breakpoint = self.has_execute_breakpoint(address);
        if interact_resp.clicked() {
            self.toggle_execute_breakpoint(address);
        } else if interact_resp.hovered() || has_execute_breakpoint {
            let fill_color = if interact_resp.hovered() && has_execute_breakpoint {
                egui::Color32::LIGHT_RED
            } else if has_execute_breakpoint {
                egui::Color32::RED
            } else {
                ui.visuals().panel_fill
            };
            let stroke_color = ui.visuals().text_color();

            let radius = if max_rect.width() < max_rect.height() {
                0.5 * max_rect.width()
            } else {
                0.5 * max_rect.height()
            };

            ui.painter().circle(
                [
                    0.5 * max_rect.width() + max_rect.left(),
                    0.5 * max_rect.height() + max_rect.top(),
                ]
                .into(),
                radius,
                fill_color,
                egui::Stroke::new(1.0, stroke_color),
            );
        }
    }
}
