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

const YM2612_LPF_COEFFICIENT_0: f64 = -0.0007538050296794788;
const YM2612_LPF_COEFFICIENTS: [f64; 59] = [
    -0.0007538050296794789,
    -0.0009176788275392454,
    -0.001011355455685635,
    -0.0009840966912578936,
    -0.0007570500937125532,
    -0.0002483168503789343,
    0.0005891318158791983,
    0.001726592427101347,
    0.003028608542441683,
    0.004244349041460328,
    0.005030288051779324,
    0.005006520635447458,
    0.00384084100038095,
    0.001346660186996573,
    -0.002425114538825101,
    -0.007122335506687448,
    -0.01207564225306805,
    -0.01634277811470872,
    -0.01881498824470909,
    -0.01837489219886146,
    -0.01408360032564335,
    -0.005366472166730043,
    0.007836222332174221,
    0.02498434333862389,
    0.04493999288731917,
    0.0660641400684459,
    0.08639899696994358,
    0.1039118754953764,
    0.1167644055583891,
    0.1235651579457279,
    0.1235651579457279,
    0.1167644055583891,
    0.1039118754953764,
    0.08639899696994358,
    0.06606414006844588,
    0.04493999288731919,
    0.02498434333862389,
    0.007836222332174222,
    -0.005366472166730044,
    -0.01408360032564336,
    -0.01837489219886147,
    -0.0188149882447091,
    -0.01634277811470872,
    -0.01207564225306806,
    -0.007122335506687449,
    -0.002425114538825101,
    0.001346660186996574,
    0.003840841000380949,
    0.005006520635447462,
    0.005030288051779326,
    0.004244349041460329,
    0.003028608542441687,
    0.001726592427101347,
    0.0005891318158791985,
    -0.0002483168503789345,
    -0.0007570500937125534,
    -0.0009840966912578934,
    -0.001011355455685636,
    -0.0009176788275392455,
];

const YM2612_HPF_CHARGE_FACTOR: f64 = 0.9966982656608827;

// -7dB (10 ^ -7/20)
pub const PSG_COEFFICIENT: f64 = 0.44668359215096315;

pub type Ym2612Resampler = SignalResampler<59, 2>;

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

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
