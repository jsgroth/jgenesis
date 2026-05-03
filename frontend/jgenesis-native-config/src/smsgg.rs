use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::deserialize_default_on_error;
use serde::{Deserialize, Serialize};
use smsgg_config::{GgAspectRatio, SmsAspectRatio, SmsGgRegion, SmsModel, Sn76489Version};
use std::num::NonZeroU32;
use std::path::PathBuf;

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmsGgAppConfig {
    pub psg_version: Option<Sn76489Version>,
    pub remove_sprite_limit: bool,
    pub sms_aspect_ratio: SmsAspectRatio,
    pub gg_aspect_ratio: GgAspectRatio,
    pub forced_region: Option<SmsGgRegion>,
    pub sms_timing_mode: TimingMode,
    pub sms_model: SmsModel,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub gg_frame_blending: bool,
    pub gg_use_sms_resolution: bool,
    pub fm_sound_unit_enabled: bool,
    pub z80_divider: NonZeroU32,
    pub sms_boot_from_bios: bool,
    pub gg_boot_from_bios: bool,
    pub sms_bios_path: Option<PathBuf>,
    pub gg_bios_path: Option<PathBuf>,
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        Self {
            psg_version: None,
            remove_sprite_limit: false,
            sms_aspect_ratio: SmsAspectRatio::default(),
            gg_aspect_ratio: GgAspectRatio::default(),
            forced_region: None,
            sms_timing_mode: TimingMode::default(),
            sms_model: SmsModel::default(),
            sms_crop_vertical_border: true,
            sms_crop_left_border: false,
            gg_frame_blending: false,
            gg_use_sms_resolution: false,
            fm_sound_unit_enabled: true,
            z80_divider: NonZeroU32::new(smsgg_config::NATIVE_Z80_DIVIDER).unwrap(),
            sms_boot_from_bios: false,
            gg_boot_from_bios: false,
            sms_bios_path: None,
            gg_bios_path: None,
        }
    }
}
