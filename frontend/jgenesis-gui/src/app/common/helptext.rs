use crate::app::HelpText;

pub const FULLSCREEN: HelpText = HelpText {
    heading: "Fullscreen",
    text: &["If enabled, launch fullscreen instead of windowed."],
};

pub const FULLSCREEN_MODE: HelpText = HelpText {
    heading: "Fullscreen Mode",
    text: &[
        "Choose whether fullscreen is borderless or exclusive. Exclusive fullscreen may not work correctly on some platforms.",
    ],
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

pub const FRAME_TIME_SYNC: HelpText = HelpText {
    heading: "Frame Time Sync",
    text: &[
        "If enabled, make a best effort to present video frames with consistent frame timing while maintaining the emulated system's native framerate.",
        "This is different from VSync in that it's based purely on system time and does not perform any kind of synchronization with the graphics driver.",
    ],
};

pub const AUDIO_SYNC: HelpText = HelpText {
    heading: "Audio Sync",
    text: &[
        "If enabled, synchronize emulation speed to the audio output stream by blocking when the audio buffer is full.",
        "This prevents audio popping caused by audio buffer overflows.",
    ],
};

pub const AUDIO_DYNAMIC_RESAMPLING: HelpText = HelpText {
    heading: "Audio Dynamic Resampling Ratio",
    text: &[
        "If enabled, periodically adjust the audio resampling ratio to try and keep the audio buffer as close as possible to the target buffer size. The audio buffer is allowed to grow to double size when this is enabled.",
        "This should reduce audio pops caused by audio buffer underflow and overflow when using frame time sync or VSync.",
        "Changing the resampling ratio this way does slightly change the audio pitch, but the difference should be inaudible - the adjusted ratio is restricted to being within 0.5% of the original ratio.",
    ],
};

pub const AUDIO_HARDWARE_QUEUE_SIZE: HelpText = HelpText {
    heading: "Audio Hardware Queue Size",
    text: &[
        "Configure audio device queue size in samples.",
        "Decreasing this value can increase CPU usage, and decreasing it too much can also cause various audio playback issues.",
        "Increasing this value makes audio sync and dynamic resampling ratio less accurate, and it will also increase audio latency.",
    ],
};

pub const AUDIO_BUFFER_SIZE: HelpText = HelpText {
    heading: "Audio Buffer Size",
    text: &[
        "Configure the size of the audio buffer in samples. This is where audio is buffered before sending to the audio device.",
        "This value is the max buffer capacity when audio sync is disabled, and the size at which the emulator will block when audio sync is enabled.",
        "Dynamic resampling ratio uses this value as the target buffer size and allows the buffer to grow to double this value before dropping samples or blocking.",
        "This setting affects audio latency.",
    ],
};

pub const AUDIO_GAIN: HelpText = HelpText {
    heading: "Audio Gain",
    text: &[
        "Optionally configure a gain value to apply to final mixed audio output.",
        "Positive values increase volume and negative values decrease volume. Can be an integer or decimal value.",
        "Setting this too high can cause audio distortion.",
    ],
};
