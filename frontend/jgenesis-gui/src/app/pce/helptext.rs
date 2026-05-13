use crate::app::HelpText;

pub const REGION: HelpText = HelpText {
    heading: "Console Region",
    text: &[
        "Configure the region that the emulated hardware reports to games.",
        "TurboGrafx-16 / US is generally recommended because most US games will not run on a PC Engine due to region locking code, but JP games will run on a TG16.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Configure aspect ratio.",
        "NTSC is an 8:7 pixel aspect ratio in H256px mode, and a 6:7 pixel aspect ratio in H341px mode.",
    ],
};

pub const PALETTE: HelpText = HelpText {
    heading: "Palette",
    text: &[
        "Choose the palette used to map the PC Engine's GRB333 colors to RGB888 colors for emulator display.",
        "The PCE composite palette (by Kitrinx) is more accurate to actual hardware's colors over composite video output.",
    ],
};

pub const CROP_OVERSCAN: HelpText = HelpText {
    heading: "Crop Overscan",
    text: &[
        "If enabled, crop parts of the frame that were likely not visible on most contemporary TVs.",
    ],
};

pub const REMOVE_SPRITE_LIMITS: HelpText = HelpText {
    heading: "Remove Sprite Limits",
    text: &[
        "Optionally disable the hardware's 16-sprite-per-scanline limit along with time-based sprite limits.",
        "This typically reduces sprite flickering, but may cause visual glitches in games that use the limits to intentionally hide sprites.",
    ],
};
