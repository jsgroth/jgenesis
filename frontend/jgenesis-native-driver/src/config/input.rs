use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay, EnumFromStr};
use sdl2::keyboard::Keycode;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

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

macro_rules! key_input {
    ($key:ident) => {
        Some(KeyboardInput { keycode: Keycode::$key.name() })
    };
}

macro_rules! define_input_config {
    (
        controller_cfg_name: $controller_cfg_name:ident,
        input_cfg_name: $input_cfg_name:ident,
        buttons: [$($button:ident: default $keycode:ident),* $(,)?] $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
        pub struct $controller_cfg_name<Input> {
            $(
                pub $button: Option<Input>,
            )*
        }

        impl<Input> Default for $controller_cfg_name<Input> {
            fn default() -> Self {
                Self {
                    $(
                        $button: None,
                    )*
                }
            }
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ConfigDisplay)]
        pub struct $input_cfg_name<Input> {
            #[indent_nested]
            pub p1: $controller_cfg_name<Input>,
            #[indent_nested]
            pub p2: $controller_cfg_name<Input>,
        }

        impl Default for $input_cfg_name<KeyboardInput> {
            fn default() -> Self {
                Self {
                    p1: $controller_cfg_name {
                        $(
                            $button: key_input!($keycode),
                        )*
                    },
                    p2: $controller_cfg_name::default(),
                }
            }
        }

        impl Default for $input_cfg_name<JoystickInput> {
            fn default() -> Self {
                Self {
                    p1: $controller_cfg_name::default(),
                    p2: $controller_cfg_name::default(),
                }
            }
        }
    }
}

define_input_config! {
    controller_cfg_name: SmsGgControllerConfig,
    input_cfg_name: SmsGgInputConfig,
    buttons: [
        up: default Up,
        left: default Left,
        right: default Right,
        down: default Down,
        button_1: default S,
        button_2: default A,
        pause: default Return,
    ],
}

define_input_config! {
    controller_cfg_name: GenesisControllerConfig,
    input_cfg_name: GenesisInputConfig,
    buttons: [
        up: default Up,
        left: default Left,
        right: default Right,
        down: default Down,
        a: default A,
        b: default S,
        c: default D,
        x: default Q,
        y: default W,
        z: default E,
        start: default Return,
        mode: default RShift,
    ],
}

define_input_config! {
    controller_cfg_name: NesControllerConfig,
    input_cfg_name: NesInputConfig,
    buttons: [
        up: default Up,
        left: default Left,
        right: default Right,
        down: default Down,
        a: default A,
        b: default S,
        start: default Return,
        select: default RShift,
    ],
}

define_input_config! {
    controller_cfg_name: SnesControllerConfig,
    input_cfg_name: SnesInputConfig,
    buttons: [
        up: default Up,
        left: default Left,
        right: default Right,
        down: default Down,
        a: default S,
        b: default X,
        x: default A,
        y: default Z,
        l: default D,
        r: default C,
        start: default Return,
        select: default RShift,
    ],
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
