use toml_edit::DocumentMut;

#[must_use]
pub fn migrate_document(document: &mut DocumentMut) -> bool {
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

        // v0.12.0: Changed scanlines from an enum to a bool+f64 pair
        if let Some((_, scanlines)) = common.get_key_value_mut("scanlines") {
            match scanlines.as_str() {
                Some("Dim") => {
                    *scanlines = toml_edit::Item::None;
                    common.insert("scanlines_enabled", toml_edit::value(true));
                    common.insert("scanlines_brightness", toml_edit::value(0.5));

                    changed = true;
                }
                Some("Black") => {
                    *scanlines = toml_edit::Item::None;
                    common.insert("scanlines_enabled", toml_edit::value(true));
                    common.insert("scanlines_brightness", toml_edit::value(0.0));

                    changed = true;
                }
                _ => {}
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use crate::{AppConfig, migrate_config_str};
    use jgenesis_renderer::config::{AntiDitherShader, PreprocessShader, WgpuBackend};

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

    fn v0_12_0_scanlines(prev_enum_value: &str, expected_brightness: f64) {
        let mut config_str = format!(
            "
[common]
scanlines = \"{prev_enum_value}\"
        "
        );

        migrate_config_str(&mut config_str);

        let config: AppConfig = toml::from_str(&config_str).expect("Failed to parse config");
        assert!(config.common.scanlines_enabled);
        assert_eq!(config.common.scanlines_brightness, expected_brightness);
    }

    #[test]
    fn v0_12_0_scanlines_dim() {
        v0_12_0_scanlines("Dim", 0.5);
    }

    #[test]
    fn v0_12_0_scanlines_black() {
        v0_12_0_scanlines("Black", 0.0);
    }
}
