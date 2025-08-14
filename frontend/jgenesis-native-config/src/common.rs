use genesis_config::GenesisAspectRatio;
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, Scanlines, VSyncMode, WgpuBackend,
};
use nes_config::NesAspectRatio;
use serde::{Deserialize, Serialize};
use smsgg_config::{GgAspectRatio, SmsAspectRatio};
use snes_config::SnesAspectRatio;
use std::fmt::{Display, Formatter};
use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    const SMS_HEIGHT: f64 = 192.0;
    const SMS_WIDTH: f64 = 256.0;

    const GG_HEIGHT: f64 = 144.0;
    const GG_WIDTH: f64 = 160.0;

    const GENESIS_HEIGHT: f64 = 224.0;
    const GENESIS_WIDTH_H40: f64 = 320.0;

    const NES_NTSC_HEIGHT: f64 = 224.0;
    const NES_PAL_HEIGHT: f64 = 240.0;
    const NES_WIDTH: f64 = 256.0;

    const SNES_HEIGHT: f64 = 224.0;
    const SNES_WIDTH: f64 = 256.0;

    const GB_HEIGHT: f64 = 144.0;
    const GB_WIDTH: f64 = 160.0;

    const GBA_HEIGHT: f64 = 160.0;
    const GBA_WIDTH: f64 = 240.0;

    #[must_use]
    pub fn new(native_width: f64, native_height: f64, size: NonZeroU8) -> Self {
        let size: f64 = size.get().into();

        let width = (native_width * size).ceil() as u32;
        let height = (native_height * size).ceil() as u32;
        Self { width, height }
    }

    #[must_use]
    pub fn new_sms(size: NonZeroU8, aspect_ratio: SmsAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            SmsAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::SMS_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::SMS_HEIGHT, size)
    }

    #[must_use]
    pub fn new_game_gear(size: NonZeroU8, aspect_ratio: GgAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio
            .to_pixel_aspect_ratio_f64()
            .unwrap_or_else(|| GgAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0));
        let width = Self::GG_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::GG_HEIGHT, size)
    }

    #[must_use]
    pub fn new_genesis(
        size: NonZeroU8,
        aspect_ratio: GenesisAspectRatio,
        force_square_pixels_in_h40: bool,
        timing_mode: TimingMode,
    ) -> Self {
        Self::new(
            Self::genesis_width(aspect_ratio, force_square_pixels_in_h40, timing_mode),
            Self::GENESIS_HEIGHT,
            size,
        )
    }

    fn genesis_width(
        aspect_ratio: GenesisAspectRatio,
        force_square_pixels_in_h40: bool,
        timing_mode: TimingMode,
    ) -> f64 {
        if force_square_pixels_in_h40 {
            return Self::GENESIS_WIDTH_H40;
        }

        let h40_par = aspect_ratio.to_h40_pixel_aspect_ratio(timing_mode).unwrap_or_else(|| {
            GenesisAspectRatio::default().to_h40_pixel_aspect_ratio(timing_mode).unwrap_or(1.0)
        });
        Self::GENESIS_WIDTH_H40 * h40_par
    }

    #[must_use]
    pub fn new_32x(
        size: NonZeroU8,
        aspect_ratio: GenesisAspectRatio,
        force_square_pixels_in_h40: bool,
        timing_mode: TimingMode,
    ) -> Self {
        // Make 32X window a little wider than Genesis by default so that the frame won't shrink if a
        // game switches to H32 mode while the renderer has forced integer height scaling enabled
        let genesis_width =
            Self::genesis_width(aspect_ratio, force_square_pixels_in_h40, timing_mode);
        let width = genesis_width * 323.25 / 320.0;

        Self::new(width, Self::GENESIS_HEIGHT, size)
    }

    #[must_use]
    pub fn new_nes(
        size: NonZeroU8,
        aspect_ratio: NesAspectRatio,
        timing_mode: TimingMode,
        ntsc_crop_v_overscan: bool,
    ) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            NesAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::NES_WIDTH * pixel_aspect_ratio;

        let height = match timing_mode {
            TimingMode::Ntsc if ntsc_crop_v_overscan => Self::NES_NTSC_HEIGHT,
            _ => Self::NES_PAL_HEIGHT,
        };

        Self::new(width, height, size)
    }

    #[must_use]
    pub fn new_snes(size: NonZeroU8, aspect_ratio: SnesAspectRatio) -> Self {
        let pixel_aspect_ratio = aspect_ratio.to_pixel_aspect_ratio_f64().unwrap_or_else(|| {
            SnesAspectRatio::default().to_pixel_aspect_ratio_f64().unwrap_or(1.0)
        });
        let width = Self::SNES_WIDTH * pixel_aspect_ratio;

        Self::new(width, Self::SNES_HEIGHT, size)
    }

    #[must_use]
    pub fn new_gb(size: NonZeroU8) -> Self {
        // Only GB aspect ratio options are square pixels and stretched
        Self::new(Self::GB_WIDTH, Self::GB_HEIGHT, size)
    }

    #[must_use]
    pub fn new_gba(size: NonZeroU8) -> Self {
        Self::new(Self::GBA_WIDTH, Self::GBA_HEIGHT, size)
    }

    #[must_use]
    pub fn scale(self, scale_factor: f32) -> Self {
        Self {
            width: (self.width as f32 * scale_factor).round() as u32,
            height: (self.height as f32 * scale_factor).round() as u32,
        }
    }
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
    #[serde(default = "default_audio_buffer_size")]
    pub audio_buffer_size: u32,
    #[serde(default = "default_audio_hardware_queue_size")]
    pub audio_hardware_queue_size: u32,
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
    #[serde(default = "default_initial_window_size")]
    pub initial_window_size: NonZeroU8,
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
    #[inline]
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

const fn default_audio_buffer_size() -> u32 {
    2048
}

const fn default_audio_hardware_queue_size() -> u32 {
    256
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

fn default_initial_window_size() -> NonZeroU8 {
    NonZeroU8::new(3).unwrap()
}

fn default_fast_forward_multiplier() -> u64 {
    2
}

fn default_rewind_buffer_length() -> u64 {
    10
}
