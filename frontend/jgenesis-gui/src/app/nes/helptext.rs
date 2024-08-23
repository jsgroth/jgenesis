use crate::app::HelpText;

pub const TIMING_MODE: HelpText = HelpText {
    heading: "Timing Mode",
    text: &[
        "Set timing mode to NTSC (60Hz) or PAL (50Hz).",
        "The Auto setting will choose automatically based on the region in the iNES header.",
    ],
};

pub const OPPOSING_DIRECTIONAL_INPUTS: HelpText = HelpText {
    heading: "Opposing Directional Inputs",
    text: &[
        "Whether to allow simultaneous opposing directional inputs (left+right, up+down).",
        "Much moreso than on other consoles, some NES games exhibit severe glitches if the game reads opposing directional inputs pressed simultaneously. This setting makes it impossible for the game to see that happen.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Configure aspect ratio.",
        "NTSC - 8:7 pixel aspect ratio",
        "PAL - 11:8 pixel aspect ratio",
    ],
};

pub const REMOVE_SPRITE_LIMIT: HelpText = HelpText {
    heading: "Remove Sprite Limit",
    text: &[
        "If enabled, ignore the hardware's 8 sprite-per-scanline limit.",
        "This typically eliminates sprite flickering, but it can cause visual glitches in games that use the limit to intentionally hide sprites.",
    ],
};

pub const PAL_BLACK_BORDER: HelpText = HelpText {
    heading: "Emulate PAL Border",
    text: &[
        "Emulate PAL border cropping. This crops the topmost line of pixels, the leftmost 2 columns of pixels, and the rightmost 2 columns of pixels.",
    ],
};

pub const OVERSCAN: HelpText = HelpText {
    heading: "Overscan",
    text: &[
        "Optionally crop pixels from each individual edge of the frame.",
        "Some NES games have noticeable visual glitches close to the borders due to video hardware limitations. This setting makes it possible to hide those glitches by cropping graphics close to the borders.",
    ],
};

pub const ULTRASONIC_TRIANGLE: HelpText = HelpText {
    heading: "Silence Ultrasonic Triangle Output",
    text: &[
        "If enabled and a game sets the triangle channel's period to an absurdly low value (0 or 1), mute it instead of oscillating it at an ultrasonic frequency.",
        "This is less accurate but can reduce audio popping in some games.",
    ],
};

pub const AUDIO_TIMING_HACK: HelpText = HelpText {
    heading: "Audio Timing Hack",
    text: &[
        "If enabled, slightly adjust the emulator's audio timing so that audio sync will target exactly 60 fps (NTSC) / 50 fps (PAL) instead of the console's native framerate which is slightly higher.",
        "Native framerate is approximately 60.0988 fps for NTSC and 50.007 fps for PAL.",
    ],
};
