use crate::AppConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GgAspectRatio, SmsAspectRatio, SmsGgConfig};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsRegion, VdpVersion};
use std::ffi::OsStr;
use std::path::Path;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
pub enum SmsModel {
    Sms1,
    #[default]
    Sms2,
}

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
        let vdp_version = if Path::new(&path).extension().and_then(OsStr::to_str) == Some("sms") {
            match (self.smsgg.sms_timing_mode, self.smsgg.sms_model) {
                (TimingMode::Ntsc, SmsModel::Sms2) => Some(VdpVersion::NtscMasterSystem2),
                (TimingMode::Pal, SmsModel::Sms2) => Some(VdpVersion::PalMasterSystem2),
                (TimingMode::Ntsc, SmsModel::Sms1) => Some(VdpVersion::NtscMasterSystem1),
                (TimingMode::Pal, SmsModel::Sms1) => Some(VdpVersion::PalMasterSystem1),
            }
        } else {
            None
        };

        Box::new(SmsGgConfig {
            common: self.common_config(
                path,
                self.inputs.smsgg_keyboard.clone(),
                self.inputs.smsgg_joystick.clone(),
            ),
            vdp_version,
            psg_version: self.smsgg.psg_version,
            remove_sprite_limit: self.smsgg.remove_sprite_limit,
            sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
            gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
            sms_region: self.smsgg.sms_region,
            sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
            sms_crop_left_border: self.smsgg.sms_crop_left_border,
            fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
            overclock_z80: self.smsgg.overclock_z80,
        })
    }
}
