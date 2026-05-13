use jgenesis_proc_macros::deserialize_default_on_error;
use pce_config::{PceAspectRatio, PcePaletteType, PceRegion};
use serde::{Deserialize, Serialize};

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PcEngineAppConfig {
    pub region: PceRegion,
    pub aspect_ratio: PceAspectRatio,
    pub palette: PcePaletteType,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
    pub allow_simultaneous_run_select: bool,
}

impl Default for PcEngineAppConfig {
    fn default() -> Self {
        Self {
            region: PceRegion::default(),
            aspect_ratio: PceAspectRatio::default(),
            palette: PcePaletteType::default(),
            crop_overscan: true,
            remove_sprite_limits: false,
            allow_simultaneous_run_select: false,
        }
    }
}
