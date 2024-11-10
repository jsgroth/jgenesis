use crate::AppConfig;
use jgenesis_native_driver::config::GameBoyAdvanceConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbaAppConfig {
    pub bios_path: Option<String>,
}

impl Default for GbaAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    pub fn gba_config(&self, path: String) -> Box<GameBoyAdvanceConfig> {
        Box::new(GameBoyAdvanceConfig {
            common: self.common_config(path),
            bios_path: self.game_boy_advance.bios_path.clone(),
        })
    }
}
