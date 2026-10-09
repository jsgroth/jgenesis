use jgenesis_proc_macros::deserialize_default_on_error;
use pce_config::{
    PceAspectRatio, PceInputDevice, PcePaletteType, PcePsgResampler, PceRegion, PceSystemCardModel,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use std::path::PathBuf;

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PcEngineAppConfig {
    pub cd_bios_path: Option<PathBuf>,
    pub load_disc_into_ram: bool,
    pub always_emulate_cd_rom: bool,
    pub region: PceRegion,
    pub system_card_model: PceSystemCardModel,
    pub cpu_fast_clock_divider: NonZeroU64,
    pub aspect_ratio: PceAspectRatio,
    pub palette: PcePaletteType,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
    pub quantize_adpcm_output: bool,
    pub audio_resampler: PcePsgResampler,
    pub psg_channels_enabled: [bool; 6],
    pub psg_enabled: bool,
    pub cd_da_enabled: bool,
    pub adpcm_enabled: bool,
    pub psg_volume_adjustment_db: f64,
    pub cd_da_volume_adjustment_db: f64,
    pub adpcm_volume_adjustment_db: f64,
    pub input_device: PceInputDevice,
    pub turbo_tap_connected: [bool; pce_config::TURBO_TAP_GAMEPADS as usize],
    pub allow_opposing_joypad_directions: bool,
    pub allow_simultaneous_run_select: bool,
}

impl Default for PcEngineAppConfig {
    fn default() -> Self {
        Self {
            cd_bios_path: None,
            load_disc_into_ram: false,
            always_emulate_cd_rom: false,
            region: PceRegion::default(),
            system_card_model: PceSystemCardModel::default(),
            cpu_fast_clock_divider: NonZeroU64::new(pce_config::NATIVE_FAST_CPU_DIVIDER).unwrap(),
            aspect_ratio: PceAspectRatio::default(),
            palette: PcePaletteType::default(),
            crop_overscan: true,
            remove_sprite_limits: false,
            quantize_adpcm_output: true,
            audio_resampler: PcePsgResampler::default(),
            psg_channels_enabled: [true; 6],
            psg_enabled: true,
            cd_da_enabled: true,
            adpcm_enabled: true,
            psg_volume_adjustment_db: 0.0,
            cd_da_volume_adjustment_db: 0.0,
            adpcm_volume_adjustment_db: 0.0,
            input_device: PceInputDevice::default(),
            turbo_tap_connected: [true, true, false, false, false],
            allow_opposing_joypad_directions: false,
            allow_simultaneous_run_select: false,
        }
    }
}
