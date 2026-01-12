mod helptext;

use crate::app::widgets::{
    BiosErrorStrings, NumericTextEdit, OptionalPathSelector, RenderErrorEffect,
};
use crate::app::{App, Console, OpenWindow, widgets};
use crate::emuthread::EmuThreadStatus;
use crate::widgets::{ClockModifier, OverclockSlider};
use egui::style::ScrollStyle;
use egui::{Color32, Context, Slider, Ui, Window};
use genesis_config::{GenesisAspectRatio, GenesisRegion, Opn2BusyBehavior};
use genesis_config::{PcmInterpolation, S32XColorTint};
use genesis_config::{S32XVideoOut, S32XVoidColorType};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::genesis::Sega32XAppConfig;
use rfd::FileDialog;
use std::num::{NonZeroU16, NonZeroU64};
use std::path::PathBuf;

pub struct S32XPriorityState {
    pub void_palette: String,
    pub void_direct_r: String,
    pub void_direct_g: String,
    pub void_direct_b: String,
}

impl S32XPriorityState {
    pub fn from_config(config: &Sega32XAppConfig) -> Self {
        Self {
            void_palette: config.void_palette_index.to_string(),
            void_direct_r: config.void_direct[0].to_string(),
            void_direct_g: config.void_direct[1].to_string(),
            void_direct_b: config.void_direct[2].to_string(),
        }
    }
}

pub struct GenesisVolumeState {
    pub ym2612_text: String,
    pub ym2612_invalid: bool,
    pub psg_text: String,
    pub psg_invalid: bool,
    pub pcm_text: String,
    pub pcm_invalid: bool,
    pub cd_text: String,
    pub cd_invalid: bool,
    pub pwm_text: String,
    pub pwm_invalid: bool,
}

impl GenesisVolumeState {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            ym2612_text: fmt_volume(config.genesis.ym2612_volume_adjustment_db),
            ym2612_invalid: false,
            psg_text: fmt_volume(config.genesis.psg_volume_adjustment_db),
            psg_invalid: false,
            pcm_text: fmt_volume(config.sega_cd.pcm_volume_adjustment_db),
            pcm_invalid: false,
            cd_text: fmt_volume(config.sega_cd.cd_volume_adjustment_db),
            cd_invalid: false,
            pwm_text: fmt_volume(config.sega_32x.pwm_volume_adjustment_db),
            pwm_invalid: false,
        }
    }

    fn any_invalid(&self) -> bool {
        self.ym2612_invalid
            || self.psg_invalid
            || self.pcm_invalid
            || self.cd_invalid
            || self.pwm_invalid
    }
}

fn fmt_volume(volume_adjustment: f64) -> String {
    format!("{volume_adjustment:.1}")
}

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
                .group(|ui| {
                    ui.label("Sega CD BIOS paths");

                    ui.add(OptionalPathSelector::new(
                        "US",
                        &mut self.config.sega_cd.bios_path,
                        pick_scd_bios_path,
                    ));

                    ui.add_enabled_ui(self.config.sega_cd.per_region_bios, |ui| {
                        ui.add(OptionalPathSelector::new(
                            "Europe",
                            &mut self.config.sega_cd.eu_bios_path,
                            pick_scd_bios_path,
                        ));
                        ui.add(OptionalPathSelector::new(
                            "Japan",
                            &mut self.config.sega_cd.jp_bios_path,
                            pick_scd_bios_path,
                        ));
                    });

                    let mut us_bios_only = !self.config.sega_cd.per_region_bios;
                    ui.checkbox(&mut us_bios_only, "Use US BIOS for all regions");
                    self.config.sega_cd.per_region_bios = !us_bios_only;
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
                    "(Sega CD) Enable RAM cartridge",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_RAM_CARTRIDGE);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.sega_cd.load_disc_into_ram,
                    "(Sega CD) Load CD-ROM images into host RAM at startup",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCD_CDROM_IN_RAM);
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
            widgets::render_vertical_scroll_area(ui, |ui| {
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
                    .on_hover_text(
                        "Can reduce sprite flickering, but can also cause visual glitches",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMITS);
                }

                let rect = ui
                    .checkbox(
                        &mut self.config.genesis.render_vertical_border,
                        "Render vertical border",
                    )
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

                self.render_32x_video_settings(ui, WINDOW);
            });

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

    fn render_32x_video_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .checkbox(
                &mut self.config.sega_32x.darken_genesis_colors,
                "(32X) Darken Genesis colors",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::S32X_DARKEN_GEN_COLORS);
        }

        ui.add_space(5.0);

        let rect = ui
            .group(|ui| {
                ui.label("32X color tint");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (S32XColorTint::None, "None"),
                        (S32XColorTint::SlightYellow, "Slight yellow"),
                        (S32XColorTint::Yellow, "Yellow"),
                        (S32XColorTint::SlightPurple, "Slight purple"),
                        (S32XColorTint::Purple, "Purple"),
                    ] {
                        ui.radio_value(&mut self.config.sega_32x.color_tint, value, label);
                    }
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::S32X_COLOR_TINT);
        }

        let rect = ui
            .group(|ui| {
                ui.label("32X video output");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (S32XVideoOut::Combined, "Combined"),
                        (S32XVideoOut::GenesisOnly, "Genesis VDP only"),
                        (S32XVideoOut::S32XOnly, "32X VDP only"),
                    ] {
                        ui.radio_value(&mut self.config.sega_32x.video_out, value, label);
                    }
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::S32X_VIDEO_OUT);
        }

        let rect = ui
            .checkbox(
                &mut self.config.sega_32x.emulate_pixel_switch_delay,
                "(32X) Emulate pixel switch delay",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::S32X_PIXEL_SWITCH_DELAY);
        }

        let response = ui.collapsing("32X priority masking", |ui| {
            ui.checkbox(&mut self.config.sega_32x.show_high_priority, "Show high-priority pixels");
            ui.checkbox(&mut self.config.sega_32x.show_low_priority, "Show low-priority pixels");

            ui.add_space(5.0);

            ui.label("Replace hidden pixels with:");

            let mut invalid_ignored = false;

            let valid_rgb5 = |color: u8| color < 32;

            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut self.config.sega_32x.void_color_type,
                    S32XVoidColorType::Direct,
                    "Direct color (0-31):",
                );

                ui.label("R");
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.s32x_priority.void_direct_r,
                        &mut self.config.sega_32x.void_direct[0],
                        &mut invalid_ignored,
                    )
                    .with_validation(valid_rgb5)
                    .desired_width(20.0),
                );

                ui.label("G");
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.s32x_priority.void_direct_g,
                        &mut self.config.sega_32x.void_direct[1],
                        &mut invalid_ignored,
                    )
                    .with_validation(valid_rgb5)
                    .desired_width(20.0),
                );

                ui.label("B");
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.s32x_priority.void_direct_b,
                        &mut self.config.sega_32x.void_direct[2],
                        &mut invalid_ignored,
                    )
                    .with_validation(valid_rgb5)
                    .desired_width(20.0),
                );

                ui.label("P");
                ui.checkbox(&mut self.config.sega_32x.void_direct_priority, "");
            });

            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut self.config.sega_32x.void_color_type,
                    S32XVoidColorType::PaletteRam,
                    "Palette color (0-255):",
                );

                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.s32x_priority.void_palette,
                        &mut self.config.sega_32x.void_palette_index,
                        &mut invalid_ignored,
                    )
                    .desired_width(40.0),
                );
            });
        });
        let contains_rect = ui.rect_contains_pointer(response.header_response.interact_rect)
            || response
                .body_response
                .is_some_and(|resp| ui.rect_contains_pointer(resp.interact_rect));
        if contains_rect {
            self.state.help_text.insert(window, helptext::S32X_PRIORITY_MASKING);
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
                self.render_scd_pcm_interpolation_setting(ui);

                ui.add_space(5.0);
                self.render_volume_adjustments(ui);

                ui.add_space(5.0);
                self.render_enabled_sound_sources(ui);

                ui.add_space(5.0);
                self.render_ym2612_channels_enabled(ui);

                ui.add_space(5.0);
                self.render_opn2_busy_flag_setting(ui);
            });

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_genesis_overclock_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::GenesisOverclock;

        let mut open = true;
        Window::new("Genesis Overclocking Settings").open(&mut open).resizable(false).show(
            ctx,
            |ui| {
                let rect = ui
                    .add(OverclockSlider {
                        label: "Genesis 68000 clock divider",
                        current_value: &mut self.config.genesis.m68k_clock_divider,
                        range: 1..=7,
                        master_clock: genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY,
                        default_divider: genesis_core::timing::NATIVE_M68K_DIVIDER as f64,
                        modifier: ClockModifier::Divider,
                    })
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::M68K_CLOCK_DIVIDER);
                }

                ui.add_space(5.0);
                let rect = ui
                    .add(OverclockSlider {
                        label: "Sega CD sub 68000 clock divider",
                        current_value: &mut self.config.sega_cd.sub_cpu_divider,
                        range: NonZeroU64::new(1).unwrap()..=NonZeroU64::new(4).unwrap(),
                        master_clock: segacd_core::api::SEGA_CD_MASTER_CLOCK_RATE as f64,
                        default_divider: segacd_core::api::DEFAULT_SUB_CPU_DIVIDER as f64,
                        modifier: ClockModifier::Divider,
                    })
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::SCD_SUB_CPU_DIVIDER);
                }

                ui.add_space(5.0);
                let rect = ui
                    .add(OverclockSlider {
                        label: "32X SH-2 clock multiplier",
                        current_value: &mut self.config.sega_32x.sh2_clock_multiplier,
                        range: NonZeroU64::new(3).unwrap()..=NonZeroU64::new(10).unwrap(),
                        master_clock: genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY / 7.0,
                        default_divider: genesis_config::NATIVE_SH2_MULTIPLIER as f64,
                        modifier: ClockModifier::Multiplier,
                    })
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::SH2_CLOCK_MULTIPLIER);
                }

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
            },
        );
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

            if ui.button("Model 1 VA2").clicked() {
                self.config.genesis.genesis_lpf_enabled = true;
                self.config.genesis.genesis_lpf_cutoff = genesis_config::MODEL_1_VA2_LPF_CUTOFF;
                self.config.genesis.ym2612_2nd_lpf_enabled = false;
            }

            if ui.button("Model 1 VA3").clicked() {
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

            if ui.button("None").clicked() {
                self.config.genesis.genesis_lpf_enabled = false;
                self.config.genesis.ym2612_2nd_lpf_enabled = false;
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

    fn render_ym2612_channels_enabled(&mut self, ui: &mut Ui) {
        let rect = ui
            .group(|ui| {
                ui.label("Enabled YM2612 channels");

                ui.horizontal(|ui| {
                    for i in 0..6 {
                        ui.checkbox(
                            &mut self.config.genesis.ym2612_channels_enabled[i],
                            (i + 1).to_string(),
                        );
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Enable all").clicked() {
                        self.config.genesis.ym2612_channels_enabled.fill(true);
                    }

                    if ui.button("Disable all").clicked() {
                        self.config.genesis.ym2612_channels_enabled.fill(false);
                    }
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state
                .help_text
                .insert(OpenWindow::GenesisAudio, helptext::ENABLED_YM2612_CHANNELS);
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

    fn render_volume_adjustments(&mut self, ui: &mut Ui) {
        let rect = ui
            .group(|ui| {
                ui.label("Volume adjustments (dB) (+/-)");

                ui.add_space(2.0);

                render_volume_adjustment(
                    "YM2612 FM synth chip",
                    &mut self.state.genesis_volume.ym2612_text,
                    &mut self.state.genesis_volume.ym2612_invalid,
                    &mut self.config.genesis.ym2612_volume_adjustment_db,
                    ui,
                );
                render_volume_adjustment(
                    "SN76489 PSG chip",
                    &mut self.state.genesis_volume.psg_text,
                    &mut self.state.genesis_volume.psg_invalid,
                    &mut self.config.genesis.psg_volume_adjustment_db,
                    ui,
                );
                render_volume_adjustment(
                    "(Sega CD) RF5C164 PCM chip",
                    &mut self.state.genesis_volume.pcm_text,
                    &mut self.state.genesis_volume.pcm_invalid,
                    &mut self.config.sega_cd.pcm_volume_adjustment_db,
                    ui,
                );
                render_volume_adjustment(
                    "(Sega CD) CD-DA playback",
                    &mut self.state.genesis_volume.cd_text,
                    &mut self.state.genesis_volume.cd_invalid,
                    &mut self.config.sega_cd.cd_volume_adjustment_db,
                    ui,
                );
                render_volume_adjustment(
                    "(32X) PWM chip",
                    &mut self.state.genesis_volume.pwm_text,
                    &mut self.state.genesis_volume.pwm_invalid,
                    &mut self.config.sega_32x.pwm_volume_adjustment_db,
                    ui,
                );

                if ui.button("Clear all").clicked() {
                    self.config.genesis.ym2612_volume_adjustment_db = 0.0;
                    self.config.genesis.psg_volume_adjustment_db = 0.0;
                    self.config.sega_cd.pcm_volume_adjustment_db = 0.0;
                    self.config.sega_cd.cd_volume_adjustment_db = 0.0;
                    self.config.sega_32x.pwm_volume_adjustment_db = 0.0;

                    self.state.genesis_volume = GenesisVolumeState::from_config(&self.config);
                }

                if self.state.genesis_volume.any_invalid() {
                    ui.colored_label(Color32::RED, "Values must be numbers");
                }
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(OpenWindow::GenesisAudio, helptext::VOLUME_ADJUSTMENTS);
        }
    }

    #[must_use]
    pub(super) fn render_scd_bios_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
        region: GenesisRegion,
    ) -> RenderErrorEffect {
        let bios_path = match region {
            GenesisRegion::Americas => &mut self.config.sega_cd.bios_path,
            GenesisRegion::Europe => &mut self.config.sega_cd.eu_bios_path,
            GenesisRegion::Japan => &mut self.config.sega_cd.jp_bios_path,
        };

        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: format!("Missing Sega CD {} BIOS", region.short_name()),
                text: format!(
                    "No Sega CD {} BIOS path is configured. A Sega CD BIOS ROM is required for Sega CD emulation.",
                    region.long_name()
                ),
                button_label: format!("Configure Sega CD {} BIOS path", region.short_name()),
            },
            bios_path,
            Console::SegaCd,
            pick_scd_bios_path,
        )
    }
}

fn render_volume_adjustment(
    label: &str,
    text: &mut String,
    invalid: &mut bool,
    value: &mut f64,
    ui: &mut Ui,
) {
    ui.horizontal(|ui| {
        ui.add(
            NumericTextEdit::new(text, value, invalid)
                .desired_width(40.0)
                .with_validation(f64::is_finite),
        );

        ui.label(label);
    });
}

fn pick_scd_bios_path() -> Option<PathBuf> {
    FileDialog::new().add_filter("bin", &["bin"]).add_filter("All Types", &["*"]).pick_file()
}
