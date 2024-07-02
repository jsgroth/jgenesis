//! SNES audio resampling code

use crate::apu;
use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::AudioOutput;

const SNES_AUDIO_FREQUENCY: f64 = apu::OUTPUT_FREQUENCY as f64;

const LPF_COEFFICIENT_0: f64 = -0.0009760874485880359;
const LPF_COEFFICIENTS: [f64; 47] = [
    -0.0009760874485880362,
    -0.001191618936665736,
    -0.00123977719260148,
    -0.0009670359927051247,
    -0.0001779539811170322,
    0.001234952541090226,
    0.003141366973564437,
    0.005076441670191683,
    0.006265430529146149,
    0.005803200699006192,
    0.002966554011724607,
    -0.002420998891123016,
    -0.009691884187766918,
    -0.01722780511799606,
    -0.02262775423885356,
    -0.0231602275849691,
    -0.01642000075282259,
    -0.001033465455283805,
    0.02278099955743031,
    0.05297400801372747,
    0.0858523270042171,
    0.1166883935878489,
    0.14063248239635,
    0.1537184527961952,
    0.1537184527961952,
    0.14063248239635,
    0.1166883935878489,
    0.08585232700421712,
    0.05297400801372749,
    0.02278099955743032,
    -0.001033465455283805,
    -0.01642000075282259,
    -0.0231602275849691,
    -0.02262775423885356,
    -0.01722780511799606,
    -0.009691884187766918,
    -0.002420998891123017,
    0.002966554011724607,
    0.005803200699006195,
    0.006265430529146155,
    0.005076441670191687,
    0.00314136697356444,
    0.001234952541090225,
    -0.0001779539811170324,
    -0.0009670359927051251,
    -0.001239777192601481,
    -0.001191618936665737,
];

const HPF_CHARGE_FACTOR: f64 = 0.9946028448191855;

type SnesResampler = SignalResampler<47, 3>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    resampler: SnesResampler,
}

fn new_snes_resampler() -> SnesResampler {
    SnesResampler::new(SNES_AUDIO_FREQUENCY, LPF_COEFFICIENT_0, LPF_COEFFICIENTS, HPF_CHARGE_FACTOR)
}

impl AudioResampler {
    pub fn new() -> Self {
        Self { resampler: new_snes_resampler() }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
