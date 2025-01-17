//! Master System / Game Gear audio resampling code

pub mod constants;

use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
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

pub type PsgResampler = FirResampler<{ constants::PSG_LPF_TAPS }, 0>;

#[must_use]
pub fn new_psg_resampler(console_mclk_frequency: f64) -> PsgResampler {
    let psg_frequency = compute_psg_frequency(console_mclk_frequency);
    PsgResampler::new(
        psg_frequency,
        constants::PSG_SHARP_LPF_COEFFICIENTS,
        constants::PSG_HPF_CHARGE_FACTOR,
    )
}

fn compute_psg_frequency(console_mclk_frequency: f64) -> f64 {
    console_mclk_frequency / 15.0 / 16.0
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct AudioResampler {
    psg_resampler: PsgResampler,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        let psg_resampler = new_psg_resampler(timing_mode.mclk_frequency());
        Self { psg_resampler }
    }

    pub fn update_timing_mode(&mut self, timing_mode: TimingMode) {
        let psg_frequency = compute_psg_frequency(timing_mode.mclk_frequency());
        self.psg_resampler.update_source_frequency(psg_frequency);
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.psg_resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.psg_resampler.update_output_frequency(output_frequency);
    }
}
