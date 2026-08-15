use crate::AppConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn migrate_config_0_10_2(config: &mut AppConfig, config_str: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
