use jgenesis_common::frontend::TimingMode;
use nes_config::{NesAspectRatio, NesAudioResampler, NesPalette, Overscan};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NesAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    pub aspect_ratio: NesAspectRatio,
    #[serde(default = "true_fn")]
    pub ntsc_crop_vertical_overscan: bool,
    #[serde(default)]
    pub overscan: Overscan,
    #[serde(default)]
    pub remove_sprite_limit: bool,
    #[serde(default)]
    pub pal_black_border: bool,
    #[serde(default)]
    pub silence_ultrasonic_triangle_output: bool,
    #[serde(default)]
    pub audio_resampler: NesAudioResampler,
    #[serde(default)]
    pub audio_60hz_hack: bool,
    #[serde(default)]
    pub allow_opposing_joypad_inputs: bool,
    #[serde(default = "true_fn")]
    pub dma_dummy_joy_reads: bool,
    #[serde(default)]
    pub palette: NesPalette,
}

const fn true_fn() -> bool {
    true
}

impl NesAppConfig {
    #[must_use]
    pub fn overscan(&self) -> Overscan {
        self.overscan
    }
}

impl Default for NesAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
