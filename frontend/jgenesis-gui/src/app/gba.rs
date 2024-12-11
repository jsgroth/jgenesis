use crate::app::{App, Console, OpenWindow};
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
                    if let Some(bios_path) = pick_gba_bios_path() {
                        self.config.game_boy_advance.bios_path = Some(bios_path);
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

    pub(super) fn render_gba_bios_error(&mut self, ctx: &Context, open: &mut bool) {
        let mut path_configured = false;
        Window::new("Missing GBA BIOS").open(open).resizable(false).show(ctx, |ui| {
            ui.label("No Game Boy Advance BIOS path is configured. A Game Boy Advance BIOS ROM is required for GBA emulation.");

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("Configure now:");
                if ui.button("Configure GBA BIOS path").clicked() {
                    if let Some(bios_path) = pick_gba_bios_path() {
                        self.config.game_boy_advance.bios_path = Some(bios_path);
                        path_configured = true;
                    }
                }
            });
        });

        if path_configured {
            *open = false;
            self.launch_emulator(
                self.state.current_file_path.clone(),
                Some(Console::GameBoyAdvance),
            );
        }
    }
}

fn pick_gba_bios_path() -> Option<String> {
    let path = FileDialog::new()
        .add_filter("bin", &["bin"])
        .add_filter("All Types", &["*"])
        .pick_file()?;

    path.to_str().map(String::from)
}
