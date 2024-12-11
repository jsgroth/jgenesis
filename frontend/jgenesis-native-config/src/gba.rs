use crate::AppConfig;
use gba_core::api::GbaAspectRatio;
use jgenesis_native_driver::config::GameBoyAdvanceConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbaAppConfig {
    pub bios_path: Option<String>,
    #[serde(default)]
    pub aspect_ratio: GbaAspectRatio,
    #[serde(default)]
    pub skip_bios_intro_animation: bool,
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
            common: self.common_config(path),
            inputs: self.input.gba.clone(),
            bios_path: self.game_boy_advance.bios_path.clone(),
            aspect_ratio: self.game_boy_advance.aspect_ratio,
            skip_bios_intro_animation: self.game_boy_advance.skip_bios_intro_animation,
        })
    }
}
