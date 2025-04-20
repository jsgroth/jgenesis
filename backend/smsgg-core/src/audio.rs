//! Master System / Game Gear audio resampling code

use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::FirstOrderIirFilter;
use dsp::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};

pub const NTSC_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_MCLK_FREQUENCY: f64 = 53_203_424.0;

pub(crate) trait TimingModeExt {
    fn mclk_frequency(self) -> f64;
}

impl TimingModeExt for TimingMode {
    fn mclk_frequency(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_MCLK_FREQUENCY,
            Self::Pal => PAL_MCLK_FREQUENCY,
        }
    }
}

fn compute_psg_frequency(console_mclk_frequency: f64) -> f64 {
    console_mclk_frequency / 15.0 / 16.0
}

#[must_use]
pub fn new_psg_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(
        5.0,
        compute_psg_frequency(timing_mode.mclk_frequency()),
        FilterType::HighPass,
    )
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct AudioResampler {
    dc_offset_l: FirstOrderIirFilter,
    dc_offset_r: FirstOrderIirFilter,
    psg_resampler: PerformanceSincResampler<2>,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            dc_offset_l: new_psg_dc_offset(timing_mode),
            dc_offset_r: new_psg_dc_offset(timing_mode),
            psg_resampler: PerformanceSincResampler::new(
                compute_psg_frequency(timing_mode.mclk_frequency()),
                48000.0,
            ),
        }
    }

    pub fn update_timing_mode(&mut self, timing_mode: TimingMode) {
        let psg_frequency = compute_psg_frequency(timing_mode.mclk_frequency());
        self.psg_resampler.update_source_frequency(psg_frequency);
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l = self.dc_offset_l.filter(sample_l);
        let sample_r = self.dc_offset_r.filter(sample_r);
        self.psg_resampler.collect([sample_l, sample_r]);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.psg_resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.psg_resampler.update_output_frequency(output_frequency as f64);
    }
}
