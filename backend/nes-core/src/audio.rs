#![allow(clippy::excessive_precision)]

mod constants;

use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
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

type NesResampler = FirResampler<{ constants::LPF_TAPS }, 0>;

fn new_nes_resampler(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> NesResampler {
    let source_frequency = compute_source_frequency(timing_mode, apply_refresh_rate_adjustment);
    NesResampler::new(source_frequency, constants::LPF_COEFFICIENTS, constants::HPF_CHARGE_FACTOR)
}

fn compute_source_frequency(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> f64 {
    let refresh_rate_multiplier = if apply_refresh_rate_adjustment {
        timing_mode.refresh_rate_multiplier() * 60.0 / timing_mode.nes_native_display_rate()
    } else {
        1.0
    };

    timing_mode.nes_audio_frequency() * refresh_rate_multiplier
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    timing_mode: TimingMode,
    resampler: NesResampler,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> Self {
        Self {
            timing_mode,
            resampler: new_nes_resampler(timing_mode, apply_refresh_rate_adjustment),
        }
    }

    pub fn collect_sample(&mut self, sample: f64) {
        self.resampler.collect_sample(sample, sample);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
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
        self.resampler.update_output_frequency(output_frequency);
    }
}
