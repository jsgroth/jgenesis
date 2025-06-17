use jgenesis_common::frontend::TimingMode;
use serde::{Deserialize, Serialize};
use smsgg_config::{GgAspectRatio, SmsAspectRatio, SmsGgRegion, SmsModel, Sn76489Version};
use std::num::NonZeroU32;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsGgAppConfig {
    pub psg_version: Option<Sn76489Version>,
    #[serde(default)]
    pub remove_sprite_limit: bool,
    #[serde(default)]
    pub sms_aspect_ratio: SmsAspectRatio,
    #[serde(default)]
    pub gg_aspect_ratio: GgAspectRatio,
    #[serde(default)]
    pub forced_region: Option<SmsGgRegion>,
    #[serde(default)]
    pub sms_timing_mode: TimingMode,
    #[serde(default)]
    pub sms_model: SmsModel,
    #[serde(default = "true_fn")]
    pub sms_crop_vertical_border: bool,
    #[serde(default)]
    pub sms_crop_left_border: bool,
    #[serde(default)]
    pub gg_use_sms_resolution: bool,
    #[serde(default = "true_fn")]
    pub fm_sound_unit_enabled: bool,
    #[serde(default = "default_z80_divider")]
    pub z80_divider: NonZeroU32,
    #[serde(default)]
    pub sms_boot_from_bios: bool,
    #[serde(default)]
    pub gg_boot_from_bios: bool,
    #[serde(default)]
    pub sms_bios_path: Option<PathBuf>,
    #[serde(default)]
    pub gg_bios_path: Option<PathBuf>,
}

const fn true_fn() -> bool {
    true
}

fn default_z80_divider() -> NonZeroU32 {
    NonZeroU32::new(smsgg_config::NATIVE_Z80_DIVIDER).unwrap()
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
