use bincode::{Decode, Encode};
use jgenesis_common::define_controller_inputs;
use jgenesis_common::frontend::MappableInputs;
use jgenesis_common::input::Player;
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};

define_controller_inputs! {
    buttons: PceButton {
        Up -> up,
        Left -> left,
        Right -> right,
        Down -> down,
        Button1 -> button1,
        Button2 -> button2,
        Run -> run,
        Select -> select,
    },
    joypad: PceInputs,
}

impl MappableInputs<PceButton> for PceInputs {
    fn set_field(&mut self, button: PceButton, _player: Player, pressed: bool) {
        self.set_button(button, pressed);
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
