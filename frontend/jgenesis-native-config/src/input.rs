use jgenesis_native_driver::config::input::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, NesInputConfig, SmsGgInputConfig,
    SnesInputConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAppConfig {
    #[serde(default)]
    pub smsgg: SmsGgInputConfig,
    #[serde(default)]
    pub genesis: GenesisInputConfig,
    #[serde(default)]
    pub nes: NesInputConfig,
    #[serde(default)]
    pub snes: SnesInputConfig,
    #[serde(default)]
    pub game_boy: GameBoyInputConfig,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
    #[serde(default = "default_axis_deadzone")]
    pub axis_deadzone: i16,
}

fn default_axis_deadzone() -> i16 {
    8000
}

impl Default for InputAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
