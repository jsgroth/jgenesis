mod helptext;

use crate::app::{App, OpenWindow};
use egui::{Context, Window};
use pce_config::{PceAspectRatio, PcePaletteType, PceRegion};

impl App {
    pub(super) fn render_pce_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::PceGeneral;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).show(ctx, |ui| {
            ui.group(|ui| {
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
            });

            self.state.help_text.insert(WINDOW, helptext::REGION);
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
                            "PC Engine Composite",
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
}
