use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::{FirstOrderIirFilter, IirFilter, SecondOrderIirFilter};
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::frontend::TimingMode;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

// -7dB (10 ^ -7/20)
pub const PSG_COEFFICIENT: f64 = 0.44668359215096315;

#[must_use]
pub fn new_ym2612_low_pass<const N: usize>(timing_mode: TimingMode, cutoff: u32) -> IirFilter<N> {
    dsp::design::butterworth(cutoff.into(), ym2612_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_psg_low_pass(timing_mode: TimingMode, cutoff: u32) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff.into(), psg_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_ym2612_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, ym2612_frequency(timing_mode), FilterType::HighPass)
}

#[must_use]
pub fn new_psg_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, psg_frequency(timing_mode), FilterType::HighPass)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct LowPassSettings {
    pub genesis_enabled: bool,
    pub genesis_cutoff: u32,
    pub ym2612_2nd_enabled: bool,
    pub ym2612_2nd_cutoff: u32,
}

impl LowPassSettings {
    #[must_use]
    pub fn from_config(config: &GenesisEmulatorConfig) -> Self {
        Self {
            genesis_enabled: config.genesis_lpf_enabled,
            genesis_cutoff: config.genesis_lpf_cutoff,
            ym2612_2nd_enabled: config.ym2612_2nd_lpf_enabled,
            ym2612_2nd_cutoff: config.ym2612_2nd_lpf_cutoff,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioFilter {
    ym2612_dc_offset_l: FirstOrderIirFilter,
    ym2612_dc_offset_r: FirstOrderIirFilter,
    psg_dc_offset: FirstOrderIirFilter,
    low_pass_settings: LowPassSettings,
    ym2612_gen_low_pass_l: FirstOrderIirFilter,
    ym2612_gen_low_pass_r: FirstOrderIirFilter,
    ym2612_2nd_low_pass_l: SecondOrderIirFilter,
    ym2612_2nd_low_pass_r: SecondOrderIirFilter,
    psg_low_pass: FirstOrderIirFilter,
}

impl GenesisAudioFilter {
    #[must_use]
    pub fn new(timing_mode: TimingMode, low_pass_settings: LowPassSettings) -> Self {
        Self {
            ym2612_dc_offset_l: new_ym2612_dc_offset(timing_mode),
            ym2612_dc_offset_r: new_ym2612_dc_offset(timing_mode),
            psg_dc_offset: new_psg_dc_offset(timing_mode),
            low_pass_settings,
            ym2612_gen_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_gen_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_2nd_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            ym2612_2nd_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            psg_low_pass: new_psg_low_pass(timing_mode, low_pass_settings.genesis_cutoff),
        }
    }

    #[must_use]
    pub fn filter_ym2612(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        let (mut sample_l, mut sample_r) =
            (self.ym2612_dc_offset_l.filter(sample_l), self.ym2612_dc_offset_r.filter(sample_r));

        if self.low_pass_settings.ym2612_2nd_enabled {
            sample_l = self.ym2612_2nd_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_2nd_low_pass_r.filter(sample_r);
        }

        if self.low_pass_settings.genesis_enabled {
            sample_l = self.ym2612_gen_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_gen_low_pass_r.filter(sample_r);
        }

        (sample_l, sample_r)
    }

    #[must_use]
    pub fn filter_psg(&mut self, sample: f64) -> f64 {
        let sample = self.psg_dc_offset.filter(sample);

        if !self.low_pass_settings.genesis_enabled {
            return sample;
        }

        self.psg_low_pass.filter(sample)
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        let low_pass_settings = LowPassSettings::from_config(config);
        if self.low_pass_settings == low_pass_settings {
            return;
        }

        *self = Self::new(timing_mode, low_pass_settings);
    }
}

#[must_use]
pub fn ym2612_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 7.0 / 6.0 / 24.0
}

#[must_use]
pub fn psg_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 15.0 / 16.0
}

#[must_use]
pub fn volume_multiplier(enabled: bool, adjustment_db: f64) -> f64 {
    if !enabled {
        return 0.0;
    }

    // Decibels to linear
    10.0_f64.powf(adjustment_db / 20.0)
}
