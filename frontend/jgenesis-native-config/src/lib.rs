pub mod common;
pub mod gb;
pub mod gba;
pub mod genesis;
pub mod input;
mod migration;
pub mod nes;
pub mod smsgg;
pub mod snes;

pub use migration::{current_config_version, migrate_config, migrate_config_str};

use crate::common::CommonAppConfig;
use crate::gb::GameBoyAppConfig;
use crate::gba::GameBoyAdvanceAppConfig;
use crate::genesis::{GenesisAppConfig, Sega32XAppConfig, SegaCdAppConfig};
use crate::input::InputAppConfig;
use crate::nes::NesAppConfig;
use crate::smsgg::SmsGgAppConfig;
use crate::snes::SnesAppConfig;
use jgenesis_proc_macros::{EnumDisplay, deserialize_default_on_error};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl Default for ListFilters {
    fn default() -> Self {
        Self {
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
        }
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

pub const CONFIG_FILENAME: &str = "jgenesis-config.toml";

#[must_use]
pub fn default_config_path() -> PathBuf {
    cfg_select! {
        target_os = "linux" => default_linux_config_path(),
        _ => CONFIG_FILENAME.into(),
    }
}

#[cfg(target_os = "linux")]
fn default_linux_config_path() -> PathBuf {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        log::error!("Unable to determine config dir; app config will probably not save");
        return CONFIG_FILENAME.into();
    };

    let jgenesis_dir = base_dirs.config_dir().join("jgenesis");
    if !jgenesis_dir.exists()
        && let Err(err) = fs::create_dir_all(&jgenesis_dir)
    {
        log::error!(
            "Unable to create config directory '{}', app config will probably not save: {err}",
            jgenesis_dir.display()
        );
        return CONFIG_FILENAME.into();
    }

    jgenesis_dir.join(CONFIG_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }
}
