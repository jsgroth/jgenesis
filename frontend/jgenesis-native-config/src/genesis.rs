use genesis_config::{
    GenesisAspectRatio, GenesisRegion, Opn2BusyBehavior, PcmInterpolation, S32XVideoOut,
};
use jgenesis_common::frontend::TimingMode;
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU16, NonZeroU64};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisAppConfig {
    #[serde(default)]
    pub forced_timing_mode: Option<TimingMode>,
    #[serde(default)]
    pub forced_region: Option<GenesisRegion>,
    #[serde(default)]
    pub aspect_ratio: GenesisAspectRatio,
    #[serde(default = "true_fn")]
    pub adjust_aspect_ratio_in_2x_resolution: bool,
    #[serde(default)]
    pub remove_sprite_limits: bool,
    #[serde(default = "default_68k_divider")]
    pub m68k_clock_divider: u64,
    #[serde(default = "true_fn")]
    pub non_linear_color_scale: bool,
    #[serde(default = "true_fn")]
    pub deinterlace: bool,
    #[serde(default)]
    pub render_vertical_border: bool,
    #[serde(default)]
    pub render_horizontal_border: bool,
    #[serde(default = "true_fn")]
    pub plane_a_enabled: bool,
    #[serde(default = "true_fn")]
    pub plane_b_enabled: bool,
    #[serde(default = "true_fn")]
    pub window_enabled: bool,
    #[serde(default = "true_fn")]
    pub backdrop_enabled: bool,
    #[serde(default = "true_fn")]
    pub sprites_enabled: bool,
    #[serde(default = "true_fn")]
    pub quantize_ym2612_output: bool,
    #[serde(default)]
    pub opn2_busy_behavior: Opn2BusyBehavior,
    #[serde(default = "true_fn")]
    pub emulate_ym2612_ladder_effect: bool,
    #[serde(default = "true_fn")]
    pub genesis_lpf_enabled: bool,
    #[serde(default = "default_genesis_lpf_cutoff")]
    pub genesis_lpf_cutoff: u32,
    #[serde(default)]
    pub ym2612_2nd_lpf_enabled: bool,
    #[serde(default = "default_ym2612_2nd_lpf_cutoff")]
    pub ym2612_2nd_lpf_cutoff: u32,
    #[serde(default = "true_fn")]
    pub ym2612_enabled: bool,
    #[serde(default = "true_fn")]
    pub psg_enabled: bool,
}

const fn true_fn() -> bool {
    true
}

const fn default_68k_divider() -> u64 {
    7
}

const fn default_genesis_lpf_cutoff() -> u32 {
    3390
}

const fn default_ym2612_2nd_lpf_cutoff() -> u32 {
    7973
}

impl Default for GenesisAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegaCdAppConfig {
    pub bios_path: Option<PathBuf>,
    #[serde(default)]
    pub pcm_interpolation: PcmInterpolation,
    #[serde(default = "true_fn")]
    pub enable_ram_cartridge: bool,
    #[serde(default)]
    pub load_disc_into_ram: bool,
    #[serde(default = "default_drive_speed")]
    pub disc_drive_speed: NonZeroU16,
    #[serde(default = "default_sub_divider")]
    pub sub_cpu_divider: NonZeroU64,
    #[serde(default = "true_fn")]
    pub pcm_lpf_enabled: bool,
    #[serde(default = "default_pcm_lpf_cutoff")]
    pub pcm_lpf_cutoff: u32,
    #[serde(default)]
    pub apply_genesis_lpf_to_pcm: bool,
    #[serde(default)]
    pub apply_genesis_lpf_to_cd_da: bool,
    #[serde(default = "true_fn")]
    pub pcm_enabled: bool,
    #[serde(default = "true_fn")]
    pub cd_audio_enabled: bool,
}

fn default_drive_speed() -> NonZeroU16 {
    NonZeroU16::new(1).unwrap()
}

fn default_sub_divider() -> NonZeroU64 {
    NonZeroU64::new(genesis_config::DEFAULT_SUB_CPU_DIVIDER).unwrap()
}

const fn default_pcm_lpf_cutoff() -> u32 {
    genesis_config::DEFAULT_PCM_LPF_CUTOFF
}

impl Default for SegaCdAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sega32XAppConfig {
    #[serde(default)]
    pub video_out: S32XVideoOut,
    #[serde(default = "true_fn")]
    pub apply_genesis_lpf_to_pwm: bool,
    #[serde(default = "true_fn")]
    pub pwm_enabled: bool,
}

impl Default for Sega32XAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}
