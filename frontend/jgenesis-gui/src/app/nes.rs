use crate::app::{App, NumericTextEdit, OpenWindow};
use crate::emuthread::EmuThreadStatus;
use eframe::emath::Align;
use eframe::epaint::Color32;
use egui::{Context, Layout, Window};
use jgenesis_common::frontend::TimingMode;
use nes_core::api::{NesAspectRatio, Overscan};

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
        let mut open = true;
        Window::new("NES General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.add_enabled_ui(self.emu_thread.status() != EmuThreadStatus::RunningNes, |ui| {
                    ui.label("Timing / display mode");

                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.config.nes.forced_timing_mode, None, "Auto");
                        ui.radio_value(
                            &mut self.config.nes.forced_timing_mode,
                            Some(TimingMode::Ntsc),
                            "NTSC",
                        );
                        ui.radio_value(
                            &mut self.config.nes.forced_timing_mode,
                            Some(TimingMode::Pal),
                            "PAL",
                        );
                    });
                });

                ui.checkbox(&mut self.config.nes.allow_opposing_joypad_inputs, "Allow simultaneous opposing directional inputs")
                    .on_hover_text("Some games exhibit major glitches when opposing directions are pressed simultaneously");
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesGeneral);
        }
    }

    pub(super) fn render_nes_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("NES Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.nes.aspect_ratio, NesAspectRatio::Ntsc, "NTSC")
                        .on_hover_text("8:7 pixel aspect ratio");
                    ui.radio_value(&mut self.config.nes.aspect_ratio, NesAspectRatio::Pal, "PAL")
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
            });

            ui.checkbox(
                &mut self.config.nes.remove_sprite_limit,
                "Remove sprite-per-scanline limit",
            )
            .on_hover_text("Eliminates most sprite flickering but can cause visual glitches");

            ui.checkbox(&mut self.config.nes.pal_black_border, "Render PAL black border")
                .on_hover_text("Crops the image from 256x240 to 252x239");

            ui.group(|ui| {
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
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesVideo);
        }
    }

    pub(super) fn render_nes_audio_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("NES Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.nes.silence_ultrasonic_triangle_output, "Silence ultrasonic triangle channel output")
                .on_hover_text("Less accurate but can reduce audio popping in some games");

            ui.checkbox(&mut self.config.nes.audio_60hz_hack, "Enable audio 60Hz/50Hz hack")
                .on_hover_text("Enabling this option will very slightly increase the audio signal frequency to time to 60Hz NTSC / 50Hz PAL");
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::NesAudio);
        }
    }
}
