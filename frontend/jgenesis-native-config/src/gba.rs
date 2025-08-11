use gba_config::{GbaAspectRatio, GbaColorCorrection, GbaSaveMemory};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBoyAdvanceAppConfig {
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
    #[serde(default)]
    pub skip_bios_animation: bool,
    #[serde(default)]
    pub aspect_ratio: GbaAspectRatio,
    #[serde(default)]
    pub color_correction: GbaColorCorrection,
    #[serde(default)]
    pub forced_save_memory_type: Option<GbaSaveMemory>,
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
