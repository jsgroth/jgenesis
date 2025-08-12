mod helptext;

use crate::app::widgets::{BiosErrorStrings, OptionalPathSelector, RenderErrorEffect};
use crate::app::{App, OpenWindow, widgets};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Slider, Ui, Window};
use gb_config::{GbAspectRatio, GbAudioResampler, GbPalette, GbcColorCorrection};
use jgenesis_native_config::gb::GameBoyAppConfig;
use jgenesis_native_driver::extensions::Console;
use rfd::FileDialog;
use std::path::PathBuf;

impl App {
    pub(super) fn render_gb_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GameBoyGeneral;

        let mut open = true;
        Window::new("Game Boy General Settings")
            .default_width(400.0)
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let rect = ui
                    .checkbox(
                        &mut self.config.game_boy.pretend_to_be_gba,
                        "Pretend to be a Game Boy Advance",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::PRETEND_GBA_MODE);
                }

                ui.add_space(5.0);

                let running_gb = self.emu_thread.status() != EmuThreadStatus::RunningGameBoy;
                ui.add_enabled_ui(running_gb, |ui| {
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
                            &mut self.config.game_boy.force_cgb_mode,
                            "Force CGB mode in DMG-only software (requires CGB boot ROM)",
                        )
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::FORCE_CGB_MODE);
                    }

                    // Require CGB boot ROM when forcing CGB mode
                    self.config.game_boy.cgb_boot_rom |= self.config.game_boy.force_cgb_mode;

                    ui.add_space(5.0);

                    let rect = ui
                        .checkbox(
                            &mut self.config.game_boy.dmg_boot_rom,
                            "Boot from boot ROM in DMG mode",
                        )
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::BOOT_ROM);
                    }

                    ui.add_enabled_ui(!self.config.game_boy.force_cgb_mode, |ui| {
                        let rect = ui
                            .checkbox(
                                &mut self.config.game_boy.cgb_boot_rom,
                                "Boot from boot ROM in CGB mode",
                            )
                            .interact_rect;
                        if ui.rect_contains_pointer(rect) {
                            self.state.help_text.insert(WINDOW, helptext::BOOT_ROM);
                        }
                    });

                    ui.add_space(5.0);

                    let rect = ui
                        .add(OptionalPathSelector::new(
                            "DMG boot ROM",
                            &mut self.config.game_boy.dmg_boot_rom_path,
                            || pick_boot_rom_path("gb"),
                        ))
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::BOOT_ROM);
                    }

                    let rect = ui
                        .add(OptionalPathSelector::new(
                            "CGB boot ROM",
                            &mut self.config.game_boy.cgb_boot_rom_path,
                            || pick_boot_rom_path("gbc"),
                        ))
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::BOOT_ROM);
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

            ui.add_space(5.0);

            let rect = ui
                .checkbox(&mut self.config.game_boy.frame_blending, "Frame blending enabled")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::FRAME_BLENDING);
            }

            ui.add_space(5.0);

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

                    ui.add_enabled_ui(
                        self.config.game_boy.gbc_color_correction != GbcColorCorrection::None,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Console screen gamma");

                                let (gamma, default) =
                                    match self.config.game_boy.gbc_color_correction {
                                        GbcColorCorrection::GbaLcd => (
                                            &mut self.config.game_boy.gba_correction_gamma,
                                            GameBoyAppConfig::default().gba_correction_gamma,
                                        ),
                                        _ => (
                                            &mut self.config.game_boy.gbc_correction_gamma,
                                            GameBoyAppConfig::default().gbc_correction_gamma,
                                        ),
                                    };

                                ui.add(
                                    Slider::new(gamma, 0.1..=5.0)
                                        .custom_formatter(|value, _| format!("{value:.1}")),
                                );

                                if ui.button("Default").clicked() {
                                    *gamma = default;
                                }
                            });
                        },
                    );
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

    #[must_use]
    pub(super) fn render_dmg_boot_rom_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing DMG Boot ROM",
                text: "The boot from boot ROM option is set but no DMG boot ROM is configured.",
                button_label: "Configure DMG boot ROM",
            },
            &mut self.config.game_boy.dmg_boot_rom_path,
            Console::GameBoy,
            || pick_boot_rom_path("gb"),
        )
    }

    #[must_use]
    pub(super) fn render_cgb_boot_rom_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing CGB Boot ROM",
                text: "The boot from boot ROM option is set but no CGB boot ROM is configured.",
                button_label: "Configure CGB boot ROM",
            },
            &mut self.config.game_boy.cgb_boot_rom_path,
            Console::GameBoyColor,
            || pick_boot_rom_path("gbc"),
        )
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

fn pick_boot_rom_path(default_extension: &str) -> Option<PathBuf> {
    FileDialog::new()
        .add_filter("Boot ROM", &[default_extension, "bin"])
        .add_filter("All Files", &["*"])
        .pick_file()
}
