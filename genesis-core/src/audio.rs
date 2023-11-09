//! Genesis audio resampling, filtering, and mixing code

#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
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

const PSG_LPF_COEFFICIENT_0: f64 = -0.001070923693774405;
const PSG_LPF_COEFFICIENTS: [f64; 35] = [
    -0.001070923693774405,
    -0.001685350075225726,
    -0.001863413379276681,
    -0.0009568415472585112,
    0.001640749268572869,
    0.005567205576795833,
    0.00885183238540499,
    0.008348379350607549,
    0.00146034668504318,
    -0.0115355816923585,
    -0.02594911574769958,
    -0.03323373392428908,
    -0.02401612130459429,
    0.007419221555475174,
    0.05927546269924155,
    0.1211627893325687,
    0.1768103438836072,
    0.2097747506271597,
    0.2097747506271597,
    0.1768103438836073,
    0.1211627893325687,
    0.05927546269924156,
    0.007419221555475176,
    -0.02401612130459429,
    -0.03323373392428909,
    -0.02594911574769959,
    -0.0115355816923585,
    0.001460346685043181,
    0.008348379350607551,
    0.00885183238540499,
    0.005567205576795835,
    0.00164074926857287,
    -0.0009568415472585116,
    -0.001863413379276682,
    -0.001685350075225726,
];

const PSG_HPF_CHARGE_FACTOR: f64 = 0.999212882632514;

// -8dB (10 ^ -8/20)
pub const PSG_COEFFICIENT: f64 = 0.3981071705534972;

pub type Ym2612Resampler = SignalResampler<25, 2>;
pub type PsgResampler = SignalResampler<35, 0>;

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

#[must_use]
pub fn new_psg_resampler(genesis_mclk_frequency: f64) -> PsgResampler {
    let psg_frequency = genesis_mclk_frequency / 15.0 / 16.0;
    PsgResampler::new(
        psg_frequency,
        PSG_LPF_COEFFICIENT_0,
        PSG_LPF_COEFFICIENTS,
        PSG_HPF_CHARGE_FACTOR,
    )
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    ym2612_resampler: Ym2612Resampler,
    psg_resampler: PsgResampler,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        let ym2612_resampler = new_ym2612_resampler(genesis_mclk_frequency);
        let psg_resampler = new_psg_resampler(genesis_mclk_frequency);

        Self { ym2612_resampler, psg_resampler }
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
            let (ym2612_l, ym2612_r) = self.ym2612_resampler.output_buffer_pop_front().unwrap();
            let (psg_l, psg_r) = self.psg_resampler.output_buffer_pop_front().unwrap();

            let sample_l = (ym2612_l + PSG_COEFFICIENT * psg_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + PSG_COEFFICIENT * psg_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
