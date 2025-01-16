//! Genesis audio resampling, filtering, and mixing code

mod constants;

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use smsgg_core::audio::PsgResampler;
use std::cmp;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

// -7dB (10 ^ -7/20)
pub const PSG_COEFFICIENT: f64 = 0.44668359215096315;

pub type Ym2612Resampler =
    FirResampler<{ constants::YM2612_LPF_TAPS }, { constants::YM2612_ZERO_PADDING }>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumAll)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum LowPassFilter {
    // Roughly 15000 Hz cutoff
    Sharp,
    // Roughly 10000 Hz cutoff
    #[default]
    Moderate,
    // Roughly 7500 Hz cutoff
    Soft,
    // Roughly 5000 Hz cutoff
    VerySoft,
    // Roughly 3390 Hz cutoff
    Hardware,
}

impl LowPassFilter {
    #[inline]
    #[must_use]
    pub fn psg_coefficients(self) -> &'static [f64; smsgg_core::audio::constants::PSG_LPF_TAPS] {
        match self {
            Self::Sharp => &smsgg_core::audio::constants::PSG_SHARP_LPF_COEFFICIENTS,
            Self::Moderate => &smsgg_core::audio::constants::PSG_MID_LPF_COEFFICIENTS,
            Self::Soft => &smsgg_core::audio::constants::PSG_SOFT_LPF_COEFFICIENTS,
            Self::VerySoft => &smsgg_core::audio::constants::PSG_VSOFT_LPF_COEFFICIENTS,
            Self::Hardware => &smsgg_core::audio::constants::PSG_HARDWARE_LPF_COEFFICIENTS,
        }
    }

    #[inline]
    #[must_use]
    pub fn ym2612_coefficients(self) -> &'static [f64; constants::YM2612_LPF_TAPS] {
        match self {
            Self::Sharp => &constants::YM2612_SHARP_LPF_COEFFICIENTS,
            Self::Moderate => &constants::YM2612_MID_LPF_COEFFICIENTS,
            Self::Soft => &constants::YM2612_SOFT_LPF_COEFFICIENTS,
            Self::VerySoft => &constants::YM2612_VSOFT_LPF_COEFFICIENTS,
            Self::Hardware => &constants::YM2612_HARDWARE_LPF_COEFFICIENTS,
        }
    }
}

#[must_use]
pub fn new_ym2612_resampler(genesis_mclk_frequency: f64, lpf: LowPassFilter) -> Ym2612Resampler {
    let ym2612_frequency = genesis_mclk_frequency / 7.0 / 6.0 / 24.0;
    Ym2612Resampler::new(
        ym2612_frequency,
        *lpf.ym2612_coefficients(),
        constants::YM2612_HPF_CHARGE_FACTOR,
    )
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    ym2612_resampler: Ym2612Resampler,
    psg_resampler: PsgResampler,
    ym2612_enabled: bool,
    psg_enabled: bool,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: GenesisEmulatorConfig) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        let lpf = config.low_pass_filter;
        let ym2612_resampler = new_ym2612_resampler(genesis_mclk_frequency, lpf);
        let psg_resampler =
            smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency, *lpf.psg_coefficients());

        Self {
            ym2612_resampler,
            psg_resampler,
            ym2612_enabled: config.ym2612_enabled,
            psg_enabled: config.psg_enabled,
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
    }

    /// Push all samples that are ready to the given audio output.
    ///
    /// # Errors
    ///
    /// Will propagate any error returned by the audio output while pushing samples.
    // Should never panic, the unwraps are guarded by length checks
    #[allow(clippy::missing_panics_doc)]
    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
        );

        for _ in 0..sample_count {
            let (ym2612_l, ym2612_r) = check_enabled(
                self.ym2612_resampler.output_buffer_pop_front().unwrap(),
                self.ym2612_enabled,
            );
            let (psg_l, psg_r) = check_enabled(
                self.psg_resampler.output_buffer_pop_front().unwrap(),
                self.psg_enabled,
            );

            let sample_l = (ym2612_l + PSG_COEFFICIENT * psg_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + PSG_COEFFICIENT * psg_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: GenesisEmulatorConfig) {
        self.ym2612_enabled = config.ym2612_enabled;
        self.psg_enabled = config.psg_enabled;

        let lpf = config.low_pass_filter;
        self.ym2612_resampler.update_lpf_coefficients(*lpf.ym2612_coefficients());
        self.psg_resampler.update_lpf_coefficients(*lpf.psg_coefficients());
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
