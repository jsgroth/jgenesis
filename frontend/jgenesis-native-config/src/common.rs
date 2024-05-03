use crate::AppConfig;
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
    #[serde(default)]
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
    #[must_use]
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
    pub fn common_config<KC, JC>(
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
