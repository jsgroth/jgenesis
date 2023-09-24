pub mod input;

use crate::config::input::{
    GenesisInputConfig, HotkeyConfig, JoystickInput, KeyboardInput, SmsGgInputConfig,
};
use genesis_core::{
    GenesisAspectRatio, GenesisControllerType, GenesisEmulatorConfig, GenesisRegion,
};
use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::RendererConfig;
use jgenesis_traits::frontend::{PixelAspectRatio, TimingMode};
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulatorConfig, SmsRegion, VdpVersion};

pub(crate) const DEFAULT_GENESIS_WINDOW_SIZE: WindowSize = WindowSize { width: 878, height: 672 };

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
pub enum SmsAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl SmsAspectRatio {
    pub(crate) fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::Ntsc => {
                Some(PixelAspectRatio::try_from(smsgg_core::SMS_NTSC_ASPECT_RATIO).unwrap())
            }
            Self::Pal => {
                Some(PixelAspectRatio::try_from(smsgg_core::SMS_PAL_ASPECT_RATIO).unwrap())
            }
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
pub enum GgAspectRatio {
    #[default]
    GgLcd,
    SquarePixels,
    Stretched,
}

impl GgAspectRatio {
    pub(crate) fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::GgLcd => {
                Some(PixelAspectRatio::try_from(smsgg_core::GAME_GEAR_LCD_ASPECT_RATIO).unwrap())
            }
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct CommonConfig<KeyboardConfig, JoystickConfig> {
    pub rom_file_path: String,
    pub audio_sync: bool,
    #[debug_fmt]
    pub window_size: Option<WindowSize>,
    #[indent_nested]
    pub renderer_config: RendererConfig,
    pub fast_forward_multiplier: u64,
    pub rewind_buffer_length_seconds: u64,
    pub launch_in_fullscreen: bool,
    #[indent_nested]
    pub keyboard_inputs: KeyboardConfig,
    pub axis_deadzone: i16,
    #[indent_nested]
    pub joystick_inputs: JoystickConfig,
    #[indent_nested]
    pub hotkeys: HotkeyConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgConfig {
    #[indent_nested]
    pub common: CommonConfig<SmsGgInputConfig<KeyboardInput>, SmsGgInputConfig<JoystickInput>>,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
    pub remove_sprite_limit: bool,
    pub sms_aspect_ratio: SmsAspectRatio,
    pub gg_aspect_ratio: GgAspectRatio,
    pub sms_region: SmsRegion,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub fm_sound_unit_enabled: bool,
    pub overclock_z80: bool,
}

impl SmsGgConfig {
    pub(crate) fn to_emulator_config(
        &self,
        vdp_version: VdpVersion,
        psg_version: PsgVersion,
    ) -> SmsGgEmulatorConfig {
        let pixel_aspect_ratio = if vdp_version.is_master_system() {
            self.sms_aspect_ratio.to_pixel_aspect_ratio()
        } else {
            self.gg_aspect_ratio.to_pixel_aspect_ratio()
        };
        SmsGgEmulatorConfig {
            psg_version,
            pixel_aspect_ratio,
            remove_sprite_limit: self.remove_sprite_limit,
            sms_region: self.sms_region,
            sms_crop_vertical_border: self.sms_crop_vertical_border,
            sms_crop_left_border: self.sms_crop_left_border,
            fm_sound_unit_enabled: self.fm_sound_unit_enabled,
            overclock_z80: self.overclock_z80,
        }
    }
}

pub(crate) fn default_vdp_version_for_ext(file_ext: &str) -> VdpVersion {
    match file_ext {
        "sms" => VdpVersion::NtscMasterSystem2,
        "gg" => VdpVersion::GameGear,
        _ => {
            log::warn!("Unknown file extension {file_ext}, defaulting to NTSC SMS VDP");
            VdpVersion::NtscMasterSystem2
        }
    }
}

pub(crate) fn default_psg_version_for_ext(file_ext: &str) -> PsgVersion {
    match file_ext {
        "sms" => PsgVersion::MasterSystem2,
        _ => PsgVersion::Standard,
    }
}

pub(crate) fn default_smsgg_window_size(vdp_version: VdpVersion) -> WindowSize {
    match vdp_version {
        VdpVersion::NtscMasterSystem1 | VdpVersion::NtscMasterSystem2 => {
            WindowSize { width: 940, height: 720 }
        }
        VdpVersion::PalMasterSystem1 | VdpVersion::PalMasterSystem2 => {
            WindowSize { width: 1056, height: 720 }
        }
        VdpVersion::GameGear => WindowSize { width: 576, height: 432 },
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisConfig {
    #[indent_nested]
    pub common: CommonConfig<GenesisInputConfig<KeyboardInput>, GenesisInputConfig<JoystickInput>>,
    pub p1_controller_type: GenesisControllerType,
    pub p2_controller_type: GenesisControllerType,
    pub forced_timing_mode: Option<TimingMode>,
    pub forced_region: Option<GenesisRegion>,
    pub aspect_ratio: GenesisAspectRatio,
    // Whether or not to automatically double the pixel aspect ratio when the VDP is in interlaced
    // double resolution mode
    pub adjust_aspect_ratio_in_2x_resolution: bool,
    pub remove_sprite_limits: bool,
}

impl GenesisConfig {
    pub(crate) fn to_emulator_config(&self) -> GenesisEmulatorConfig {
        GenesisEmulatorConfig {
            forced_timing_mode: self.forced_timing_mode,
            forced_region: self.forced_region,
            aspect_ratio: self.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
            remove_sprite_limits: self.remove_sprite_limits,
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SegaCdConfig {
    #[indent_nested]
    pub genesis: GenesisConfig,
    pub bios_file_path: Option<String>,
    pub run_without_disc: bool,
}
