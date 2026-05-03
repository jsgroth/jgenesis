use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::deserialize_default_on_error;
use serde::{Deserialize, Serialize};
use snes_config::{AudioInterpolationMode, SnesAspectRatio};
use std::num::NonZeroU64;
use std::path::PathBuf;

const DEFAULT_GSU_OVERCLOCK: NonZeroU64 = NonZeroU64::new(1).unwrap();

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnesAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    pub aspect_ratio: SnesAspectRatio,
    pub deinterlace: bool,
    pub audio_interpolation: AudioInterpolationMode,
    pub audio_60hz_hack: bool,
    pub gsu_overclock_factor: NonZeroU64,
    pub dsp1_rom_path: Option<PathBuf>,
    pub dsp2_rom_path: Option<PathBuf>,
    pub dsp3_rom_path: Option<PathBuf>,
    pub dsp4_rom_path: Option<PathBuf>,
    pub st010_rom_path: Option<PathBuf>,
    pub st011_rom_path: Option<PathBuf>,
    pub st018_rom_path: Option<PathBuf>,
}

impl Default for SnesAppConfig {
    fn default() -> Self {
        Self {
            forced_timing_mode: None,
            aspect_ratio: SnesAspectRatio::default(),
            deinterlace: true,
            audio_interpolation: AudioInterpolationMode::default(),
            audio_60hz_hack: false,
            gsu_overclock_factor: DEFAULT_GSU_OVERCLOCK,
            dsp1_rom_path: None,
            dsp2_rom_path: None,
            dsp3_rom_path: None,
            dsp4_rom_path: None,
            st010_rom_path: None,
            st011_rom_path: None,
            st018_rom_path: None,
        }
    }
}
