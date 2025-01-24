#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};

// 236.25MHz / 11 / 12
const NTSC_NES_AUDIO_FREQUENCY: f64 = 1789772.7272727272727273;
pub const NTSC_NES_NATIVE_DISPLAY_RATE: f64 = 60.0988;

// 26.6017125.MHz / 16
const PAL_NES_AUDIO_FREQUENCY: f64 = 1662607.03125;
pub const PAL_NES_NATIVE_DISPLAY_RATE: f64 = 50.0070;

trait TimingModeAudioExt {
    fn nes_audio_frequency(self) -> f64;

    fn nes_native_display_rate(self) -> f64;

    fn refresh_rate_multiplier(self) -> f64;
}

impl TimingModeAudioExt for TimingMode {
    fn nes_audio_frequency(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_AUDIO_FREQUENCY,
            Self::Pal => PAL_NES_AUDIO_FREQUENCY,
        }
    }

    fn nes_native_display_rate(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_NATIVE_DISPLAY_RATE,
            Self::Pal => PAL_NES_NATIVE_DISPLAY_RATE,
        }
    }

    fn refresh_rate_multiplier(self) -> f64 {
        match self {
            Self::Ntsc => 1.0,
            Self::Pal => 50.0 / 60.0,
        }
    }
}

fn compute_source_frequency(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> f64 {
    let refresh_rate_multiplier = if apply_refresh_rate_adjustment {
        timing_mode.refresh_rate_multiplier() * 60.0 / timing_mode.nes_native_display_rate()
    } else {
        1.0
    };

    timing_mode.nes_audio_frequency() * refresh_rate_multiplier
}

fn new_dc_offset_filter() -> FirstOrderIirFilter {
    // Butterworth high-pass with cutoff frequency 5 Hz, source frequency 1789772 Hz
    FirstOrderIirFilter::new(&[0.9999912235642162, -0.9999912235642162], &[
        1.0,
        -0.9999824471284324,
    ])
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    timing_mode: TimingMode,
    dc_offset_filter: FirstOrderIirFilter,
    resampler: PerformanceSincResampler<1>,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> Self {
        Self {
            timing_mode,
            dc_offset_filter: new_dc_offset_filter(),
            resampler: PerformanceSincResampler::new(
                compute_source_frequency(timing_mode, apply_refresh_rate_adjustment),
                48000.0,
            ),
        }
    }

    pub fn collect_sample(&mut self, sample: f64) {
        let sample = self.dc_offset_filter.filter(sample);
        self.resampler.collect([sample]);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample, sample)?;
        }

        Ok(())
    }

    pub fn set_apply_refresh_rate_adjustment(&mut self, apply_refresh_rate_adjustment: bool) {
        self.resampler.update_source_frequency(compute_source_frequency(
            self.timing_mode,
            apply_refresh_rate_adjustment,
        ));
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }
}
