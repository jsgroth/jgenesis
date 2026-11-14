use crate::AppConfig;
use genesis_config::GenesisRegion;
use serde::Deserialize;

pub fn migrate_config_0_15_0(config: &mut AppConfig, config_str: &str) {
    // Genesis region strings changed:
    //   Americas -> NtscUs
    //   Europe -> Pal
    //   Japan -> NtscJp
    migrate_0_15_0_genesis_region(config, config_str);
}

fn migrate_0_15_0_genesis_region(config: &mut AppConfig, config_str: &str) {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    enum OldGenesisRegion {
        Americas,
        Europe,
        Japan,
    }

    #[derive(Deserialize)]
    struct OldGenesisConfig {
        forced_region: Option<OldGenesisRegion>,
    }

    #[derive(Deserialize)]
    struct OldAppConfig {
        genesis: OldGenesisConfig,
    }

    let Ok(old_config) = toml::from_str::<OldAppConfig>(config_str) else { return };
    let Some(old_region) = old_config.genesis.forced_region else { return };

    config.genesis.forced_region = Some(match old_region {
        OldGenesisRegion::Americas => GenesisRegion::NtscUs,
        OldGenesisRegion::Europe => GenesisRegion::Pal,
        OldGenesisRegion::Japan => GenesisRegion::NtscJp,
    });
}

#[cfg(test)]
mod tests {
    use crate::AppConfig;
    use crate::genesis::GenesisAppConfig;
    use crate::migration::v0_15::migrate_0_15_0_genesis_region;
    use genesis_config::GenesisRegion;

    #[test]
    fn migrate_genesis_region_missing() {
        let config_str = r#"
[genesis]
"#;

        let mut config = AppConfig::default();
        migrate_0_15_0_genesis_region(&mut config, config_str);
        assert_eq!(config.genesis.forced_region, GenesisAppConfig::default().forced_region);

        config.genesis.forced_region = Some(GenesisRegion::Pal);
        migrate_0_15_0_genesis_region(&mut config, config_str);
        assert_eq!(config.genesis.forced_region, Some(GenesisRegion::Pal));
    }

    fn migrate_genesis_region_single(region_str: &str, expected: Option<GenesisRegion>) {
        let config_str = format!(
            r#"
[genesis]
forced_region = "{region_str}"
"#
        );

        let mut config = AppConfig::default();
        migrate_0_15_0_genesis_region(&mut config, &config_str);
        assert_eq!(
            config.genesis.forced_region, expected,
            "mismatch for region string {region_str}"
        );
    }

    #[test]
    fn migrate_genesis_region() {
        migrate_genesis_region_single("asdfasdfasdf", GenesisAppConfig::default().forced_region);
        migrate_genesis_region_single("Americas", Some(GenesisRegion::NtscUs));
        migrate_genesis_region_single("Europe", Some(GenesisRegion::Pal));
        migrate_genesis_region_single("Japan", Some(GenesisRegion::NtscJp));
    }
}
