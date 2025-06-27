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
        "Much moreso than on other consoles, some NES games exhibit severe glitches if the game reads opposing directional inputs pressed simultaneously. Unchecking this option makes it impossible for the game to see that happen.",
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

pub const NTSC_V_OVERSCAN: HelpText = HelpText {
    heading: "Crop NTSC Vertical Overscan",
    text: &[
        "If enabled, crop NTSC frames from 256x240 to 256x224.",
        "The PPU always renders 240 lines even on NTSC consoles, but the top 8 lines and bottom 8 lines were not visible on most contemporary NTSC TVs, leaving 224 lines visible.",
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

pub const PALETTE: HelpText = HelpText {
    heading: "Palette",
    text: &[
        "Customize the display palette, either by loading from a file or using the builtin palette generator. Supports loading both 512-color and 64-color palette files, though 512-color is preferred.",
        "The displayed graphic shows 64 colors by default. The 512-color version shows the 64 colors for each of the 8 combinations of color emphasis bits.",
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

pub const AUDIO_RESAMPLING: HelpText = HelpText {
    heading: "Audio Resampling Algorithm",
    text: &[
        "Choose the algorithm used to resample from the NES native sample rate to the output sample rate.",
        "Windowed sinc interpolation is higher quality and sharper, but it can be much more performance-intensive.",
    ],
};
