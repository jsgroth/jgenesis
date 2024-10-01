use crate::AppConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GgAspectRatio, SmsAspectRatio, SmsGgConfig};
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsModel, SmsRegion};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsGgAppConfig {
    pub psg_version: Option<PsgVersion>,
    #[serde(default)]
    pub remove_sprite_limit: bool,
    #[serde(default)]
    pub sms_aspect_ratio: SmsAspectRatio,
    #[serde(default)]
    pub gg_aspect_ratio: GgAspectRatio,
    #[serde(default)]
    pub sms_region: SmsRegion,
    #[serde(default)]
    pub sms_timing_mode: TimingMode,
    #[serde(default)]
    pub sms_model: SmsModel,
    #[serde(default)]
    pub sms_crop_vertical_border: bool,
    #[serde(default)]
    pub sms_crop_left_border: bool,
    #[serde(default)]
    pub gg_use_sms_resolution: bool,
    #[serde(default = "true_fn")]
    pub fm_sound_unit_enabled: bool,
    #[serde(default)]
    pub overclock_z80: bool,
}

const fn true_fn() -> bool {
    true
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn smsgg_config(&self, path: String) -> Box<SmsGgConfig> {
        Box::new(SmsGgConfig {
            common: self.common_config(
                path,
                self.inputs.smsgg_keyboard.clone(),
                self.inputs.smsgg_joystick.clone(),
            ),
            sms_timing_mode: self.smsgg.sms_timing_mode,
            sms_model: self.smsgg.sms_model,
            forced_psg_version: self.smsgg.psg_version,
            remove_sprite_limit: self.smsgg.remove_sprite_limit,
            sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
            gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
            sms_region: self.smsgg.sms_region,
            sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
            sms_crop_left_border: self.smsgg.sms_crop_left_border,
            gg_use_sms_resolution: self.smsgg.gg_use_sms_resolution,
            fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
            overclock_z80: self.smsgg.overclock_z80,
        })
    }
}
