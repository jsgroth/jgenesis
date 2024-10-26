mod helptext;

use crate::app::{App, NumericTextEdit, OpenWindow};
use eframe::epaint::Color32;
use egui::{Context, Response, Slider, Ui, Widget, Window};
use jgenesis_native_config::common::ConfigSavePath;
use jgenesis_renderer::config::{FilterMode, PreprocessShader, Scanlines, VSyncMode, WgpuBackend};
use rfd::FileDialog;
use std::num::NonZeroU32;
use std::path::PathBuf;

impl App {
    pub(super) fn render_common_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonVideo;

        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui.checkbox(&mut self.config.common.launch_in_fullscreen, "Launch in fullscreen")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::FULLSCREEN);
            }

            let rect = ui.group(|ui| {
                ui.add_enabled_ui(!self.emu_thread.status().is_running(), |ui| {
                    ui.label("wgpu backend");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.config.common.wgpu_backend, WgpuBackend::Auto, "Auto");
                        ui.radio_value(
                            &mut self.config.common.wgpu_backend,
                            WgpuBackend::Vulkan,
                            "Vulkan",
                        );
                        ui.radio_value(
                            &mut self.config.common.wgpu_backend,
                            WgpuBackend::DirectX12,
                            "DirectX 12",
                        );
                        ui.radio_value(
                            &mut self.config.common.wgpu_backend,
                            WgpuBackend::OpenGl,
                            "OpenGL",
                        );
                    });
                });
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::WGPU_BACKEND);
            }

            // At least as of wgpu 0.20, wgpu w/ the OpenGL backend only supports present mode Fifo (VSync enabled)
            let is_opengl = self.config.common.wgpu_backend == WgpuBackend::OpenGl;
            if is_opengl {
                self.config.common.vsync_mode = VSyncMode::Enabled;
            }

            let rect = ui.group(|ui| {
                ui.add_enabled_ui(!is_opengl, |ui| {
                    let disabled_text = "VSync mode is not configurable with the OpenGL backend";

                    ui.label("VSync mode").on_disabled_hover_text(disabled_text);

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.common.vsync_mode,
                            VSyncMode::Enabled,
                            "Enabled",
                        ).on_disabled_hover_text(disabled_text);
                        ui.radio_value(
                            &mut self.config.common.vsync_mode,
                            VSyncMode::Disabled,
                            "Disabled",
                        ).on_disabled_hover_text(disabled_text);
                        ui.radio_value(&mut self.config.common.vsync_mode, VSyncMode::Fast, "Fast")
                            .on_disabled_hover_text(disabled_text);
                    });
                });
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::VSYNC_MODE);
            }

            let rect = ui.group(|ui| {
                ui.label("Filter mode");
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.filter_mode,
                        FilterMode::Nearest,
                        "Nearest neighbor",
                    );
                    ui.radio_value(
                        &mut self.config.common.filter_mode,
                        FilterMode::Linear,
                        "Linear interpolation",
                    );
                });
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::FILTER_MODE);
            }

            let rect = ui.group(|ui| {
                ui.label("Preprocess shader");

                ui.radio_value(
                    &mut self.config.common.preprocess_shader,
                    PreprocessShader::None,
                    "None",
                );

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::HorizontalBlurTwoPixels,
                        "Horizontal blur (2px)",
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::HorizontalBlurThreePixels,
                        "Horizontal blur (3px)",
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::HorizontalBlurSnesAdaptive,
                        "Horizontal blur (SNES adaptive)",
                    )
                        .on_hover_text("Always maintains the effect of blurring 3px horizontally at 512px horizontal resolution");
                });

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherWeak,
                        "Anti-dither (conservative)"
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherStrong,
                        "Anti-dither (aggressive)"
                    );
                });
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::PREPROCESS_SHADER);
            }

            let rect = ui.group(|ui| {
                ui.label("Scanlines");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::None, "None");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Dim, "Dim");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Black, "Black");
                });
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::SCANLINES);
            }

            let rect = ui.group(|ui| {
                ui.label("Prescaling");

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.config.common.auto_prescale, |ui| {
                        ui.label("Prescale factor:");

                        if ui.add(Slider::new(&mut self.state.prescale_factor_raw, 1..=16)).changed() {
                            if let Some(prescale_factor) = NonZeroU32::new(self.state.prescale_factor_raw) {
                                self.config.common.prescale_factor = prescale_factor.into();
                            }
                        };
                    });
                });

                ui.checkbox(&mut self.config.common.auto_prescale, "Enable auto-prescale");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::PRESCALING);
            }

            let rect = ui.checkbox(
                &mut self.config.common.force_integer_height_scaling,
                "Force integer height scaling",
            ).interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::INTEGER_HEIGHT_SCALING);
            }

            if self.state.display_scanlines_warning {
                ui.colored_label(Color32::RED, "Integer height scaling + even-numbered prescale factor strongly recommended when scanlines are enabled");
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_common_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonAudio;

        const TEXT_EDIT_WIDTH: f32 = 50.0;
        const MIN_DEVICE_QUEUE_SIZE: u16 = 8;
        const MIN_AUDIO_SYNC_THRESHOLD: u32 = 64;

        let mut open = true;
        Window::new("General Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui.group(|ui| {
                ui.label("Output sample rate");

                ui.radio_value(&mut self.config.common.audio_output_frequency, 48000, "48000 Hz (Recommended)");
                ui.radio_value(&mut self.config.common.audio_output_frequency, 44100, "44100 Hz");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_SAMPLE_RATE);
            }

            ui.add_space(10.0);

            let rect = ui.checkbox(&mut self.config.common.audio_sync, "Audio sync enabled").interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_SYNC);
            }

            ui.add_space(10.0);

            let rect = ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.audio_device_queue_size_text,
                        &mut self.config.common.audio_device_queue_size,
                        &mut self.state.audio_device_queue_size_invalid,
                    )
                        .with_validation(|value| value.is_power_of_two() && value >= MIN_DEVICE_QUEUE_SIZE)
                        .desired_width(TEXT_EDIT_WIDTH)
                );

                ui.label("Audio device queue size (samples)");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_DEVICE_QUEUE_SIZE);
            }

            if self.state.audio_device_queue_size_invalid {
                ui.colored_label(Color32::RED, format!("Audio device queue size must be a power of 2 and must be at least {MIN_DEVICE_QUEUE_SIZE}"));
            }

            let rect = ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.internal_audio_buffer_size_text,
                        &mut self.config.common.internal_audio_buffer_size,
                        &mut self.state.internal_audio_buffer_size_invalid,
                    )
                        .with_validation(|value| value != 0)
                        .desired_width(TEXT_EDIT_WIDTH)
                );

                ui.label("Internal audio buffer size (samples)");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::INTERNAL_AUDIO_BUFFER_SIZE);
            }

            if self.state.internal_audio_buffer_size_invalid {
                ui.colored_label(
                    Color32::RED,
                    "Internal audio buffer size must be a positive integer",
                );
            }

            let rect = ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.audio_sync_threshold_text,
                        &mut self.config.common.audio_sync_threshold,
                        &mut self.state.audio_sync_threshold_invalid,
                    )
                        .with_validation(|value| value >= MIN_AUDIO_SYNC_THRESHOLD)
                        .desired_width(TEXT_EDIT_WIDTH)
                );

                ui.label("Audio sync threshold (bytes)");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_SYNC_THRESHOLD);
            }

            if self.state.audio_sync_threshold_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!("Audio sync threshold must be at least {MIN_AUDIO_SYNC_THRESHOLD}"),
                );
            }

            let rect = ui.horizontal(|ui| {
                ui.add(
                    NumericTextEdit::new(
                        &mut self.state.audio_gain_text,
                        &mut self.config.common.audio_gain_db,
                        &mut self.state.audio_gain_invalid,
                    )
                        .with_validation(f64::is_finite)
                        .desired_width(TEXT_EDIT_WIDTH)
                );

                ui.label("Audio gain (dB) (+/-)");
            }).response.interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_GAIN);
            }

            if self.state.audio_gain_invalid {
                ui.colored_label(Color32::RED, "Audio gain must be a finite decimal number");
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}

pub struct SavePathSelect<'a> {
    label: &'a str,
    save_path: &'a mut ConfigSavePath,
    custom_path: &'a mut PathBuf,
}

impl<'a> SavePathSelect<'a> {
    pub fn new(
        label: &'a str,
        save_path: &'a mut ConfigSavePath,
        custom_path: &'a mut PathBuf,
    ) -> Self {
        Self { label, save_path, custom_path }
    }
}

impl<'a> Widget for SavePathSelect<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.label(self.label);

            ui.horizontal(|ui| {
                ui.radio_value(self.save_path, ConfigSavePath::RomFolder, "Same folder as ROM");
                ui.radio_value(self.save_path, ConfigSavePath::EmulatorFolder, "Emulator folder");
                ui.radio_value(self.save_path, ConfigSavePath::Custom, "Custom");
            });

            ui.add_enabled_ui(*self.save_path == ConfigSavePath::Custom, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Custom path:");

                    let button_label = self.custom_path.to_string_lossy();
                    if ui.button(button_label).clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            *self.custom_path = path;
                        }
                    }
                });
            });
        })
        .response
    }
}
