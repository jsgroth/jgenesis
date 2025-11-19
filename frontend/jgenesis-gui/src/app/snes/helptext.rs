use crate::app::HelpText;

pub const TIMING_MODE: HelpText = HelpText {
    heading: "Timing Mode",
    text: &[
        "Set timing mode to NTSC (60Hz) or PAL (50Hz).",
        "The Auto setting will choose automatically based on the region in the cartridge header.",
    ],
};

pub const SUPER_FX_OVERCLOCK: HelpText = HelpText {
    heading: "Super FX Overclocking",
    text: &[
        "Optionally overclock the GSU coprocessor in Super FX cartridges.",
        "This typically increases framerate in Super FX games but also typically increases game speed, since most Super FX games tied game speed to framerate.",
    ],
};

pub const COPROCESSOR_ROM_PATHS: HelpText = HelpText {
    heading: "Coprocessor ROM Paths",
    text: &[
        "The DSP-n and ST01x coprocessors are all low-level emulated, which means that the emulator requires the corresponding coprocessor ROM image in order to run games that used that coprocessor.",
        "The emulator will display an error if it tries to load a game using one of these coprocessors and the coprocessor ROM is not configured.",
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

pub const DEINTERLACING: HelpText = HelpText {
    heading: "Deinterlacing",
    text: &[
        "If enabled and a game turns on interlaced display mode, render in progressive mode instead of interlaced.",
        "In high-res interlaced mode (512x448i), this causes the PPU to render all 448 lines every frame (or 478 in 239-line mode).",
    ],
};

pub const ADPCM_INTERPOLATION: HelpText = HelpText {
    heading: "ADPCM Sample Interpolation",
    text: &[
        "Configure the method used to interpolate between decoded ADPCM samples.",
        "Gaussian interpolation emulates how actual hardware interpolates between samples.",
        "Cubic Hermite interpolation uses a more advanced algorithm that usually creates a much sharper and less muffled sound, particularly in games with low sample rate audio.",
    ],
};

pub const AUDIO_TIMING_HACK: HelpText = HelpText {
    heading: "Audio Timing Hack",
    text: &[
        "If enabled, slightly adjust the emulator's audio timing so that audio sync will target exactly 60 fps (NTSC) / 50 fps (PAL) instead of the console's native framerate which is slightly higher.",
        "Native framerate is approximately 60.0988 fps for NTSC and 50.007 fps for PAL.",
    ],
};
