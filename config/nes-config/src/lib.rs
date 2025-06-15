use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::PixelAspectRatio;
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum NesAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl NesAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio_f64(self) -> Option<f64> {
        match self {
            Self::Ntsc => Some(8.0 / 7.0),
            Self::Pal => Some(11.0 / 8.0),
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE.into()),
            Self::Stretched => None,
        }
    }

    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        self.to_pixel_aspect_ratio_f64().map(|par| PixelAspectRatio::try_from(par).unwrap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Overscan {
    pub top: u16,
    pub bottom: u16,
    pub left: u16,
    pub right: u16,
}

impl Overscan {
    pub const NONE: Self = Self { top: 0, bottom: 0, left: 0, right: 0 };
}

impl Display for Overscan {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Overscan {{ top={}, bottom={}, left={}, right={} }}",
            self.top, self.bottom, self.left, self.right
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum NesAudioResampler {
    LowPassNearestNeighbor,
    #[default]
    WindowedSinc,
}

define_controller_inputs! {
    buttons: NesButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        Start -> start,
        Select -> select,
    },
    non_gamepad_buttons: [ZapperFire, ZapperForceOffscreen],
    joypad: NesJoypadState,
}

impl NesButton {
    #[inline]
    #[must_use]
    pub fn is_zapper(self) -> bool {
        matches!(self, Self::ZapperFire | Self::ZapperForceOffscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_aspect_ratios_valid() {
        for par in NesAspectRatio::ALL {
            let _ = par.to_pixel_aspect_ratio();
        }
    }
}
