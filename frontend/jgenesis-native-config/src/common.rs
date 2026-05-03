use genesis_config::{GenParParams, GenesisAspectRatio};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, deserialize_default_on_error};
use jgenesis_renderer::config::{
    AntiDitherShader, FilterMode, FrameRotation, NtscShaderConfig, PreprocessShader,
    PrescaleFactor, VSyncMode, WgpuBackend, WgpuPowerPreference,
};
use nes_config::NesAspectRatio;
use serde::{Deserialize, Serialize};
use smsgg_config::{GgAspectRatio, SmsAspectRatio};
use snes_config::SnesAspectRatio;
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

    const PCE_WIDTH: f64 = 256.0;
    const PCE_HEIGHT: f64 = 224.0;

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
        timing_mode: TimingMode,
        params: GenParParams,
    ) -> Self {
        Self::new(
            Self::genesis_width(aspect_ratio, timing_mode, params),
            Self::GENESIS_HEIGHT,
            size,
        )
    }

    fn genesis_width(
        aspect_ratio: GenesisAspectRatio,
        timing_mode: TimingMode,
        params: GenParParams,
    ) -> f64 {
        let mut h40_width = if params.force_square_in_h40 {
            Self::GENESIS_WIDTH_H40
        } else {
            let h40_par =
                aspect_ratio.to_h40_pixel_aspect_ratio(timing_mode).unwrap_or_else(|| {
                    GenesisAspectRatio::default()
                        .to_h40_pixel_aspect_ratio(timing_mode)
                        .unwrap_or(1.0)
                });
            Self::GENESIS_WIDTH_H40 * h40_par
        };

        if params.anamorphic_widescreen {
            h40_width *= 4.0 / 3.0;
        }

        h40_width
    }

    #[must_use]
    pub fn new_32x(
        size: NonZeroU8,
        aspect_ratio: GenesisAspectRatio,
        timing_mode: TimingMode,
        params: GenParParams,
    ) -> Self {
        // Make 32X window a little wider than Genesis by default so that the frame won't shrink if a
        // game switches to H32 mode while the renderer has forced integer height scaling enabled
        let genesis_width = Self::genesis_width(aspect_ratio, timing_mode, params);
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
    pub fn new_pce(size: NonZeroU8) -> Self {
        // TODO aspect ratio
        Self::new(Self::PCE_WIDTH, Self::PCE_HEIGHT, size)
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
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub window_scale_factor: Option<f32>,
    pub launch_in_fullscreen: bool,
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
            window_width: None,
            window_height: None,
            window_scale_factor: None,
            launch_in_fullscreen: false,
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

fn default_prescale_factor() -> PrescaleFactor {
    PrescaleFactor::from(NonZeroU32::new(3).unwrap())
}
