use crate::AppConfig;
use jgenesis_native_driver::config::GameBoyAdvanceConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbaAppConfig {}

impl AppConfig {
    pub fn gba_config(&self, path: String) -> Box<GameBoyAdvanceConfig> {
        Box::new(GameBoyAdvanceConfig { common: self.common_config(path) })
    }
}
