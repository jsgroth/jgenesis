use jgenesis_proc_macros::deserialize_default_on_error;
use pce_config::{PceAspectRatio, PceRegion};
use serde::{Deserialize, Serialize};

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PcEngineAppConfig {
    pub region: PceRegion,
    pub aspect_ratio: PceAspectRatio,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
}

impl Default for PcEngineAppConfig {
    fn default() -> Self {
        Self {
            region: PceRegion::default(),
            aspect_ratio: PceAspectRatio::default(),
            crop_overscan: true,
            remove_sprite_limits: false,
        }
    }
}
