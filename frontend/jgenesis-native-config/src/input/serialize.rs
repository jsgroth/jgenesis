use crate::input::{GamepadAction, GenericInput, KeyboardInput};
use sdl3::mouse::MouseButton;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Formatter;
use std::str::FromStr;

impl Serialize for KeyboardInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.serialize_to_str())
    }
}

impl<'de> Deserialize<'de> for KeyboardInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyboardInputVisitor;

        impl Visitor<'_> for KeyboardInputVisitor {
            type Value = KeyboardInput;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "KeyboardInput")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let key = KeyboardInput::deserialize_from_str(v).ok_or_else(|| {
                    Error::custom(format!("Invalid keyboard input string: '{v}'"))
                })?;
                Ok(key)
            }
        }

        deserializer.deserialize_str(KeyboardInputVisitor)
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
    Keyboard { key: KeyboardInput },
    Gamepad { gamepad_idx: u32, action: SerializableGamepadAction },
    Mouse { button: SerializableMouseButton },
}

impl Serialize for GenericInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serializable = match *self {
            Self::Keyboard(key) => SerializableGenericInput::Keyboard { key },
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
            SerializableGenericInput::Keyboard { key } => Self::Keyboard(key),
            SerializableGenericInput::Gamepad { gamepad_idx, action } => {
                Self::Gamepad { gamepad_idx, action: action.0 }
            }
            SerializableGenericInput::Mouse { button } => Self::Mouse(button.into()),
        })
    }
}
