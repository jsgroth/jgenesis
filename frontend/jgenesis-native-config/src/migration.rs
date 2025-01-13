use crate::AppConfig;
use jgenesis_native_driver::config::input::HotkeyConfig;
use jgenesis_native_driver::input::GenericInput;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemVer {
    major: u32,
    minor: u32,
    patch: u32,
}

impl SemVer {
    const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

impl Display for SemVer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for SemVer {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_fn = || format!("Invalid semver string: {s}");

        let Ok(split) = s.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>() else {
            return Err(err_fn());
        };

        match split.as_slice() {
            &[major, minor, patch] => Ok(Self { major, minor, patch }),
            _ => Err(err_fn()),
        }
    }
}

#[must_use]
pub const fn current_config_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[must_use]
pub fn migrate_config(config: &AppConfig, config_str: &str) -> Option<AppConfig> {
    if config
        .config_version
        .as_ref()
        .is_some_and(|version| version.as_str() == current_config_version())
    {
        return None;
    }

    let old_version = config
        .config_version
        .as_ref()
        .and_then(|s| s.parse::<SemVer>().ok())
        .unwrap_or(SemVer::new(0, 0, 0));

    log::info!("Migrating config from version {old_version} to {}", current_config_version());

    let mut new_config = config.clone();
    if old_version < SemVer::new(0, 8, 3) {
        migrate_config_0_8_3(&mut new_config, config_str);
    }

    if old_version < SemVer::new(0, 8, 4) {
        migrate_config_0_8_4(&mut new_config);
    }

    new_config.config_version = Some(current_config_version().into());

    Some(new_config)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OldHotkeyMapping {
    #[serde(default)]
    pub quit: Option<Vec<GenericInput>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OldHotkeyConfig {
    #[serde(default)]
    pub mapping_1: OldHotkeyMapping,
    #[serde(default)]
    pub mapping_2: OldHotkeyMapping,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OldInputConfig {
    #[serde(default)]
    pub hotkeys: OldHotkeyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OldAppConfig {
    #[serde(default)]
    pub input: OldInputConfig,
}

fn migrate_config_0_8_3(config: &mut AppConfig, config_str: &str) {
    // Quit hotkey renamed to PowerOff
    if let Ok(mut old_config) = toml::from_str::<OldAppConfig>(config_str) {
        if let Some(mapping) = old_config.input.hotkeys.mapping_1.quit.take() {
            log::info!(
                "Migrating hotkey mapping #1 for 'quit' to 'power_off': ({})",
                stringify_mapping(&mapping)
            );
            config.input.hotkeys.mapping_1.power_off = Some(mapping);
        }

        if let Some(mapping) = old_config.input.hotkeys.mapping_2.quit.take() {
            log::info!(
                "Migrating hotkey mapping #2 for 'quit' to 'power_off': ({})",
                stringify_mapping(&mapping)
            );
            config.input.hotkeys.mapping_2.power_off = Some(mapping);
        }
    }

    // New hotkey Exit
    if config.input.hotkeys.mapping_1.exit.is_none() {
        let default = HotkeyConfig::default();
        if let Some(mapping) = default.mapping_1.exit {
            log::info!(
                "Setting default mapping for new 'exit' hotkey: ({})",
                stringify_mapping(&mapping)
            );
            config.input.hotkeys.mapping_1.exit = Some(mapping);
        }
    }
}

fn stringify_mapping(mapping: &[GenericInput]) -> String {
    let strings: Vec<_> = mapping.iter().map(GenericInput::to_string).collect();
    strings.join(" + ")
}

fn migrate_config_0_8_4(config: &mut AppConfig) {
    // New hotkey ToggleOverclocking
    config.input.hotkeys.mapping_1.toggle_overclocking =
        HotkeyConfig::default().mapping_1.toggle_overclocking;
}
