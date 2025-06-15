use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{MappableInputs, PixelAspectRatio};
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbAspectRatio {
    #[default]
    SquarePixels,
    Stretched,
}

impl GbAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbPalette {
    BlackAndWhite,
    #[default]
    GreenTint,
    LimeGreen,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbcColorCorrection {
    None,
    #[default]
    GbcLcd,
    GbaLcd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbAudioResampler {
    LowPassNearestNeighbor,
    #[default]
    WindowedSinc,
}

define_controller_inputs! {
    buttons: GameBoyButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        Start -> start,
        Select -> select,
    },
    joypad: GameBoyInputs,
}

impl MappableInputs<GameBoyButton> for GameBoyInputs {
    #[inline]
    fn set_field(&mut self, button: GameBoyButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
    }
}
