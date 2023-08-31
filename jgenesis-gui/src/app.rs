use eframe::Frame;
use egui::Context;
use jgenesis_native_driver::config::{GgAspectRatio, SmsAspectRatio};
use serde::{Deserialize, Serialize};
use smsgg_core::psg::PsgVersion;
use smsgg_core::VdpVersion;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SmsGgAppConfig {
    vdp_version: Option<VdpVersion>,
    psg_version: Option<PsgVersion>,
    #[serde(default)]
    sms_aspect_ratio: SmsAspectRatio,
    #[serde(default)]
    gg_aspect_ratio: GgAspectRatio,
    #[serde(default)]
    sms_crop_vertical_border: bool,
    #[serde(default)]
    sms_crop_left_border: bool,
}

impl Default for SmsGgAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    smsgg: SmsGgAppConfig,
}

impl AppConfig {
    #[allow(clippy::missing_panics_doc)]
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let config_str = fs::read_to_string(path).unwrap_or(String::new());
        toml::from_str(&config_str).unwrap_or_else(|err| {
            log::error!("Error deserializing app config: {err}");
            toml::from_str("").unwrap()
        })
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

pub struct App {
    _config: AppConfig,
}

impl App {
    #[must_use]
    pub fn new(config: AppConfig) -> Self {
        Self { _config: config }
    }
}

impl eframe::App for App {
    fn update(&mut self, _ctx: &Context, _frame: &mut Frame) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_does_not_panic() {
        let _ = AppConfig::default();
    }
}
