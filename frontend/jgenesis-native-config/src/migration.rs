mod old_default_nes_palette;

use crate::AppConfig;
use crate::input::GenericInput;
use crate::input::mappings::HotkeyConfig;
use nes_config::NesPalette;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use toml_edit::DocumentMut;

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

pub fn migrate_config_str(config_str: &mut String) {
    let Ok(mut document) = config_str.parse::<DocumentMut>() else { return };

    let mut changed = false;

    if let Some((_, common_value)) = document.get_key_value_mut("common")
        && let Some(common) = common_value.as_table_like_mut()
    {
        // v0.12.0: Removed OpenGL wgpu backend option
        if let Some((_, wgpu_backend)) = common.get_key_value_mut("wgpu_backend")
            && wgpu_backend.as_str() == Some("OpenGl")
        {
            log::info!("OpenGL wgpu backend option no longer exists; changing to Auto");

            *wgpu_backend = toml_edit::value("Auto");
            changed = true;
        }

        // v0.12.0: Moved anti-dither shaders from preprocess_shader to their own config field
        if let Some((_, preprocess_shader)) = common.get_key_value_mut("preprocess_shader") {
            match preprocess_shader.as_str() {
                Some("AntiDitherWeak") => {
                    *preprocess_shader = toml_edit::value("None");
                    common.insert("anti_dither_shader", toml_edit::value("Weak"));

                    changed = true;
                }
                Some("AntiDitherStrong") => {
                    *preprocess_shader = toml_edit::value("None");
                    common.insert("anti_dither_shader", toml_edit::value("Strong"));

                    changed = true;
                }
                _ => {}
            }
        }
    }

    if changed {
        *config_str = document.to_string();
    }
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

    if old_version < SemVer::new(0, 10, 2) {
        migrate_config_0_10_2(&mut new_config, config_str);
    }

    if old_version < SemVer::new(0, 11, 4) {
        migrate_config_0_11_4(&mut new_config, config_str);
    }

    new_config.config_version = Some(current_config_version().into());

    Some(new_config)
}

fn migrate_config_0_8_3(config: &mut AppConfig, config_str: &str) {
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

fn migrate_config_0_10_2(config: &mut AppConfig, config_str: &str) {
    // smsgg.bios_path -> smsgg.sms_bios_path
    // smsgg.boot_from_bios -> smsgg.sms_boot_from_bios

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct OldSmsGgConfig {
        bios_path: Option<PathBuf>,
        boot_from_bios: bool,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct OldAppConfig {
        smsgg: OldSmsGgConfig,
    }

    let Ok(old_config) = toml::from_str::<OldAppConfig>(config_str) else { return };

    config.smsgg.sms_bios_path = old_config.smsgg.bios_path;
    config.smsgg.sms_boot_from_bios = old_config.smsgg.boot_from_bios;
}

fn migrate_config_0_11_4(config: &mut AppConfig, config_str: &str) {
    // NES default palette changed; change it if currently configured to use the old default
    // nes.palette

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct LimitedNesConfig {
        palette: NesPalette,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct LimitedAppConfig {
        nes: LimitedNesConfig,
    }

    let Ok(old_config) = toml::from_str::<LimitedAppConfig>(config_str) else { return };

    if old_config.nes.palette == old_default_nes_palette::PALETTE {
        log::info!("Detected old default NES palette; changing to new default");
        config.nes.palette = NesPalette::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jgenesis_renderer::config::{AntiDitherShader, PreprocessShader, WgpuBackend};

    #[test]
    fn v0_10_2() {
        const OLD_STR: &str = "
[smsgg]
boot_from_bios = true
bios_path = \"/path/to/bios.sms\"
";

        let mut config = AppConfig::default();
        migrate_config_0_10_2(&mut config, OLD_STR);
        assert!(config.smsgg.sms_boot_from_bios);
        assert_eq!(config.smsgg.sms_bios_path, Some("/path/to/bios.sms".into()));
    }

    #[test]
    fn migrate_empty_string_does_not_panic() {
        migrate_config_str(&mut String::new());
    }

    #[test]
    fn v0_12_0_opengl() {
        const OLD_STR: &str = "
[common]
wgpu_backend = \"OpenGl\"
";

        let mut config_str = OLD_STR.to_owned();
        migrate_config_str(&mut config_str);
        let config: AppConfig = toml::from_str(&config_str).expect("Failed to parse config");
        assert_eq!(config.common.wgpu_backend, WgpuBackend::Auto);
    }

    fn v0_12_0_anti_dither(preprocess_str: &str, expected_anti_dither: AntiDitherShader) {
        let mut config_str = format!(
            "
[common]
preprocess_shader = \"{preprocess_str}\"
"
        );

        migrate_config_str(&mut config_str);
        let config: AppConfig = toml::from_str(&config_str).expect("Failed to parse config");
        assert_eq!(config.common.anti_dither_shader, expected_anti_dither);
        assert_eq!(config.common.preprocess_shader, PreprocessShader::None);
    }

    #[test]
    fn v0_12_0_anti_dither_weak() {
        v0_12_0_anti_dither("AntiDitherWeak", AntiDitherShader::Weak);
    }

    #[test]
    fn v0_12_0_anti_dither_strong() {
        v0_12_0_anti_dither("AntiDitherStrong", AntiDitherShader::Strong);
    }
}
