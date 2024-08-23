mod helptext;

use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use gb_core::api::{GbAspectRatio, GbPalette, GbcColorCorrection};

impl App {
    pub(super) fn render_gb_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GameBoyGeneral;

        let mut open = true;
        Window::new("Game Boy General Settings")
            .default_width(400.0)
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let is_running_gb = self.emu_thread.status() == EmuThreadStatus::RunningGameBoy;

                ui.add_enabled_ui(!is_running_gb, |ui| {
                    let rect = ui
                        .checkbox(
                            &mut self.config.game_boy.force_dmg_mode,
                            "Force DMG mode in software with CGB support",
                        )
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::FORCE_DMG_MODE);
                    }

                    let rect = ui
                        .checkbox(
                            &mut self.config.game_boy.pretend_to_be_gba,
                            "Pretend to be a Game Boy Advance",
                        )
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::PRETEND_GBA_MODE);
                    }
                });

                let rect = ui
                    .checkbox(
                        &mut self.config.game_boy.audio_60hz_hack,
                        "Enable audio sync timing hack",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::AUDIO_TIMING_HACK);
                }

                self.render_help_text(ui, WINDOW);
            });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_gb_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GameBoyVideo;

        let mut open = true;
        Window::new("Game Boy Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Aspect ratio");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.game_boy.aspect_ratio,
                            GbAspectRatio::SquarePixels,
                            "Square pixels",
                        );
                        ui.radio_value(
                            &mut self.config.game_boy.aspect_ratio,
                            GbAspectRatio::Stretched,
                            "Stretched",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ASPECT_RATIO);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("GB color palette");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.game_boy.gb_palette,
                            GbPalette::BlackAndWhite,
                            "Black and white",
                        );
                        ui.radio_value(
                            &mut self.config.game_boy.gb_palette,
                            GbPalette::GreenTint,
                            "Green tint",
                        );
                        ui.radio_value(
                            &mut self.config.game_boy.gb_palette,
                            GbPalette::LimeGreen,
                            "Lime green",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::GB_COLOR_PALETTE);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("GBC color correction");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.game_boy.gbc_color_correction,
                            GbcColorCorrection::None,
                            "None",
                        );
                        ui.radio_value(
                            &mut self.config.game_boy.gbc_color_correction,
                            GbcColorCorrection::GbcLcd,
                            "Game Boy Color LCD",
                        );
                        ui.radio_value(
                            &mut self.config.game_boy.gbc_color_correction,
                            GbcColorCorrection::GbaLcd,
                            "Game Boy Advance LCD",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::GBC_COLOR_CORRECTION);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}
