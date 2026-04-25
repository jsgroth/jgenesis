mod helptext;

use crate::app::widgets::NumericTextEdit;
use crate::app::{App, OpenWindow, widgets};
use eframe::epaint::Color32;
use egui::{Context, Slider, Ui, Window};
use jgenesis_renderer::config::{
    AntiDitherShader, FilterMode, FrameRotation, NtscShaderConfig, PreprocessShader,
    PrescaleFactor, VSyncMode, WgpuBackend, WgpuPowerPreference,
};
use std::num::{NonZeroU8, NonZeroU32};

impl App {
    pub(super) fn render_common_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonVideo;

        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            widgets::render_vertical_scroll_area(ui, |ui| {
                self.render_fullscreen_settings(ui, WINDOW);
                self.render_window_size_setting(ui, WINDOW);
                self.render_integer_height_scaling_settings(ui, WINDOW);
                self.render_wgpu_backend_setting(ui, WINDOW);
                self.render_wgpu_power_preference_setting(ui, WINDOW);
                self.render_frame_rotation_setting(ui, WINDOW);
            });

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_video_filtering_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::CommonFilter;

        let mut open = true;
        Window::new("Video Filtering Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            widgets::render_vertical_scroll_area(ui, |ui| {
                self.render_preprocess_shader_setting(ui, WINDOW);
                self.render_anti_dither_setting(ui, WINDOW);
                self.render_scanlines_setting(ui, WINDOW);
                self.render_prescaling_settings(ui, WINDOW);
                self.render_filter_mode_setting(ui, WINDOW);
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
                        for (value, label) in [
                            (WgpuBackend::Auto, "Auto"),
                            (WgpuBackend::Vulkan, "Vulkan"),
                            (WgpuBackend::DirectX12, "DirectX 12"),
                        ] {
                            ui.radio_value(&mut self.config.common.wgpu_backend, value, label);
                        }
                    });
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::WGPU_BACKEND);
        }
    }

    fn render_wgpu_power_preference_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.add_enabled_ui(!self.emu_thread.status().is_running(), |ui| {
                    ui.label("wgpu power preference");
                    ui.horizontal(|ui| {
                        for (value, label) in [
                            (WgpuPowerPreference::HighPerformance, "High performance"),
                            (WgpuPowerPreference::LowPower, "Low power"),
                            (WgpuPowerPreference::None, "No preference"),
                        ] {
                            ui.radio_value(
                                &mut self.config.common.wgpu_power_preference,
                                value,
                                label,
                            );
                        }
                    });
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::WGPU_POWER_PREFERENCE);
        }
    }

    fn render_filter_mode_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        ui.group(|ui| {
            let rect = ui
                .scope(|ui| {
                    ui.label("Image filtering");
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

            let rect = ui
                .checkbox(
                    &mut self.config.common.supersample_minification,
                    "Supersample when minifying",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(window, helptext::SUPERSAMPLE_MINIFICATION);
            }
        });
    }

    fn render_frame_rotation_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Frame rotation");

                ui.horizontal(|ui| {
                    for (value, label) in [
                        (FrameRotation::None, "None"),
                        (FrameRotation::Clockwise, "90° clockwise"),
                        (FrameRotation::OneEighty, "180°"),
                        (FrameRotation::Counterclockwise, "90° counterclockwise"),
                    ] {
                        ui.radio_value(&mut self.config.common.frame_rotation, value, label);
                    }
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::FRAME_ROTATION);
        }
    }

    fn render_preprocess_shader_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Video shader");

                ui.radio_value(
                    &mut self.config.common.preprocess_shader,
                    PreprocessShader::None,
                    "None",
                );

                let composite_enabled = !self.emu_thread.status().is_running_handheld();

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(composite_enabled, |ui| {
                        ui.radio_value(
                            &mut self.config.common.preprocess_shader,
                            PreprocessShader::NtscComposite,
                            "NTSC composite",
                        );
                    });

                    ui.collapsing("NTSC shader settings", |ui| {
                        ui.vertical(|ui| {
                            let fmt_2f = |v: f64, _| format!("{v:.2}");

                            ui.style_mut().spacing.slider_width = 225.0;

                            ui.add(
                                Slider::new(&mut self.config.common.ntsc.brightness, 0.0..=5.0)
                                    .step_by(0.01)
                                    .text("Brightness")
                                    .custom_formatter(fmt_2f),
                            );

                            ui.add(
                                Slider::new(&mut self.config.common.ntsc.saturation, 0.0..=5.0)
                                    .step_by(0.01)
                                    .text("Saturation")
                                    .custom_formatter(fmt_2f),
                            );

                            ui.add(
                                Slider::new(&mut self.config.common.ntsc.gamma, 0.1..=5.0)
                                    .step_by(0.1)
                                    .text("Gamma")
                                    .custom_formatter(|v: f64, _| format!("{v:.1}")),
                            );

                            if ui.button("Default").clicked() {
                                self.config.common.ntsc = NtscShaderConfig::default();
                            }
                        });
                    });
                });

                ui.horizontal(|ui| {
                    for (shader, label) in [
                        (PreprocessShader::Xbrz2x, "xBRZ 2x"),
                        (PreprocessShader::Xbrz3x, "xBRZ 3x"),
                        (PreprocessShader::Xbrz4x, "xBRZ 4x"),
                        (PreprocessShader::Xbrz5x, "xBRZ 5x"),
                        (PreprocessShader::Xbrz6x, "xBRZ 6x"),
                    ] {
                        ui.radio_value(&mut self.config.common.preprocess_shader, shader, label);
                    }
                });

                ui.radio_value(
                    &mut self.config.common.preprocess_shader,
                    PreprocessShader::Mmpx,
                    "MMPX",
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
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::PREPROCESS_SHADER);
        }
    }

    fn render_anti_dither_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        ui.add_enabled_ui(!self.config.common.preprocess_shader.exclude_anti_dither(), |ui| {
            let rect = ui
                .group(|ui| {
                    ui.label("Anti-dither shader");

                    ui.horizontal(|ui| {
                        for (value, label) in [
                            (AntiDitherShader::None, "None"),
                            (AntiDitherShader::Weak, "Conservative"),
                            (AntiDitherShader::Strong, "Aggressive"),
                        ] {
                            ui.radio_value(
                                &mut self.config.common.anti_dither_shader,
                                value,
                                label,
                            );
                        }
                    });
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(window, helptext::ANTI_DITHER_SHADER);
            }
        });
    }

    fn render_scanlines_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Scanlines");

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.common.scanlines_enabled, "Enabled");

                    ui.add_enabled_ui(self.config.common.scanlines_enabled, |ui| {
                        ui.add(
                            Slider::new(&mut self.config.common.scanlines_brightness, 0.0..=1.0)
                                .text("Brightness")
                                .step_by(0.1)
                                .custom_formatter(|value, _| {
                                    format!("{:.0}%", (value * 100.0).round())
                                }),
                        );
                    });
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

                ui.checkbox(&mut self.config.common.auto_prescale, "Auto-prescale enabled");

                ui.add_enabled_ui(!self.config.common.auto_prescale, |ui| {
                    let mut add_slider =
                        |factor: &mut PrescaleFactor, factor_raw: &mut u32, label: &str| {
                            if ui.add(Slider::new(factor_raw, 1..=16).text(label)).changed()
                                && let Some(prescale_factor) = NonZeroU32::new(*factor_raw)
                            {
                                *factor = prescale_factor.into();
                            }
                        };

                    add_slider(
                        &mut self.config.common.prescale_width,
                        &mut self.state.prescale_width_raw,
                        "Width scale",
                    );
                    add_slider(
                        &mut self.config.common.prescale_height,
                        &mut self.state.prescale_height_raw,
                        "Height scale",
                    );
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::PRESCALING);
        }
    }

    fn render_integer_height_scaling_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .checkbox(
                &mut self.config.common.force_integer_height_scaling,
                "Force integer height scaling",
            )
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::INTEGER_HEIGHT_SCALING);
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
                    ui.label("VSync mode");

                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.common.vsync_mode,
                            VSyncMode::Enabled,
                            "Enabled",
                        );
                        ui.radio_value(
                            &mut self.config.common.vsync_mode,
                            VSyncMode::Disabled,
                            "Disabled",
                        );
                        ui.radio_value(&mut self.config.common.vsync_mode, VSyncMode::Fast, "Fast");
                    });
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
