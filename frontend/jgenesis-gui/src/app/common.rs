use crate::app::{App, AppConfig, NumericTextEdit, OpenWindow};
use eframe::epaint::Color32;
use egui::{Context, TextEdit, Widget, Window};
use jgenesis_native_driver::config::{CommonConfig, WindowSize};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommonAppConfig {
    #[serde(default = "true_fn")]
    pub audio_sync: bool,
    #[serde(default = "default_audio_device_queue_size")]
    pub audio_device_queue_size: u16,
    #[serde(default = "default_internal_audio_buffer_size")]
    pub internal_audio_buffer_size: u32,
    #[serde(default = "default_audio_sync_threshold")]
    pub audio_sync_threshold: u32,
    #[serde(default)]
    pub audio_gain_db: f64,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    #[serde(default)]
    pub launch_in_fullscreen: bool,
    #[serde(default)]
    pub wgpu_backend: WgpuBackend,
    #[serde(default)]
    pub vsync_mode: VSyncMode,
    #[serde(default = "true_fn")]
    pub auto_prescale: bool,
    #[serde(default = "default_prescale_factor")]
    pub prescale_factor: PrescaleFactor,
    #[serde(default)]
    pub scanlines: Scanlines,
    #[serde(default)]
    pub force_integer_height_scaling: bool,
    #[serde(default)]
    pub filter_mode: FilterMode,
    #[serde(default)]
    pub preprocess_shader: PreprocessShader,
    #[serde(default = "default_fast_forward_multiplier")]
    pub fast_forward_multiplier: u64,
    #[serde(default = "default_rewind_buffer_length")]
    pub rewind_buffer_length_seconds: u64,
    #[serde(default)]
    pub hide_cursor_over_window: bool,
}

impl CommonAppConfig {
    pub fn window_size(&self) -> Option<WindowSize> {
        match (self.window_width, self.window_height) {
            (Some(width), Some(height)) => Some(WindowSize { width, height }),
            _ => None,
        }
    }
}

impl Default for CommonAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn true_fn() -> bool {
    true
}

fn default_audio_device_queue_size() -> u16 {
    512
}

fn default_internal_audio_buffer_size() -> u32 {
    64
}

fn default_audio_sync_threshold() -> u32 {
    8192
}

fn default_prescale_factor() -> PrescaleFactor {
    PrescaleFactor::from(NonZeroU32::new(3).unwrap())
}

fn default_fast_forward_multiplier() -> u64 {
    2
}

fn default_rewind_buffer_length() -> u64 {
    10
}

impl AppConfig {
    pub(super) fn common_config<KC, JC>(
        &self,
        path: String,
        keyboard_inputs: KC,
        joystick_inputs: JC,
    ) -> CommonConfig<KC, JC> {
        CommonConfig {
            rom_file_path: path,
            audio_sync: self.common.audio_sync,
            audio_device_queue_size: self.common.audio_device_queue_size,
            internal_audio_buffer_size: self.common.internal_audio_buffer_size,
            audio_sync_threshold: self.common.audio_sync_threshold,
            audio_gain_db: self.common.audio_gain_db,
            window_size: self.common.window_size(),
            renderer_config: RendererConfig {
                wgpu_backend: self.common.wgpu_backend,
                vsync_mode: self.common.vsync_mode,
                prescale_mode: if self.common.auto_prescale {
                    PrescaleMode::Auto
                } else {
                    PrescaleMode::Manual(self.common.prescale_factor)
                },
                scanlines: self.common.scanlines,
                force_integer_height_scaling: self.common.force_integer_height_scaling,
                filter_mode: self.common.filter_mode,
                preprocess_shader: self.common.preprocess_shader,
                use_webgl2_limits: false,
            },
            fast_forward_multiplier: self.common.fast_forward_multiplier,
            rewind_buffer_length_seconds: self.common.rewind_buffer_length_seconds,
            launch_in_fullscreen: self.common.launch_in_fullscreen,
            keyboard_inputs,
            axis_deadzone: self.inputs.axis_deadzone,
            joystick_inputs,
            hotkeys: self.inputs.hotkeys.clone(),
            hide_cursor_over_window: self.common.hide_cursor_over_window,
        }
    }
}

const MAX_PRESCALE_FACTOR: u32 = 20;

impl App {
    pub(super) fn render_common_video_settings(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new("General Video Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.common.launch_in_fullscreen, "Launch in fullscreen");

            ui.group(|ui| {
                ui.set_enabled(!self.emu_thread.status().is_running());

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

            ui.group(|ui| {
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
            });

            ui.group(|ui| {
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
            });

            ui.group(|ui| {
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
            });

            ui.group(|ui| {
                ui.label("Scanlines");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::None, "None");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Dim, "Dim");
                    ui.radio_value(&mut self.config.common.scanlines, Scanlines::Black, "Black");
                });
            });

            ui.horizontal(|ui| {
                ui.set_enabled(!self.config.common.auto_prescale);

                if TextEdit::singleline(&mut self.state.prescale_factor_text)
                    .desired_width(30.0)
                    .ui(ui)
                    .changed()
                {
                    match self
                        .state
                        .prescale_factor_text
                        .parse::<u32>()
                        .ok()
                        .filter(|&n| n <= MAX_PRESCALE_FACTOR)
                        .and_then(|n| PrescaleFactor::try_from(n).ok())
                    {
                        Some(prescale_factor) => {
                            self.config.common.prescale_factor = prescale_factor;
                            self.state.prescale_factor_invalid = false;
                        }
                        None => {
                            self.state.prescale_factor_invalid = true;
                        }
                    }
                }

                ui.label("Prescale factor");
            });
            if self.state.prescale_factor_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!(
                        "Prescale factor must be a non-negative integer <= {MAX_PRESCALE_FACTOR}"
                    ),
                );
            }

            ui.checkbox(&mut self.config.common.auto_prescale, "Enable auto-prescale")
                .on_hover_text("Automatically adjust prescale factor based on viewport size");

            ui.checkbox(
                &mut self.config.common.force_integer_height_scaling,
                "Force integer height scaling",
            ).on_hover_text("Display area will be the largest possible integer multiple of native height that preserves aspect ratio");

            if self.state.display_scanlines_warning {
                ui.colored_label(Color32::RED, "Integer height scaling + even-numbered prescale factor strongly recommended when scanlines are enabled");
            }
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::CommonVideo);
        }
    }

    pub(super) fn render_common_audio_settings(&mut self, ctx: &Context) {
        const TEXT_EDIT_WIDTH: f32 = 50.0;
        const MIN_DEVICE_QUEUE_SIZE: u16 = 8;
        const MIN_AUDIO_SYNC_THRESHOLD: u32 = 64;

        let mut open = true;
        Window::new("General Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.checkbox(&mut self.config.common.audio_sync, "Audio sync enabled");

            ui.add_space(10.0);

            ui.horizontal(|ui| {
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
            });
            if self.state.audio_device_queue_size_invalid {
                ui.colored_label(Color32::RED, format!("Audio device queue size must be a power of 2 and must be at least {MIN_DEVICE_QUEUE_SIZE}"));
            }

            ui.horizontal(|ui| {
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
            });
            if self.state.internal_audio_buffer_size_invalid {
                ui.colored_label(
                    Color32::RED,
                    "Internal audio buffer size must be a positive integer",
                );
            }

            ui.horizontal(|ui| {
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
            });
            if self.state.audio_sync_threshold_invalid {
                ui.colored_label(
                    Color32::RED,
                    format!("Audio sync threshold must be at least {MIN_AUDIO_SYNC_THRESHOLD}"),
                );
            }

            ui.horizontal(|ui| {
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
            });
            if self.state.audio_gain_invalid {
                ui.colored_label(Color32::RED, "Audio gain must be a finite decimal number");
            }
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::CommonAudio);
        }
    }
}
