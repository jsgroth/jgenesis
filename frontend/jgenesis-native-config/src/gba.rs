use gba_config::{GbaAspectRatio, GbaColorCorrection, GbaSaveMemory};
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
    pub color_correction_gamma: f32,
    #[serde(default)]
    pub frame_blending: bool,
    #[serde(default)]
    pub forced_save_memory_type: Option<GbaSaveMemory>,
}

const fn default_correction_gamma() -> f32 {
    3.2 // Significantly darken
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
