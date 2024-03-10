use crate::app::{App, AppConfig, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::SnesConfig;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use snes_core::api::SnesAspectRatio;
use std::num::NonZeroU64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnesAppConfig {
    forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    aspect_ratio: SnesAspectRatio,
    #[serde(default = "true_fn")]
    audio_60hz_hack: bool,
    #[serde(default = "default_gsu_overclock")]
    gsu_overclock_factor: NonZeroU64,
    dsp1_rom_path: Option<String>,
    dsp2_rom_path: Option<String>,
    dsp3_rom_path: Option<String>,
    dsp4_rom_path: Option<String>,
    st010_rom_path: Option<String>,
    st011_rom_path: Option<String>,
}

const fn true_fn() -> bool {
    true
}

fn default_gsu_overclock() -> NonZeroU64 {
    NonZeroU64::new(1).unwrap()
}

impl Default for SnesAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    pub(super) fn snes_config(&self, path: String) -> Box<SnesConfig> {
        Box::new(SnesConfig {
            common: self.common_config(
                path,
                self.inputs.snes_keyboard.clone(),
                self.inputs.snes_joystick.clone(),
            ),
            p2_controller_type: self.inputs.snes_p2_type,
            super_scope_config: self.inputs.snes_super_scope.clone(),
            forced_timing_mode: self.snes.forced_timing_mode,
            aspect_ratio: self.snes.aspect_ratio,
            audio_60hz_hack: self.snes.audio_60hz_hack,
            gsu_overclock_factor: self.snes.gsu_overclock_factor,
            dsp1_rom_path: self.snes.dsp1_rom_path.clone(),
            dsp2_rom_path: self.snes.dsp2_rom_path.clone(),
            dsp3_rom_path: self.snes.dsp3_rom_path.clone(),
            dsp4_rom_path: self.snes.dsp4_rom_path.clone(),
            st010_rom_path: self.snes.st010_rom_path.clone(),
            st011_rom_path: self.snes.st011_rom_path.clone(),
        })
    }
}

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
