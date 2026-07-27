//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::{FirstOrderIirFilter, IirFilter, SecondOrderIirFilter};
use genesis_components::audio::LowPassSettings;
use genesis_config::GenesisEmulatorConfig;

pub const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
pub const CD_DA_FREQUENCY: f64 = 44_100.0;

fn new_pcm_low_pass<const N: usize>(cutoff: f64) -> IirFilter<N> {
    dsp::design::butterworth(cutoff, SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0, FilterType::LowPass)
}

fn new_cd_da_low_pass(cutoff: f64) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff, CD_DA_FREQUENCY, FilterType::LowPass)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SegaCdAudioFilter {
    gen_low_pass_setting: LowPassSettings,
    pcm_lpf_enabled: bool,
    pcm_lpf_cutoff: u32,
    apply_gen_lpf_to_pcm: bool,
    apply_gen_lpf_to_cd_da: bool,
    pcm_lpf_l: SecondOrderIirFilter,
    pcm_lpf_r: SecondOrderIirFilter,
    pcm_gen_lpf_l: FirstOrderIirFilter,
    pcm_gen_lpf_r: FirstOrderIirFilter,
    cd_da_gen_lpf_l: FirstOrderIirFilter,
    cd_da_gen_lpf_r: FirstOrderIirFilter,
}

impl SegaCdAudioFilter {
    pub fn new(config: &GenesisEmulatorConfig) -> Self {
        let gen_cutoff: f64 = config.genesis_lpf_cutoff.into();
        let pcm_cutoff: f64 = config.sega_cd.pcm_lpf_cutoff.into();

        Self {
            gen_low_pass_setting: LowPassSettings::from_config(config),
            pcm_lpf_enabled: config.sega_cd.pcm_lpf_enabled,
            pcm_lpf_cutoff: config.sega_cd.pcm_lpf_cutoff,
            apply_gen_lpf_to_pcm: config.sega_cd.apply_genesis_lpf_to_pcm,
            apply_gen_lpf_to_cd_da: config.sega_cd.apply_genesis_lpf_to_cd_da,
            pcm_lpf_l: new_pcm_low_pass(pcm_cutoff),
            pcm_lpf_r: new_pcm_low_pass(pcm_cutoff),
            pcm_gen_lpf_l: new_pcm_low_pass(gen_cutoff),
            pcm_gen_lpf_r: new_pcm_low_pass(gen_cutoff),
            cd_da_gen_lpf_l: new_cd_da_low_pass(gen_cutoff),
            cd_da_gen_lpf_r: new_cd_da_low_pass(gen_cutoff),
        }
    }

    pub fn filter_pcm(&mut self, (mut sample_l, mut sample_r): (f64, f64)) -> (f64, f64) {
        if self.pcm_lpf_enabled {
            sample_l = self.pcm_lpf_l.filter(sample_l);
            sample_r = self.pcm_lpf_r.filter(sample_r);
        }

        if self.gen_low_pass_setting.genesis_enabled && self.apply_gen_lpf_to_pcm {
            sample_l = self.pcm_gen_lpf_l.filter(sample_l);
            sample_r = self.pcm_gen_lpf_r.filter(sample_r);
        }

        (sample_l, sample_r)
    }

    pub fn filter_cd_da(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.gen_low_pass_setting.genesis_enabled || !self.apply_gen_lpf_to_cd_da {
            return (sample_l, sample_r);
        }

        (self.cd_da_gen_lpf_l.filter(sample_l), self.cd_da_gen_lpf_r.filter(sample_r))
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        if self.gen_low_pass_setting == LowPassSettings::from_config(config)
            && self.pcm_lpf_enabled == config.sega_cd.pcm_lpf_enabled
            && self.pcm_lpf_cutoff == config.sega_cd.pcm_lpf_cutoff
            && self.apply_gen_lpf_to_pcm == config.sega_cd.apply_genesis_lpf_to_pcm
            && self.apply_gen_lpf_to_cd_da == config.sega_cd.apply_genesis_lpf_to_cd_da
        {
            return;
        }

        *self = Self::new(config);
    }
}
