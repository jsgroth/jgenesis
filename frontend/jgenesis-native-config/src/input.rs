pub mod mappings;
mod serialize;

use crate::input::mappings::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, NesInputConfig, SmsGgInputConfig,
    SnesInputConfig,
};
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    Positive,
    Negative,
}

impl AxisDirection {
    #[inline]
    #[must_use]
    pub fn from_value(value: i16) -> Self {
        if value >= 0 { Self::Positive } else { Self::Negative }
    }

    #[inline]
    #[must_use]
    pub fn inverse(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}

impl Display for AxisDirection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Positive => write!(f, "+"),
            Self::Negative => write!(f, "-"),
        }
    }
}

impl FromStr for AxisDirection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "+" => Ok(Self::Positive),
            "-" => Ok(Self::Negative),
            _ => Err(format!("Invalid AxisDirection string: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumDisplay, EnumFromStr, EnumAll)]
pub enum HatDirection {
    Up,
    Left,
    Right,
    Down,
}

impl HatDirection {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadAction {
    Button(u8),
    Axis(u8, AxisDirection),
    Hat(u8, HatDirection),
}

impl Display for GamepadAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button(idx) => write!(f, "Button {idx}"),
            Self::Axis(idx, direction) => write!(f, "Axis {idx} {direction}"),
            Self::Hat(idx, direction) => write!(f, "Hat {idx} {direction}"),
        }
    }
}

impl FromStr for GamepadAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_fn = || format!("Invalid gamepad action string: {s}");

        let mut split = s.split_ascii_whitespace();
        let Some(input_type) = split.next() else {
            return Err(err_fn());
        };

        let Some(idx) = split.next().and_then(|idx| idx.parse().ok()) else {
            return Err(err_fn());
        };

        match input_type {
            "Button" | "button" => Ok(Self::Button(idx)),
            "Axis" | "axis" => {
                let Some(direction) = split.next().and_then(|direction| direction.parse().ok())
                else {
                    return Err(err_fn());
                };

                Ok(Self::Axis(idx, direction))
            }
            "Hat" | "hat" => {
                let Some(direction) = split.next().and_then(|direction| direction.parse().ok())
                else {
                    return Err(err_fn());
                };

                Ok(Self::Hat(idx, direction))
            }
            _ => Err(err_fn()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericInput {
    Keyboard(Keycode),
    Gamepad { gamepad_idx: u32, action: GamepadAction },
    Mouse(MouseButton),
}

impl Display for GenericInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            &Self::Keyboard(keycode) => write!(f, "Key: {}", keycode_to_str(keycode)),
            Self::Gamepad { gamepad_idx, action } => write!(f, "Gamepad {gamepad_idx}: {action}"),
            Self::Mouse(mouse_button) => write!(f, "Mouse: {mouse_button:?}"),
        }
    }
}

fn keycode_to_str(keycode: Keycode) -> Cow<'static, str> {
    match keycode {
        Keycode::LShift | Keycode::RShift => "Shift".into(),
        Keycode::LCtrl | Keycode::RCtrl => "Ctrl".into(),
        Keycode::LAlt | Keycode::RAlt => "Alt".into(),
        _ => keycode.name().into(),
    }
}

fn keycode_from_str(s: &str) -> Option<Keycode> {
    match s {
        "Shift" => Some(Keycode::LShift),
        "Ctrl" => Some(Keycode::LCtrl),
        "Alt" => Some(Keycode::LAlt),
        _ => {
            if s == Keycode::RShift.name().as_str() {
                Some(Keycode::LShift)
            } else if s == Keycode::RCtrl.name().as_str() {
                Some(Keycode::LCtrl)
            } else if s == Keycode::RAlt.name().as_str() {
                Some(Keycode::LAlt)
            } else {
                Keycode::from_name(s)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumDisplay, EnumAll)]
pub enum Hotkey {
    Exit,
    ToggleFullscreen,
    SoftReset,
    HardReset,
    PowerOff,
    Pause,
    StepFrame,
    FastForward,
    Rewind,
    ToggleOverclocking,
    OpenDebugger,
    SaveState,
    LoadState,
    NextSaveStateSlot,
    PrevSaveStateSlot,
    SaveStateSlot0,
    LoadStateSlot0,
    SaveStateSlot1,
    LoadStateSlot1,
    SaveStateSlot2,
    LoadStateSlot2,
    SaveStateSlot3,
    LoadStateSlot3,
    SaveStateSlot4,
    LoadStateSlot4,
    SaveStateSlot5,
    LoadStateSlot5,
    SaveStateSlot6,
    LoadStateSlot6,
    SaveStateSlot7,
    LoadStateSlot7,
    SaveStateSlot8,
    LoadStateSlot8,
    SaveStateSlot9,
    LoadStateSlot9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactHotkey {
    PowerOff,
    Exit,
    ToggleFullscreen,
    SaveState,
    LoadState,
    SaveStateSlot(usize),
    LoadStateSlot(usize),
    NextSaveStateSlot,
    PrevSaveStateSlot,
    SoftReset,
    HardReset,
    Pause,
    StepFrame,
    FastForward,
    Rewind,
    ToggleOverclocking,
    OpenDebugger,
}

impl Hotkey {
    #[inline]
    #[must_use]
    pub fn to_compact(self) -> CompactHotkey {
        match self {
            Self::PowerOff => CompactHotkey::PowerOff,
            Self::Exit => CompactHotkey::Exit,
            Self::ToggleFullscreen => CompactHotkey::ToggleFullscreen,
            Self::SaveState => CompactHotkey::SaveState,
            Self::LoadState => CompactHotkey::LoadState,
            Self::NextSaveStateSlot => CompactHotkey::NextSaveStateSlot,
            Self::PrevSaveStateSlot => CompactHotkey::PrevSaveStateSlot,
            Self::SoftReset => CompactHotkey::SoftReset,
            Self::HardReset => CompactHotkey::HardReset,
            Self::Pause => CompactHotkey::Pause,
            Self::StepFrame => CompactHotkey::StepFrame,
            Self::FastForward => CompactHotkey::FastForward,
            Self::Rewind => CompactHotkey::Rewind,
            Self::ToggleOverclocking => CompactHotkey::ToggleOverclocking,
            Self::OpenDebugger => CompactHotkey::OpenDebugger,
            Self::SaveStateSlot0 => CompactHotkey::SaveStateSlot(0),
            Self::SaveStateSlot1 => CompactHotkey::SaveStateSlot(1),
            Self::SaveStateSlot2 => CompactHotkey::SaveStateSlot(2),
            Self::SaveStateSlot3 => CompactHotkey::SaveStateSlot(3),
            Self::SaveStateSlot4 => CompactHotkey::SaveStateSlot(4),
            Self::SaveStateSlot5 => CompactHotkey::SaveStateSlot(5),
            Self::SaveStateSlot6 => CompactHotkey::SaveStateSlot(6),
            Self::SaveStateSlot7 => CompactHotkey::SaveStateSlot(7),
            Self::SaveStateSlot8 => CompactHotkey::SaveStateSlot(8),
            Self::SaveStateSlot9 => CompactHotkey::SaveStateSlot(9),
            Self::LoadStateSlot0 => CompactHotkey::LoadStateSlot(0),
            Self::LoadStateSlot1 => CompactHotkey::LoadStateSlot(1),
            Self::LoadStateSlot2 => CompactHotkey::LoadStateSlot(2),
            Self::LoadStateSlot3 => CompactHotkey::LoadStateSlot(3),
            Self::LoadStateSlot4 => CompactHotkey::LoadStateSlot(4),
            Self::LoadStateSlot5 => CompactHotkey::LoadStateSlot(5),
            Self::LoadStateSlot6 => CompactHotkey::LoadStateSlot(6),
            Self::LoadStateSlot7 => CompactHotkey::LoadStateSlot(7),
            Self::LoadStateSlot8 => CompactHotkey::LoadStateSlot(8),
            Self::LoadStateSlot9 => CompactHotkey::LoadStateSlot(9),
        }
    }
}

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
