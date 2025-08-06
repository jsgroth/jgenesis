#![cfg(feature = "gba")]

use gba_config::{GbaAspectRatio, GbaColorCorrection};
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
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
