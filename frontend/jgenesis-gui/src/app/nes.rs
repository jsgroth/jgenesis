mod helptext;

use crate::app::widgets::NumericTextEdit;
use crate::app::{App, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use eframe::emath::Align;
use eframe::epaint::Color32;
use egui::{Context, Layout, Window};
use jgenesis_common::frontend::TimingMode;
use nes_config::{NesAspectRatio, NesAudioResampler, Overscan};

pub struct OverscanState {
    top_text: String,
    top_invalid: bool,
    bottom_text: String,
    bottom_invalid: bool,
    left_text: String,
    left_invalid: bool,
    right_text: String,
    right_invalid: bool,
}

impl From<Overscan> for OverscanState {
    fn from(value: Overscan) -> Self {
        Self {
            top_text: value.top.to_string(),
            top_invalid: false,
            bottom_text: value.bottom.to_string(),
            bottom_invalid: false,
            left_text: value.left.to_string(),
            left_invalid: false,
            right_text: value.right.to_string(),
            right_invalid: false,
        }
    }
}

impl App {
    pub(super) fn render_nes_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesGeneral;

        let mut open = true;
        Window::new("NES General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(
                        self.emu_thread.status() != EmuThreadStatus::RunningNes,
                        |ui| {
                            ui.label("Timing / display mode");

                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
                                    None,
                                    "Auto",
                                );
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
                                    Some(TimingMode::Ntsc),
                                    "NTSC (60Hz)",
                                );
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
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
                .checkbox(
                    &mut self.config.nes.allow_opposing_joypad_inputs,
                    "Allow simultaneous opposing directional inputs",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::OPPOSING_DIRECTIONAL_INPUTS);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_nes_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesVideo;

        let mut open = true;
        Window::new("NES Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Aspect ratio");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.nes.aspect_ratio,
                            NesAspectRatio::Ntsc,
                            "NTSC",
                        )
                        .on_hover_text("8:7 pixel aspect ratio");
                        ui.radio_value(
                            &mut self.config.nes.aspect_ratio,
                            NesAspectRatio::Pal,
                            "PAL",
                        )
                        .on_hover_text("11:8 pixel aspect ratio");
                        ui.radio_value(
                            &mut self.config.nes.aspect_ratio,
                            NesAspectRatio::SquarePixels,
                            "Square pixels",
                        )
                        .on_hover_text("1:1 pixel aspect ratio");
                        ui.radio_value(
                            &mut self.config.nes.aspect_ratio,
                            NesAspectRatio::Stretched,
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

            let rect = ui
                .checkbox(
                    &mut self.config.nes.ntsc_crop_vertical_overscan,
                    "(NTSC) Crop vertical overscan",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::NTSC_V_OVERSCAN);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.nes.remove_sprite_limit,
                    "Remove sprite-per-scanline limit",
                )
                .on_hover_text("Eliminates most sprite flickering but can cause visual glitches")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMIT);
            }

            let rect = ui
                .checkbox(&mut self.config.nes.pal_black_border, "Render PAL black border")
                .on_hover_text("Crops the image from 256x240 to 252x239")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::PAL_BLACK_BORDER);
            }

            let rect = ui
                .group(|ui| {
                    ui.label("Overscan in pixels");

                    ui.vertical_centered(|ui| {
                        ui.label("Top");
                        ui.add(
                            NumericTextEdit::new(
                                &mut self.state.overscan.top_text,
                                &mut self.config.nes.overscan.top,
                                &mut self.state.overscan.top_invalid,
                            )
                            .desired_width(30.0),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.label("Left");
                        ui.add(
                            NumericTextEdit::new(
                                &mut self.state.overscan.left_text,
                                &mut self.config.nes.overscan.left,
                                &mut self.state.overscan.left_invalid,
                            )
                            .desired_width(30.0),
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label("Right");
                            ui.add(
                                NumericTextEdit::new(
                                    &mut self.state.overscan.right_text,
                                    &mut self.config.nes.overscan.right,
                                    &mut self.state.overscan.right_invalid,
                                )
                                .desired_width(30.0),
                            );
                        });
                    });

                    ui.vertical_centered(|ui| {
                        ui.add(
                            NumericTextEdit::new(
                                &mut self.state.overscan.bottom_text,
                                &mut self.config.nes.overscan.bottom,
                                &mut self.state.overscan.bottom_invalid,
                            )
                            .desired_width(30.0),
                        );
                        ui.label("Bottom");
                    });

                    for (invalid, label) in [
                        (self.state.overscan.top_invalid, "Top"),
                        (self.state.overscan.bottom_invalid, "Bottom"),
                        (self.state.overscan.left_invalid, "Left"),
                        (self.state.overscan.right_invalid, "Right"),
                    ] {
                        if invalid {
                            ui.colored_label(
                                Color32::RED,
                                format!("{label} value must be a non-negative integer"),
                            );
                        }
                    }
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::OVERSCAN);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_nes_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesAudio;

        let mut open = true;
        Window::new("NES Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .checkbox(
                    &mut self.config.nes.silence_ultrasonic_triangle_output,
                    "Silence ultrasonic triangle channel output",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ULTRASONIC_TRIANGLE);
            }

            let rect = ui
                .checkbox(&mut self.config.nes.audio_60hz_hack, "Enable audio sync timing hack")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_TIMING_HACK);
            }

            ui.add_space(5.0);
            let rect = ui
                .group(|ui| {
                    ui.label("Audio resampling algorithm");

                    ui.radio_value(
                        &mut self.config.nes.audio_resampler,
                        NesAudioResampler::LowPassNearestNeighbor,
                        "Low-pass filter + nearest neighbor (Faster)",
                    );
                    ui.radio_value(
                        &mut self.config.nes.audio_resampler,
                        NesAudioResampler::WindowedSinc,
                        "Windowed sinc interpolation (Higher quality)",
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_RESAMPLING);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}
