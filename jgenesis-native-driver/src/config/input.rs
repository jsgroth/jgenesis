use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay};
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
pub struct SmsGgControllerConfig<Input> {
    pub up: Option<Input>,
    pub left: Option<Input>,
    pub right: Option<Input>,
    pub down: Option<Input>,
    pub button_1: Option<Input>,
    pub button_2: Option<Input>,
    // Pause is actually shared between the two controllers but it's simpler to map it this way
    pub pause: Option<Input>,
}

impl<Input> Default for SmsGgControllerConfig<Input> {
    fn default() -> Self {
        Self {
            up: None,
            left: None,
            right: None,
            down: None,
            button_1: None,
            button_2: None,
            pause: None,
        }
    }
}

impl<Input: Display> Display for SmsGgControllerConfig<Input> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ up: {}, left: {}, right: {}, down: {}, 1: {}, 2: {}, pause: {} }}",
            fmt_option(self.up.as_ref()),
            fmt_option(self.left.as_ref()),
            fmt_option(self.right.as_ref()),
            fmt_option(self.down.as_ref()),
            fmt_option(self.button_1.as_ref()),
            fmt_option(self.button_2.as_ref()),
            fmt_option(self.pause.as_ref())
        )
    }
}

fn fmt_option<T: Display>(option: Option<&T>) -> String {
    match option {
        Some(value) => format!("{value}"),
        None => "<None>".into(),
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgInputConfig<Input> {
    pub p1: SmsGgControllerConfig<Input>,
    pub p2: SmsGgControllerConfig<Input>,
}

macro_rules! key_input {
    ($key:ident) => {
        Some(KeyboardInput { keycode: Keycode::$key.name() })
    };
}

impl Default for SmsGgInputConfig<KeyboardInput> {
    fn default() -> Self {
        Self {
            p1: SmsGgControllerConfig {
                up: key_input!(Up),
                left: key_input!(Left),
                right: key_input!(Right),
                down: key_input!(Down),
                button_1: key_input!(S),
                button_2: key_input!(A),
                pause: key_input!(Return),
            },
            p2: SmsGgControllerConfig::default(),
        }
    }
}

impl Default for SmsGgInputConfig<JoystickInput> {
    fn default() -> Self {
        Self { p1: SmsGgControllerConfig::default(), p2: SmsGgControllerConfig::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisControllerConfig<Input> {
    pub up: Option<Input>,
    pub left: Option<Input>,
    pub right: Option<Input>,
    pub down: Option<Input>,
    pub a: Option<Input>,
    pub b: Option<Input>,
    pub c: Option<Input>,
    pub x: Option<Input>,
    pub y: Option<Input>,
    pub z: Option<Input>,
    pub start: Option<Input>,
    pub mode: Option<Input>,
}

impl<Input> Default for GenesisControllerConfig<Input> {
    fn default() -> Self {
        Self {
            up: None,
            left: None,
            right: None,
            down: None,
            a: None,
            b: None,
            c: None,
            x: None,
            y: None,
            z: None,
            start: None,
            mode: None,
        }
    }
}

impl<Input: Display> Display for GenesisControllerConfig<Input> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ up: {}, left: {}, right: {}, down: {}, a: {}, b: {}, c: {}, x: {}, y: {}, z: {}, start: {}, mode: {} }}",
            fmt_option(self.up.as_ref()),
            fmt_option(self.left.as_ref()),
            fmt_option(self.right.as_ref()),
            fmt_option(self.down.as_ref()),
            fmt_option(self.a.as_ref()),
            fmt_option(self.b.as_ref()),
            fmt_option(self.c.as_ref()),
            fmt_option(self.x.as_ref()),
            fmt_option(self.y.as_ref()),
            fmt_option(self.z.as_ref()),
            fmt_option(self.start.as_ref()),
            fmt_option(self.mode.as_ref())
        )
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisInputConfig<Input> {
    pub p1: GenesisControllerConfig<Input>,
    pub p2: GenesisControllerConfig<Input>,
}

impl Default for GenesisInputConfig<KeyboardInput> {
    fn default() -> Self {
        Self {
            p1: GenesisControllerConfig {
                up: key_input!(Up),
                left: key_input!(Left),
                right: key_input!(Right),
                down: key_input!(Down),
                a: key_input!(A),
                b: key_input!(S),
                c: key_input!(D),
                x: key_input!(Q),
                y: key_input!(W),
                z: key_input!(E),
                start: key_input!(Return),
                mode: key_input!(RShift),
            },
            p2: GenesisControllerConfig::default(),
        }
    }
}

impl Default for GenesisInputConfig<JoystickInput> {
    fn default() -> Self {
        Self { p1: GenesisControllerConfig::default(), p2: GenesisControllerConfig::default() }
    }
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
    #[serde(default = "default_cram_debug")]
    pub open_cram_debug: Option<KeyboardInput>,
    #[serde(default = "default_vram_debug")]
    pub open_vram_debug: Option<KeyboardInput>,
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
            open_cram_debug: default_cram_debug(),
            open_vram_debug: default_vram_debug(),
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

fn default_cram_debug() -> Option<KeyboardInput> {
    key_input!(Semicolon)
}

fn default_vram_debug() -> Option<KeyboardInput> {
    key_input!(Quote)
}
