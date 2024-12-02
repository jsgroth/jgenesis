use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay, EnumFromStr};
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum WgpuBackend {
    #[default]
    Auto,
    Vulkan,
    DirectX12,
    OpenGl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum VSyncMode {
    Enabled,
    Disabled,
    Fast,
}

impl VSyncMode {
    pub(crate) fn to_wgpu_present_mode(self) -> wgpu::PresentMode {
        match self {
            Self::Enabled => wgpu::PresentMode::Fifo,
            Self::Disabled => wgpu::PresentMode::Immediate,
            Self::Fast => wgpu::PresentMode::Mailbox,
        }
    }
}

impl Default for VSyncMode {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PrescaleFactor(u32);

impl PrescaleFactor {
    pub const ONE: Self = Self(1);

    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl From<PrescaleFactor> for u32 {
    fn from(value: PrescaleFactor) -> Self {
        value.0
    }
}

impl TryFrom<u32> for PrescaleFactor {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Err(format!("invalid prescale factor: {value}")),
            _ => Ok(Self(value)),
        }
    }
}

impl From<NonZeroU32> for PrescaleFactor {
    fn from(value: NonZeroU32) -> Self {
        Self(value.get())
    }
}

impl Default for PrescaleFactor {
    fn default() -> Self {
        Self::ONE
    }
}

impl Display for PrescaleFactor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrescaleMode {
    Auto,
    Manual(PrescaleFactor),
}

impl Display for PrescaleMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "Auto"),
            Self::Manual(factor) => write!(f, "Manual({factor})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum Scanlines {
    #[default]
    None,
    Dim,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum FilterMode {
    Nearest,
    #[default]
    Linear,
}

impl FilterMode {
    pub(crate) fn to_wgpu_filter_mode(self) -> wgpu::FilterMode {
        match self {
            Self::Nearest => wgpu::FilterMode::Nearest,
            Self::Linear => wgpu::FilterMode::Linear,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumFromStr, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum PreprocessShader {
    #[default]
    None,
    HorizontalBlurTwoPixels,
    HorizontalBlurThreePixels,
    HorizontalBlurSnesAdaptive,
    AntiDitherWeak,
    AntiDitherStrong,
}

#[derive(Debug, Clone, Copy, ConfigDisplay)]
pub struct RendererConfig {
    pub wgpu_backend: WgpuBackend,
    pub vsync_mode: VSyncMode,
    pub frame_time_sync: bool,
    pub prescale_mode: PrescaleMode,
    pub scanlines: Scanlines,
    pub force_integer_height_scaling: bool,
    pub filter_mode: FilterMode,
    pub preprocess_shader: PreprocessShader,
    pub use_webgl2_limits: bool,
}
