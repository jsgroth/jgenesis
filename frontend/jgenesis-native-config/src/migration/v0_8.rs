use crate::AppConfig;
use crate::input::GenericInput;
use crate::input::mappings::HotkeyConfig;
use serde::{Deserialize, Serialize};

pub fn migrate_config_0_8_3(config: &mut AppConfig, config_str: &str) {
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

pub fn migrate_config_0_8_4(config: &mut AppConfig) {
    // New hotkey ToggleOverclocking
    config.input.hotkeys.mapping_1.toggle_overclocking =
        HotkeyConfig::default().mapping_1.toggle_overclocking;
}
