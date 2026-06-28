use jgenesis_proc_macros::deserialize_default_on_error;
use pce_config::{PceAspectRatio, PceAudioResampler, PceInputDevice, PcePaletteType, PceRegion};
use serde::{Deserialize, Serialize};

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PcEngineAppConfig {
    pub region: PceRegion,
    pub aspect_ratio: PceAspectRatio,
    pub palette: PcePaletteType,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
    pub audio_resampler: PceAudioResampler,
    pub input_device: PceInputDevice,
    pub turbo_tap_connected: [bool; pce_config::TURBO_TAP_GAMEPADS as usize],
    pub allow_opposing_joypad_directions: bool,
    pub allow_simultaneous_run_select: bool,
}

impl Default for PcEngineAppConfig {
    fn default() -> Self {
        Self {
            region: PceRegion::default(),
            aspect_ratio: PceAspectRatio::default(),
            palette: PcePaletteType::default(),
            crop_overscan: true,
            remove_sprite_limits: false,
            audio_resampler: PceAudioResampler::default(),
            input_device: PceInputDevice::default(),
            turbo_tap_connected: [true, true, false, false, false],
            allow_opposing_joypad_directions: false,
            allow_simultaneous_run_select: false,
        }
    }
}
