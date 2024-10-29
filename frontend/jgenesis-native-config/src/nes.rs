use crate::AppConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::NesConfig;
use nes_core::api::{NesAspectRatio, Overscan};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NesAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    pub aspect_ratio: NesAspectRatio,
    #[serde(default)]
    pub overscan: Overscan,
    #[serde(default)]
    pub remove_sprite_limit: bool,
    #[serde(default)]
    pub pal_black_border: bool,
    #[serde(default)]
    pub silence_ultrasonic_triangle_output: bool,
    #[serde(default)]
    pub audio_60hz_hack: bool,
    #[serde(default)]
    pub allow_opposing_joypad_inputs: bool,
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

impl AppConfig {
    #[must_use]
    pub fn nes_config(&self, path: String) -> Box<NesConfig> {
        Box::new(NesConfig {
            common: self.common_config(
                path,
                self.inputs.nes_keyboard.clone(),
                self.inputs.nes_joystick.clone(),
            ),
            p2_controller_type: self.inputs.nes_p2_type,
            zapper_config: self.inputs.nes_zapper.clone(),
            forced_timing_mode: self.nes.forced_timing_mode,
            aspect_ratio: self.nes.aspect_ratio,
            overscan: self.nes.overscan,
            remove_sprite_limit: self.nes.remove_sprite_limit,
            pal_black_border: self.nes.pal_black_border,
            silence_ultrasonic_triangle_output: self.nes.silence_ultrasonic_triangle_output,
            audio_refresh_rate_adjustment: self.nes.audio_60hz_hack,
            allow_opposing_joypad_inputs: self.nes.allow_opposing_joypad_inputs,
        })
    }
}
