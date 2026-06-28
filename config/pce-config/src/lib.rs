use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

pub const TURBO_TAP_GAMEPADS: u8 = 5;

define_controller_inputs! {
    buttons: PceButton {
        Up -> up "Up",
        Left -> left "Left",
        Right -> right "Right",
        Down -> down "Down",
        Button1 -> button1 "Button I",
        Button2 -> button2 "Button II",
        Run -> run "Run",
        Select -> select "Select",
    },
    joypad: PceJoypadState impl with_allow_opposing_directions,
    inputs: PceInputs {
        players: {
            p1: Player::One,
            p2: Player::Two,
            p3: Player::Three,
            p4: Player::Four,
            p5: Player::Five,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PceRegion {
    #[default]
    TurboGrafx16,
    PcEngine,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PceAspectRatio {
    #[default]
    Ntsc,
    SquarePixels,
    Stretched,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PcePaletteType {
    #[default]
    PceComposite,
    Linear,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PceAudioResampler {
    #[default]
    WindowedSinc,
    LowPassNearestNeighbor,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PceInputDevice {
    #[default]
    TwoButtonGamepad,
    TurboTap,
}
