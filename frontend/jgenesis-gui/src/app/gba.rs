use crate::app::{App, OpenWindow};
use egui::{Context, Window};
use gba_core::api::GbaAspectRatio;
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

            ui.add_space(5.0);

            ui.checkbox(
                &mut self.config.game_boy_advance.skip_bios_intro_animation,
                "Skip BIOS intro animation",
            );
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_gba_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaVideo;

        let mut open = true;
        Window::new("GBA Video Settings").open(&mut open).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.game_boy_advance.aspect_ratio,
                        GbaAspectRatio::SquarePixels,
                        "Square pixels",
                    );
                    ui.radio_value(
                        &mut self.config.game_boy_advance.aspect_ratio,
                        GbaAspectRatio::Stretched,
                        "Stretched",
                    );
                });
            });
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}
