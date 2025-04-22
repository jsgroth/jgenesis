use crate::AppConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::SmsGgConfig;
use serde::{Deserialize, Serialize};
use smsgg_core::psg::Sn76489Version;
use smsgg_core::{GgAspectRatio, SmsAspectRatio, SmsGgEmulatorConfig, SmsGgRegion, SmsModel};
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
    pub boot_from_bios: bool,
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
}

const fn true_fn() -> bool {
    true
}

fn default_z80_divider() -> NonZeroU32 {
    NonZeroU32::new(smsgg_core::NATIVE_Z80_DIVIDER).unwrap()
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn smsgg_config(&self, path: PathBuf) -> Box<SmsGgConfig> {
        Box::new(SmsGgConfig {
            common: self.common_config(path),
            inputs: self.input.smsgg.clone(),
            emulator_config: SmsGgEmulatorConfig {
                sms_timing_mode: self.smsgg.sms_timing_mode,
                sms_model: self.smsgg.sms_model,
                forced_psg_version: self.smsgg.psg_version,
                remove_sprite_limit: self.smsgg.remove_sprite_limit,
                sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
                gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
                forced_region: self.smsgg.forced_region,
                sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
                sms_crop_left_border: self.smsgg.sms_crop_left_border,
                gg_use_sms_resolution: self.smsgg.gg_use_sms_resolution,
                fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
                z80_divider: self.smsgg.z80_divider,
            },
            boot_from_bios: self.smsgg.boot_from_bios,
            run_without_cartridge: false,
            bios_path: self.smsgg.bios_path.clone(),
        })
    }
}
