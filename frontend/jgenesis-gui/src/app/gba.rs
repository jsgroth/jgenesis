use crate::app::widgets::{BiosErrorStrings, OptionalPathSelector, RenderErrorEffect};
use crate::app::{App, OpenWindow, widgets};
use egui::{Context, Slider, Window};
use gba_config::{GbaAspectRatio, GbaColorCorrection, GbaSaveMemory};
use jgenesis_native_config::gba::GameBoyAdvanceAppConfig;
use jgenesis_native_driver::extensions::Console;
use rfd::FileDialog;
use std::path::PathBuf;

mod helptext;

impl App {
    pub(super) fn render_gba_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaGeneral;

        let mut open = true;

        Window::new("GBA General Settings").open(&mut open).show(ctx, |ui| {
            let rect = ui
                .add(OptionalPathSelector::new(
                    "BIOS path",
                    &mut self.config.game_boy_advance.bios_path,
                    pick_bios_path,
                ))
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::BIOS_PATH);
            }

            ui.add_space(5.0);

            let rect = ui
                .checkbox(
                    &mut self.config.game_boy_advance.skip_bios_animation,
                    "Skip BIOS intro animation",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SKIP_BIOS_ANIMATION);
            }

            ui.add_space(5.0);

            let rect = ui
                .group(|ui| {
                    ui.label("Save memory type");

                    for (value, label) in [
                        (None, "Auto-detect"),
                        (Some(GbaSaveMemory::Sram), "SRAM"),
                        (Some(GbaSaveMemory::EepromUnknownSize), "EEPROM - Auto-detect size"),
                        (Some(GbaSaveMemory::Eeprom512), "EEPROM - 512 bytes"),
                        (Some(GbaSaveMemory::Eeprom8K), "EEPROM - 8 KB"),
                        (Some(GbaSaveMemory::FlashRom64K), "Flash ROM - 64 KB"),
                        (Some(GbaSaveMemory::FlashRom128K), "Flash ROM - 128 KB"),
                        (Some(GbaSaveMemory::None), "No save memory"),
                    ] {
                        ui.radio_value(
                            &mut self.config.game_boy_advance.forced_save_memory_type,
                            value,
                            label,
                        );
                    }
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SAVE_MEMORY_TYPE);
            }

            self.render_help_text(ui, WINDOW);
        });

        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_gba_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GbaVideo;

        let mut open = true;

        Window::new("GBA Video Settings").open(&mut open).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
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
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ASPECT_RATIO);
            }

            let rect = ui
                .group(|ui| {
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

                    ui.add_enabled_ui(
                        self.config.game_boy_advance.color_correction == GbaColorCorrection::GbaLcd,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Console screen gamma");

                                ui.add(
                                    Slider::new(
                                        &mut self.config.game_boy_advance.color_correction_gamma,
                                        0.1..=5.0,
                                    )
                                    .custom_formatter(|value, _| format!("{value:.1}")),
                                );

                                if ui.button("Default").clicked() {
                                    self.config.game_boy_advance.color_correction_gamma =
                                        GameBoyAdvanceAppConfig::default().color_correction_gamma;
                                }
                            });
                        },
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::COLOR_CORRECTION);
            }

            ui.add_space(5.0);

            let rect = ui
                .checkbox(
                    &mut self.config.game_boy_advance.frame_blending,
                    "Frame blending enabled",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::FRAME_BLENDING);
            }

            self.render_help_text(ui, WINDOW);
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
