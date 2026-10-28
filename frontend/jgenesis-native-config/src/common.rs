use jgenesis_common::frontend::WindowSize;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, deserialize_default_on_error};
use jgenesis_renderer::config::{
    AntiDitherShader, FilterMode, FrameRotation, NtscShaderConfig, PreprocessShader,
    PrescaleFactor, VSyncMode, WgpuBackend, WgpuPowerPreference,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;

const DEFAULT_OUTPUT_FREQUENCY: u64 = jgenesis_common::audio::DEFAULT_OUTPUT_FREQUENCY;
const DEFAULT_AUDIO_BUFFER_SIZE: u32 = 2048;
const DEFAULT_AUDIO_HW_QUEUE_SIZE: u32 = 256;
const DEFAULT_INITIAL_WINDOW_SIZE: NonZeroU8 = NonZeroU8::new(3).unwrap();
const DEFAULT_SCANLINES_BRIGHTNESS: f64 = 0.5;
const DEFAULT_FAST_FORWARD_MULTIPLIER: u64 = 2;
const DEFAULT_REWIND_BUFFER_LENGTH: u64 = 10;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum WindowSizeMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone)]
pub enum SavePath {
    RomFolder,
    EmulatorFolder,
    Custom(PathBuf),
}

impl SavePath {
    pub const SAVE_SUBDIR: &'static str = "saves";
    pub const STATE_SUBDIR: &'static str = "states";
}

impl Display for SavePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomFolder => write!(f, "ROM Folder"),
            Self::EmulatorFolder => write!(f, "Emulator Folder"),
            Self::Custom(path) => write!(f, "{}", path.display()),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum ApplyPatches {
    No,
    #[default]
    Yes,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PauseEmulator {
    #[default]
    Never,
    EmulatorLosesFocus,
    ApplicationLosesFocus,
}

impl PauseEmulator {
    #[inline]
    #[must_use]
    pub fn should_pause(self, emulator_focused: bool, any_focused: bool) -> bool {
        match self {
            Self::Never => false,
            Self::EmulatorLosesFocus => !emulator_focused,
            Self::ApplicationLosesFocus => !any_focused,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum HideMouseCursor {
    #[default]
    Fullscreen,
    Never,
    Always,
}

impl HideMouseCursor {
    #[inline]
    #[must_use]
    pub fn should_hide(self, fullscreen: bool) -> bool {
        self == Self::Always || (fullscreen && self == Self::Fullscreen)
    }
}

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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumAll,
)]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum CheatPath {
    #[default]
    SettingsFolder,
    EmulatorFolder,
    Custom,
}

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommonAppConfig {
    pub audio_output_frequency: u64,
    pub mute_audio: bool,
    pub audio_sync: bool,
    pub audio_dynamic_resampling_ratio: bool,
    pub audio_buffer_size: u32,
    pub audio_hardware_queue_size: u32,
    pub audio_gain_db: f64,
    pub save_path: ConfigSavePath,
    pub custom_save_path: PathBuf,
    pub state_path: ConfigSavePath,
    pub custom_state_path: PathBuf,
    pub apply_patches: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub window_size_mode: WindowSizeMode,
    pub auto_resize_window: bool,
    pub launch_in_fullscreen: bool,
    pub borderless_window: bool,
    pub initial_window_size: NonZeroU8,
    pub wgpu_backend: WgpuBackend,
    pub wgpu_power_preference: WgpuPowerPreference,
    pub vsync_mode: VSyncMode,
    pub frame_time_sync: bool,
    pub auto_prescale: bool,
    pub prescale_width: PrescaleFactor,
    pub prescale_height: PrescaleFactor,
    pub scanlines_enabled: bool,
    pub scanlines_brightness: f64,
    pub force_integer_height_scaling: bool,
    pub filter_mode: FilterMode,
    pub supersample_minification: bool,
    pub preprocess_shader: PreprocessShader,
    pub anti_dither_shader: AntiDitherShader,
    pub frame_rotation: FrameRotation,
    pub ntsc: NtscShaderConfig,
    pub load_recent_state_at_launch: bool,
    pub fast_forward_multiplier: u64,
    pub rewind_buffer_length_seconds: u64,
    pub pause_emulator: PauseEmulator,
    pub hide_mouse_cursor: HideMouseCursor,
    pub cheats_enabled: bool,
    pub cheats_path: CheatPath,
    pub cheats_custom_path: PathBuf,
}

impl CommonAppConfig {
    #[must_use]
    pub fn window_size(&self) -> Option<WindowSize> {
        match self.window_size_mode {
            WindowSizeMode::Auto => None,
            WindowSizeMode::Manual => {
                Some(WindowSize { width: self.window_width, height: self.window_height })
            }
        }
    }
}

impl Default for CommonAppConfig {
    fn default() -> Self {
        Self {
            audio_output_frequency: DEFAULT_OUTPUT_FREQUENCY,
            mute_audio: false,
            audio_sync: true,
            audio_dynamic_resampling_ratio: true,
            audio_buffer_size: DEFAULT_AUDIO_BUFFER_SIZE,
            audio_hardware_queue_size: DEFAULT_AUDIO_HW_QUEUE_SIZE,
            audio_gain_db: 0.0,
            save_path: ConfigSavePath::default(),
            custom_save_path: default_custom_save_path(),
            state_path: ConfigSavePath::default(),
            custom_state_path: default_custom_state_path(),
            apply_patches: true,
            window_width: 878,
            window_height: 672,
            window_size_mode: WindowSizeMode::default(),
            auto_resize_window: true,
            launch_in_fullscreen: false,
            borderless_window: false,
            initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE,
            wgpu_backend: WgpuBackend::default(),
            wgpu_power_preference: WgpuPowerPreference::default(),
            vsync_mode: VSyncMode::default(),
            frame_time_sync: true,
            auto_prescale: true,
            prescale_width: default_prescale_factor(),
            prescale_height: default_prescale_factor(),
            scanlines_enabled: false,
            scanlines_brightness: DEFAULT_SCANLINES_BRIGHTNESS,
            force_integer_height_scaling: false,
            filter_mode: FilterMode::default(),
            supersample_minification: true,
            preprocess_shader: PreprocessShader::default(),
            anti_dither_shader: AntiDitherShader::default(),
            frame_rotation: FrameRotation::default(),
            ntsc: NtscShaderConfig::default(),
            load_recent_state_at_launch: false,
            fast_forward_multiplier: DEFAULT_FAST_FORWARD_MULTIPLIER,
            rewind_buffer_length_seconds: DEFAULT_REWIND_BUFFER_LENGTH,
            pause_emulator: PauseEmulator::default(),
            hide_mouse_cursor: HideMouseCursor::default(),
            cheats_enabled: true,
            cheats_path: CheatPath::default(),
            cheats_custom_path: default_custom_cheats_path(),
        }
    }
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

fn default_custom_cheats_path() -> PathBuf {
    default_custom_path("cheats")
}

fn default_prescale_factor() -> PrescaleFactor {
    PrescaleFactor::from(NonZeroU32::new(3).unwrap())
}
