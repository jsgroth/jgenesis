use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::{FiniteF64, FrameSize};
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum SnesAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl SnesAspectRatio {
    #[inline]
    #[must_use]
    pub fn to_pixel_aspect_ratio_f64(self) -> Option<f64> {
        match self {
            Self::Ntsc => Some(8.0 / 7.0),
            Self::Pal => Some(11.0 / 8.0),
            Self::SquarePixels => Some(1.0),
            Self::Stretched => None,
        }
    }

    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn to_pixel_aspect_ratio(self, frame_size: FrameSize) -> Option<FiniteF64> {
        let mut pixel_aspect_ratio = self.to_pixel_aspect_ratio_f64()?;

        if frame_size.width == 512 && frame_size.height < 240 {
            // Cut pixel aspect ratio in half to account for the screen being squished horizontally
            pixel_aspect_ratio *= 0.5;
        }

        if frame_size.width == 256 && frame_size.height >= 240 {
            // Double pixel aspect ratio to account for the screen being stretched horizontally
            pixel_aspect_ratio *= 2.0;
        }

        Some(FiniteF64::try_from(pixel_aspect_ratio).unwrap())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum AudioInterpolationMode {
    #[default]
    Gaussian,
    Hermite,
}

define_controller_inputs! {
    buttons: SnesButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        A -> a,
        B -> b,
        X -> x,
        Y -> y,
        L -> l,
        R -> r,
        Start -> start,
        Select -> select,
    },
    non_gamepad_buttons: [SuperScopeFire, SuperScopeCursor, SuperScopePause, SuperScopeTurboToggle],
    joypad: SnesJoypadState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperScopeButton {
    Fire,
    Cursor,
    Pause,
    TurboToggle,
}

impl SnesButton {
    #[inline]
    #[must_use]
    pub fn to_super_scope(self) -> Option<SuperScopeButton> {
        match self {
            Self::SuperScopeFire => Some(SuperScopeButton::Fire),
            Self::SuperScopeCursor => Some(SuperScopeButton::Cursor),
            Self::SuperScopePause => Some(SuperScopeButton::Pause),
            Self::SuperScopeTurboToggle => Some(SuperScopeButton::TurboToggle),
            _ => None,
        }
    }
}

impl SuperScopeButton {
    #[inline]
    #[must_use]
    pub fn to_snes_button(self) -> SnesButton {
        match self {
            Self::Fire => SnesButton::SuperScopeFire,
            Self::Cursor => SnesButton::SuperScopeCursor,
            Self::Pause => SnesButton::SuperScopePause,
            Self::TurboToggle => SnesButton::SuperScopeTurboToggle,
        }
    }
}
