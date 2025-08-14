use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{MappableInputs, PixelAspectRatio};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

define_controller_inputs! {
    buttons: GbaButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        L -> l,
        R -> r,
        Start -> start,
        Select -> select,
    },
    joypad: GbaInputs,
}

impl MappableInputs<GbaButton> for GbaInputs {
    fn set_field(&mut self, button: GbaButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaAspectRatio {
    #[default]
    SquarePixels,
    Stretched,
}

impl GbaAspectRatio {
    #[must_use]
    pub fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaColorCorrection {
    None,
    #[default]
    GbaLcd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaSaveMemory {
    Sram,
    EepromUnknownSize,
    Eeprom512,
    Eeprom8K,
    FlashRom64K,
    FlashRom128K,
    None,
}

impl GbaSaveMemory {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sram => "SRAM",
            Self::EepromUnknownSize => "EEPROM (unspecified size)",
            Self::Eeprom512 => "EEPROM 512 bytes",
            Self::Eeprom8K => "EEPROM 8 KB",
            Self::FlashRom64K => "Flash ROM 64 KB",
            Self::FlashRom128K => "Flash ROM 128 KB",
            Self::None => "None",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumFromStr, EnumDisplay,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaAudioInterpolation {
    #[default]
    NearestNeighbor,
    WindowedSinc,
}
