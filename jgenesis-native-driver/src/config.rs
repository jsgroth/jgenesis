use crate::RendererConfig;
use genesis_core::GenesisAspectRatio;
use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay, EnumFromStr};
use jgenesis_traits::frontend::PixelAspectRatio;
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;

pub(crate) const DEFAULT_GENESIS_WINDOW_SIZE: WindowSize = WindowSize { width: 878, height: 672 };

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr)]
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
            Self::Ntsc => Some(PixelAspectRatio::try_from(8.0 / 7.0).unwrap()),
            Self::Pal => Some(PixelAspectRatio::try_from(11.0 / 8.0).unwrap()),
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr)]
pub enum GgAspectRatio {
    #[default]
    GgLcd,
    SquarePixels,
    Stretched,
}

impl GgAspectRatio {
    pub(crate) fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::GgLcd => Some(PixelAspectRatio::try_from(6.0 / 5.0).unwrap()),
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    #[debug_fmt]
    pub vdp_version: Option<VdpVersion>,
    #[debug_fmt]
    pub psg_version: Option<PsgVersion>,
    pub remove_sprite_limit: bool,
    pub sms_aspect_ratio: SmsAspectRatio,
    pub gg_aspect_ratio: GgAspectRatio,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub audio_sync: bool,
    #[debug_fmt]
    pub window_size: Option<WindowSize>,
    pub renderer_config: RendererConfig,
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
        VdpVersion::NtscMasterSystem2 => WindowSize { width: 940, height: 720 },
        VdpVersion::PalMasterSystem2 => WindowSize { width: 1131, height: 720 },
        VdpVersion::GameGear => WindowSize { width: 576, height: 432 },
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisConfig {
    pub rom_file_path: String,
    pub aspect_ratio: GenesisAspectRatio,
    pub audio_sync: bool,
    #[debug_fmt]
    pub window_size: Option<WindowSize>,
    pub renderer_config: RendererConfig,
}
