use gb_config::{GbAspectRatio, GbAudioResampler, GbPalette, GbcColorCorrection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBoyAppConfig {
    #[serde(default)]
    pub force_dmg_mode: bool,
    #[serde(default)]
    pub force_cgb_mode: bool,
    #[serde(default)]
    pub pretend_to_be_gba: bool,
    #[serde(default)]
    pub dmg_boot_rom: bool,
    #[serde(default)]
    pub cgb_boot_rom: bool,
    #[serde(default)]
    pub dmg_boot_rom_path: Option<PathBuf>,
    #[serde(default)]
    pub cgb_boot_rom_path: Option<PathBuf>,
    #[serde(default)]
    pub aspect_ratio: GbAspectRatio,
    #[serde(default)]
    pub gb_palette: GbPalette,
    #[serde(default = "default_gb_custom_palette")]
    pub gb_custom_palette: [(u8, u8, u8); 4],
    #[serde(default)]
    pub gbc_color_correction: GbcColorCorrection,
    #[serde(default)]
    pub audio_resampler: GbAudioResampler,
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
