use crate::app::HelpText;

pub const FULLSCREEN: HelpText = HelpText {
    heading: "Fullscreen",
    text: &["If enabled, launch fullscreen instead of windowed."],
};

pub const WGPU_BACKEND: HelpText = HelpText {
    heading: "wgpu Backend",
    text: &[
        "Specify which graphics API wgpu will use.",
        "Auto is generally Vulkan if supported but may use DirectX 12 on Windows.",
    ],
};

pub const VSYNC_MODE: HelpText = HelpText {
    heading: "VSync Mode",
    text: &[
        "Enable video synchronization. Prevents screen tearing and may improve frame pacing, but may also increase input latency.",
        "Fast VSync prevents screen tearing but otherwise behaves similarly to disabled VSync.",
    ],
};

pub const FILTER_MODE: HelpText = HelpText {
    heading: "Filter Mode",
    text: &[
        "Configure texture filtering mode used when rendering frames to the display.",
        "Nearest-neighbor is very sharp but may cause aliasing when using a non-integer resolution scale or non-square pixels.",
        "Linear is smooth but can be blurry if not combined with prescaling (see below).",
    ],
};

pub const PREPROCESS_SHADER: HelpText = HelpText {
    heading: "Preprocess Shader",
    text: &[
        "Configure an optional blur or anti-dither shader. All preprocess shaders are applied at the console's native resolution except for SNES Adaptive blur.",
        "The SNES Adaptive option blurs horizontally at 2x native resolution and will also correctly handle SNES games that use 512px high-resolution modes.",
    ],
};

pub const SCANLINES: HelpText = HelpText {
    heading: "Scanlines",
    text: &[
        "Configure an optional scanlines filter. The dim option renders scanlines at 50% color on a linear scale.",
        "Note that this filter only works properly when using integer height scaling and an even-numbered prescale factor (e.g. 2x or 4x).",
    ],
};

pub const PRESCALING: HelpText = HelpText {
    heading: "Prescaling",
    text: &[
        "Apply integer upscaling before rendering frames to display. When combined with linear filtering, this creates an image that is sharp but with minimal aliasing.",
        "Auto-prescale dynamically adjusts the upscale factor based on the ratio between the viewport size and the console's native resolution.",
    ],
};

pub const INTEGER_HEIGHT_SCALING: HelpText = HelpText {
    heading: "Force Integer Height Scaling",
    text: &[
        "If enabled, frames will always be displayed at the largest possible integer multiple of the console's native vertical resolution that will fit in the current viewport.",
    ],
};

pub const AUDIO_SAMPLE_RATE: HelpText = HelpText {
    heading: "Audio Sample Rate",
    text: &[
        "Configure the output sample rate.",
        "Most audio devices should support both of these settings, but some may only support one.",
    ],
};

pub const AUDIO_SYNC: HelpText = HelpText {
    heading: "Audio Sync",
    text: &[
        "If enabled, synchronize emulation speed to the audio output stream.",
        "This is more accurate than video sync and prevents audio pops caused by buffer overflow, but it may cause poor frame pacing if the display refresh rate does not match the console's native refresh rate.",
    ],
};

pub const AUDIO_DEVICE_QUEUE_SIZE: HelpText = HelpText {
    heading: "Device Queue Size",
    text: &[
        "Configure audio device queue size in samples.",
        "Setting this too low can cause audio popping due to buffer underflow. Setting this too high can cause noticeable audio latency.",
    ],
};

pub const INTERNAL_AUDIO_BUFFER_SIZE: HelpText = HelpText {
    heading: "Internal Audio Buffer Size",
    text: &[
        "Configure size of the internal audio buffer, where samples are buffered before they're pushed to the SDL2 audio queue.",
        "This is generally fine to leave at the default value. Setting this too low can hurt performance due to excessive synchronization with the SDL2 audio queue.",
    ],
};

pub const AUDIO_SYNC_THRESHOLD: HelpText = HelpText {
    heading: "Audio Sync Threshold",
    text: &[
        "If audio sync is enabled, configure the audio queue size at which the emulation thread will block and wait for the audio thread.",
        "Setting this too low can cause various performance issues. Setting this too high will cause noticeable audio latency.",
    ],
};

pub const AUDIO_GAIN: HelpText = HelpText {
    heading: "Audio Gain",
    text: &[
        "Optionally configure a gain value to apply to final mixed audio output.",
        "Positive values increase volume and negative values decrease volume. Can be an integer or decimal value.",
    ],
};
