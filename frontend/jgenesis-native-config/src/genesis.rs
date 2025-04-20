use crate::AppConfig;
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig, GenesisRegion, Opn2BusyBehavior};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GenesisConfig, Sega32XConfig, SegaCdConfig};
use s32x_core::api::{S32XVideoOut, Sega32XEmulatorConfig};
use segacd_core::api::{PcmInterpolation, SegaCdEmulatorConfig};
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
    #[serde(default = "true_fn")]
    pub ym2612_enabled: bool,
    #[serde(default = "true_fn")]
    pub psg_enabled: bool,
}

const fn true_fn() -> bool {
    true
}

const fn default_68k_divider() -> u64 {
    genesis_core::timing::NATIVE_M68K_DIVIDER
}

const fn default_genesis_lpf_cutoff() -> u32 {
    genesis_core::audio::DEFAULT_GENESIS_LPF_CUTOFF
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
    NonZeroU64::new(segacd_core::api::DEFAULT_SUB_CPU_DIVIDER).unwrap()
}

const fn default_pcm_lpf_cutoff() -> u32 {
    segacd_core::DEFAULT_PCM_LPF_CUTOFF
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
    #[serde(default)]
    pub apply_genesis_lpf_to_pwm: bool,
    #[serde(default = "true_fn")]
    pub pwm_enabled: bool,
}

impl Default for Sega32XAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn genesis_config(&self, path: PathBuf) -> Box<GenesisConfig> {
        Box::new(GenesisConfig {
            common: self.common_config(path),
            inputs: self.input.genesis.clone(),
            emulator_config: GenesisEmulatorConfig {
                p1_controller_type: self.input.genesis.p1_type,
                p2_controller_type: self.input.genesis.p2_type,
                forced_timing_mode: self.genesis.forced_timing_mode,
                forced_region: self.genesis.forced_region,
                aspect_ratio: self.genesis.aspect_ratio,
                adjust_aspect_ratio_in_2x_resolution: self
                    .genesis
                    .adjust_aspect_ratio_in_2x_resolution,
                remove_sprite_limits: self.genesis.remove_sprite_limits,
                m68k_clock_divider: self.genesis.m68k_clock_divider,
                non_linear_color_scale: self.genesis.non_linear_color_scale,
                deinterlace: self.genesis.deinterlace,
                render_vertical_border: self.genesis.render_vertical_border,
                render_horizontal_border: self.genesis.render_horizontal_border,
                plane_a_enabled: self.genesis.plane_a_enabled,
                plane_b_enabled: self.genesis.plane_b_enabled,
                sprites_enabled: self.genesis.sprites_enabled,
                window_enabled: self.genesis.window_enabled,
                backdrop_enabled: self.genesis.backdrop_enabled,
                quantize_ym2612_output: self.genesis.quantize_ym2612_output,
                emulate_ym2612_ladder_effect: self.genesis.emulate_ym2612_ladder_effect,
                opn2_busy_behavior: self.genesis.opn2_busy_behavior,
                genesis_lpf_enabled: self.genesis.genesis_lpf_enabled,
                genesis_lpf_cutoff: self.genesis.genesis_lpf_cutoff,
                ym2612_enabled: self.genesis.ym2612_enabled,
                psg_enabled: self.genesis.psg_enabled,
            },
        })
    }

    #[must_use]
    pub fn sega_cd_config(&self, path: PathBuf) -> Box<SegaCdConfig> {
        let genesis_config = *self.genesis_config(path);
        let genesis_emu_config = genesis_config.emulator_config;
        Box::new(SegaCdConfig {
            genesis: genesis_config,
            bios_file_path: self.sega_cd.bios_path.clone(),
            run_without_disc: false,
            emulator_config: SegaCdEmulatorConfig {
                genesis: genesis_emu_config,
                pcm_interpolation: self.sega_cd.pcm_interpolation,
                enable_ram_cartridge: self.sega_cd.enable_ram_cartridge,
                load_disc_into_ram: self.sega_cd.load_disc_into_ram,
                disc_drive_speed: self.sega_cd.disc_drive_speed,
                sub_cpu_divider: self.sega_cd.sub_cpu_divider,
                pcm_lpf_enabled: self.sega_cd.pcm_lpf_enabled,
                pcm_lpf_cutoff: self.sega_cd.pcm_lpf_cutoff,
                apply_genesis_lpf_to_pcm: self.sega_cd.apply_genesis_lpf_to_pcm,
                apply_genesis_lpf_to_cd_da: self.sega_cd.apply_genesis_lpf_to_cd_da,
                pcm_enabled: self.sega_cd.pcm_enabled,
                cd_audio_enabled: self.sega_cd.cd_audio_enabled,
            },
        })
    }

    #[must_use]
    pub fn sega_32x_config(&self, path: PathBuf) -> Box<Sega32XConfig> {
        let genesis_config = *self.genesis_config(path);
        let genesis_emu_config = genesis_config.emulator_config;
        Box::new(Sega32XConfig {
            genesis: genesis_config,
            emulator_config: Sega32XEmulatorConfig {
                genesis: genesis_emu_config,
                video_out: self.sega_32x.video_out,
                apply_genesis_lpf_to_pwm: self.sega_32x.apply_genesis_lpf_to_pwm,
                pwm_enabled: self.sega_32x.pwm_enabled,
            },
        })
    }
}
