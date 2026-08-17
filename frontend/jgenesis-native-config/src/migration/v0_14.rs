use crate::input::GenericInput;
use crate::input::mappings::{GenesisControllerMapping, HotkeyConfig, SnesControllerMapping};
use crate::{AppConfig, RomSearchDirectory};
use serde::Deserialize;

pub fn migrate_config_0_14_0(config: &mut AppConfig, config_str: &str) {
    // rom_search_dirs field changed from Vec<String> to Vec<RomSearchDirectory>
    migrate_0_14_0_rom_search_dirs(config, config_str);

    // New buttons/hotkeys for Mega Mouse and SNES Mouse
    migrate_0_14_0_mouse_inputs(config);

    // SNES Super Scope input config format changed
    migrate_0_14_0_super_scope_inputs(config, config_str);
}

fn migrate_0_14_0_rom_search_dirs(config: &mut AppConfig, config_str: &str) {
    #[derive(Deserialize)]
    struct OldConfig {
        rom_search_dirs: Vec<String>,
    }

    let Ok(old_config) = toml::from_str::<OldConfig>(config_str) else { return };

    if !old_config.rom_search_dirs.is_empty() && config.rom_search_dirs.is_empty() {
        log::info!("Converting rom_search_dirs to new config format");

        config.rom_search_dirs = old_config
            .rom_search_dirs
            .into_iter()
            .map(|path| RomSearchDirectory { path: path.into(), recursive: false })
            .collect();
    }
}

fn migrate_0_14_0_mouse_inputs(config: &mut AppConfig) {
    let defaults = GenesisControllerMapping::keyboard_wasd();
    for (mapping, default) in [
        (&mut config.input.genesis.mapping_1.p1.mega_mouse_left, defaults.mega_mouse_left),
        (&mut config.input.genesis.mapping_1.p1.mega_mouse_right, defaults.mega_mouse_right),
        (&mut config.input.genesis.mapping_1.p1.mega_mouse_middle, defaults.mega_mouse_middle),
        (&mut config.input.genesis.mapping_1.p1.mega_mouse_start, defaults.mega_mouse_start),
        (
            &mut config.input.snes.mapping_1.p1.mouse_left,
            SnesControllerMapping::default().mouse_left,
        ),
        (
            &mut config.input.snes.mapping_1.p1.mouse_right,
            SnesControllerMapping::default().mouse_right,
        ),
        (
            &mut config.input.hotkeys.mapping_1.cancel_mouse_input,
            HotkeyConfig::default().mapping_1.cancel_mouse_input,
        ),
    ] {
        if mapping.is_none() {
            *mapping = default;
        }
    }
}

fn migrate_0_14_0_super_scope_inputs(config: &mut AppConfig, config_str: &str) {
    #[derive(Default, Deserialize)]
    struct OldSuperScopeConfig {
        fire: Option<Vec<GenericInput>>,
        cursor: Option<Vec<GenericInput>>,
        pause: Option<Vec<GenericInput>>,
        turbo_toggle: Option<Vec<GenericInput>>,
    }

    #[derive(Default, Deserialize)]
    struct OldSnesInputMapping {
        super_scope: OldSuperScopeConfig,
    }

    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct OldSnesInputConfig {
        mapping_1: OldSnesInputMapping,
        mapping_2: OldSnesInputMapping,
    }

    #[derive(Deserialize)]
    struct OldInputConfig {
        snes: OldSnesInputConfig,
    }

    #[derive(Deserialize)]
    struct OldConfig {
        input: OldInputConfig,
    }

    let Ok(old_config) = toml::from_str::<OldConfig>(config_str) else { return };

    for (new_mapping, old_mapping) in [
        (&mut config.input.snes.mapping_1.p2, &old_config.input.snes.mapping_1.super_scope),
        (&mut config.input.snes.mapping_2.p2, &old_config.input.snes.mapping_2.super_scope),
    ] {
        for (new_field, old_field) in [
            (&mut new_mapping.super_scope_fire, &old_mapping.fire),
            (&mut new_mapping.super_scope_cursor, &old_mapping.cursor),
            (&mut new_mapping.super_scope_pause, &old_mapping.pause),
            (&mut new_mapping.super_scope_turbo_toggle, &old_mapping.turbo_toggle),
        ] {
            if new_field.is_none() {
                new_field.clone_from(old_field);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::KeyboardInput;
    use crate::input::mappings::SnesControllerMapping;
    use sdl3::keyboard::Keycode;
    use sdl3::mouse::MouseButton;

    #[test]
    fn migrate_super_scope_config() {
        let config_str = r#"
[[input.snes.mapping_1.super_scope.fire]]
type = "Mouse"
button = "Left"

[[input.snes.mapping_1.super_scope.cursor]]
type = "Mouse"
button = "Right"

[[input.snes.mapping_1.super_scope.pause]]
type = "Mouse"
button = "Middle"

[[input.snes.mapping_1.super_scope.turbo_toggle]]
type = "Keyboard"
key = "T"
        "#;

        let mut config = AppConfig::default();
        config.input.snes.mapping_1.p2 = SnesControllerMapping::default();
        assert!(config.input.snes.mapping_1.p2.super_scope_fire.is_none());
        assert!(config.input.snes.mapping_1.p2.super_scope_cursor.is_none());
        assert!(config.input.snes.mapping_1.p2.super_scope_pause.is_none());
        assert!(config.input.snes.mapping_1.p2.super_scope_turbo_toggle.is_none());

        migrate_config_0_14_0(&mut config, config_str);

        assert_eq!(
            config.input.snes.mapping_1.p2.super_scope_fire,
            Some(vec![GenericInput::Mouse(MouseButton::Left)])
        );
        assert_eq!(
            config.input.snes.mapping_1.p2.super_scope_cursor,
            Some(vec![GenericInput::Mouse(MouseButton::Right)])
        );
        assert_eq!(
            config.input.snes.mapping_1.p2.super_scope_pause,
            Some(vec![GenericInput::Mouse(MouseButton::Middle)])
        );
        assert_eq!(
            config.input.snes.mapping_1.p2.super_scope_turbo_toggle,
            Some(vec![GenericInput::Keyboard(KeyboardInput::Keycode(Keycode::T))])
        );
    }
}
