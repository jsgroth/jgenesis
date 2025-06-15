mod helptext;

use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Ui, Window};
use gb_config::{GbAspectRatio, GbAudioResampler, GbPalette, GbcColorCorrection};

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
                        ui.radio_value(
                            &mut self.config.game_boy.gb_palette,
                            GbPalette::Custom,
                            "Custom",
                        );
                    });

                    ui.add_enabled_ui(self.config.game_boy.gb_palette == GbPalette::Custom, |ui| {
                        ui.label("Custom palette:");
                        render_custom_palette_widget(
                            &mut self.config.game_boy.gb_custom_palette,
                            ui,
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

    pub(super) fn render_gb_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GameBoyAudio;
        let mut open = true;
        Window::new("Game Boy Audio Settings").open(&mut open).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Audio resampling algorithm");

                    ui.radio_value(
                        &mut self.config.game_boy.audio_resampler,
                        GbAudioResampler::LowPassNearestNeighbor,
                        "Low-pass filter + nearest neighbor (Faster)",
                    );
                    ui.radio_value(
                        &mut self.config.game_boy.audio_resampler,
                        GbAudioResampler::WindowedSinc,
                        "Windowed sinc interpolation (Higher quality)",
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_RESAMPLING);
            }

            ui.add_space(5.0);
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
}

fn render_custom_palette_widget(custom_palette: &mut [(u8, u8, u8); 4], ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(30.0);
        ui.label("Light");

        for (r, g, b) in custom_palette.iter_mut() {
            let mut rgb = [*r, *g, *b];
            ui.color_edit_button_srgb(&mut rgb);
            [*r, *g, *b] = rgb;
        }

        ui.label("Dark");
    });

    ui.add_space(5.0);

    if ui.button("Default custom palette").clicked() {
        *custom_palette = jgenesis_native_config::gb::default_gb_custom_palette();
    }
}
