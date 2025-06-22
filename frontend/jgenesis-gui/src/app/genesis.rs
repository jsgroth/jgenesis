mod helptext;

use crate::app::widgets::{BiosErrorStrings, OptionalPathSelector, RenderErrorEffect};
use crate::app::{App, Console, OpenWindow, widgets};
use crate::emuthread::EmuThreadStatus;
use crate::widgets::OverclockSlider;
use egui::style::ScrollStyle;
use egui::{Context, Slider, Ui, Window};
use genesis_config::PcmInterpolation;
use genesis_config::S32XVideoOut;
use genesis_config::{GenesisAspectRatio, GenesisRegion, Opn2BusyBehavior};
use jgenesis_common::frontend::TimingMode;
use rfd::FileDialog;
use std::num::{NonZeroU16, NonZeroU64};
use std::path::PathBuf;

impl App {
    pub(super) fn render_genesis_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GenesisGeneral;

        let mut open = true;
        Window::new("Genesis General Settings").open(&mut open).resizable(true).show(ctx, |ui| {
            let emu_thread_status = self.emu_thread.status();
            let running_genesis = emu_thread_status == EmuThreadStatus::RunningGenesis
                || emu_thread_status == EmuThreadStatus::RunningSegaCd
                || emu_thread_status == EmuThreadStatus::Running32X;

            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(!running_genesis, |ui| {
                        ui.label("Timing / display mode");

                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut self.config.genesis.forced_timing_mode,
                                None,
                                "Auto",
                            );
                            ui.radio_value(
                                &mut self.config.genesis.forced_timing_mode,
                                Some(TimingMode::Ntsc),
                                "NTSC (60Hz)",
                            );
                            ui.radio_value(
                                &mut self.config.genesis.forced_timing_mode,
                                Some(TimingMode::Pal),
                                "PAL (50Hz)",
                            );
                        });
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::TIMING_MODE);
            }

            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(!running_genesis, |ui| {
                        ui.label("Region");

                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.config.genesis.forced_region, None, "Auto");
                            ui.radio_value(
                                &mut self.config.genesis.forced_region,
                                Some(GenesisRegion::Americas),
                                "Americas",
                            );
                            ui.radio_value(
                                &mut self.config.genesis.forced_region,
                                Some(GenesisRegion::Japan),
                                "Japan",
                            );
                            ui.radio_value(
                                &mut self.config.genesis.forced_region,
                                Some(GenesisRegion::Europe),
                                "Europe",
                            );
                        });
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::REGION);
            }

            ui.add_space(5.0);
            let rect = ui
                .add(OptionalPathSelector::new(
                    "Sega CD BIOS path",
                    &mut self.config.sega_cd.bios_path,
                    pick_scd_bios_path,
                ))
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_BIOS_PATH);
            }

            ui.add_space(5.0);
            let rect = ui
                .checkbox(
                    &mut self.config.sega_cd.enable_ram_cartridge,
                    "(Sega CD) Enable RAM cartridge",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_RAM_CARTRIDGE);
            }

            ui.add_space(5.0);
            let rect = ui
                .checkbox(
                    &mut self.config.sega_cd.load_disc_into_ram,
                    "(Sega CD) Load CD-ROM images into host RAM at startup",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_CDROM_IN_RAM);
            }

            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let rect = ui
                        .add(OverclockSlider {
                            label: "Genesis 68000 clock divider",
                            current_value: &mut self.config.genesis.m68k_clock_divider,
                            range: 1..=7,
                            master_clock: genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY,
                            default_divider: genesis_core::timing::NATIVE_M68K_DIVIDER as f64,
                        })
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::M68K_CLOCK_DIVIDER);
                    }
                });

                ui.vertical(|ui| {
                    let rect = ui
                        .add(OverclockSlider {
                            label: "Sega CD sub 68000 clock divider",
                            current_value: &mut self.config.sega_cd.sub_cpu_divider,
                            range: NonZeroU64::new(1).unwrap()..=NonZeroU64::new(4).unwrap(),
                            master_clock: segacd_core::api::SEGA_CD_MASTER_CLOCK_RATE as f64,
                            default_divider: segacd_core::api::DEFAULT_SUB_CPU_DIVIDER as f64,
                        })
                        .interact_rect;
                    if ui.rect_contains_pointer(rect) {
                        self.state.help_text.insert(WINDOW, helptext::SCD_SUB_CPU_DIVIDER);
                    }
                });
            });

            ui.add_space(5.0);
            let rect = ui
                .group(|ui| {
                    ui.label("Sega CD disc drive speed (low compatibility)");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.sega_cd.disc_drive_speed,
                            NonZeroU16::new(1).unwrap(),
                            "1x (Native)",
                        );
                        ui.radio_value(
                            &mut self.config.sega_cd.disc_drive_speed,
                            NonZeroU16::new(2).unwrap(),
                            "2x",
                        );
                        ui.radio_value(
                            &mut self.config.sega_cd.disc_drive_speed,
                            NonZeroU16::new(3).unwrap(),
                            "3x",
                        );
                        ui.radio_value(
                            &mut self.config.sega_cd.disc_drive_speed,
                            NonZeroU16::new(4).unwrap(),
                            "4x",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_DRIVE_SPEED);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_genesis_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GenesisVideo;

        let mut open = true;
        Window::new("Genesis Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            self.render_aspect_ratio_settings(ui, WINDOW);

            let rect = ui
                .checkbox(&mut self.config.genesis.deinterlace, "Deinterlacing enabled")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::DEINTERLACING);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.genesis.adjust_aspect_ratio_in_2x_resolution,
                    "Automatically double pixel aspect ratio in double-screen interlaced mode",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::DOUBLE_SCREEN_INTERLACED_ASPECT);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.genesis.non_linear_color_scale,
                    "Emulate non-linear VDP color scale",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::NON_LINEAR_COLOR_SCALE);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.genesis.remove_sprite_limits,
                    "Remove sprite-per-scanline and sprite-pixel-per-scanline limits",
                )
                .on_hover_text("Can reduce sprite flickering, but can also cause visual glitches")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMITS);
            }

            let rect = ui
                .checkbox(&mut self.config.genesis.render_vertical_border, "Render vertical border")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::RENDER_BORDERS);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.genesis.render_horizontal_border,
                    "Render horizontal border",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::RENDER_BORDERS);
            }

            ui.add_space(5.0);

            self.render_enabled_layers_setting(ui, WINDOW);

            ui.add_space(5.0);

            self.render_32x_video_out_setting(ui, WINDOW);

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn render_aspect_ratio_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let mut displayed_other_help_text = false;

        let rect = ui
            .group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Auto,
                        "Auto",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Ntsc,
                        "NTSC",
                    )
                    .on_hover_text("32:35 pixel aspect ratio in 320px mode, 8:7 in 256px mode");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Pal,
                        "PAL",
                    )
                    .on_hover_text("11:10 pixel aspect ratio in 320px mode, 11:8 in 256px mode");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.genesis.aspect_ratio,
                        GenesisAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretch image to fill the screen");
                });

                let rect = ui
                    .add_enabled_ui(
                        self.config.genesis.aspect_ratio != GenesisAspectRatio::Stretched,
                        |ui| {
                            ui.checkbox(
                                &mut self.config.genesis.force_square_pixels_in_h40,
                                "Force square pixels in H320px mode",
                            );
                        },
                    )
                    .response
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(window, helptext::FORCE_SQUARE_PIXELS_H40);
                    displayed_other_help_text = true;
                }
            })
            .response
            .interact_rect;
        if !displayed_other_help_text && ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::ASPECT_RATIO);
        }
    }

    fn render_enabled_layers_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Enabled layers");

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.genesis.plane_a_enabled, "Plane A");
                    ui.checkbox(&mut self.config.genesis.plane_b_enabled, "Plane B");
                    ui.checkbox(&mut self.config.genesis.sprites_enabled, "Sprites");
                    ui.checkbox(&mut self.config.genesis.window_enabled, "Window");
                    ui.checkbox(&mut self.config.genesis.backdrop_enabled, "Backdrop");
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::ENABLED_LAYERS);
        }
    }

    fn render_32x_video_out_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("32X video output");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.sega_32x.video_out,
                        S32XVideoOut::Combined,
                        "Combined",
                    );
                    ui.radio_value(
                        &mut self.config.sega_32x.video_out,
                        S32XVideoOut::GenesisOnly,
                        "Genesis VDP only",
                    );
                    ui.radio_value(
                        &mut self.config.sega_32x.video_out,
                        S32XVideoOut::S32XOnly,
                        "32X VDP only",
                    );
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::S32X_VIDEO_OUT);
        }
    }

    pub(super) fn render_genesis_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GenesisAudio;

        let mut open = true;
        Window::new("Genesis Audio Settings").open(&mut open).show(ctx, |ui| {
            ui.ctx().style_mut(|style| style.spacing.scroll = ScrollStyle::solid());

            widgets::render_vertical_scroll_area(ui, |ui| {
                let rect = ui
                    .checkbox(
                        &mut self.config.genesis.quantize_ym2612_output,
                        "Quantize YM2612 channel output",
                    )
                    .on_hover_text("Quantize channel outputs from 14 bits to 9 bits")
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::QUANTIZE_YM2612_OUTPUT);
                }

                let rect = ui
                    .checkbox(
                        &mut self.config.genesis.emulate_ym2612_ladder_effect,
                        "Emulate YM2612 DAC distortion (\"ladder effect\")",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::YM2612_LADDER_EFFECT);
                }

                ui.add_space(5.0);
                ui.group(|ui| {
                    self.render_low_pass_filter_settings(ui);
                });

                ui.add_space(5.0);
                self.render_opn2_busy_flag_setting(ui);

                ui.add_space(5.0);
                self.render_scd_pcm_interpolation_setting(ui);

                ui.add_space(5.0);
                self.render_enabled_sound_sources(ui);
            });

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn render_low_pass_filter_settings(&mut self, ui: &mut Ui) {
        const WINDOW: OpenWindow = OpenWindow::GenesisAudio;

        ui.heading("Low-pass filtering");

        let rect = ui
            .checkbox(
                &mut self.config.genesis.genesis_lpf_enabled,
                "Emulate Genesis first-order low-pass filter",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::GENESIS_LOW_PASS);
        }

        let rect = ui
            .horizontal(|ui| {
                ui.add_space(15.0);

                ui.label("Cutoff frequency");

                ui.add(
                    Slider::new(&mut self.config.genesis.genesis_lpf_cutoff, 1..=15000).text("Hz"),
                );
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::GENESIS_LOW_PASS);
        }

        let rect = ui
            .checkbox(
                &mut self.config.genesis.ym2612_2nd_lpf_enabled,
                "Emulate YM2612 second-order low-pass filter",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::YM2612_2ND_LOW_PASS);
        }

        let rect = ui
            .horizontal(|ui| {
                ui.add_space(15.0);

                ui.label("Cutoff frequency");

                ui.add(
                    Slider::new(&mut self.config.genesis.ym2612_2nd_lpf_cutoff, 1..=15000)
                        .text("Hz"),
                );
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::YM2612_2ND_LOW_PASS);
        }

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label("Presets:");

            if ui.button("Model 1 VA0-VA2").clicked() {
                self.config.genesis.genesis_lpf_enabled = true;
                self.config.genesis.genesis_lpf_cutoff = genesis_config::MODEL_1_VA2_LPF_CUTOFF;
                self.config.genesis.ym2612_2nd_lpf_enabled = false;
            }

            if ui.button("Model 1 VA3-VA6").clicked() {
                self.config.genesis.genesis_lpf_enabled = true;
                self.config.genesis.genesis_lpf_cutoff = genesis_config::MODEL_1_VA3_LPF_CUTOFF;
                self.config.genesis.ym2612_2nd_lpf_enabled = false;
            }

            if ui.button("Model 2").clicked() {
                self.config.genesis.genesis_lpf_enabled = true;
                self.config.genesis.genesis_lpf_cutoff = genesis_config::MODEL_2_1ST_LPF_CUTOFF;
                self.config.genesis.ym2612_2nd_lpf_enabled = true;
                self.config.genesis.ym2612_2nd_lpf_cutoff = genesis_config::MODEL_2_2ND_LPF_CUTOFF;
            }
        });

        ui.separator();

        let rect = ui
            .checkbox(
                &mut self.config.sega_cd.pcm_lpf_enabled,
                "(Sega CD) Apply second-order low-pass filter to PCM chip",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::PCM_LOW_PASS);
        }

        ui.horizontal(|ui| {
            ui.add_space(15.0);

            ui.label("Cutoff frequency");

            ui.add(Slider::new(&mut self.config.sega_cd.pcm_lpf_cutoff, 1..=15000).text("Hz"));

            if ui.button("Default").clicked() {
                self.config.sega_cd.pcm_lpf_cutoff = segacd_core::DEFAULT_PCM_LPF_CUTOFF;
            }
        });

        let rect = ui
            .add_enabled_ui(self.config.genesis.genesis_lpf_enabled, |ui| {
                ui.checkbox(
                    &mut self.config.sega_cd.apply_genesis_lpf_to_pcm,
                    "(Sega CD) Apply Genesis low-pass filter to PCM chip",
                );
                ui.checkbox(
                    &mut self.config.sega_cd.apply_genesis_lpf_to_cd_da,
                    "(Sega CD) Apply Genesis low-pass filter to CD-DA",
                );
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::SCD_GEN_LOW_PASS);
        }

        let rect = ui
            .add_enabled_ui(self.config.genesis.genesis_lpf_enabled, |ui| {
                ui.checkbox(
                    &mut self.config.sega_32x.apply_genesis_lpf_to_pwm,
                    "(32X) Apply Genesis low-pass filter to PWM chip",
                );
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(WINDOW, helptext::S32X_GEN_LOW_PASS);
        }
    }

    fn render_opn2_busy_flag_setting(&mut self, ui: &mut Ui) {
        let rect = ui
            .group(|ui| {
                ui.label("OPN2 busy flag behavior");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.genesis.opn2_busy_behavior,
                        Opn2BusyBehavior::Ym2612,
                        "YM2612",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.opn2_busy_behavior,
                        Opn2BusyBehavior::Ym3438,
                        "YM3438",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.opn2_busy_behavior,
                        Opn2BusyBehavior::AlwaysZero,
                        "Always 0",
                    );
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(OpenWindow::GenesisAudio, helptext::OPN2_BUSY_BEHAVIOR);
        }
    }

    fn render_scd_pcm_interpolation_setting(&mut self, ui: &mut Ui) {
        let rect = ui
            .group(|ui| {
                ui.label("Sega CD PCM chip interpolation");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (PcmInterpolation::None, "None"),
                        (PcmInterpolation::Linear, "Linear"),
                        (PcmInterpolation::CubicHermite, "4-point Cubic"),
                        (PcmInterpolation::CubicHermite6Point, "6-point Cubic"),
                    ] {
                        ui.radio_value(&mut self.config.sega_cd.pcm_interpolation, value, label);
                    }
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(OpenWindow::GenesisAudio, helptext::SCD_PCM_INTERPOLATION);
        }
    }

    fn render_enabled_sound_sources(&mut self, ui: &mut Ui) {
        let rect = ui
            .group(|ui| {
                ui.label("Enabled sound sources");

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.genesis.ym2612_enabled, "YM2612 FM synth chip");
                    ui.checkbox(&mut self.config.genesis.psg_enabled, "SN76489 PSG chip");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.sega_cd.pcm_enabled, "(Sega CD) RF5C164 PCM chip");
                    ui.checkbox(
                        &mut self.config.sega_cd.cd_audio_enabled,
                        "(Sega CD) CD-DA playback",
                    );
                });

                ui.checkbox(&mut self.config.sega_32x.pwm_enabled, "(32X) PWM chip");
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(OpenWindow::GenesisAudio, helptext::SOUND_SOURCES);
        }
    }

    #[must_use]
    pub(super) fn render_scd_bios_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing Sega CD BIOS",
                text: "No Sega CD BIOS path is configured. A Sega CD BIOS ROM is required for Sega CD emulation.",
                button_label: "Configure Sega CD BIOS path",
            },
            &mut self.config.sega_cd.bios_path,
            Console::SegaCd,
            pick_scd_bios_path,
        )
    }
}

fn pick_scd_bios_path() -> Option<PathBuf> {
    FileDialog::new().add_filter("bin", &["bin"]).add_filter("All Types", &["*"]).pick_file()
}
