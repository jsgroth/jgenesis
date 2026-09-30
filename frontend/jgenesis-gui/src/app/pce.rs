mod helptext;

use crate::app::widgets::{
    BiosErrorStrings, ClockModifier, OptionalPathSelector, OverclockSlider, RenderErrorEffect,
};
use crate::app::{App, OpenWindow, widgets};
use egui::{Context, Window};
use jgenesis_native_driver::extensions::Console;
use pce_config::{PceAspectRatio, PcePaletteType, PcePsgResampler, PceRegion};
use rfd::FileDialog;
use std::num::NonZeroU64;
use std::path::PathBuf;

impl App {
    pub(super) fn render_pce_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::PceGeneral;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Console region");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.pc_engine.region,
                            PceRegion::TurboGrafx16,
                            "TurboGrafx-16 (US)",
                        );
                        ui.radio_value(
                            &mut self.config.pc_engine.region,
                            PceRegion::PcEngine,
                            "PC Engine (JP)",
                        );
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
                    "CD-ROM² System Card path",
                    &mut self.config.pc_engine.cd_bios_path,
                    pick_pce_bios_path,
                ))
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::CD_BIOS);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.pc_engine.load_disc_into_ram,
                    "(CD-ROM²) Load CD-ROM images into host RAM",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::LOAD_DISC_INTO_RAM);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_pce_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::PceVideo;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Aspect ratio");

                    ui.horizontal(|ui| {
                        for (value, label) in [
                            (PceAspectRatio::Ntsc, "NTSC"),
                            (PceAspectRatio::SquarePixels, "Square pixels"),
                            (PceAspectRatio::Stretched, "Stretched"),
                        ] {
                            ui.radio_value(&mut self.config.pc_engine.aspect_ratio, value, label);
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
                    ui.label("Palette");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.pc_engine.palette,
                            PcePaletteType::PceComposite,
                            "PC Engine composite",
                        );

                        ui.radio_value(
                            &mut self.config.pc_engine.palette,
                            PcePaletteType::Linear,
                            "Linear",
                        );
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::PALETTE);
            }

            ui.add_space(3.0);

            let rect = ui
                .checkbox(&mut self.config.pc_engine.crop_overscan, "Crop overscan area")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::CROP_OVERSCAN);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.pc_engine.remove_sprite_limits,
                    "Remove sprite-per-scanline limits",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMITS);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_pce_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::PceAudio;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("PSG audio resampling algorithm");

                ui.radio_value(
                    &mut self.config.pc_engine.audio_resampler,
                    PcePsgResampler::WindowedSinc,
                    "Windowed sinc interpolation (Higher quality)",
                );
                ui.radio_value(
                    &mut self.config.pc_engine.audio_resampler,
                    PcePsgResampler::LowPassNearestNeighbor,
                    "Low-pass filter + nearest neighbor (Faster)",
                );
            });

            self.state.help_text.insert(WINDOW, helptext::PSG_AUDIO_RESAMPLER);
            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_pce_overclock_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::PceOverclock;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).show(ctx, |ui| {
            let range = NonZeroU64::new(1).unwrap()
                ..=NonZeroU64::new(pce_config::NATIVE_FAST_CPU_DIVIDER).unwrap();

            ui.add(OverclockSlider {
                label: "CPU High-Speed Clock Divider",
                current_value: &mut self.config.pc_engine.cpu_fast_clock_divider,
                range,
                master_clock: pce_core::api::MASTER_CLOCK_FREQUENCY,
                default_divider: pce_config::NATIVE_FAST_CPU_DIVIDER as f64,
                modifier: ClockModifier::Divider,
            });

            self.state.help_text.insert(WINDOW, helptext::CPU_OVERCLOCK);

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_pce_bios_error(
        &mut self,
        ctx: &Context,
        open: &mut bool,
    ) -> RenderErrorEffect {
        widgets::render_bios_error(
            ctx,
            open,
            BiosErrorStrings {
                title: "Missing CD-ROM² System Card ROM",
                text: "No PC-Engine CD-ROM² System Card ROM path is configured. A System Card ROM is required for CD-ROM² emulation.",
                button_label: "Configure System Card ROM path",
            },
            &mut self.config.pc_engine.cd_bios_path,
            Console::PcEngine,
            pick_pce_bios_path,
        )
    }
}

fn pick_pce_bios_path() -> Option<PathBuf> {
    FileDialog::new()
        .add_filter("PC Engine", &["pce", "bin"])
        .add_filter("All Files", &["*"])
        .pick_file()
}
