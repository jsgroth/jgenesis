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
    heading: "Sega CD BIOS Paths",
    text: &[
        "Path to a Sega CD BIOS ROM for each region. This is required for Sega CD emulation.",
        "Can optionally use the US BIOS for all regions rather than using a different BIOS per region.",
    ],
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

pub const SCD_SUB_CPU_DIVIDER: HelpText = HelpText {
    heading: "Sega CD 68000 Clock Divider",
    text: &[
        "Optionally overclock the Sega CD sub CPU by reducing the master clock divider.",
        "Overclocking can cause major glitches. Some games may additionally require overclocking the main CPU when the sub CPU is overclocked.",
        "Note that a clock divider lower than 3 may significantly increase the emulator's CPU usage.",
    ],
};

pub const SH2_CLOCK_MULTIPLIER: HelpText = HelpText {
    heading: "32X SH-2 Clock Multiplier",
    text: &[
        "Optionally overclock the 32X SH-2s by increasing their master clock multiplier.",
        "This may reduce slowdown in some games, but it may also cause major glitches.",
        "Overclocking the SH-2s is extremely CPU-intensive and may cause the emulator to not run at full speed.",
    ],
};

pub const SCD_DRIVE_SPEED: HelpText = HelpText {
    heading: "Sega CD Disc Drive Speed",
    text: &[
        "Optionally increase the speed of the Sega CD's CD-ROM drive when reading data tracks. This may shorten loading times in some games.",
        "Warning: Increasing the drive speed is VERY likely to cause major glitches, particularly in games that play FMVs or animated cutscenes. Overclocking the sub CPU may improve compatibility with some games.",
    ],
};

pub const ASPECT_RATIO: HelpText = HelpText {
    heading: "Aspect Ratio",
    text: &[
        "Configure aspect ratio.",
        "NTSC - 8:7 pixel aspect ratio in H256px mode, 32:35 pixel aspect ratio in H320px mode",
        "PAL - 11:8 pixel aspect ratio in H256px mode, 11:10 pixel aspect ratio in H320px mode",
        "The Auto option will automatically select NTSC or PAL based on the timing/display mode.",
    ],
};

pub const FORCE_SQUARE_PIXELS_H40: HelpText = HelpText {
    heading: "Force Square Pixels in H320px",
    text: &[
        "If enabled, ignore the configured aspect ratio and always display square pixels when a game enables H320px mode.",
    ],
};

pub const DEINTERLACING: HelpText = HelpText {
    heading: "Deinterlacing",
    text: &[
        "If enabled and a game sets the VDP to an interlaced screen mode, render in progressive mode instead of interlaced.",
        "In double-screen interlaced mode, this causes the VDP to render all 448 lines every frame (or 480 in V30 mode).",
    ],
};

pub const NON_LINEAR_COLOR_SCALE: HelpText = HelpText {
    heading: "Non-Linear Color Scale",
    text: &[
        "Emulate the VDP's non-linear color scale when converting from Genesis RGB333 colors to displayed colors. Enabling this is more accurate to actual hardware.",
        "In practice, compared to a linear color scale, this slightly brightens darker colors and slightly darkens brighter colors.",
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

pub const S32X_DARKEN_GEN_COLORS: HelpText = HelpText {
    heading: "Darken Genesis Colors",
    text: &[
        "On actual hardware, the brightest 32X colors are slightly brighter than the brightest Genesis colors.",
        "This setting simulates that behavior by slightly darkening Genesis colors relative to 32X colors.",
    ],
};

pub const S32X_COLOR_TINT: HelpText = HelpText {
    heading: "32X Color Tint",
    text: &[
        "Most 32X consoles have been observed to have a slight yellow or purple tint in their video output, with yellow seemingly being more common.",
        "Games were probably not designed around this, but a slight color tint is more accurate to actual hardware.",
    ],
};

pub const S32X_VIDEO_OUT: HelpText = HelpText {
    heading: "32X Video Output",
    text: &[
        "Configure 32X video frame composition, optionally displaying only the Genesis VDP output or only the 32X VDP output.",
    ],
};

pub const S32X_PRIORITY_MASKING: HelpText = HelpText {
    heading: "32X Priority Masking",
    text: &[
        "Optionally replace all 32X pixels of a given priority with a fixed color.",
        "The fixed color can come from 32X palette RAM or it can be set to a specific RGBA5551 color value.",
    ],
};

pub const QUANTIZE_YM2612_OUTPUT: HelpText = HelpText {
    heading: "Quantize YM2612 Output",
    text: &[
        "If enabled, quantize YM2612 FM channel output from 14 bits to 9 bits by truncating the lowest bits. This makes audio output somewhat noisier.",
        "Enabling this is more accurate to actual hardware. Some games have audio designed around quantization.",
    ],
};

pub const YM2612_LADDER_EFFECT: HelpText = HelpText {
    heading: "YM2612 Ladder Effect",
    text: &[
        "If enabled, emulate YM2612 DAC crossover distortion, commonly known as the ladder effect.",
        "This effectively amplifies low-volume audio waves and has little effect on high-volume waves. Some games have audio designed around this effect.",
        "This distortion is not present on later consoles that have a YM3438.",
    ],
};

pub const OPN2_BUSY_BEHAVIOR: HelpText = HelpText {
    heading: "OPN2 Busy Flag Behavior",
    text: &[
        "Control which FM chip busy flag behavior to emulate.",
        "This has no effect on most games, but a few games have audio issues with one behavior or the other, such as Earthworm Jim (issues with YM2612 behavior) and Hellfire (issues with YM3438 behavior).",
    ],
};

pub const GENESIS_LOW_PASS: HelpText = HelpText {
    heading: "Genesis Low-Pass Filter",
    text: &[
        "If enabled, apply a first-order low-pass filter to Genesis audio output, both YM2612 and PSG.",
        "Low-pass filtering makes the audio sound softer and somewhat muffled. Some game audio is designed around this effect.",
    ],
};

pub const YM2612_2ND_LOW_PASS: HelpText = HelpText {
    heading: "YM2612 2nd-Order Low-Pass Filter",
    text: &[
        "If enabled, apply a second-order low-pass filter only to YM2612 audio output, applied before the first-order Genesis low-pass filter.",
        "This should be similar to the audio circuitry found in Model 2 consoles.",
    ],
};

pub const PCM_LOW_PASS: HelpText = HelpText {
    heading: "Sega CD PCM Low-Pass Filter",
    text: &["If enabled, apply a second-order low-pass filter to PCM chip audio output."],
};

pub const SCD_GEN_LOW_PASS: HelpText = HelpText {
    heading: "Apply Genesis LPF to Sega CD",
    text: &[
        "Choose whether to apply the Genesis low-pass filter to Sega CD audio output. This can be configured independently for the PCM chip and CD-DA.",
        "In actual hardware, Sega CD audio output may or may not pass through the Genesis low-pass filter depending on where the Sega CD audio output is connected.",
    ],
};

pub const S32X_GEN_LOW_PASS: HelpText = HelpText {
    heading: "Apply Genesis LPF to 32X",
    text: &[
        "Choose whether to apply the Genesis low-pass filter to 32X PWM audio output.",
        "Enabling this is more accurate to actual hardware but can make PWM audio sound somewhat muffled.",
    ],
};

pub const SCD_PCM_INTERPOLATION: HelpText = HelpText {
    heading: "Sega CD PCM interpolation",
    text: &[
        "Choose the method used to interpolate when a PCM sound chip channel is partway between samples.",
        "Not interpolating is more accurate to hardware but tends to cause significant audio aliasing in PCM chip audio output.",
        "In terms of quality, generally 6-point cubic is best and linear is worst, but higher-quality interpolation may sound more muffled at low sample rates.",
    ],
};

pub const ENABLED_YM2612_CHANNELS: HelpText = HelpText {
    heading: "Enabled YM2612 Channels",
    text: &["Enable or disable individual YM2612 audio channels."],
};

pub const SOUND_SOURCES: HelpText = HelpText {
    heading: "Sound Sources",
    text: &["Enable or disable specific sound sources in final audio mixing."],
};

pub const VOLUME_ADJUSTMENTS: HelpText = HelpText {
    heading: "Volume Adjustments",
    text: &[
        "Adjust the volume of individual sound sources.",
        "Values can be positive or negative. Positive values increase volume and negative values decrease volume.",
    ],
};
