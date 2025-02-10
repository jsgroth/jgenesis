//! SNES audio resampling code

use crate::apu;
use bincode::{Decode, Encode};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::sinc::QualitySincResampler;
use jgenesis_common::frontend::AudioOutput;

const SNES_AUDIO_FREQUENCY: f64 = apu::OUTPUT_FREQUENCY as f64;

fn new_dc_offset_filter() -> FirstOrderIirFilter {
    // Butterworth high-pass with 5 Hz cutoff frequency and 32040 Hz source frequency
    FirstOrderIirFilter::new(&[0.9995099791730125, -0.9995099791730125], &[1.0, -0.999019958346025])
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    dc_offset_l: FirstOrderIirFilter,
    dc_offset_r: FirstOrderIirFilter,
    resampler: QualitySincResampler<2>,
}

impl AudioResampler {
    pub fn new() -> Self {
        Self {
            dc_offset_l: new_dc_offset_filter(),
            dc_offset_r: new_dc_offset_filter(),
            resampler: QualitySincResampler::new(SNES_AUDIO_FREQUENCY, 48000.0),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l = self.dc_offset_l.filter(sample_l);
        let sample_r = self.dc_offset_r.filter(sample_r);
        self.resampler.collect([sample_l, sample_r]);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }
}
