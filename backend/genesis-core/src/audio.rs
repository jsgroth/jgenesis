//! Genesis audio resampling, filtering, and mixing code

#![allow(clippy::excessive_precision)]

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;
use std::cmp;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

const YM2612_LPF_COEFFICIENT_0: f64 = -0.001478342773457343;
const YM2612_LPF_COEFFICIENTS: [f64; 25] = [
    -0.001478342773457343,
    -0.002579939173264984,
    -0.001815391014296705,
    0.003232249258559727,
    0.010914665789461,
    0.01180369689254257,
    -0.00423226347744078,
    -0.03255778315532309,
    -0.04631404301025462,
    -0.01139190330985419,
    0.08276070429927576,
    0.2033479308228996,
    0.2883104188511529,
    0.2883104188511529,
    0.2033479308228996,
    0.08276070429927578,
    -0.01139190330985419,
    -0.04631404301025461,
    -0.03255778315532309,
    -0.004232263477440783,
    0.01180369689254257,
    0.01091466578946099,
    0.00323224925855973,
    -0.001815391014296708,
    -0.002579939173264985,
];

const YM2612_HPF_CHARGE_FACTOR: f64 = 0.9966982656608827;

// -8dB (10 ^ -8/20)
pub const PSG_COEFFICIENT: f64 = 0.3981071705534972;

pub type Ym2612Resampler = SignalResampler<25, 2>;

#[must_use]
pub fn new_ym2612_resampler(genesis_mclk_frequency: f64) -> Ym2612Resampler {
    let ym2612_frequency = genesis_mclk_frequency / 7.0 / 6.0 / 24.0;
    Ym2612Resampler::new(
        ym2612_frequency,
        YM2612_LPF_COEFFICIENT_0,
        YM2612_LPF_COEFFICIENTS,
        YM2612_HPF_CHARGE_FACTOR,
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

        let ym2612_resampler = new_ym2612_resampler(genesis_mclk_frequency);
        let psg_resampler = smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency);

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
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
