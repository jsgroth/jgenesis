#![cfg(feature = "gba")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBoyAdvanceAppConfig {
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
}

impl Default for GameBoyAdvanceAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
