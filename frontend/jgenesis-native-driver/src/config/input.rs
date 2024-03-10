use std::fmt::{Display, Formatter};

use sdl2::keyboard::Keycode;
use serde::{Deserialize, Serialize};

use gb_core::inputs::GameBoyButton;
use genesis_core::input::GenesisButton;
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay, EnumFromStr};
use nes_core::input::NesButton;
use smsgg_core::SmsGgButton;
use snes_core::input::{SnesControllerButton, SuperScopeButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AxisDirection {
    Positive,
    Negative,
}

impl Display for AxisDirection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Positive => write!(f, "+"),
            Self::Negative => write!(f, "-"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumDisplay)]
pub enum HatDirection {
    Up,
    Left,
    Right,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JoystickAction {
    Button { button_idx: u8 },
    Axis { axis_idx: u8, direction: AxisDirection },
    Hat { hat_idx: u8, direction: HatDirection },
}

impl Display for JoystickAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button { button_idx } => write!(f, "Button {button_idx}"),
            Self::Axis { axis_idx, direction } => write!(f, "Axis {axis_idx} {direction}"),
            Self::Hat { hat_idx, direction } => write!(f, "Hat {hat_idx} {direction}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JoystickDeviceId {
    pub name: String,
    pub idx: u32, // Used to disambiguate if multiple controllers with the same name are connected
}

impl JoystickDeviceId {
    #[must_use]
    pub fn new(name: String, idx: u32) -> Self {
        Self { name, idx }
    }
}

impl Display for JoystickDeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} #{}", self.name, self.idx)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JoystickInput {
    pub device: JoystickDeviceId,
    pub action: JoystickAction,
}

impl Display for JoystickInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.action, self.device)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardInput {
    pub keycode: String,
}

impl Display for KeyboardInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.keycode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardOrMouseInput {
    Keyboard(String),
    MouseLeft,
    MouseRight,
    MouseMiddle,
    MouseX1,
    MouseX2,
}

impl Display for KeyboardOrMouseInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyboard(keycode) => write!(f, "{keycode}"),
            Self::MouseLeft => write!(f, "Mouse Left Button"),
            Self::MouseRight => write!(f, "Mouse Right Button"),
            Self::MouseMiddle => write!(f, "Mouse Middle Button"),
            Self::MouseX1 => write!(f, "Mouse Extra Button 1"),
            Self::MouseX2 => write!(f, "Mouse Extra Button 2"),
        }
    }
}

pub trait InputConfig {
    type Button;
    type Input;

    fn get_input(&self, button: Self::Button, player: Player) -> Option<&Self::Input>;

    fn set_input(&mut self, button: Self::Button, player: Player, input: Self::Input);

    fn clear_input(&mut self, button: Self::Button, player: Player);
}

macro_rules! define_controller_config {
    (
        controller_cfg: $controller_cfg_name:ident,
        button: $button_t:ident,
        fields: [$($field:ident: button $button:ident default $keycode:ident),* $(,)?]
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
        pub struct $controller_cfg_name<Input> {
            $(
                pub $field: Option<Input>,
            )*
        }

        impl<Input> Default for $controller_cfg_name<Input> {
            fn default() -> Self {
                Self {
                    $(
                        $field: None,
                    )*
                }
            }
        }

        impl<Input> $controller_cfg_name<Input> {
            #[inline]
            #[must_use]
            #[allow(unreachable_patterns)]
            pub fn get_button(&self, button: $button_t) -> Option<&Input> {
                match button {
                    $(
                        $button_t::$button => self.$field.as_ref(),
                    )*
                    _ => None,
                }
            }

            #[inline]
            #[allow(unreachable_patterns)]
            pub fn set_button(&mut self, button: $button_t, input: Input) {
                match button {
                    $(
                        $button_t::$button => self.$field = Some(input),
                    )*
                    _ => {}
                }
            }

            #[inline]
            #[allow(unreachable_patterns)]
            pub fn clear_button(&mut self, button: $button_t) {
                match button {
                    $(
                        $button_t::$button => self.$field = None,
                    )*
                    _ => {}
                }
            }
        }

        impl $controller_cfg_name<KeyboardInput> {
            #[must_use]
            pub fn default_p1() -> Self {
                Self {
                    $(
                        $field: Some(KeyboardInput { keycode: Keycode::$keycode.name() }),
                    )*
                }
            }
        }
    }
}

macro_rules! define_input_config {
    (
        input_cfg: $input_cfg:ident,
        controller_cfg: $controller_cfg:ident,
        button: $button_t:ident
        $(, console_button: $console_btn_field:ident: button $console_btn:ident default $console_btn_default:ident),*
        $(, extra: $extra_field:ident: $extra_field_t:ident),*
        $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
        pub struct $input_cfg<Input> {
            #[indent_nested]
            pub p1: $controller_cfg<Input>,
            #[indent_nested]
            pub p2: $controller_cfg<Input>,
            $(pub $console_btn_field: Option<Input>,)*
            $(pub $extra_field: $extra_field_t,)?
        }

        impl Default for $input_cfg<KeyboardInput> {
            fn default() -> Self {
                Self {
                    p1: $controller_cfg::default_p1(),
                    p2: $controller_cfg::default(),
                    $($console_btn_field: Some(KeyboardInput { keycode: Keycode::$console_btn_default.name() }),)*
                    $($extra_field: $extra_field_t::default(),)?
                }
            }
        }

        impl Default for $input_cfg<JoystickInput> {
            fn default() -> Self {
                Self {
                    p1: $controller_cfg::default(),
                    p2: $controller_cfg::default(),
                    $($console_btn_field: None,)*
                    $($extra_field: $extra_field_t::default(),)?
                }
            }
        }

        impl<Input> InputConfig for $input_cfg<Input> {
            type Button = $button_t;
            type Input = Input;

            #[inline]
            #[must_use]
            fn get_input(&self, button: $button_t, player: Player) -> Option<&Input> {
                match (button, player) {
                    $(
                        ($button_t::$console_btn, _) => self.$console_btn_field.as_ref(),
                    )*
                    (_, Player::One) => self.p1.get_button(button),
                    (_, Player::Two) => self.p2.get_button(button),
                }
            }

            #[inline]
            fn set_input(&mut self, button: $button_t, player: Player, input: Input) {
                match (button, player) {
                    $(
                        ($button_t::$console_btn, _) => self.$console_btn_field = Some(input),
                    )*
                    (_, Player::One) => self.p1.set_button(button, input),
                    (_, Player::Two) => self.p2.set_button(button, input),
                }
            }

            #[inline]
            fn clear_input(&mut self, button: $button_t, player: Player) {
                match (button, player) {
                    $(
                        ($button_t::$console_btn, _) => self.$console_btn_field = None,
                    )*
                    (_, Player::One) => self.p1.clear_button(button),
                    (_, Player::Two) => self.p2.clear_button(button),
                }
            }
        }
    }
}

define_controller_config!(controller_cfg: SmsGgControllerConfig, button: SmsGgButton, fields: [
    up: button Up default Up,
    left: button Left default Left,
    right: button Right default Right,
    down: button Down default Down,
    button1: button Button1 default S,
    button2: button Button2 default A,
]);

define_input_config!(
    input_cfg: SmsGgInputConfig,
    controller_cfg: SmsGgControllerConfig,
    button: SmsGgButton,
    console_button: pause: button Pause default Return,
);

define_controller_config!(controller_cfg: GenesisControllerConfig, button: GenesisButton, fields: [
    up: button Up default Up,
    left: button Left default Left,
    right: button Right default Right,
    down: button Down default Down,
    a: button A default A,
    b: button B default S,
    c: button C default D,
    x: button X default Q,
    y: button Y default W,
    z: button Z default E,
    start: button Start default Return,
    mode: button Mode default RShift,
]);

define_input_config!(input_cfg: GenesisInputConfig, controller_cfg: GenesisControllerConfig, button: GenesisButton);

define_controller_config!(controller_cfg: NesControllerConfig, button: NesButton, fields: [
    up: button Up default Up,
    left: button Left default Left,
    right: button Right default Right,
    down: button Down default Down,
    a: button A default A,
    b: button B default S,
    start: button Start default Return,
    select: button Select default RShift,
]);

define_input_config!(input_cfg: NesInputConfig, controller_cfg: NesControllerConfig, button: NesButton);

define_controller_config!(controller_cfg: SnesControllerConfig, button: SnesControllerButton, fields: [
    up: button Up default Up,
    left: button Left default Left,
    right: button Right default Right,
    down: button Down default Down,
    a: button A default S,
    b: button B default X,
    x: button X default A,
    y: button Y default Z,
    l: button L default D,
    r: button R default C,
    start: button Start default Return,
    select: button Select default RShift,
]);

define_input_config!(
    input_cfg: SnesInputConfig,
    controller_cfg: SnesControllerConfig,
    button: SnesControllerButton,
    extra: super_scope: SuperScopeConfig,
);

define_controller_config!(controller_cfg: GameBoyInputConfig, button: GameBoyButton, fields: [
    up: button Up default Up,
    left: button Left default Left,
    right: button Right default Right,
    down: button Down default Down,
    a: button A default A,
    b: button B default S,
    start: button Start default Return,
    select: button Select default RShift,
]);

impl<Input> InputConfig for GameBoyInputConfig<Input> {
    type Button = GameBoyButton;
    type Input = Input;

    #[inline]
    #[must_use]
    fn get_input(&self, button: Self::Button, player: Player) -> Option<&Self::Input> {
        if player != Player::One {
            return None;
        }

        self.get_button(button)
    }

    #[inline]
    fn set_input(&mut self, button: Self::Button, player: Player, input: Self::Input) {
        if player != Player::One {
            return;
        }

        self.set_button(button, input);
    }

    #[inline]
    fn clear_input(&mut self, button: Self::Button, player: Player) {
        if player != Player::One {
            return;
        }

        self.clear_button(button);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
pub struct SuperScopeConfig {
    pub fire: Option<KeyboardOrMouseInput>,
    pub cursor: Option<KeyboardOrMouseInput>,
    pub pause: Option<KeyboardOrMouseInput>,
    pub turbo_toggle: Option<KeyboardOrMouseInput>,
}

impl Default for SuperScopeConfig {
    fn default() -> Self {
        Self {
            fire: Some(KeyboardOrMouseInput::MouseLeft),
            cursor: Some(KeyboardOrMouseInput::MouseRight),
            pause: Some(KeyboardOrMouseInput::MouseMiddle),
            turbo_toggle: Some(KeyboardOrMouseInput::Keyboard("T".into())),
        }
    }
}

impl SuperScopeConfig {
    #[inline]
    #[must_use]
    pub fn get_button(&self, button: SuperScopeButton) -> Option<&KeyboardOrMouseInput> {
        match button {
            SuperScopeButton::Fire => self.fire.as_ref(),
            SuperScopeButton::Cursor => self.cursor.as_ref(),
            SuperScopeButton::Pause => self.pause.as_ref(),
            SuperScopeButton::TurboToggle => self.turbo_toggle.as_ref(),
        }
    }

    #[inline]
    pub fn set_button(&mut self, button: SuperScopeButton, input: KeyboardOrMouseInput) {
        match button {
            SuperScopeButton::Fire => self.fire = Some(input),
            SuperScopeButton::Cursor => self.cursor = Some(input),
            SuperScopeButton::Pause => self.pause = Some(input),
            SuperScopeButton::TurboToggle => self.turbo_toggle = Some(input),
        }
    }

    #[inline]
    pub fn clear_button(&mut self, button: SuperScopeButton) {
        match button {
            SuperScopeButton::Fire => self.fire = None,
            SuperScopeButton::Cursor => self.cursor = None,
            SuperScopeButton::Pause => self.pause = None,
            SuperScopeButton::TurboToggle => self.turbo_toggle = None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
pub enum SnesControllerType {
    #[default]
    Gamepad,
    SuperScope,
}

#[derive(Debug, Clone, PartialEq, Eq, ConfigDisplay, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_quit")]
    pub quit: Option<KeyboardInput>,
    #[serde(default = "default_toggle_fullscreen")]
    pub toggle_fullscreen: Option<KeyboardInput>,
    #[serde(default = "default_save_state")]
    pub save_state: Option<KeyboardInput>,
    #[serde(default = "default_load_state")]
    pub load_state: Option<KeyboardInput>,
    #[serde(default = "default_soft_reset")]
    pub soft_reset: Option<KeyboardInput>,
    #[serde(default = "default_hard_reset")]
    pub hard_reset: Option<KeyboardInput>,
    #[serde(default = "default_pause")]
    pub pause: Option<KeyboardInput>,
    #[serde(default = "default_step_frame")]
    pub step_frame: Option<KeyboardInput>,
    #[serde(default = "default_fast_forward")]
    pub fast_forward: Option<KeyboardInput>,
    #[serde(default = "default_rewind")]
    pub rewind: Option<KeyboardInput>,
    #[serde(default = "default_open_debugger")]
    pub open_debugger: Option<KeyboardInput>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            quit: default_quit(),
            toggle_fullscreen: default_toggle_fullscreen(),
            save_state: default_save_state(),
            load_state: default_load_state(),
            soft_reset: default_soft_reset(),
            hard_reset: default_hard_reset(),
            pause: default_pause(),
            step_frame: default_step_frame(),
            fast_forward: default_fast_forward(),
            rewind: default_rewind(),
            open_debugger: default_open_debugger(),
        }
    }
}

macro_rules! key_input {
    ($key:ident) => {
        Some(KeyboardInput { keycode: Keycode::$key.name() })
    };
}

fn default_quit() -> Option<KeyboardInput> {
    key_input!(Escape)
}

fn default_toggle_fullscreen() -> Option<KeyboardInput> {
    key_input!(F9)
}

fn default_save_state() -> Option<KeyboardInput> {
    key_input!(F5)
}

fn default_load_state() -> Option<KeyboardInput> {
    key_input!(F6)
}

fn default_soft_reset() -> Option<KeyboardInput> {
    key_input!(F1)
}

fn default_hard_reset() -> Option<KeyboardInput> {
    key_input!(F2)
}

fn default_pause() -> Option<KeyboardInput> {
    key_input!(P)
}

fn default_step_frame() -> Option<KeyboardInput> {
    key_input!(N)
}

fn default_fast_forward() -> Option<KeyboardInput> {
    key_input!(Tab)
}

fn default_rewind() -> Option<KeyboardInput> {
    key_input!(Backquote)
}

fn default_open_debugger() -> Option<KeyboardInput> {
    key_input!(Quote)
}
