use crate::RendererConfig;
use jgenesis_traits::frontend::PixelAspectRatio;
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

impl Display for SmsAspectRatio {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ntsc => write!(f, "Ntsc"),
            Self::Pal => write!(f, "Pal"),
            Self::SquarePixels => write!(f, "SquarePixels"),
            Self::Stretched => write!(f, "Stretched"),
        }
    }
}

impl FromStr for SmsAspectRatio {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Ntsc" => Ok(Self::Ntsc),
            "Pal" => Ok(Self::Pal),
            "SquarePixels" => Ok(Self::SquarePixels),
            "Stretched" => Ok(Self::Stretched),
            _ => Err(format!("invalid SMS aspect ratio string: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

impl Display for GgAspectRatio {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GgLcd => write!(f, "GgLcd"),
            Self::SquarePixels => write!(f, "SquarePixels"),
            Self::Stretched => write!(f, "Stretched"),
        }
    }
}

impl FromStr for GgAspectRatio {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "GgLcd" => Ok(Self::GgLcd),
            "SquarePixels" => Ok(Self::SquarePixels),
            "Stretched" => Ok(Self::Stretched),
            _ => Err(format!("invalid GG aspect ratio string: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
    pub remove_sprite_limit: bool,
    pub sms_aspect_ratio: SmsAspectRatio,
    pub gg_aspect_ratio: GgAspectRatio,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub window_size: Option<WindowSize>,
    pub renderer_config: RendererConfig,
}

impl Display for SmsGgConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f)?;
        writeln!(f, "  rom_file_path: {}", self.rom_file_path)?;
        writeln!(f, "  vdp_version: {:?}", self.vdp_version)?;
        writeln!(f, "  psg_version: {:?}", self.psg_version)?;
        writeln!(f, "  remove_sprite_limit: {}", self.remove_sprite_limit)?;
        writeln!(f, "  sms_aspect_ratio: {}", self.sms_aspect_ratio)?;
        writeln!(f, "  gg_aspect_ratio: {}", self.gg_aspect_ratio)?;
        writeln!(
            f,
            "  sms_crop_vertical_border: {}",
            self.sms_crop_vertical_border
        )?;
        writeln!(f, "  sms_crop_left_border: {}", self.sms_crop_left_border)?;
        writeln!(f, "  window_size: {:?}", self.window_size)?;
        write!(f, "  renderer_config: {}", self.renderer_config)
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
        VdpVersion::NtscMasterSystem2 => WindowSize {
            width: 940,
            height: 720,
        },
        VdpVersion::PalMasterSystem2 => WindowSize {
            width: 1131,
            height: 720,
        },
        VdpVersion::GameGear => WindowSize {
            width: 576,
            height: 432,
        },
    }
}

#[derive(Debug, Clone)]
pub struct GenesisConfig {
    pub rom_file_path: String,
    pub renderer_config: RendererConfig,
}

impl Display for GenesisConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f)?;
        writeln!(f, "  rom_file_path: {}", self.rom_file_path)?;
        write!(f, "  renderer_config: {}", self.renderer_config)
    }
}
