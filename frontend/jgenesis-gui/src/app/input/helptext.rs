use crate::app::HelpText;

pub const PCE_OPPOSING_JOYPAD_DIRECTIONS: HelpText = HelpText {
    heading: "Allow Opposing Gamepad Directions",
    text: &["Whether to allow games to see Left+Right or Up+Down inputs pressed simultaneously."],
};

pub const PCE_SIMULTANEOUS_RUN_SELECT: HelpText = HelpText {
    heading: "Allow Simultaneous Run+Select",
    text: &[
        "Whether to allow games to see Run+Select pressed simultaneously.",
        "Many games perform a soft reset when Run+Select are both pressed, which can be easy to trigger accidentally.",
    ],
};
