mod helptext;

use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Slider, Window};
use genesis_core::audio::LowPassFilter;
use genesis_core::{GenesisAspectRatio, GenesisRegion};
use jgenesis_common::frontend::TimingMode;
use rfd::FileDialog;
use s32x_core::api::S32XVideoOut;
use segacd_core::api::PcmInterpolation;

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
                .horizontal(|ui| {
                    ui.add_enabled_ui(
                        self.emu_thread.status() != EmuThreadStatus::RunningSegaCd,
                        |ui| {
                            let bios_path_str = self
                                .config
                                .sega_cd
                                .bios_path
                                .as_ref()
                                .map_or("<None>", String::as_str);
                            if ui.button(bios_path_str).clicked() {
                                if let Some(bios_path) =
                                    FileDialog::new().add_filter("bin", &["bin"]).pick_file()
                                {
                                    self.config.sega_cd.bios_path =
                                        Some(bios_path.to_string_lossy().to_string());
                                }
                            }

                            ui.label("Sega CD BIOS path");
                        },
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_BIOS_PATH);
            }

            ui.add_space(5.0);
            let rect = ui
                .checkbox(
                    &mut self.config.sega_cd.enable_ram_cartridge,
                    "Enable Sega CD RAM cartridge",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_RAM_CARTRIDGE);
            }

            ui.add_space(5.0);
            let rect = ui
                .checkbox(
                    &mut self.config.sega_cd.load_disc_into_ram,
                    "(Sega CD) Load CD-ROM images into RAM at startup",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_CDROM_IN_RAM);
            }

            ui.add_space(5.0);
            let rect = ui
                .group(|ui| {
                    ui.label("Genesis 68000 clock divider");
                    ui.add(Slider::new(&mut self.config.genesis.m68k_clock_divider, 1..=7));

                    let effective_speed_ratio = 100.0
                        * genesis_core::timing::NATIVE_M68K_DIVIDER as f64
                        / self.config.genesis.m68k_clock_divider as f64;
                    let effective_speed_mhz = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY
                        / self.config.genesis.m68k_clock_divider as f64
                        / 1_000_000.0;
                    ui.label(format!(
                        "Effective speed: {effective_speed_mhz:.2} MHz ({}%)",
                        effective_speed_ratio.round()
                    ));
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::M68K_CLOCK_DIVIDER);
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
            let rect = ui
                .group(|ui| {
                    ui.label("Aspect ratio");

                    ui.horizontal(|ui| {
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
                        .on_hover_text(
                            "11:10 pixel aspect ratio in 320px mode, 11:8 in 256px mode",
                        );
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
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ASPECT_RATIO);
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
                    &mut self.config.genesis.remove_sprite_limits,
                    "Remove sprite-per-scanline and sprite-pixel-per-scanline limits",
                )
                .on_hover_text("Can reduce sprite flickering, but can also cause visual glitches")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMITS);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.genesis.emulate_non_linear_vdp_dac,
                    "Emulate the VDP's non-linear color DAC",
                )
                .on_hover_text("Tends to brighten darker colors and darken brighter colors")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::NON_LINEAR_COLOR_DAC);
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
                self.state.help_text.insert(WINDOW, helptext::ENABLED_LAYERS);
            }

            ui.add_space(5.0);

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
                self.state.help_text.insert(WINDOW, helptext::S32X_VIDEO_OUT);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_genesis_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GenesisAudio;

        let mut open = true;
        Window::new("Genesis Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui.checkbox(
                &mut self.config.genesis.quantize_ym2612_output,
                "Quantize YM2612 channel output",
            )
            .on_hover_text(
                "Quantize channel outputs from 14 bits to 9 bits to emulate the YM2612's 9-bit DAC",
            ).interact_rect;
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

            let rect = ui
                .group(|ui| {
                    ui.label("Low-pass filter");

                    ui.radio_value(
                        &mut self.config.genesis.low_pass_filter,
                        LowPassFilter::Sharp,
                        "Sharp (~15000 Hz cutoff)",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.low_pass_filter,
                        LowPassFilter::Moderate,
                        "Moderate (~10000 Hz cutoff)",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.low_pass_filter,
                        LowPassFilter::Soft,
                        "Soft (~8000 Hz cutoff)",
                    );
                    ui.radio_value(
                        &mut self.config.genesis.low_pass_filter,
                        LowPassFilter::VerySoft,
                        "Very soft (~5000 Hz cutoff)",
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::LOW_PASS_FILTER);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("Sega CD PCM chip interpolation");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.sega_cd.pcm_interpolation,
                            PcmInterpolation::None,
                            "None",
                        );
                        ui.radio_value(
                            &mut self.config.sega_cd.pcm_interpolation,
                            PcmInterpolation::Linear,
                            "Linear",
                        );
                        ui.radio_value(
                            &mut self.config.sega_cd.pcm_interpolation,
                            PcmInterpolation::CubicHermite,
                            "Cubic",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_PCM_INTERPOLATION);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("Enabled sound sources");

                    ui.checkbox(&mut self.config.genesis.ym2612_enabled, "YM2612 FM chip");
                    ui.checkbox(&mut self.config.genesis.psg_enabled, "SN76489 PSG chip");
                    ui.checkbox(&mut self.config.sega_cd.pcm_enabled, "RF5C164 PCM chip (Sega CD)");
                    ui.checkbox(
                        &mut self.config.sega_cd.cd_audio_enabled,
                        "CD-DA playback (Sega CD)",
                    );
                    ui.checkbox(&mut self.config.sega_32x.pwm_enabled, "PWM chip (32X)");
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SOUND_SOURCES);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}
