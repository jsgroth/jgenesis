use genesis_core::GenesisControllerType;
use jgenesis_native_driver::config::input::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, JoystickInput, KeyboardInput,
    NesControllerType, NesInputConfig, SmsGgInputConfig, SnesControllerType, SnesInputConfig,
    SuperScopeConfig, ZapperConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAppConfig {
    #[serde(default)]
    pub smsgg_keyboard: SmsGgInputConfig<KeyboardInput>,
    #[serde(default)]
    pub smsgg_joystick: SmsGgInputConfig<JoystickInput>,
    #[serde(default)]
    pub genesis_p1_type: GenesisControllerType,
    #[serde(default)]
    pub genesis_p2_type: GenesisControllerType,
    #[serde(default)]
    pub genesis_keyboard: GenesisInputConfig<KeyboardInput>,
    #[serde(default)]
    pub genesis_joystick: GenesisInputConfig<JoystickInput>,
    #[serde(default)]
    pub nes_keyboard: NesInputConfig<KeyboardInput>,
    #[serde(default)]
    pub nes_joystick: NesInputConfig<JoystickInput>,
    #[serde(default)]
    pub nes_p2_type: NesControllerType,
    #[serde(default)]
    pub nes_zapper: ZapperConfig,
    #[serde(default)]
    pub snes_keyboard: SnesInputConfig<KeyboardInput>,
    #[serde(default)]
    pub snes_joystick: SnesInputConfig<JoystickInput>,
    #[serde(default)]
    pub snes_p2_type: SnesControllerType,
    #[serde(default)]
    pub snes_super_scope: SuperScopeConfig,
    #[serde(default = "default_gb_keyboard_config")]
    pub gb_keyboard: GameBoyInputConfig<KeyboardInput>,
    #[serde(default)]
    pub gb_joystick: GameBoyInputConfig<JoystickInput>,
    #[serde(default = "default_axis_deadzone")]
    pub axis_deadzone: i16,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

impl Default for InputAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_gb_keyboard_config() -> GameBoyInputConfig<KeyboardInput> {
    GameBoyInputConfig::default_p1()
}

fn default_axis_deadzone() -> i16 {
    8000
}
