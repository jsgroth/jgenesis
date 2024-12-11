mod helptext;

use crate::app::{App, Console, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use egui::{Context, Window};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_config::snes::SnesAppConfig;
use rfd::FileDialog;
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio, SnesLoadError};
use std::num::NonZeroU64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandledError {
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoprocessorRom {
    Dsp1,
    Dsp2,
    Dsp3,
    Dsp4,
    St010,
    St011,
}

impl CoprocessorRom {
    fn name(self) -> &'static str {
        match self {
            Self::Dsp1 => "DSP-1",
            Self::Dsp2 => "DSP-2",
            Self::Dsp3 => "DSP-3",
            Self::Dsp4 => "DSP-4",
            Self::St010 => "ST010",
            Self::St011 => "ST011",
        }
    }

    fn path_field(self, config: &mut SnesAppConfig) -> &mut Option<String> {
        match self {
            Self::Dsp1 => &mut config.dsp1_rom_path,
            Self::Dsp2 => &mut config.dsp2_rom_path,
            Self::Dsp3 => &mut config.dsp3_rom_path,
            Self::Dsp4 => &mut config.dsp4_rom_path,
            Self::St010 => &mut config.st010_rom_path,
            Self::St011 => &mut config.st011_rom_path,
        }
    }
}

impl App {
    pub(super) fn render_snes_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::SnesGeneral;

        let mut open = true;
        Window::new("SNES General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(
                        self.emu_thread.status() != EmuThreadStatus::RunningSnes,
                        |ui| {
                            ui.label("Timing / display mode");

                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut self.config.snes.forced_timing_mode,
                                    None,
                                    "Auto",
                                );
                                ui.radio_value(
                                    &mut self.config.snes.forced_timing_mode,
                                    Some(TimingMode::Ntsc),
                                    "NTSC (60Hz)",
                                );
                                ui.radio_value(
                                    &mut self.config.snes.forced_timing_mode,
                                    Some(TimingMode::Pal),
                                    "PAL (50Hz)",
                                );
                            });
                        },
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::TIMING_MODE);
            }

            let rect = ui
                .group(|ui| {
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
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SUPER_FX_OVERCLOCK);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("Coprocessor ROM Paths");

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
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::COPROCESSOR_ROM_PATHS);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_snes_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::SnesVideo;

        let mut open = true;
        Window::new("SNES Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Aspect ratio");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.snes.aspect_ratio,
                            SnesAspectRatio::Ntsc,
                            "NTSC",
                        )
                        .on_hover_text("8:7 pixel aspect ratio");
                        ui.radio_value(
                            &mut self.config.snes.aspect_ratio,
                            SnesAspectRatio::Pal,
                            "PAL",
                        )
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
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ASPECT_RATIO);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_snes_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::SnesAudio;

        let mut open = true;
        Window::new("SNES Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("ADPCM sample interpolation");

                    ui.radio_value(
                        &mut self.config.snes.audio_interpolation,
                        AudioInterpolationMode::Gaussian,
                        "Gaussian (Native)",
                    )
                    .on_hover_text("Emulate actual hardware's behavior");
                    ui.radio_value(
                        &mut self.config.snes.audio_interpolation,
                        AudioInterpolationMode::Hermite,
                        "Cubic Hermite (Sharp)",
                    )
                    .on_hover_text("More advanced algorithm than actual hardware");
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ADPCM_INTERPOLATION);
            }

            ui.add_space(10.0);

            let rect = ui
                .checkbox(&mut self.config.snes.audio_60hz_hack, "Enable audio sync timing hack")
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
    pub(super) fn render_snes_load_error(
        &mut self,
        ctx: &Context,
        err: &SnesLoadError,
        open: &mut bool,
    ) -> HandledError {
        let coprocessor_rom = match err {
            SnesLoadError::MissingDsp1Rom => CoprocessorRom::Dsp1,
            SnesLoadError::MissingDsp2Rom => CoprocessorRom::Dsp2,
            SnesLoadError::MissingDsp3Rom => CoprocessorRom::Dsp3,
            SnesLoadError::MissingDsp4Rom => CoprocessorRom::Dsp4,
            SnesLoadError::MissingSt010Rom => CoprocessorRom::St010,
            SnesLoadError::MissingSt011Rom => CoprocessorRom::St011,
            SnesLoadError::CoprocessorRomLoad { .. } => return HandledError::No,
        };

        let name = coprocessor_rom.name();
        let mut configured_path = false;
        Window::new(format!("Missing {name} ROM")).open(open).resizable(false).show(ctx, |ui| {
            ui.label(format!("No {name} firmware ROM is configured. This is required to run games that use the {name} coprocessor."));

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("Configure now:");

                if ui.button(format!("Configure {name} ROM path")).clicked() {
                    let path_field = coprocessor_rom.path_field(&mut self.config.snes);
                    pick_coprocessor_rom_path(path_field);
                    configured_path = path_field.is_some();
                }
            });
        });

        if configured_path {
            *open = false;
            self.launch_emulator(self.state.current_file_path.clone(), Some(Console::Snes));
        }

        HandledError::Yes
    }
}

fn pick_coprocessor_rom_path(out_path: &mut Option<String>) {
    let Some(path) = FileDialog::new()
        .add_filter("bin", &["rom", "bin"])
        .add_filter("All Types", &["*"])
        .pick_file()
    else {
        return;
    };

    match path.to_str() {
        Some(path) => {
            *out_path = Some(path.into());
        }
        None => {
            log::error!("Unable to convert path to string: '{}'", path.display());
        }
    }
}
