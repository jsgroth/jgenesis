mod constants;

use crate::apu::PwmCycleShift;
use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
use jgenesis_common::frontend::AudioOutput;

type GbaResampler = FirResampler<{ constants::LPF_TAPS }>;

impl PwmCycleShift {
    const fn zero_padding(self) -> u32 {
        (Self::Six.sample_rate_hz() / self.sample_rate_hz()) - 1
    }
}

const DEFAULT_ZERO_PADDING: u32 = PwmCycleShift::Nine.zero_padding();

#[derive(Debug, Clone, Encode, Decode)]
pub struct GbaAudioResampler {
    resampler: GbaResampler,
    last_output: (f64, f64),
}

impl GbaAudioResampler {
    pub fn new() -> Self {
        Self {
            resampler: GbaResampler::new(
                constants::SOURCE_FREQUENCY,
                constants::LPF_COEFFICIENTS,
                // TODO high-pass filter
                1.0,
                DEFAULT_ZERO_PADDING,
            ),
            last_output: (0.0, 0.0),
        }
    }

    pub fn change_cycle_shift(&mut self, cycle_shift: PwmCycleShift) {
        // Hack: When the sample rate changes, fill the resampler's input buffer with the last
        // output sample that it generated, divided by the new zero padding value (plus one).
        // This is to avoid volume spikes caused by changing the zero padding, particularly when
        // increasing it.
        // This code path should not be triggered often because games should not frequently change
        // the sample rate.
        let zero_padding = cycle_shift.zero_padding();
        let last_output_divisor = f64::from(zero_padding + 1);
        self.resampler.fill_input_buffer_with((
            self.last_output.0 / last_output_divisor,
            self.last_output.1 / last_output_divisor,
        ));
        self.resampler.update_zero_padding(zero_padding);
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
    }

    pub fn drain_output<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
            self.last_output = (sample_l, sample_r);
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}
