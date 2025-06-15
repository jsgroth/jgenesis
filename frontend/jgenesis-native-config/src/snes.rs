use jgenesis_common::frontend::TimingMode;
use serde::{Deserialize, Serialize};
use snes_config::{AudioInterpolationMode, SnesAspectRatio};
use std::num::NonZeroU64;
use std::path::PathBuf;

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
    pub dsp1_rom_path: Option<PathBuf>,
    pub dsp2_rom_path: Option<PathBuf>,
    pub dsp3_rom_path: Option<PathBuf>,
    pub dsp4_rom_path: Option<PathBuf>,
    pub st010_rom_path: Option<PathBuf>,
    pub st011_rom_path: Option<PathBuf>,
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
