//! SNES audio resampling code

mod constants;

use crate::apu;
use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
use jgenesis_common::frontend::AudioOutput;

const SNES_AUDIO_FREQUENCY: f64 = apu::OUTPUT_FREQUENCY as f64;

type SnesResampler = FirResampler<{ constants::LPF_TAPS }, { constants::ZERO_PADDING }>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    resampler: SnesResampler,
}

fn new_snes_resampler() -> SnesResampler {
    SnesResampler::new(
        SNES_AUDIO_FREQUENCY,
        constants::LPF_COEFFICIENTS,
        constants::HPF_CHARGE_FACTOR,
    )
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

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}
