use crate::app::widgets::{BiosErrorStrings, RenderErrorEffect};
use crate::app::{App, OpenWindow, widgets};
use egui::{Context, Window};
use gba_config::{GbaAspectRatio, GbaColorCorrection};
use jgenesis_native_driver::extensions::Console;
use rfd::FileDialog;
use std::path::PathBuf;

impl App {
    pub(super) fn render_gba_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaVideo;

        let mut open = true;

        Window::new("GBA Video Settings").open(&mut open).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (GbaAspectRatio::SquarePixels, "Square pixels"),
                        (GbaAspectRatio::Stretched, "Stretched"),
                    ] {
                        ui.radio_value(
                            &mut self.config.game_boy_advance.aspect_ratio,
                            value,
                            label,
                        );
                    }
                });
            });

            ui.group(|ui| {
                ui.label("Color correction");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (GbaColorCorrection::None, "None"),
                        (GbaColorCorrection::GbaLcd, "Game Boy Advance LCD"),
                    ] {
                        ui.radio_value(
                            &mut self.config.game_boy_advance.color_correction,
                            value,
                            label,
                        );
                    }
                });
            });
        });

        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_gba_bios_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing GBA BIOS",
                text: "No GBA BIOS path is configured. A BIOS ROM is required for GBA emulation.",
                button_label: "Configure GBA BIOS path",
            },
            &mut self.config.game_boy_advance.bios_path,
            Console::GameBoyAdvance,
            pick_bios_path,
        )
    }
}

fn pick_bios_path() -> Option<PathBuf> {
    FileDialog::new().add_filter("bin", &["bin"]).add_filter("All Files", &["*"]).pick_file()
}
