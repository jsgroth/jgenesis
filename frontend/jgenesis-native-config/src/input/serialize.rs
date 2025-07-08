use crate::input::{GamepadAction, GenericInput};
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Formatter;
use std::str::FromStr;

// Keycode does not implement serde traits - serialize as the key name from Keycode::name()
struct SerializableKeycode(Keycode);

impl Serialize for SerializableKeycode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&super::keycode_to_str(self.0))
    }
}

impl<'de> Deserialize<'de> for SerializableKeycode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeycodeVisitor;

        impl Visitor<'_> for KeycodeVisitor {
            type Value = SerializableKeycode;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "SerializableKeycode")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let keycode = super::keycode_from_str(v)
                    .ok_or_else(|| Error::custom(format!("Invalid SDL3 keycode string: '{v}'")))?;
                Ok(SerializableKeycode(keycode))
            }
        }

        deserializer.deserialize_str(KeycodeVisitor)
    }
}

// Serialize GamepadAction as a single string to avoid making the TOML config extremely messy
struct SerializableGamepadAction(GamepadAction);

impl Serialize for SerializableGamepadAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for SerializableGamepadAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct GamepadActionVisitor;

        impl Visitor<'_> for GamepadActionVisitor {
            type Value = SerializableGamepadAction;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "SerializableGamepadAction")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(SerializableGamepadAction(
                    GamepadAction::from_str(v).map_err(|_| {
                        Error::custom(format!("Invalid gamepad action string: '{v}'"))
                    })?,
                ))
            }
        }

        deserializer.deserialize_str(GamepadActionVisitor)
    }
}

// Only exists because MouseButton does not implement serde traits
#[derive(Serialize, Deserialize)]
enum SerializableMouseButton {
    Unknown,
    Left,
    Right,
    Middle,
    X1,
    X2,
}

macro_rules! impl_from_mouse_button {
    ($a:ty, $b:ty) => {
        impl From<$a> for $b {
            fn from(value: $a) -> Self {
                match value {
                    <$a>::Unknown => Self::Unknown,
                    <$a>::Left => Self::Left,
                    <$a>::Right => Self::Right,
                    <$a>::Middle => Self::Middle,
                    <$a>::X1 => Self::X1,
                    <$a>::X2 => Self::X2,
                }
            }
        }
    };
}

impl_from_mouse_button!(MouseButton, SerializableMouseButton);
impl_from_mouse_button!(SerializableMouseButton, MouseButton);

// Alternate representation of GenericInput that serializes in a nicer format
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum SerializableGenericInput {
    Keyboard { key: SerializableKeycode },
    Gamepad { gamepad_idx: u32, action: SerializableGamepadAction },
    Mouse { button: SerializableMouseButton },
}

impl Serialize for GenericInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serializable = match *self {
            Self::Keyboard(keycode) => {
                SerializableGenericInput::Keyboard { key: SerializableKeycode(keycode) }
            }
            Self::Gamepad { gamepad_idx, action } => SerializableGenericInput::Gamepad {
                gamepad_idx,
                action: SerializableGamepadAction(action),
            },
            Self::Mouse(button) => SerializableGenericInput::Mouse { button: button.into() },
        };

        serializable.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GenericInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serializable = SerializableGenericInput::deserialize(deserializer)?;

        Ok(match serializable {
            SerializableGenericInput::Keyboard { key } => Self::Keyboard(key.0),
            SerializableGenericInput::Gamepad { gamepad_idx, action } => {
                Self::Gamepad { gamepad_idx, action: action.0 }
            }
            SerializableGenericInput::Mouse { button } => Self::Mouse(button.into()),
        })
    }
}
