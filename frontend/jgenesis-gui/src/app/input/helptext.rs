#![cfg(feature = "unstable-cores")]

use crate::app::HelpText;

pub const PCE_SIMULTANEOUS_RUN_SELECT: HelpText = HelpText {
    heading: "Allow Simultaneous Run+Select",
    text: &[
        "Whether to allow games to see Run+Select pressed simultaneously.",
        "Many games perform a soft reset when Run+Select are both pressed, which can be easy to trigger accidentally.",
    ],
};
