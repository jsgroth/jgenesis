use crate::app::HelpText;

pub const TIMING_MODE: HelpText = HelpText {
    heading: "Timing Mode",
    text: &[
        "Set timing mode to NTSC (60Hz) or PAL (50Hz).",
        "The Auto setting will choose automatically based on the region in the cartridge header, preferring NTSC if both NTSC and PAL are supported.",
    ],
};

pub const REGION: HelpText = HelpText {
    heading: "Region",
    text: &[
        "Configure the hardware region that the emulator will report to games.",
        "The Auto setting will report the same region as the cartridge header. If multiple regions are supported, the preference order is US first then JP then EU.",
    ],
};

pub const SCD_BIOS_PATH: HelpText = HelpText {
    heading: "Sega CD BIOS Path",
    text: &["Path to a Sega CD BIOS ROM. This is required for Sega CD emulation."],
};

pub const SCD_RAM_CARTRIDGE: HelpText = HelpText {
    heading: "Sega CD RAM Cartridge",
    text: &[
        "Configure whether the Sega CD's 128KB RAM cartridge is emulated.",
        "If disabled, games can only save to the console's 8KB of builtin backup RAM.",
    ],
};

pub const SCD_CDROM_IN_RAM: HelpText = HelpText {
    heading: "Load CD-ROM Images into RAM",
    text: &[
        "If enabled, load CD-ROM images fully into host RAM when starting a game.",
        "This increases RAM usage but removes the need for the emulator to read from disk during emulation.",
    ],
};

pub const M68K_CLOCK_DIVIDER: HelpText = HelpText {
    heading: "Genesis 68000 Clock Divider",
    text: &[
        "Optionally overclock the main Genesis CPU by reducing the master clock divider. Overclocking can reduce or eliminate slowdown, but it can also cause major glitches in some games. Use with caution.",
        "Note that a clock divider lower than 3 or 4 will significantly increase the emulator's CPU usage.",
        "This setting also affects the main Genesis CPU speed in Sega CD and 32X mode.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Configure aspect ratio.",
        "NTSC - 8:7 pixel aspect ratio in H256px mode, 32:35 pixel aspect ratio in H320px mode",
        "PAL - 11:8 pixel aspect ratio in H256px mode, 11:10 pixel aspect ratio in H320px mode",
    ],
};

pub const DOUBLE_SCREEN_INTERLACED_ASPECT: HelpText = HelpText {
    heading: "Double-Screen Aspect Adjustment",
    text: &[
        "If enabled, automatically adjust the pixel aspect ratio appropriately if a game enables double-screen interlaced mode.",
        "If disabled, the emulator will keep the same pixel aspect ratio, resulting in the screen size doubling vertically.",
    ],
};

pub const REMOVE_SPRITE_LIMITS: HelpText = HelpText {
    heading: "Remove Sprite Limits",
    text: &[
        "If enabled, ignore the hardware's sprite-per-scanline and sprite-pixel-per-scanline limits when rendering.",
        "This typically reduces sprite flickering, but it may cause visual glitches in games that use the limits to intentionally hide sprites.",
    ],
};

pub const NON_LINEAR_COLOR_DAC: HelpText = HelpText {
    heading: "Non-Linear Color DAC",
    text: &[
        "If enabled, attempt to emulate the VDP's non-linear color DAC rather than treating VDP color values as raw sRGB.",
        "In practice, this pushes most colors slightly towards gray, darkening brighter colors and brightening darker colors.",
    ],
};

pub const RENDER_BORDERS: HelpText = HelpText {
    heading: "Render Border",
    text: &[
        "If enabled, render the border area instead of cropping it.",
        "The border area normally only contains the backdrop color, but some demos abuse hardware quirks to render graphics in the borders, namely Overdrive 2.",
    ],
};

pub const ENABLED_LAYERS: HelpText = HelpText {
    heading: "Enabled Layers",
    text: &[
        "Control which layers are rendered.",
        "Disabling the backdrop causes the VDP to render black instead of the backdrop color.",
    ],
};

pub const S32X_VIDEO_OUT: HelpText = HelpText {
    heading: "32X Video Output",
    text: &[
        "Configure 32X video frame composition, optionally displaying only the Genesis VDP output or only the 32X VDP output.",
    ],
};

pub const QUANTIZE_YM2612_OUTPUT: HelpText = HelpText {
    heading: "Quantize YM2612 Output",
    text: &[
        "If enabled, quantize YM2612 FM channel output from 14 bits to 9 bits by truncating the lowest bits.",
        "This makes audio somewhat less dynamic, but enabling this is more accurate, and some game audio is designed around quantization.",
    ],
};

pub const YM2612_LADDER_EFFECT: HelpText = HelpText {
    heading: "YM2612 Ladder Effect",
    text: &[
        "If enabled, emulate YM2612 DAC crossover distortion, commonly known as the ladder effect.",
        "This effectively amplifies low-volume audio waves and has little effect on high-volume waves. Some games have audio designed around this effect.",
    ],
};

pub const LOW_PASS_FILTER: HelpText = HelpText {
    heading: "Low-Pass Filter",
    text: &[
        "Configure which low-pass filter to use on audio output.",
        "Some Genesis hardware models had low-pass filters with low cutoff frequencies, which makes the audio sound softer and somewhat muffled. Some game audio is designed around a lower cutoff frequency.",
    ],
};

pub const SOUND_SOURCES: HelpText = HelpText {
    heading: "Sound Sources",
    text: &["Enable or disable specific sound sources in final audio mixing."],
};
