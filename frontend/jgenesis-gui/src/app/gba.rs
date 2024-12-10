use crate::app::{App, OpenWindow};
use egui::{Context, Window};
use rfd::FileDialog;

impl App {
    pub(super) fn render_gba_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaGeneral;

        let mut open = true;
        Window::new("GBA General Settings").open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let current_bios_path =
                    self.config.game_boy_advance.bios_path.as_deref().unwrap_or("<None>");
                if ui.button(current_bios_path).clicked() {
                    if let Some(bios_path) = FileDialog::new()
                        .add_filter("bin", &["bin"])
                        .add_filter("All Types", &["*"])
                        .pick_file()
                    {
                        if let Some(bios_path) = bios_path.to_str() {
                            self.config.game_boy_advance.bios_path = Some(bios_path.into());
                        }
                    }
                }

                ui.label("BIOS path");
            });
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}
