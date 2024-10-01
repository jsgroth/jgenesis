use crate::app::HelpText;

pub const TIMING_MODE: HelpText = HelpText {
    heading: "SMS Timing Mode",
    text: &[
        "Set Master System timing mode to NTSC (60Hz) or PAL (50Hz). This setting has no effect for Game Gear.",
        "NTSC vs. PAL cannot easily be auto-detected for Master System, so this needs to be explicitly configured. NA and JP releases generally prefer NTSC, and EU releases generally prefer PAL.",
    ],
};

pub const VDP_VERSION: HelpText = HelpText {
    heading: "SMS VDP Version",
    text: &[
        "Configure which VDP model to use for Master System emulation.",
        "The only difference is that the SMS1 option emulates a VDP hardware bug that is required for the Japanese version of Ys to render correctly.",
    ],
};

pub const REGION: HelpText = HelpText {
    heading: "Region",
    text: &[
        "Configure which hardware region the emulator reports to games.",
        "For international releases, this sometimes changes the title screen or in-game text.",
    ],
};

pub const Z80_OVERCLOCK: HelpText = HelpText {
    heading: "Overclock Z80 CPU",
    text: &[
        "If enabled, double the emulated Z80 CPU speed.",
        "This can reduce or eliminate slowdown in some games but can also cause major glitches. Use with caution.",
    ],
};

pub const SMS_ASPECT_RATIO: HelpText = HelpText {
    heading: "SMS Aspect Ratio",
    text: &[
        "Configure aspect ratio for Master System emulation.",
        "NTSC - 8:7 pixel aspect ratio",
        "PAL - 11:8 pixel aspect ratio",
    ],
};

pub const GG_ASPECT_RATIO: HelpText = HelpText {
    heading: "Game Gear Aspect Ratio",
    text: &[
        "Configure aspect ratio for Game Gear emulation.",
        "Game Gear LCD - 6:5 pixel aspect ratio",
    ],
};

pub const REMOVE_SPRITE_LIMIT: HelpText = HelpText {
    heading: "Remove Sprite Limit",
    text: &[
        "If enabled, ignore the hardware's 8 sprite-per-scanline limit when rendering.",
        "This typically eliminates sprite flickering.",
    ],
};

pub const SMS_CROP_VERTICAL_BORDER: HelpText = HelpText {
    heading: "SMS Crop Vertical Border",
    text: &[
        "If enabled, crop the top and bottom borders before display.",
        "The vertical borders only ever contain the backdrop color.",
    ],
};

pub const SMS_CROP_LEFT_BORDER: HelpText = HelpText {
    heading: "SMS Crop Left Border",
    text: &[
        "If enabled, crop the leftmost 8 pixels before display.",
        "Most games only display the backdrop color here, but some games do display actual game graphics in this area.",
    ],
};

pub const GG_USE_SMS_RESOLUTION: HelpText = HelpText {
    heading: "Game Gear Expanded Resolution",
    text: &[
        "If enabled, display the full 256x192 frame rendered by the VDP rather than only the center 160x144 pixels.",
        "Only the center pixels display on actual hardware, so the expanded parts of the frame may contain garbage.",
    ],
};

pub const PSG_VERSION: HelpText = HelpText {
    heading: "PSG Version",
    text: &[
        "Configure which PSG model to use.",
        "The SMS2 PSG has been observed to clip channels playing at the highest volumes, and some games have extremely loud sound effects if this is not emulated.",
        "The Auto setting uses the SMS2 option for Master System emulation and the SMS1 / Game Gear option for Game Gear emulation.",
    ],
};

pub const SMS_FM_UNIT: HelpText = HelpText {
    heading: "SMS FM Sound Unit",
    text: &[
        "Enable the Master System FM sound unit expansion, which contains a Yamaha YM2413 FM synthesis sound chip (aka OPLL).",
        "Not all games support the FM sound unit. Games that support it will usually use it automatically if they detect it.",
    ],
};
