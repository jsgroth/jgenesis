//! Master System / Game Gear audio resampling code

#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};

const NTSC_MCLK_FREQUENCY: f64 = 53_693_175.0;
const PAL_MCLK_FREQUENCY: f64 = 53_203_424.0;

trait TimingModeExt {
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

const PSG_LPF_COEFFICIENT_0: f64 = -0.001070923693774405;
const PSG_LPF_COEFFICIENTS: [f64; 35] = [
    -0.001070923693774405,
    -0.001685350075225726,
    -0.001863413379276681,
    -0.0009568415472585112,
    0.001640749268572869,
    0.005567205576795833,
    0.00885183238540499,
    0.008348379350607549,
    0.00146034668504318,
    -0.0115355816923585,
    -0.02594911574769958,
    -0.03323373392428908,
    -0.02401612130459429,
    0.007419221555475174,
    0.05927546269924155,
    0.1211627893325687,
    0.1768103438836072,
    0.2097747506271597,
    0.2097747506271597,
    0.1768103438836073,
    0.1211627893325687,
    0.05927546269924156,
    0.007419221555475176,
    -0.02401612130459429,
    -0.03323373392428909,
    -0.02594911574769959,
    -0.0115355816923585,
    0.001460346685043181,
    0.008348379350607551,
    0.00885183238540499,
    0.005567205576795835,
    0.00164074926857287,
    -0.0009568415472585116,
    -0.001863413379276682,
    -0.001685350075225726,
];

const PSG_HPF_CHARGE_FACTOR: f64 = 0.999212882632514;

pub type PsgResampler = SignalResampler<35, 0>;

#[must_use]
pub fn new_psg_resampler(console_mclk_frequency: f64) -> PsgResampler {
    let psg_frequency = compute_psg_frequency(console_mclk_frequency);
    PsgResampler::new(
        psg_frequency,
        PSG_LPF_COEFFICIENT_0,
        PSG_LPF_COEFFICIENTS,
        PSG_HPF_CHARGE_FACTOR,
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
        self.psg_resampler.update_source_frequency(timing_mode.mclk_frequency());
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
}
