use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::deserialize_default_on_error;
use nes_config::{NesAspectRatio, NesAudioResampler, NesPalette, Overscan};
use serde::{Deserialize, Serialize};

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NesAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    pub aspect_ratio: NesAspectRatio,
    pub ntsc_crop_vertical_overscan: bool,
    pub overscan: Overscan,
    pub remove_sprite_limit: bool,
    pub pal_black_border: bool,
    pub silence_ultrasonic_triangle_output: bool,
    pub audio_resampler: NesAudioResampler,
    pub audio_60hz_hack: bool,
    pub allow_opposing_joypad_inputs: bool,
    pub dma_dummy_joy_reads: bool,
    pub palette: NesPalette,
}

impl NesAppConfig {
    #[must_use]
    pub fn overscan(&self) -> Overscan {
        self.overscan
    }
}

impl Default for NesAppConfig {
    fn default() -> Self {
        Self {
            forced_timing_mode: None,
            aspect_ratio: NesAspectRatio::default(),
            ntsc_crop_vertical_overscan: true,
            overscan: Overscan::default(),
            remove_sprite_limit: false,
            pal_black_border: false,
            silence_ultrasonic_triangle_output: false,
            audio_resampler: NesAudioResampler::default(),
            audio_60hz_hack: false,
            allow_opposing_joypad_inputs: false,
            dma_dummy_joy_reads: true,
            palette: NesPalette::default(),
        }
    }
}
