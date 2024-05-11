//! SNES audio resampling code

use crate::apu;
use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::AudioOutput;

const SNES_AUDIO_FREQUENCY: f64 = apu::OUTPUT_FREQUENCY as f64;

const LPF_COEFFICIENT_0: f64 = -0.001032167331725023;
const LPF_COEFFICIENTS: [f64; 21] = [
    -0.001032167331725023,
    -0.00337362854293201,
    -0.002300741105977643,
    0.007438828683983638,
    0.01718256624704002,
    0.002040390827841266,
    -0.04030652783842427,
    -0.05506118523737572,
    0.02814357569062969,
    0.2004791993149999,
    0.3467896892919401,
    0.3467896892919402,
    0.2004791993149999,
    0.02814357569062969,
    -0.05506118523737575,
    -0.04030652783842429,
    0.002040390827841267,
    0.01718256624704001,
    0.00743882868398364,
    -0.002300741105977646,
    -0.003373628542932013,
];

const HPF_CHARGE_FACTOR: f64 = 0.9946028448191855;

type SnesResampler = SignalResampler<21, 3>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    resampler: SnesResampler,
}

fn new_snes_resampler() -> SnesResampler {
    SnesResampler::new(SNES_AUDIO_FREQUENCY, LPF_COEFFICIENT_0, LPF_COEFFICIENTS, HPF_CHARGE_FACTOR)
}

impl AudioResampler {
    pub fn new() -> Self {
        Self { resampler: new_snes_resampler() }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
