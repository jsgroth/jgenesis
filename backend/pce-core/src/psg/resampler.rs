mod constants;

use crate::psg::PSG_FREQUENCY;
use bincode::{Decode, Encode};
use dsp::sinc::PerformanceSincResampler;
use jgenesis_common::audio::fir_resampler::{FirKernel, LpfCoefficients, StereoFirResampler};
use pce_config::PceAudioResampler;

#[derive(Debug, Clone, Encode, Decode)]
pub struct PsgLpfKernel;

impl FirKernel<{ constants::LPF_TAPS }> for PsgLpfKernel {
    fn lpf_coefficients() -> &'static LpfCoefficients<{ constants::LPF_TAPS }> {
        constants::LPF_COEFFICIENTS
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum PsgResampler {
    LowPassNearestNeighbor(StereoFirResampler<{ constants::LPF_TAPS }, PsgLpfKernel>),
    WindowedSinc(PerformanceSincResampler<2>),
}

impl PsgResampler {
    pub fn new(resampler: PceAudioResampler, output_frequency: u64) -> Self {
        match resampler {
            PceAudioResampler::WindowedSinc => Self::WindowedSinc(PerformanceSincResampler::new(
                PSG_FREQUENCY,
                output_frequency as f64,
            )),
            PceAudioResampler::LowPassNearestNeighbor => Self::LowPassNearestNeighbor(
                StereoFirResampler::new(PSG_FREQUENCY, output_frequency),
            ),
        }
    }

    pub fn collect(&mut self, sample: [f64; 2]) {
        match self {
            Self::LowPassNearestNeighbor(resampler) => resampler.collect(sample),
            Self::WindowedSinc(resampler) => resampler.collect(sample),
        }
    }

    pub fn output_buffer_pop_front(&mut self) -> Option<[f64; 2]> {
        match self {
            Self::LowPassNearestNeighbor(resampler) => resampler.output_buffer_pop_front(),
            Self::WindowedSinc(resampler) => resampler.output_buffer_pop_front(),
        }
    }

    pub fn update_output_frequency(&mut self, output_frequency: f64) {
        match self {
            Self::LowPassNearestNeighbor(resampler) => {
                resampler.update_output_frequency(output_frequency);
            }
            Self::WindowedSinc(resampler) => resampler.update_output_frequency(output_frequency),
        }
    }

    pub fn resampler_impl(&self) -> PceAudioResampler {
        match self {
            Self::LowPassNearestNeighbor(..) => PceAudioResampler::LowPassNearestNeighbor,
            Self::WindowedSinc(..) => PceAudioResampler::WindowedSinc,
        }
    }
}
