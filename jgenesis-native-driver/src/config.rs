use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;

#[derive(Debug, Clone)]
pub struct SmsGgConfig {
    pub rom_file_path: String,
    pub vdp_version: Option<VdpVersion>,
    pub psg_version: Option<PsgVersion>,
    pub crop_sms_vertical_border: bool,
    pub crop_sms_left_border: bool,
    pub remove_sprite_limit: bool,
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
}
