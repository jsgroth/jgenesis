use jgenesis_proc_macros::{ConfigDisplay, EnumAll, EnumDisplay, EnumFromStr};
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;

pub const DXIL_PATH: &str = "dxil.dll";
pub const DXCOMPILER_PATH: &str = "dxcompiler.dll";

#[must_use]
pub fn dx12_backend_options() -> wgpu::Dx12BackendOptions {
    wgpu::Dx12BackendOptions {
        shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: DXCOMPILER_PATH.into(),
            dxil_path: DXIL_PATH.into(),
            max_shader_model: wgpu::DxcShaderModel::V6_7,
        },
    }
}

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

impl WgpuBackend {
    #[must_use]
    pub fn to_wgpu(self) -> wgpu::Backends {
        #[cfg(target_os = "windows")]
        if self == WgpuBackend::Auto && supports_dx12() {
            // Prefer DX12 on Windows if supported (necessary because wgpu prefers Vulkan over DX12)
            // AMD GPUs seem to sometimes have color space bugs on Windows w/ Vulkan
            return wgpu::Backends::DX12;
        }

        match self {
            WgpuBackend::Auto => wgpu::Backends::PRIMARY,
            WgpuBackend::Vulkan => wgpu::Backends::VULKAN,
            WgpuBackend::DirectX12 => wgpu::Backends::DX12,
            WgpuBackend::OpenGl => wgpu::Backends::GL,
        }
    }
}

#[cfg(target_os = "windows")]
fn supports_dx12() -> bool {
    use std::sync::LazyLock;

    static SUPPORTS_DX12: LazyLock<bool> = LazyLock::new(|| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions {
                dx12: dx12_backend_options(),
                gl: wgpu::GlBackendOptions::default(),
                noop: wgpu::NoopBackendOptions::default(),
            },
        });

        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        adapter.is_ok()
    });

    *SUPPORTS_DX12
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum WgpuPowerPreference {
    #[default]
    HighPerformance,
    LowPower,
    None,
}

impl WgpuPowerPreference {
    #[must_use]
    pub fn to_wgpu(self) -> wgpu::PowerPreference {
        match self {
            Self::HighPerformance => wgpu::PowerPreference::HighPerformance,
            Self::LowPower => wgpu::PowerPreference::LowPower,
            Self::None => wgpu::PowerPreference::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum VSyncMode {
    Enabled,
    #[default]
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
    pub wgpu_power_preference: WgpuPowerPreference,
    pub vsync_mode: VSyncMode,
    pub frame_time_sync: bool,
    pub prescale_mode: PrescaleMode,
    pub scanlines: Scanlines,
    pub force_integer_height_scaling: bool,
    pub filter_mode: FilterMode,
    pub preprocess_shader: PreprocessShader,
    pub use_webgl2_limits: bool,
}
