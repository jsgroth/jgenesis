use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::FirstOrderIirFilter;
use dsp::sinc::QualitySincResampler;
use jgenesis_common::frontend::AudioOutput;

#[derive(Debug, Clone, Encode, Decode)]
pub struct GbaAudioResampler {
    dc_offset_l: FirstOrderIirFilter,
    dc_offset_r: FirstOrderIirFilter,
    resampler: QualitySincResampler<2>,
    source_frequency: u64,
}

fn new_dc_offset_filter(source_frequency: f64) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, source_frequency, FilterType::HighPass)
}

impl GbaAudioResampler {
    pub fn new() -> Self {
        const DEFAULT_SOURCE_FREQUENCY: f64 = 32768.0;

        Self {
            dc_offset_l: new_dc_offset_filter(DEFAULT_SOURCE_FREQUENCY),
            dc_offset_r: new_dc_offset_filter(DEFAULT_SOURCE_FREQUENCY),
            resampler: QualitySincResampler::new(DEFAULT_SOURCE_FREQUENCY, 48000.0),
            source_frequency: DEFAULT_SOURCE_FREQUENCY as u64,
        }
    }

    pub fn update_source_frequency(&mut self, source_frequency: u64) {
        self.resampler.update_source_frequency(source_frequency as f64);

        // TODO would be better to initialize these to the last sample to avoid a pop when frequency changes
        if source_frequency != self.source_frequency {
            self.dc_offset_l = new_dc_offset_filter(source_frequency as f64);
            self.dc_offset_r = new_dc_offset_filter(source_frequency as f64);
        }
        self.source_frequency = source_frequency;
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l = self.dc_offset_l.filter(sample_l);
        let sample_r = self.dc_offset_r.filter(sample_r);

        self.resampler.collect([sample_l, sample_r]);
    }

    pub fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
