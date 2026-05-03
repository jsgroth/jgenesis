use gba_config::{GbaAspectRatio, GbaAudioInterpolation, GbaColorCorrection, GbaSaveMemory};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Significantly darken
const DEFAULT_CORRECTION_GAMMA: f64 = 3.2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameBoyAdvanceAppConfig {
    pub bios_path: Option<PathBuf>,
    pub skip_bios_animation: bool,
    pub aspect_ratio: GbaAspectRatio,
    pub color_correction: GbaColorCorrection,
    pub color_correction_gamma: f64,
    pub frame_blending: bool,
    pub forced_save_memory_type: Option<GbaSaveMemory>,
    pub audio_interpolation: GbaAudioInterpolation,
    pub psg_low_pass: bool,
    pub pulse_1_enabled: bool,
    pub pulse_2_enabled: bool,
    pub wavetable_enabled: bool,
    pub noise_enabled: bool,
    pub pcm_a_enabled: bool,
    pub pcm_b_enabled: bool,
    pub solar_brightness_step: u8,
    pub solar_min_brightness: u8,
    pub solar_max_brightness: u8,
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        Self {
            bios_path: None,
            skip_bios_animation: false,
            aspect_ratio: GbaAspectRatio::default(),
            color_correction: GbaColorCorrection::default(),
            color_correction_gamma: DEFAULT_CORRECTION_GAMMA,
            frame_blending: true,
            forced_save_memory_type: None,
            audio_interpolation: GbaAudioInterpolation::default(),
            psg_low_pass: false,
            pulse_1_enabled: true,
            pulse_2_enabled: true,
            wavetable_enabled: true,
            noise_enabled: true,
            pcm_a_enabled: true,
            pcm_b_enabled: true,
            solar_brightness_step: gba_config::DEFAULT_SOLAR_BRIGHTNESS_STEP,
            solar_min_brightness: gba_config::DEFAULT_SOLAR_MIN_BRIGHTNESS,
            solar_max_brightness: gba_config::DEFAULT_SOLAR_MAX_BRIGHTNESS,
        }
    }
}
