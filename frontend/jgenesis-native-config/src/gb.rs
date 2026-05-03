use gb_config::{GbAspectRatio, GbAudioResampler, GbPalette, GbcColorCorrection};
use jgenesis_proc_macros::deserialize_default_on_error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Default to black and white
const DEFAULT_GB_CUSTOM_PALETTE: [(u8, u8, u8); 4] =
    [(0xFF, 0xFF, 0xFF), (0xAA, 0xAA, 0xAA), (0x55, 0x55, 0x55), (0x00, 0x00, 0x00)];

// Slightly brighten
const DEFAULT_GBC_GAMMA: f64 = 2.0;

// Significantly darken
const DEFAULT_GBA_GAMMA: f64 = 3.2;

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameBoyAppConfig {
    pub force_dmg_mode: bool,
    pub force_cgb_mode: bool,
    pub pretend_to_be_gba: bool,
    pub dmg_boot_rom: bool,
    pub cgb_boot_rom: bool,
    pub dmg_boot_rom_path: Option<PathBuf>,
    pub cgb_boot_rom_path: Option<PathBuf>,
    pub aspect_ratio: GbAspectRatio,
    pub gb_palette: GbPalette,
    pub gb_custom_palette: [(u8, u8, u8); 4],
    pub gbc_color_correction: GbcColorCorrection,
    pub gbc_correction_gamma: f64,
    pub gba_correction_gamma: f64,
    pub frame_blending: bool,
    pub audio_resampler: GbAudioResampler,
    pub audio_60hz_hack: bool,
}

#[must_use]
pub const fn default_gb_custom_palette() -> [(u8, u8, u8); 4] {
    // Default to black and white
    [(0xFF, 0xFF, 0xFF), (0xAA, 0xAA, 0xAA), (0x55, 0x55, 0x55), (0x00, 0x00, 0x00)]
}

impl Default for GameBoyAppConfig {
    fn default() -> Self {
        Self {
            force_dmg_mode: false,
            force_cgb_mode: false,
            pretend_to_be_gba: false,
            dmg_boot_rom: false,
            cgb_boot_rom: false,
            dmg_boot_rom_path: None,
            cgb_boot_rom_path: None,
            aspect_ratio: GbAspectRatio::default(),
            gb_palette: GbPalette::default(),
            gb_custom_palette: DEFAULT_GB_CUSTOM_PALETTE,
            gbc_color_correction: GbcColorCorrection::default(),
            gbc_correction_gamma: DEFAULT_GBC_GAMMA,
            gba_correction_gamma: DEFAULT_GBA_GAMMA,
            frame_blending: true,
            audio_resampler: GbAudioResampler::default(),
            audio_60hz_hack: false,
        }
    }
}
