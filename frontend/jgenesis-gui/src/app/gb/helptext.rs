use crate::app::HelpText;

pub const FORCE_DMG_MODE: HelpText = HelpText {
    heading: "Force DMG Mode",
    text: &[
        "Force the emulator to present as an original Game Boy even when loading Game Boy Color games.",
        "Some games support both GB and GBC, and some GBC games show unique lockout graphics when run on GB.",
    ],
};

pub const FORCE_CGB_MODE: HelpText = HelpText {
    heading: "Force CGB Mode",
    text: &[
        "Force the emulator to operate as a Game Boy Color even when loading games with no GBC functionality.",
        "Requires a CGB boot ROM in order to initialize the compatibility palettes.",
    ],
};

pub const PRETEND_GBA_MODE: HelpText = HelpText {
    heading: "Pretend GBA Mode",
    text: &[
        "Set initial register values such that GBC games will think they're running on a Game Boy Advance.",
        "No GB/GBC games use any GBA-specific functionality, but some games modify color palettes or unlock additional content if they detect that they're running on a GBA.",
    ],
};

pub const BOOT_ROM: HelpText = HelpText {
    heading: "Boot ROM",
    text: &[
        "Optionally boot from a boot ROM instead of booting directly into the game.",
        "Boot ROMs are configured separately for DMG (Game Boy) and CGB (Game Boy Color).",
    ],
};

pub const AUDIO_TIMING_HACK: HelpText = HelpText {
    heading: "Audio Timing Hack",
    text: &[
        "If enabled, adjust audio timing so that audio sync targets exactly 60 fps instead of the native refresh rate of approximately 59.73 fps.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Whether to render square pixels (as actual hardware does) or stretch to fit the viewport.",
    ],
};

pub const FRAME_BLENDING: HelpText = HelpText {
    heading: "Frame Blending",
    text: &[
        "If enabled, blend each frame with the previous frame to emulate the effect of LCD ghosting.",
        "Some games and demos depend on this for correct display due to rapidly alternating between two different images or layers that are intended to blend together.",
    ],
};

pub const GB_COLOR_PALETTE: HelpText = HelpText {
    heading: "GB Color Palette",
    text: &[
        "Configure how colors display in original Game Boy software.",
        "All Game Boy graphics are rendered using 4 different colors internally. These options present different ways of displaying those 4 colors.",
    ],
};

pub const GBC_COLOR_CORRECTION: HelpText = HelpText {
    heading: "GBC Color Correction",
    text: &[
        "Configure what color correction to apply to GBC rendering output, if any.",
        "GBC colors appear differently on its LCD screen compared to naively rendering the RGB values that games output. This option attempts to correct for that.",
        "There is also an option to attempt to emulate how the Game Boy Advance LCD displays colors.",
    ],
};

pub const AUDIO_RESAMPLING: HelpText = HelpText {
    heading: "Audio Resampling Algorithm",
    text: &[
        "Choose the algorithm used to resample from the Game Boy native sample rate to the output sample rate.",
        "Windowed sinc interpolation is higher quality and sharper, but it can be much more performance-intensive.",
    ],
};
