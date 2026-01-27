pub mod common;
pub mod gb;
pub mod gba;
pub mod genesis;
pub mod input;
mod migration;
pub mod nes;
pub mod smsgg;
pub mod snes;

pub use migration::{current_config_version, migrate_config};

use crate::common::CommonAppConfig;
use crate::gb::GameBoyAppConfig;
use crate::gba::GameBoyAdvanceAppConfig;
use crate::genesis::{GenesisAppConfig, Sega32XAppConfig, SegaCdAppConfig};
use crate::input::InputAppConfig;
use crate::nes::NesAppConfig;
use crate::smsgg::SmsGgAppConfig;
use crate::snes::SnesAppConfig;
use cfg_if::cfg_if;
use jgenesis_proc_macros::EnumDisplay;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_GUI_WIDTH: f32 = 900.0;
pub const DEFAULT_GUI_HEIGHT: f32 = 675.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListFilters {
    #[serde(default = "true_fn")]
    pub master_system: bool,
    #[serde(default = "true_fn")]
    pub game_gear: bool,
    #[serde(default = "true_fn")]
    pub genesis: bool,
    #[serde(default = "true_fn")]
    pub sega_cd: bool,
    #[serde(default = "true_fn")]
    pub sega_32x: bool,
    #[serde(default = "true_fn")]
    pub nes: bool,
    #[serde(default = "true_fn")]
    pub snes: bool,
    #[serde(default = "true_fn")]
    pub game_boy: bool,
    #[serde(default = "true_fn")]
    pub game_boy_color: bool,
    #[serde(default = "true_fn")]
    pub game_boy_advance: bool,
}

fn true_fn() -> bool {
    true
}

impl Default for ListFilters {
    fn default() -> Self {
        toml::from_str("").unwrap()
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub config_version: Option<String>,
    #[serde(default)]
    pub common: CommonAppConfig,
    #[serde(default)]
    pub smsgg: SmsGgAppConfig,
    #[serde(default)]
    pub genesis: GenesisAppConfig,
    #[serde(default)]
    pub sega_cd: SegaCdAppConfig,
    #[serde(default)]
    pub sega_32x: Sega32XAppConfig,
    #[serde(default)]
    pub nes: NesAppConfig,
    #[serde(default)]
    pub snes: SnesAppConfig,
    #[serde(default)]
    pub game_boy: GameBoyAppConfig,
    #[serde(default)]
    pub game_boy_advance: GameBoyAdvanceAppConfig,
    #[serde(default)]
    pub input: InputAppConfig,
    // TODO move GUI-specific config/state somewhere else - separate file?
    #[serde(default)]
    pub list_filters: ListFilters,
    #[serde(default)]
    pub rom_search_dirs: Vec<String>,
    #[serde(default)]
    pub recent_open_list: Vec<RecentOpen>,
    #[serde(default)]
    pub egui_theme: EguiTheme,
    #[serde(default = "default_gui_width")]
    pub gui_window_width: f32,
    #[serde(default = "default_gui_height")]
    pub gui_window_height: f32,
}

fn default_gui_width() -> f32 {
    DEFAULT_GUI_WIDTH
}

fn default_gui_height() -> f32 {
    DEFAULT_GUI_HEIGHT
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
        let mut config: AppConfig = toml::from_str("").unwrap();
        config.config_version = Some(current_config_version().into());
        config
    }
}

pub const CONFIG_FILENAME: &str = "jgenesis-config.toml";

#[must_use]
pub fn default_config_path() -> PathBuf {
    cfg_if! {
        if #[cfg(target_os = "linux")] {
            default_linux_config_path()
        } else {
            CONFIG_FILENAME.into()
        }
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
