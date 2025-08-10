use crate::app::HelpText;

pub const BIOS_PATH: HelpText = HelpText {
    heading: "BIOS Path",
    text: &["Path to a 16 KB Game Boy Advance BIOS ROM.", "This is required for GBA emulation."],
};

pub const SKIP_BIOS_ANIMATION: HelpText = HelpText {
    heading: "Skip Bios Intro Animation",
    text: &[
        "If enabled, skip the BIOS intro animation by starting execution at the cartridge entry point.",
    ],
};

pub const SAVE_MEMORY_TYPE: HelpText = HelpText {
    heading: "Save Memory Type",
    text: &[
        "Optionally force a specific cartridge save memory type.",
        "Auto-detection makes a best effort guess based on the contents of cartridge ROM and what memory accesses the game performs, but this may detect the wrong type in some cases.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Whether to render square pixels (as actual hardware does) or stretch to fit the viewport.",
    ],
};

pub const COLOR_CORRECTION: HelpText = HelpText {
    heading: "Color Correction",
    text: &[
        "If enabled, attempt to mimic how Game Boy Advance colors appear on the GBA's LCD screen.",
        "This usually makes video output darker and less saturated.",
    ],
};
