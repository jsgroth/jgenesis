pub mod common;
pub mod gb;
pub mod gba;
pub mod genesis;
pub mod input;
mod migration;
pub mod nes;
pub mod paths;
pub mod pce;
pub mod smsgg;
pub mod snes;

pub use migration::{current_config_version, migrate_config, migrate_config_str};

use crate::common::{CheatPath, CommonAppConfig};
use crate::gb::GameBoyAppConfig;
use crate::gba::GameBoyAdvanceAppConfig;
use crate::genesis::{GenesisAppConfig, Sega32XAppConfig, SegaCdAppConfig};
use crate::input::InputAppConfig;
use crate::nes::NesAppConfig;
use crate::pce::PcEngineAppConfig;
use crate::smsgg::SmsGgAppConfig;
use crate::snes::SnesAppConfig;
use jgenesis_proc_macros::{EnumDisplay, deserialize_default_on_error};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};
use thiserror::Error;

pub const CHEATS_SUBDIR: &str = "cheats";

pub const DEFAULT_GUI_WIDTH: f32 = 900.0;
pub const DEFAULT_GUI_HEIGHT: f32 = 675.0;

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListFilters {
    pub master_system: bool,
    pub game_gear: bool,
    pub sg_1000: bool,
    pub genesis: bool,
    pub sega_cd: bool,
    pub sega_32x: bool,
    pub nes: bool,
    pub snes: bool,
    pub game_boy: bool,
    pub game_boy_color: bool,
    pub game_boy_advance: bool,
    pub pc_engine: bool,
}

impl ListFilters {
    pub const ALL: Self = Self {
        master_system: true,
        game_gear: true,
        sg_1000: true,
        genesis: true,
        sega_cd: true,
        sega_32x: true,
        nes: true,
        snes: true,
        game_boy: true,
        game_boy_color: true,
        game_boy_advance: true,
        pc_engine: true,
    };

    pub const NONE: Self = Self {
        master_system: false,
        game_gear: false,
        sg_1000: false,
        genesis: false,
        sega_cd: false,
        sega_32x: false,
        nes: false,
        snes: false,
        game_boy: false,
        game_boy_color: false,
        game_boy_advance: false,
        pc_engine: false,
    };
}

impl Default for ListFilters {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentOpen {
    pub console: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay)]
pub enum EguiTheme {
    #[default]
    SystemDefault,
    Dark,
    Light,
}

#[derive(Debug, Error)]
pub enum SaveCheatsError {
    #[error("I/O error saving cheats: {0}")]
    Io(#[from] io::Error),
    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::ser::Error),
    #[error(
        "Unable to determine cheats file path; config_path='{config_path}', rom_file_path='{rom_file_path}'"
    )]
    UnableToDeterminePath { config_path: String, rom_file_path: String },
}

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub config_version: Option<String>,
    pub common: CommonAppConfig,
    pub smsgg: SmsGgAppConfig,
    pub genesis: GenesisAppConfig,
    pub sega_cd: SegaCdAppConfig,
    pub sega_32x: Sega32XAppConfig,
    pub nes: NesAppConfig,
    pub snes: SnesAppConfig,
    pub game_boy: GameBoyAppConfig,
    pub game_boy_advance: GameBoyAdvanceAppConfig,
    pub pc_engine: PcEngineAppConfig,
    pub input: InputAppConfig,
    // TODO move GUI-specific config/state somewhere else - separate file?
    pub list_filters: ListFilters,
    pub rom_search_dirs: Vec<String>,
    pub recent_open_list: Vec<RecentOpen>,
    pub egui_theme: EguiTheme,
    pub gui_window_width: f32,
    pub gui_window_height: f32,
}

impl AppConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let config_str = fs::read_to_string(path).unwrap_or_default();
        toml::from_str(&config_str).unwrap_or_else(|err| {
            log::error!("Error deserializing app config: {err}");
            Self::default()
        })
    }

    fn cheats_file_path(
        &self,
        config_path: &Path,
        rom_file_path: &Path,
        system_extension: &str,
    ) -> Option<PathBuf> {
        let cheats_dir = match self.common.cheats_path {
            CheatPath::SettingsFolder => config_path.parent()?.join(CHEATS_SUBDIR),
            CheatPath::EmulatorFolder => {
                jgenesis_common::determine_emulator_dir()?.join(CHEATS_SUBDIR)
            }
            CheatPath::Custom => self.common.cheats_custom_path.clone(),
        };

        let rom_path_toml = rom_file_path.with_extension("toml");
        let rom_file_name_toml = rom_path_toml.file_name()?;

        Some(cheats_dir.join(system_extension).join(rom_file_name_toml))
    }

    #[must_use]
    pub fn try_load_cheats<Cheats>(
        &self,
        config_path: &Path,
        rom_file_path: &Path,
        system_extension: &str,
    ) -> Option<Cheats>
    where
        Cheats: DeserializeOwned,
    {
        let cheats_path = self.cheats_file_path(config_path, rom_file_path, system_extension)?;

        log::debug!("Loading cheats from '{}'", cheats_path.display());

        let cheats_str = fs::read_to_string(&cheats_path).ok()?;
        let cheats: Cheats = toml::from_str(&cheats_str).ok()?;

        log::debug!("Successfully loaded cheats");

        Some(cheats)
    }

    /// Save cheats to a game-specific cheats file.
    ///
    /// # Errors
    ///
    /// Propagates any I/O or serialization errors encountered.
    pub fn save_cheats<Cheats>(
        &self,
        config_path: &Path,
        rom_file_path: &Path,
        system_extension: &str,
        cheats: &Cheats,
    ) -> Result<(), SaveCheatsError>
    where
        Cheats: Serialize,
    {
        let unable_determine_path_err = || SaveCheatsError::UnableToDeterminePath {
            config_path: config_path.display().to_string(),
            rom_file_path: rom_file_path.display().to_string(),
        };

        let Some(cheats_path) = self.cheats_file_path(config_path, rom_file_path, system_extension)
        else {
            return Err(unable_determine_path_err());
        };

        log::debug!("Saving cheats to '{}'", cheats_path.display());

        let Some(cheats_parent) = cheats_path.parent() else {
            return Err(unable_determine_path_err());
        };

        if !cheats_parent.exists() {
            fs::create_dir_all(cheats_parent)?;
        }

        let cheats_str = toml::to_string_pretty(cheats)?;
        fs::write(&cheats_path, &cheats_str)?;

        log::debug!("Successfully saved cheats");

        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: Some(current_config_version().into()),
            common: CommonAppConfig::default(),
            smsgg: SmsGgAppConfig::default(),
            genesis: GenesisAppConfig::default(),
            sega_cd: SegaCdAppConfig::default(),
            sega_32x: Sega32XAppConfig::default(),
            nes: NesAppConfig::default(),
            snes: SnesAppConfig::default(),
            game_boy: GameBoyAppConfig::default(),
            game_boy_advance: GameBoyAdvanceAppConfig::default(),
            pc_engine: PcEngineAppConfig::default(),
            input: InputAppConfig::default(),
            list_filters: ListFilters::default(),
            rom_search_dirs: vec![],
            recent_open_list: vec![],
            egui_theme: EguiTheme::default(),
            gui_window_width: DEFAULT_GUI_WIDTH,
            gui_window_height: DEFAULT_GUI_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }

    #[test]
    fn can_deserialize_empty_string() {
        let config: AppConfig =
            toml::from_str("").expect("Failed to deserialize empty string into AppConfig");
        assert_eq!(config, AppConfig::default());
    }
}
