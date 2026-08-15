mod old_default_nes_palette;
mod v0_10;
mod v0_11;
mod v0_12;
mod v0_13;
mod v0_14;
mod v0_8;

use crate::AppConfig;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
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

pub fn migrate_config_str(config_str: &mut String) {
    let Ok(mut document) = config_str.parse::<DocumentMut>() else { return };

    let mut changed = false;

    changed |= v0_12::migrate_document(&mut document);
    changed |= v0_13::migrate_document(&mut document);

    if changed {
        *config_str = document.to_string();
    }
}

pub(crate) const fn current_config_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[must_use]
pub fn migrate_config(config: &AppConfig, config_str: &str) -> Option<AppConfig> {
    const MIN_VERSION_NO_MIGRATION: SemVer = SemVer::new(0, 14, 0);

    if config
        .config_version
        .as_ref()
        .and_then(|version| SemVer::from_str(version).ok())
        .is_some_and(|version| version >= MIN_VERSION_NO_MIGRATION)
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
        v0_8::migrate_config_0_8_3(&mut new_config, config_str);
    }

    if old_version < SemVer::new(0, 8, 4) {
        v0_8::migrate_config_0_8_4(&mut new_config);
    }

    if old_version < SemVer::new(0, 10, 2) {
        v0_10::migrate_config_0_10_2(&mut new_config, config_str);
    }

    if old_version < SemVer::new(0, 11, 4) {
        v0_11::migrate_config_0_11_4(&mut new_config, config_str);
    }

    if old_version < SemVer::new(0, 14, 0) {
        v0_14::migrate_config_0_14_0(&mut new_config, config_str);
    }

    new_config.config_version = Some(current_config_version().into());

    Some(new_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_empty_string_does_not_panic() {
        migrate_config_str(&mut String::new());
    }
}
