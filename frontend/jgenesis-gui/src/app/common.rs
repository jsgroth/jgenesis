mod helptext;

use crate::app::widgets::NumericTextEdit;
use crate::app::{App, OpenWindow, widgets};
use eframe::epaint::Color32;
use egui::{Context, Slider, Ui, Window};
use jgenesis_renderer::config::{FilterMode, PreprocessShader, Scanlines, VSyncMode, WgpuBackend};
use std::num::{NonZeroU8, NonZeroU32};

impl App {
    pub(super) fn render_common_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonVideo;

        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            widgets::render_vertical_scroll_area(ui, |ui| {
                self.render_fullscreen_settings(ui, WINDOW);
                self.render_window_size_setting(ui, WINDOW);
                self.render_wgpu_backend_setting(ui, WINDOW);
                self.render_filter_mode_setting(ui, WINDOW);
                self.render_preprocess_shader_setting(ui, WINDOW);
                self.render_scanlines_setting(ui, WINDOW);
                self.render_prescaling_settings(ui, WINDOW);

                let rect = ui
                    .checkbox(
                        &mut self.config.common.force_integer_height_scaling,
                        "Force integer height scaling",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::INTEGER_HEIGHT_SCALING);
                }

                if self.state.display_scanlines_warning {
                    ui.colored_label(
                        Color32::RED,
                        concat!(
                            "Integer height scaling + even-numbered prescale factor",
                            " strongly recommended when scanlines are enabled"
                        ),
                    );
                }
            });

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn render_fullscreen_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .checkbox(&mut self.config.common.launch_in_fullscreen, "Launch in fullscreen")
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::FULLSCREEN);
        }
    }

    fn render_window_size_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        const MIN_SIZE: NonZeroU8 = NonZeroU8::new(1).unwrap();
        const MAX_SIZE: NonZeroU8 = NonZeroU8::new(10).unwrap();

        let rect = ui
            .horizontal(|ui| {
                ui.label("Initial window size:");

                ui.add(
                    Slider::new(&mut self.config.common.initial_window_size, MIN_SIZE..=MAX_SIZE)
                        .custom_formatter(|n, _| {
                            let n = n as u8;
                            format!("{n}x")
                        }),
                );
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::INITIAL_WINDOW_SIZE);
        }
    }

    fn render_wgpu_backend_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.add_enabled_ui(!self.emu_thread.status().is_running(), |ui| {
                    ui.label("wgpu backend");
                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.common.wgpu_backend,
                            WgpuBackend::Auto,
                            "Auto",
                        );
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
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::WGPU_BACKEND);
        }

        // At least as of wgpu 0.20, wgpu w/ the OpenGL backend only supports present mode Fifo (VSync enabled)
        let is_opengl = self.config.common.wgpu_backend == WgpuBackend::OpenGl;
        if is_opengl {
            self.config.common.vsync_mode = VSyncMode::Enabled;
        }
    }

    fn render_filter_mode_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
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
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::FILTER_MODE);
        }
    }

    fn render_preprocess_shader_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
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
                    .on_hover_text(concat!(
                        "Always maintains the effect of blurring",
                        " 3px horizontally at 512px horizontal resolution"
                    ));
                });

                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherWeak,
                        "Anti-dither (conservative)",
                    );
                    ui.radio_value(
                        &mut self.config.common.preprocess_shader,
                        PreprocessShader::AntiDitherStrong,
                        "Anti-dither (aggressive)",
                    );
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::PREPROCESS_SHADER);
        }
    }

    fn render_scanlines_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Scanlines");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::None, "None");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Dim, "Dim");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Black, "Black");
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::SCANLINES);
        }
    }

    fn render_prescaling_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Prescaling");

                ui.checkbox(&mut self.config.common.auto_prescale, "Enable auto-prescale");

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.config.common.auto_prescale, |ui| {
                        ui.label("Prescale factor:");

                        if ui
                            .add(Slider::new(&mut self.state.prescale_factor_raw, 1..=16))
                            .changed()
                            && let Some(prescale_factor) =
                                NonZeroU32::new(self.state.prescale_factor_raw)
                        {
                            self.config.common.prescale_factor = prescale_factor.into();
                        }
                    });
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::PRESCALING);
        }
    }

    pub(super) fn render_common_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonAudio;

        const TEXT_EDIT_WIDTH: f32 = 50.0;

        let mut open = true;
        Window::new("General Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui.checkbox(&mut self.config.common.mute_audio, "Mute audio").interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::MUTE_AUDIO);
            }

            ui.add_space(5.0);
            let rect = ui
                .group(|ui| {
                    ui.label("Output sample rate");

                    ui.radio_value(
                        &mut self.config.common.audio_output_frequency,
                        48000,
                        "48000 Hz (Recommended)",
                    );
                    ui.radio_value(
                        &mut self.config.common.audio_output_frequency,
                        44100,
                        "44100 Hz",
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_SAMPLE_RATE);
            }

            ui.add_space(10.0);

            let rect = ui
                .horizontal(|ui| {
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.audio_gain_text,
                            &mut self.config.common.audio_gain_db,
                            &mut self.state.audio_gain_invalid,
                        )
                        .with_validation(f64::is_finite)
                        .desired_width(TEXT_EDIT_WIDTH),
                    );

                    ui.label("Audio gain (dB) (+/-)");
                })
                .response
                .interact_rect;
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

    pub(super) fn render_sync_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::Synchronization;

        const TEXT_EDIT_WIDTH: f32 = 50.0;
        const MIN_DEVICE_QUEUE_SIZE: u32 = 8;
        const MIN_AUDIO_SYNC_THRESHOLD: u32 = 8;

        let mut open = true;
        Window::new("Synchronization Settings").open(&mut open).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(
                        self.config.common.wgpu_backend != WgpuBackend::OpenGl,
                        |ui| {
                            let disabled_text =
                                "VSync mode is not configurable with the OpenGL backend";

                            ui.label("VSync mode").on_disabled_hover_text(disabled_text);

                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut self.config.common.vsync_mode,
                                    VSyncMode::Enabled,
                                    "Enabled",
                                )
                                .on_disabled_hover_text(disabled_text);
                                ui.radio_value(
                                    &mut self.config.common.vsync_mode,
                                    VSyncMode::Disabled,
                                    "Disabled",
                                )
                                .on_disabled_hover_text(disabled_text);
                                ui.radio_value(
                                    &mut self.config.common.vsync_mode,
                                    VSyncMode::Fast,
                                    "Fast",
                                )
                                .on_disabled_hover_text(disabled_text);
                            });
                        },
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::VSYNC_MODE);
            }

            ui.add_space(10.0);

            let rect = ui
                .checkbox(&mut self.config.common.frame_time_sync, "Frame time sync enabled")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::FRAME_TIME_SYNC);
            }

            let rect =
                ui.checkbox(&mut self.config.common.audio_sync, "Audio sync enabled").interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_SYNC);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.common.audio_dynamic_resampling_ratio,
                    "Audio dynamic resampling ratio enabled",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_DYNAMIC_RESAMPLING);
            }

            let any_sync_enabled = self.config.common.vsync_mode == VSyncMode::Enabled
                || self.config.common.audio_sync
                || self.config.common.frame_time_sync;
            if !any_sync_enabled {
                ui.add_space(5.0);
                ui.colored_label(
                    Color32::RED,
                    "No sync enabled; emulator will run at uncapped speed",
                );
            }

            ui.add_space(10.0);

            let rect = ui
                .horizontal(|ui| {
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.audio_buffer_size_text,
                            &mut self.config.common.audio_buffer_size,
                            &mut self.state.audio_buffer_size_invalid,
                        )
                        .with_validation(|value| value >= MIN_AUDIO_SYNC_THRESHOLD)
                        .desired_width(TEXT_EDIT_WIDTH),
                    );

                    ui.label("Audio buffer size (samples)");
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_BUFFER_SIZE);
            }

            if self.state.audio_buffer_size_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!("Audio sync threshold must be at least {MIN_AUDIO_SYNC_THRESHOLD}"),
                );
            }

            let rect = ui
                .horizontal(|ui| {
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.audio_hardware_queue_size_text,
                            &mut self.config.common.audio_hardware_queue_size,
                            &mut self.state.audio_hardware_queue_size_invalid,
                        )
                        .with_validation(|value| value >= MIN_DEVICE_QUEUE_SIZE)
                        .desired_width(TEXT_EDIT_WIDTH),
                    );

                    ui.label("Audio hardware queue size (samples)");
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_HARDWARE_QUEUE_SIZE);
            }

            if self.state.audio_hardware_queue_size_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!("Audio device queue size must be at least {MIN_DEVICE_QUEUE_SIZE}"),
                );
            }

            ui.add_space(5.0);

            let estimated_audio_latency_ms = self.estimate_audio_latency_ms();
            ui.label(format!("Estimated audio latency: {estimated_audio_latency_ms} ms"));

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn estimate_audio_latency_ms(&self) -> u32 {
        let audio_buffer_size: f64 = self.config.common.audio_buffer_size.into();
        let audio_hardware_queue_size: f64 = self.config.common.audio_hardware_queue_size.into();
        let latency_secs = (audio_buffer_size + audio_hardware_queue_size)
            / (self.config.common.audio_output_frequency as f64);

        (latency_secs * 1000.0).round() as u32
    }
}
