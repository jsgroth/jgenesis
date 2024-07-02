use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use jgenesis_common::frontend::TimingMode;
use rfd::FileDialog;
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio};
use std::num::NonZeroU64;

impl App {
    pub(super) fn render_snes_general_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.set_enabled(self.emu_thread.status() != EmuThreadStatus::RunningSnes);

                ui.label("Timing / display mode");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.snes.forced_timing_mode, None, "Auto");
                    ui.radio_value(
                        &mut self.config.snes.forced_timing_mode,
                        Some(TimingMode::Ntsc),
                        "NTSC",
                    );
                    ui.radio_value(
                        &mut self.config.snes.forced_timing_mode,
                        Some(TimingMode::Pal),
                        "PAL",
                    );
                });
            });

            ui.group(|ui| {
                ui.label("Super FX GSU overclock factor");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.snes.gsu_overclock_factor,
                        NonZeroU64::new(1).unwrap(),
                        "None",
                    );
                    ui.radio_value(
                        &mut self.config.snes.gsu_overclock_factor,
                        NonZeroU64::new(2).unwrap(),
                        "2x",
                    );
                    ui.radio_value(
                        &mut self.config.snes.gsu_overclock_factor,
                        NonZeroU64::new(3).unwrap(),
                        "3x",
                    );
                    ui.radio_value(
                        &mut self.config.snes.gsu_overclock_factor,
                        NonZeroU64::new(4).unwrap(),
                        "4x",
                    );
                });
            });

            ui.horizontal(|ui| {
                let dsp1_rom_path = self.config.snes.dsp1_rom_path.as_deref();
                if ui.button(dsp1_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.dsp1_rom_path);
                }

                ui.label("DSP-1 ROM path");
            });

            ui.horizontal(|ui| {
                let dsp2_rom_path = self.config.snes.dsp2_rom_path.as_deref();
                if ui.button(dsp2_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.dsp2_rom_path);
                }

                ui.label("DSP-2 ROM path");
            });

            ui.horizontal(|ui| {
                let dsp3_rom_path = self.config.snes.dsp3_rom_path.as_deref();
                if ui.button(dsp3_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.dsp3_rom_path);
                }

                ui.label("DSP-3 ROM path");
            });

            ui.horizontal(|ui| {
                let dsp4_rom_path = self.config.snes.dsp4_rom_path.as_deref();
                if ui.button(dsp4_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.dsp4_rom_path);
                }

                ui.label("DSP-4 ROM path");
            });

            ui.horizontal(|ui| {
                let st010_rom_path = self.config.snes.st010_rom_path.as_deref();
                if ui.button(st010_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.st010_rom_path);
                }

                ui.label("ST010 ROM path");
            });

            ui.horizontal(|ui| {
                let st011_rom_path = self.config.snes.st011_rom_path.as_deref();
                if ui.button(st011_rom_path.unwrap_or("<None>")).clicked() {
                    pick_coprocessor_rom_path(&mut self.config.snes.st011_rom_path);
                }

                ui.label("ST011 ROM path");
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesGeneral);
        }
    }

    pub(super) fn render_snes_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.snes.aspect_ratio,
                        SnesAspectRatio::Ntsc,
                        "NTSC",
                    )
                    .on_hover_text("8:7 pixel aspect ratio");
                    ui.radio_value(&mut self.config.snes.aspect_ratio, SnesAspectRatio::Pal, "PAL")
                        .on_hover_text("11:8 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.snes.aspect_ratio,
                        SnesAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.snes.aspect_ratio,
                        SnesAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretched to fill the window");
                });
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesVideo);
        }
    }

    pub(super) fn render_snes_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("SNES Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("ADPCM sample interpolation");

                ui.radio_value(&mut self.config.snes.audio_interpolation, AudioInterpolationMode::Gaussian, "Gaussian (Native)")
                    .on_hover_text("Emulate actual hardware's behavior");
                ui.radio_value(&mut self.config.snes.audio_interpolation, AudioInterpolationMode::Hermite, "Cubic Hermite (Sharp)")
                    .on_hover_text("More advanced algorithm than actual hardware");
            });

            ui.add_space(10.0);

            ui.checkbox(&mut self.config.snes.audio_60hz_hack, "Enable audio 60Hz/50Hz hack")
                .on_hover_text("Enabling this option will very slightly increase the audio signal frequency to time to 60Hz NTSC / 50Hz PAL");
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::SnesAudio);
        }
    }
}

fn pick_coprocessor_rom_path(out_path: &mut Option<String>) {
    let Some(path) = FileDialog::new().pick_file() else { return };

    match path.to_str() {
        Some(path) => {
            *out_path = Some(path.into());
        }
        None => {
            log::error!("Unable to convert path to string: '{}'", path.display());
        }
    }
}
