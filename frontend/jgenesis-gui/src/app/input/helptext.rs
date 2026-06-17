use crate::app::HelpText;

pub const OPPOSING_JOYPAD_DIRECTIONS: HelpText = HelpText {
    heading: "Allow Opposing Gamepad Directions",
    text: &["Whether to allow games to see Left+Right or Up+Down inputs pressed simultaneously."],
};

pub const NES_OPPOSING_JOYPAD_DIRECTIONS: HelpText = HelpText {
    heading: "Opposing Directional Inputs",
    text: &[
        "Whether to allow simultaneous opposing directional inputs (left+right, up+down).",
        "Much moreso than on other consoles, some NES games exhibit severe glitches if the game reads opposing directional inputs pressed simultaneously. Unchecking this option makes it impossible for the game to see that happen.",
    ],
};

pub const PCE_SIMULTANEOUS_RUN_SELECT: HelpText = HelpText {
    heading: "Allow Simultaneous Run+Select",
    text: &[
        "Whether to allow games to see Run+Select pressed simultaneously.",
        "Many games perform a soft reset when Run+Select are both pressed, which can be easy to trigger accidentally.",
    ],
};
