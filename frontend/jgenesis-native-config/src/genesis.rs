use genesis_config::{
    GenesisAspectRatio, GenesisRegion, Opn2BusyBehavior, PcmInterpolation, S32XColorTint,
    S32XPwmResampling, S32XVideoOut, S32XVoidColorType,
};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::deserialize_default_on_error;
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU16, NonZeroU64};
use std::path::PathBuf;

const DEFAULT_DRIVE_SPEED: NonZeroU16 = NonZeroU16::new(1).unwrap();

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GenesisAppConfig {
    pub forced_timing_mode: Option<TimingMode>,
    pub forced_region: Option<GenesisRegion>,
    pub aspect_ratio: GenesisAspectRatio,
    pub force_square_pixels_in_h40: bool,
    pub adjust_aspect_ratio_in_2x_resolution: bool,
    pub anamorphic_widescreen: bool,
    pub remove_sprite_limits: bool,
    pub m68k_clock_divider: u64,
    pub non_linear_color_scale: bool,
    pub deinterlace: bool,
    pub render_vertical_border: bool,
    pub render_horizontal_border: bool,
    pub plane_a_enabled: bool,
    pub plane_b_enabled: bool,
    pub window_enabled: bool,
    pub backdrop_enabled: bool,
    pub sprites_enabled: bool,
    pub quantize_ym2612_output: bool,
    pub opn2_busy_behavior: Opn2BusyBehavior,
    pub emulate_ym2612_ladder_effect: bool,
    pub genesis_lpf_enabled: bool,
    pub genesis_lpf_cutoff: u32,
    pub ym2612_2nd_lpf_enabled: bool,
    pub ym2612_2nd_lpf_cutoff: u32,
    pub ym2612_channels_enabled: [bool; 6],
    pub ym2612_enabled: bool,
    pub psg_enabled: bool,
    pub ym2612_volume_adjustment_db: f64,
    pub psg_volume_adjustment_db: f64,
}

impl Default for GenesisAppConfig {
    fn default() -> Self {
        Self {
            forced_timing_mode: None,
            forced_region: None,
            aspect_ratio: GenesisAspectRatio::default(),
            force_square_pixels_in_h40: false,
            adjust_aspect_ratio_in_2x_resolution: true,
            anamorphic_widescreen: false,
            remove_sprite_limits: false,
            m68k_clock_divider: genesis_config::NATIVE_M68K_DIVIDER,
            non_linear_color_scale: true,
            deinterlace: true,
            render_vertical_border: false,
            render_horizontal_border: false,
            plane_a_enabled: true,
            plane_b_enabled: true,
            window_enabled: true,
            backdrop_enabled: true,
            sprites_enabled: true,
            quantize_ym2612_output: true,
            opn2_busy_behavior: Opn2BusyBehavior::default(),
            emulate_ym2612_ladder_effect: true,
            genesis_lpf_enabled: true,
            genesis_lpf_cutoff: genesis_config::MODEL_1_VA2_LPF_CUTOFF,
            ym2612_2nd_lpf_enabled: false,
            ym2612_2nd_lpf_cutoff: genesis_config::MODEL_2_2ND_LPF_CUTOFF,
            ym2612_channels_enabled: [true; 6],
            ym2612_enabled: true,
            psg_enabled: true,
            ym2612_volume_adjustment_db: 0.0,
            psg_volume_adjustment_db: 0.0,
        }
    }
}

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SegaCdAppConfig {
    pub bios_path: Option<PathBuf>,
    pub eu_bios_path: Option<PathBuf>,
    pub jp_bios_path: Option<PathBuf>,
    pub per_region_bios: bool,
    pub pcm_interpolation: PcmInterpolation,
    pub enable_ram_cartridge: bool,
    pub load_disc_into_ram: bool,
    pub disc_drive_speed: NonZeroU16,
    pub sub_cpu_divider: NonZeroU64,
    pub pcm_lpf_enabled: bool,
    pub pcm_lpf_cutoff: u32,
    pub apply_genesis_lpf_to_pcm: bool,
    pub apply_genesis_lpf_to_cd_da: bool,
    pub pcm_enabled: bool,
    pub cd_audio_enabled: bool,
    pub pcm_volume_adjustment_db: f64,
    pub cd_volume_adjustment_db: f64,
}

impl Default for SegaCdAppConfig {
    fn default() -> Self {
        Self {
            bios_path: None,
            eu_bios_path: None,
            jp_bios_path: None,
            per_region_bios: true,
            pcm_interpolation: PcmInterpolation::default(),
            enable_ram_cartridge: true,
            load_disc_into_ram: false,
            disc_drive_speed: DEFAULT_DRIVE_SPEED,
            sub_cpu_divider: NonZeroU64::new(genesis_config::NATIVE_SUB_CPU_DIVIDER).unwrap(),
            pcm_lpf_enabled: true,
            pcm_lpf_cutoff: genesis_config::DEFAULT_PCM_LPF_CUTOFF,
            apply_genesis_lpf_to_pcm: false,
            apply_genesis_lpf_to_cd_da: false,
            pcm_enabled: true,
            cd_audio_enabled: true,
            pcm_volume_adjustment_db: 0.0,
            cd_volume_adjustment_db: 0.0,
        }
    }
}

#[deserialize_default_on_error]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sega32XAppConfig {
    pub sh2_clock_multiplier: NonZeroU64,
    pub video_out: S32XVideoOut,
    pub apply_genesis_lpf_to_pwm: bool,
    pub darken_genesis_colors: bool,
    pub color_tint: S32XColorTint,
    pub show_high_priority: bool,
    pub show_low_priority: bool,
    pub void_color_type: S32XVoidColorType,
    pub void_palette_index: u8,
    pub void_direct: [u8; 3],
    pub void_direct_priority: bool,
    pub pwm_resampling: S32XPwmResampling,
    pub pwm_enabled: bool,
    pub pwm_volume_adjustment_db: f64,
}

impl Default for Sega32XAppConfig {
    fn default() -> Self {
        Self {
            sh2_clock_multiplier: NonZeroU64::new(genesis_config::NATIVE_SH2_MULTIPLIER).unwrap(),
            video_out: S32XVideoOut::default(),
            apply_genesis_lpf_to_pwm: true,
            darken_genesis_colors: true,
            color_tint: S32XColorTint::default(),
            show_high_priority: true,
            show_low_priority: true,
            void_color_type: S32XVoidColorType::default(),
            void_palette_index: 0,
            void_direct: [0; 3],
            void_direct_priority: false,
            pwm_resampling: S32XPwmResampling::default(),
            pwm_enabled: true,
            pwm_volume_adjustment_db: 0.0,
        }
    }
}
