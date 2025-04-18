use crate::AppConfig;
use jgenesis_native_driver::config::{
    CommonConfig, FullscreenMode, HideMouseCursor, SavePath, WindowSize,
};
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum ConfigSavePath {
    #[default]
    RomFolder,
    EmulatorFolder,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommonAppConfig {
    #[serde(default = "default_audio_output_frequency")]
    pub audio_output_frequency: u64,
    #[serde(default)]
    pub mute_audio: bool,
    #[serde(default = "true_fn")]
    pub audio_sync: bool,
    #[serde(default = "true_fn")]
    pub audio_dynamic_resampling_ratio: bool,
    #[serde(default = "default_audio_hardware_queue_size")]
    pub audio_hardware_queue_size: u16,
    #[serde(default = "default_audio_buffer_size")]
    pub audio_buffer_size: u32,
    #[serde(default)]
    pub audio_gain_db: f64,
    #[serde(default)]
    pub save_path: ConfigSavePath,
    #[serde(default = "default_custom_save_path")]
    pub custom_save_path: PathBuf,
    #[serde(default)]
    pub state_path: ConfigSavePath,
    #[serde(default = "default_custom_state_path")]
    pub custom_state_path: PathBuf,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub window_scale_factor: Option<f32>,
    #[serde(default)]
    pub launch_in_fullscreen: bool,
    #[serde(default)]
    pub fullscreen_mode: FullscreenMode,
    #[serde(default)]
    pub wgpu_backend: WgpuBackend,
    #[serde(default)]
    pub vsync_mode: VSyncMode,
    #[serde(default = "true_fn")]
    pub frame_time_sync: bool,
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
    #[serde(default)]
    pub load_recent_state_at_launch: bool,
    #[serde(default = "default_fast_forward_multiplier")]
    pub fast_forward_multiplier: u64,
    #[serde(default = "default_rewind_buffer_length")]
    pub rewind_buffer_length_seconds: u64,
    #[serde(default)]
    pub hide_mouse_cursor: HideMouseCursor,
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

const fn true_fn() -> bool {
    true
}

const fn default_audio_output_frequency() -> u64 {
    jgenesis_common::audio::DEFAULT_OUTPUT_FREQUENCY
}

const fn default_audio_hardware_queue_size() -> u16 {
    512
}

const fn default_audio_buffer_size() -> u32 {
    2048
}

fn default_custom_path(subdir: &str) -> PathBuf {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        log::error!("Unable to determine user base directories for default custom paths");
        return PathBuf::default();
    };

    base_dirs.data_local_dir().join("jgenesis").join(subdir)
}

fn default_custom_save_path() -> PathBuf {
    default_custom_path(SavePath::SAVE_SUBDIR)
}

fn default_custom_state_path() -> PathBuf {
    default_custom_path(SavePath::STATE_SUBDIR)
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
    #[must_use]
    pub fn common_config(&self, path: PathBuf) -> CommonConfig {
        CommonConfig {
            rom_file_path: path,
            mute_audio: self.common.mute_audio,
            audio_output_frequency: self.common.audio_output_frequency,
            audio_sync: self.common.audio_sync,
            audio_dynamic_resampling_ratio: self.common.audio_dynamic_resampling_ratio,
            audio_hardware_queue_size: self.common.audio_hardware_queue_size,
            audio_buffer_size: self.common.audio_buffer_size,
            audio_gain_db: self.common.audio_gain_db,
            save_path: save_path(self.common.save_path, &self.common.custom_save_path),
            state_path: save_path(self.common.state_path, &self.common.custom_state_path),
            window_size: self.common.window_size(),
            window_scale_factor: self.common.window_scale_factor,
            renderer_config: RendererConfig {
                wgpu_backend: self.common.wgpu_backend,
                vsync_mode: self.common.vsync_mode,
                frame_time_sync: self.common.frame_time_sync,
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
            load_recent_state_at_launch: self.common.load_recent_state_at_launch,
            launch_in_fullscreen: self.common.launch_in_fullscreen,
            fullscreen_mode: self.common.fullscreen_mode,
            axis_deadzone: self.input.axis_deadzone,
            hotkey_config: self.input.hotkeys.clone(),
            hide_mouse_cursor: self.common.hide_mouse_cursor,
            egui_theme: self.egui_theme,
        }
    }
}

fn save_path(path: ConfigSavePath, custom_path: &Path) -> SavePath {
    match path {
        ConfigSavePath::RomFolder => SavePath::RomFolder,
        ConfigSavePath::EmulatorFolder => SavePath::EmulatorFolder,
        ConfigSavePath::Custom => SavePath::Custom(custom_path.into()),
    }
}
