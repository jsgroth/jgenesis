use gba_config::{GbaAspectRatio, GbaAudioInterpolation, GbaColorCorrection, GbaSaveMemory};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameBoyAdvanceAppConfig {
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
    #[serde(default)]
    pub skip_bios_animation: bool,
    #[serde(default)]
    pub aspect_ratio: GbaAspectRatio,
    #[serde(default)]
    pub color_correction: GbaColorCorrection,
    #[serde(default = "default_correction_gamma")]
    pub color_correction_gamma: f64,
    #[serde(default = "true_fn")]
    pub frame_blending: bool,
    #[serde(default)]
    pub forced_save_memory_type: Option<GbaSaveMemory>,
    #[serde(default)]
    pub audio_interpolation: GbaAudioInterpolation,
    #[serde(default)]
    pub psg_low_pass: bool,
    #[serde(default = "true_fn")]
    pub pulse_1_enabled: bool,
    #[serde(default = "true_fn")]
    pub pulse_2_enabled: bool,
    #[serde(default = "true_fn")]
    pub wavetable_enabled: bool,
    #[serde(default = "true_fn")]
    pub noise_enabled: bool,
    #[serde(default = "true_fn")]
    pub pcm_a_enabled: bool,
    #[serde(default = "true_fn")]
    pub pcm_b_enabled: bool,
    #[serde(default = "default_solar_brightness_step")]
    pub solar_brightness_step: u8,
    #[serde(default = "default_solar_min_brightness")]
    pub solar_min_brightness: u8,
    #[serde(default = "default_solar_max_brightness")]
    pub solar_max_brightness: u8,
}

const fn true_fn() -> bool {
    true
}

const fn default_correction_gamma() -> f64 {
    3.2 // Significantly darken
}

const fn default_solar_brightness_step() -> u8 {
    gba_config::DEFAULT_SOLAR_BRIGHTNESS_STEP
}

const fn default_solar_min_brightness() -> u8 {
    gba_config::DEFAULT_SOLAR_MIN_BRIGHTNESS
}

const fn default_solar_max_brightness() -> u8 {
    gba_config::DEFAULT_SOLAR_MAX_BRIGHTNESS
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
