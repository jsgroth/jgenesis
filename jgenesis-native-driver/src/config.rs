use crate::RendererConfig;
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
    pub remove_sprite_limit: bool,
    pub renderer_config: RendererConfig,
}

impl Display for SmsGgConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f)?;
        writeln!(f, "  rom_file_path: {}", self.rom_file_path)?;
        writeln!(f, "  vdp_version: {:?}", self.vdp_version)?;
        writeln!(f, "  psg_version: {:?}", self.psg_version)?;
        writeln!(f, "  remove_sprite_limit: {}", self.remove_sprite_limit)?;
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
