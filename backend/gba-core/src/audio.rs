mod constants;

use crate::apu::PwmCycleShift;
use bincode::{Decode, Encode};
use jgenesis_common::audio::sinc::{QualitySincResampler, SincResampler};
use jgenesis_common::frontend::AudioOutput;

impl PwmCycleShift {
    const fn zero_padding(self) -> u32 {
        (Self::Six.sample_rate_hz() / self.sample_rate_hz()) - 1
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GbaAudioResampler {
    resampler: QualitySincResampler<2>,
    last_output: (f64, f64),
}

impl GbaAudioResampler {
    pub fn new() -> Self {
        Self { resampler: SincResampler::new(f64::from(1 << 15), 48000.0), last_output: (0.0, 0.0) }
    }

    pub fn change_cycle_shift(&mut self, cycle_shift: PwmCycleShift) {
        self.resampler.update_source_frequency(cycle_shift.sample_rate_hz().into());
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect([sample_l, sample_r]);
    }

    pub fn drain_output<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
            self.last_output = (sample_l, sample_r);
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }
}
