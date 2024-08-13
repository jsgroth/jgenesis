use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use genesis_core::{GenesisAspectRatio, GenesisRegion};
use jgenesis_common::frontend::TimingMode;
use rfd::FileDialog;
use s32x_core::api::S32XVideoOut;

impl App {
    pub(super) fn render_genesis_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis General Settings").open(&mut open).resizable(true).show(ctx, |ui| {
            let emu_thread_status = self.emu_thread.status();
            let running_genesis = emu_thread_status == EmuThreadStatus::RunningGenesis
                || emu_thread_status == EmuThreadStatus::RunningSegaCd
                || emu_thread_status == EmuThreadStatus::Running32X;

            ui.group(|ui| {
                ui.add_enabled_ui(!running_genesis, |ui| {
                    ui.label("Timing / display mode");

                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.config.genesis.forced_timing_mode, None, "Auto");
                        ui.radio_value(
                            &mut self.config.genesis.forced_timing_mode,
                            Some(TimingMode::Ntsc),
                            "NTSC",
                        );
                        ui.radio_value(
                            &mut self.config.genesis.forced_timing_mode,
                            Some(TimingMode::Pal),
                            "PAL",
                        );
                    });
                });
            });

            ui.group(|ui| {
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
            });

            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    self.emu_thread.status() != EmuThreadStatus::RunningSegaCd,
                    |ui| {
                        let bios_path_str =
                            self.config.sega_cd.bios_path.as_ref().map_or("<None>", String::as_str);
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
            });

            ui.add_space(5.0);
            ui.checkbox(
                &mut self.config.sega_cd.enable_ram_cartridge,
                "Enable Sega CD RAM cartridge",
            );

            ui.add_space(5.0);
            ui.checkbox(
                &mut self.config.sega_cd.load_disc_into_ram,
                "(Sega CD) Load CD-ROM images into RAM at startup",
            )
            .on_hover_text(
                "Significantly increases RAM usage but avoids reading from disk after startup",
            );
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisGeneral);
        }
    }

    pub(super) fn render_genesis_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
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
            });

            ui.checkbox(
                &mut self.config.genesis.adjust_aspect_ratio_in_2x_resolution,
                "Automatically double pixel aspect ratio in double vertical resolution mode",
            );

            ui.checkbox(
                &mut self.config.genesis.remove_sprite_limits,
                "Remove sprite-per-scanline and sprite-pixel-per-scanline limits",
            )
            .on_hover_text("Can reduce sprite flickering, but can also cause visual glitches");

            ui.checkbox(
                &mut self.config.genesis.emulate_non_linear_vdp_dac,
                "Emulate the VDP's non-linear color DAC",
            )
            .on_hover_text("Tends to brighten darker colors and darken brighter colors");

            ui.checkbox(&mut self.config.genesis.render_vertical_border, "Render vertical border");

            ui.checkbox(
                &mut self.config.genesis.render_horizontal_border,
                "Render horizontal border",
            );

            ui.add_space(5.0);

            ui.group(|ui| {
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
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisVideo);
        }
    }

    pub(super) fn render_genesis_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("Genesis Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(
                &mut self.config.genesis.quantize_ym2612_output,
                "Quantize YM2612 channel output",
            )
            .on_hover_text(
                "Quantize channel outputs from 14 bits to 9 bits to emulate the YM2612's 9-bit DAC",
            );

            ui.checkbox(
                &mut self.config.genesis.emulate_ym2612_ladder_effect,
                "Emulate YM2612 DAC distortion (\"ladder effect\")",
            )
            .on_hover_text("Effectively amplifies low-volume waves");

            ui.group(|ui| {
                ui.label("Enabled sound sources");

                ui.checkbox(&mut self.config.genesis.ym2612_enabled, "YM2612 FM chip");
                ui.checkbox(&mut self.config.genesis.psg_enabled, "SN76489 PSG chip");
                ui.checkbox(&mut self.config.sega_cd.pcm_enabled, "RF5C164 PCM chip (Sega CD)");
                ui.checkbox(&mut self.config.sega_cd.cd_audio_enabled, "CD-DA playback (Sega CD)");
                ui.checkbox(&mut self.config.sega_32x.pwm_enabled, "PWM chip (32X)");
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::GenesisAudio);
        }
    }
}
