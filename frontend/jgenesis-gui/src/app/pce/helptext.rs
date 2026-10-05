use crate::app::HelpText;

pub const REGION: HelpText = HelpText {
    heading: "Console Region",
    text: &[
        "Configure the region that the emulated hardware reports to games.",
        "TurboGrafx-16 / US is generally recommended because most US games will not run on a PC Engine due to region locking code, but JP games will run on a TG16.",
    ],
};

pub const SYSTEM_CARD_MODEL: HelpText = HelpText {
    heading: "CD-ROM² System Card Model",
    text: &[
        "Configure the CD-ROM² System Card model to emulate when running disc-based games.",
        "The Arcade Card has the highest compatibility. Many games require at least a Super System Card.",
        "The Super System Card and Arcade Card require a Super-compatible BIOS version (v3.0).",
    ],
};

pub const CD_BIOS: HelpText = HelpText {
    heading: "CD-ROM² System Card",
    text: &[
        "Path to a PC Engine CD-ROM² or TurboGrafx CD System Card ROM. This is required for CD-ROM² emulation.",
    ],
};

pub const LOAD_DISC_INTO_RAM: HelpText = HelpText {
    heading: "Load CD-ROM Images into RAM",
    text: &[
        "If enabled, load CD-ROM images fully into host RAM when starting a disc-based game.",
        "This increases RAM usage but removes the need for the emulator to read from disk during emulation.",
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

pub const PSG_AUDIO_RESAMPLER: HelpText = HelpText {
    heading: "PSG Audio Resampling Algorithm",
    text: &[
        "Choose the algorithm used to resample PSG audio output to the emulator's output sample rate.",
        "Windowed sinc interpolation is much higher quality but is fairly CPU-intensive.",
    ],
};

pub const CPU_OVERCLOCK: HelpText = HelpText {
    heading: "CPU Overclocking",
    text: &[
        "Optionally overclock the CPU when it is running at high speed (normally ~7.16 MHz), which it almost always is.",
        "This can reduce slowdown in games but may cause major glitches.",
    ],
};
