use crate::AppConfig;
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::SnesConfig;
use serde::{Deserialize, Serialize};
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio};
use std::num::NonZeroU64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnesAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    pub aspect_ratio: SnesAspectRatio,
    #[serde(default = "true_fn")]
    pub deinterlace: bool,
    #[serde(default)]
    pub audio_interpolation: AudioInterpolationMode,
    #[serde(default)]
    pub audio_60hz_hack: bool,
    #[serde(default = "default_gsu_overclock")]
    pub gsu_overclock_factor: NonZeroU64,
    pub dsp1_rom_path: Option<String>,
    pub dsp2_rom_path: Option<String>,
    pub dsp3_rom_path: Option<String>,
    pub dsp4_rom_path: Option<String>,
    pub st010_rom_path: Option<String>,
    pub st011_rom_path: Option<String>,
}

const fn true_fn() -> bool {
    true
}

fn default_gsu_overclock() -> NonZeroU64 {
    NonZeroU64::new(1).unwrap()
}

impl Default for SnesAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn snes_config(&self, path: String) -> Box<SnesConfig> {
        Box::new(SnesConfig {
            common: self.common_config(path),
            inputs: self.input.snes.clone(),
            forced_timing_mode: self.snes.forced_timing_mode,
            aspect_ratio: self.snes.aspect_ratio,
            deinterlace: self.snes.deinterlace,
            audio_interpolation: self.snes.audio_interpolation,
            audio_60hz_hack: self.snes.audio_60hz_hack,
            gsu_overclock_factor: self.snes.gsu_overclock_factor,
            dsp1_rom_path: self.snes.dsp1_rom_path.clone(),
            dsp2_rom_path: self.snes.dsp2_rom_path.clone(),
            dsp3_rom_path: self.snes.dsp3_rom_path.clone(),
            dsp4_rom_path: self.snes.dsp4_rom_path.clone(),
            st010_rom_path: self.snes.st010_rom_path.clone(),
            st011_rom_path: self.snes.st011_rom_path.clone(),
        })
    }
}
