use crate::AppConfig;
use crate::migration::old_default_nes_palette;
use nes_config::NesPalette;
use serde::{Deserialize, Serialize};

pub fn migrate_config_0_11_4(config: &mut AppConfig, config_str: &str) {
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
