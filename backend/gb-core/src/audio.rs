use bincode::{Decode, Encode};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::AudioOutput;

pub const GB_APU_FREQUENCY: f64 = 1_048_576.0;

fn new_dc_offset_filter() -> FirstOrderIirFilter {
    // Butterworth high-pass with cutoff frequency 5 Hz and source frequency 1048576 Hz
    FirstOrderIirFilter::new(&[0.9999850199432726, -0.9999850199432726], &[
        1.0,
        -0.9999700398865453,
    ])
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GameBoyResampler {
    dc_offset_filter_l: FirstOrderIirFilter,
    dc_offset_filter_r: FirstOrderIirFilter,
    resampler: PerformanceSincResampler<2>,
}

impl GameBoyResampler {
    pub fn new(audio_60hz_hack: bool) -> Self {
        Self {
            dc_offset_filter_l: new_dc_offset_filter(),
            dc_offset_filter_r: new_dc_offset_filter(),
            resampler: PerformanceSincResampler::new(gb_source_frequency(audio_60hz_hack), 48000.0),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l = self.dc_offset_filter_l.filter(sample_l);
        let sample_r = self.dc_offset_filter_r.filter(sample_r);
        self.resampler.collect([sample_l, sample_r]);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_audio_60hz_hack(&mut self, audio_60hz_hack: bool) {
        self.resampler.update_source_frequency(gb_source_frequency(audio_60hz_hack));
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }
}

fn gb_source_frequency(audio_60hz_hack: bool) -> f64 {
    if audio_60hz_hack {
        // The Game Boy's precise refresh rate is 4.194304 MHz / (154 lines * 456 cycles/line)
        // which is approximately 59.73 Hz.
        // To target 60 FPS, pretend the APU is (60 / ~59.73) faster
        GB_APU_FREQUENCY * 60.0 / (4_194_304.0 / (154.0 * 456.0))
    } else {
        GB_APU_FREQUENCY
    }
}
