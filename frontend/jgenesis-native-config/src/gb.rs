use crate::AppConfig;
use gb_core::api::{GbAspectRatio, GbPalette, GbcColorCorrection};
use jgenesis_native_driver::config::GameBoyConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBoyAppConfig {
    #[serde(default)]
    pub force_dmg_mode: bool,
    #[serde(default)]
    pub pretend_to_be_gba: bool,
    #[serde(default)]
    pub aspect_ratio: GbAspectRatio,
    #[serde(default)]
    pub gb_palette: GbPalette,
    #[serde(default = "default_gb_custom_palette")]
    pub gb_custom_palette: [(u8, u8, u8); 4],
    #[serde(default)]
    pub gbc_color_correction: GbcColorCorrection,
    #[serde(default)]
    pub audio_60hz_hack: bool,
}

#[must_use]
pub const fn default_gb_custom_palette() -> [(u8, u8, u8); 4] {
    // Default to black and white
    [(0xFF, 0xFF, 0xFF), (0xAA, 0xAA, 0xAA), (0x55, 0x55, 0x55), (0x00, 0x00, 0x00)]
}

impl Default for GameBoyAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn gb_config(&self, path: String) -> Box<GameBoyConfig> {
        Box::new(GameBoyConfig {
            common: self.common_config(path),
            inputs: self.input.game_boy.clone(),
            force_dmg_mode: self.game_boy.force_dmg_mode,
            pretend_to_be_gba: self.game_boy.pretend_to_be_gba,
            aspect_ratio: self.game_boy.aspect_ratio,
            gb_palette: self.game_boy.gb_palette,
            gb_custom_palette: self.game_boy.gb_custom_palette,
            gbc_color_correction: self.game_boy.gbc_color_correction,
            audio_60hz_hack: self.game_boy.audio_60hz_hack,
        })
    }
}
