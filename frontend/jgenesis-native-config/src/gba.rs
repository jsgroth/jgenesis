use crate::AppConfig;
use jgenesis_native_driver::config::{CommonConfig, GameBoyAdvanceConfig};
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
    #[must_use]
    pub fn gba_config(&self, path: String) -> Box<GameBoyAdvanceConfig> {
        Box::new(GameBoyAdvanceConfig {
            common: CommonConfig {
                // TODO hack: remove this after implementing GBA resampling
                audio_output_frequency: 65536,
                ..self.common_config(path)
            },
            inputs: self.input.gba.clone(),
            bios_path: self.game_boy_advance.bios_path.clone(),
        })
    }
}
