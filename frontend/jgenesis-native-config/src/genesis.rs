use crate::AppConfig;
use genesis_core::{GenesisAspectRatio, GenesisRegion};
use jgenesis_common::frontend::TimingMode;
use jgenesis_native_driver::config::{GenesisConfig, Sega32XConfig, SegaCdConfig};
use s32x_core::api::S32XVideoOut;
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub emulate_non_linear_vdp_dac: bool,
    #[serde(default)]
    pub render_vertical_border: bool,
    #[serde(default)]
    pub render_horizontal_border: bool,
    #[serde(default = "true_fn")]
    pub quantize_ym2612_output: bool,
    #[serde(default = "true_fn")]
    pub emulate_ym2612_ladder_effect: bool,
    #[serde(default = "true_fn")]
    pub ym2612_enabled: bool,
    #[serde(default = "true_fn")]
    pub psg_enabled: bool,
}

const fn true_fn() -> bool {
    true
}

impl Default for GenesisAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegaCdAppConfig {
    pub bios_path: Option<String>,
    #[serde(default = "true_fn")]
    pub enable_ram_cartridge: bool,
    #[serde(default)]
    pub load_disc_into_ram: bool,
    #[serde(default = "true_fn")]
    pub pcm_enabled: bool,
    #[serde(default = "true_fn")]
    pub cd_audio_enabled: bool,
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
    pub pwm_enabled: bool,
}

impl Default for Sega32XAppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl AppConfig {
    #[must_use]
    pub fn genesis_config(&self, path: String) -> Box<GenesisConfig> {
        Box::new(GenesisConfig {
            common: self.common_config(
                path,
                self.inputs.genesis_keyboard.clone(),
                self.inputs.genesis_joystick.clone(),
            ),
            p1_controller_type: self.inputs.genesis_p1_type,
            p2_controller_type: self.inputs.genesis_p2_type,
            forced_timing_mode: self.genesis.forced_timing_mode,
            forced_region: self.genesis.forced_region,
            aspect_ratio: self.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.genesis.adjust_aspect_ratio_in_2x_resolution,
            remove_sprite_limits: self.genesis.remove_sprite_limits,
            emulate_non_linear_vdp_dac: self.genesis.emulate_non_linear_vdp_dac,
            render_vertical_border: self.genesis.render_vertical_border,
            render_horizontal_border: self.genesis.render_horizontal_border,
            quantize_ym2612_output: self.genesis.quantize_ym2612_output,
            emulate_ym2612_ladder_effect: self.genesis.emulate_ym2612_ladder_effect,
            ym2612_enabled: self.genesis.ym2612_enabled,
            psg_enabled: self.genesis.psg_enabled,
        })
    }

    #[must_use]
    pub fn sega_cd_config(&self, path: String) -> Box<SegaCdConfig> {
        Box::new(SegaCdConfig {
            genesis: *self.genesis_config(path),
            bios_file_path: self.sega_cd.bios_path.clone(),
            enable_ram_cartridge: self.sega_cd.enable_ram_cartridge,
            run_without_disc: false,
            load_disc_into_ram: self.sega_cd.load_disc_into_ram,
            pcm_enabled: self.sega_cd.pcm_enabled,
            cd_audio_enabled: self.sega_cd.cd_audio_enabled,
        })
    }

    #[must_use]
    pub fn sega_32x_config(&self, path: String) -> Box<Sega32XConfig> {
        Box::new(Sega32XConfig {
            genesis: *self.genesis_config(path),
            video_out: self.sega_32x.video_out,
            pwm_enabled: self.sega_32x.pwm_enabled,
        })
    }
}
