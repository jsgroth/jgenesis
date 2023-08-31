use jgenesis_proc_macros::{ConfigDisplay, EnumDisplay, EnumFromStr};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
pub enum VSyncMode {
    #[default]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumDisplay, EnumFromStr,
)]
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

#[derive(Debug, Clone, ConfigDisplay)]
pub struct RendererConfig {
    pub vsync_mode: VSyncMode,
    pub prescale_factor: PrescaleFactor,
    pub filter_mode: FilterMode,
}
