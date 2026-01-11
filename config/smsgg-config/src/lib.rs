use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::FiniteF64;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

pub const NATIVE_Z80_DIVIDER: u32 = 15;

// 8:7
pub const SMS_NTSC_ASPECT_RATIO: f64 = 1.1428571428571428;

// 11:8
pub const SMS_PAL_ASPECT_RATIO: f64 = 1.375;

// 6:5
pub const GAME_GEAR_LCD_ASPECT_RATIO: f64 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum Sn76489Version {
    #[default]
    MasterSystem2,
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum SmsModel {
    #[default]
    Sms1,
    Sms2,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum SmsGgRegion {
    #[default]
    International,
    Domestic,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum SmsAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl SmsAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio_f64(self) -> Option<f64> {
        match self {
            Self::Ntsc => Some(SMS_NTSC_ASPECT_RATIO),
            Self::Pal => Some(SMS_PAL_ASPECT_RATIO),
            Self::SquarePixels => Some(1.0),
            Self::Stretched => None,
        }
    }

    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(self) -> Option<FiniteF64> {
        self.to_pixel_aspect_ratio_f64().map(|par| FiniteF64::try_from(par).unwrap())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GgAspectRatio {
    #[default]
    GgLcd,
    SquarePixels,
    Stretched,
}

impl GgAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio_f64(self) -> Option<f64> {
        match self {
            Self::GgLcd => Some(GAME_GEAR_LCD_ASPECT_RATIO),
            Self::SquarePixels => Some(1.0),
            Self::Stretched => None,
        }
    }

    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(self) -> Option<FiniteF64> {
        self.to_pixel_aspect_ratio_f64().map(|par| FiniteF64::try_from(par).unwrap())
    }
}

define_controller_inputs! {
    buttons: SmsGgButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        Button1 -> button1,
        Button2 -> button2,
    },
    non_gamepad_buttons: [Pause],
    joypad: SmsGgJoypadState,
    inputs: SmsGgInputs {
        players: {
            p1: Player::One,
            p2: Player::Two,
        },
        buttons: [Pause -> pause],
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sms_aspect_ratios_valid() {
        for par in SmsAspectRatio::ALL {
            let _ = par.to_pixel_aspect_ratio();
        }
    }

    #[test]
    fn all_gg_aspect_ratios_valid() {
        for par in GgAspectRatio::ALL {
            let _ = par.to_pixel_aspect_ratio();
        }
    }
}
