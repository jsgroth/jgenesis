// TODO remove
#![allow(dead_code)]

use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl Display for VSyncMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enabled => write!(f, "Enabled"),
            Self::Disabled => write!(f, "Disabled"),
            Self::Fast => write!(f, "Fast"),
        }
    }
}

impl FromStr for VSyncMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Enabled" => Ok(Self::Enabled),
            "Disabled" => Ok(Self::Disabled),
            "Fast" => Ok(Self::Fast),
            _ => Err(format!("invalid VSync mode string: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

impl Display for FilterMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nearest => write!(f, "Nearest"),
            Self::Linear => write!(f, "Linear"),
        }
    }
}

impl FromStr for FilterMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Nearest" => Ok(Self::Nearest),
            "Linear" => Ok(Self::Linear),
            _ => Err(format!("Invalid filter mod string: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub vsync_mode: VSyncMode,
    pub prescale_factor: PrescaleFactor,
    pub filter_mode: FilterMode,
}

impl Display for RendererConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RendererConfig{{vsync_mode={}, prescale_factor={}, filter_mode={}}}",
            self.vsync_mode, self.prescale_factor, self.filter_mode
        )
    }
}
