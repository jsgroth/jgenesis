mod constants;

use crate::api::PceEmulatorConfig;
use crate::cd;
use crate::psg::PSG_FREQUENCY;
use bincode::{Decode, Encode};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use jgenesis_common::audio::fir_resampler::{FirKernel, LpfCoefficients, StereoFirResampler};
use jgenesis_common::frontend::AudioOutput;
use pce_config::PcePsgResampler;
use std::array;

const CD_DA_FREQUENCY: f64 = 44100.0;

#[derive(Debug, Clone, Encode, Decode)]
pub struct PsgLpfKernel;

impl FirKernel<{ constants::LPF_TAPS }> for PsgLpfKernel {
    fn lpf_coefficients() -> &'static LpfCoefficients<{ constants::LPF_TAPS }> {
        constants::LPF_COEFFICIENTS
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum PsgResampler {
    LowPassNearestNeighbor(StereoFirResampler<{ constants::LPF_TAPS }, PsgLpfKernel>),
    WindowedSinc(PerformanceSincResampler<2>),
}

impl PsgResampler {
    fn new(resampler: PcePsgResampler, output_frequency: u64) -> Self {
        match resampler {
            PcePsgResampler::WindowedSinc => Self::WindowedSinc(PerformanceSincResampler::new(
                PSG_FREQUENCY,
                output_frequency as f64,
            )),
            PcePsgResampler::LowPassNearestNeighbor => Self::LowPassNearestNeighbor(
                StereoFirResampler::new(PSG_FREQUENCY, output_frequency),
            ),
        }
    }

    fn collect(&mut self, sample: [f64; 2]) {
        match self {
            Self::LowPassNearestNeighbor(resampler) => resampler.collect(sample),
            Self::WindowedSinc(resampler) => resampler.collect(sample),
        }
    }

    fn output_buffer_len(&self) -> usize {
        match self {
            Self::LowPassNearestNeighbor(resampler) => resampler.output_buffer_len(),
            Self::WindowedSinc(resampler) => resampler.output_buffer_len(),
        }
    }

    fn output_buffer_pop_front(&mut self) -> Option<[f64; 2]> {
        match self {
            Self::LowPassNearestNeighbor(resampler) => resampler.output_buffer_pop_front(),
            Self::WindowedSinc(resampler) => resampler.output_buffer_pop_front(),
        }
    }

    fn update_output_frequency(&mut self, output_frequency: f64) {
        match self {
            Self::LowPassNearestNeighbor(resampler) => {
                resampler.update_output_frequency(output_frequency);
            }
            Self::WindowedSinc(resampler) => resampler.update_output_frequency(output_frequency),
        }
    }

    fn resampler_impl(&self) -> PcePsgResampler {
        match self {
            Self::LowPassNearestNeighbor(..) => PcePsgResampler::LowPassNearestNeighbor,
            Self::WindowedSinc(..) => PcePsgResampler::WindowedSinc,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    psg: f64,
    cd_da: f64,
    adpcm: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &PceEmulatorConfig) -> Self {
        Self {
            psg: f64::from(config.psg_enabled)
                * decibels_to_linear(config.psg_volume_adjustment_db),
            cd_da: f64::from(config.cd_da_enabled)
                * decibels_to_linear(config.cd_da_volume_adjustment_db),
            adpcm: f64::from(config.adpcm_enabled)
                * decibels_to_linear(config.adpcm_volume_adjustment_db),
        }
    }
}

fn decibels_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PceAudioResampler {
    psg: PsgResampler,
    cd_da: QualitySincResampler<2>,
    adpcm: QualitySincResampler<1>,
    cd_present: bool,
    output_frequency: u64,
    volumes: VolumeMultipliers,
}

impl PceAudioResampler {
    pub fn new(config: &PceEmulatorConfig, cd_present: bool, output_frequency: u64) -> Self {
        Self {
            psg: PsgResampler::new(config.psg_audio_resampler, output_frequency),
            cd_da: QualitySincResampler::new(CD_DA_FREQUENCY, output_frequency as f64),
            adpcm: QualitySincResampler::new(cd::ADPCM_SAMPLE_RATE, output_frequency as f64),
            cd_present,
            output_frequency,
            volumes: VolumeMultipliers::from_config(config),
        }
    }

    pub fn collect_psg(&mut self, sample: [f64; 2]) {
        self.psg.collect(sample);
    }

    pub fn collect_cd_da(&mut self, sample: [f64; 2]) {
        self.cd_da.collect(sample);
    }

    pub fn collect_adpcm(&mut self, sample: f64) {
        self.adpcm.collect([sample]);
    }

    pub fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        let output_buffer_len = if self.cd_present {
            [
                self.psg.output_buffer_len(),
                self.cd_da.output_buffer_len(),
                self.adpcm.output_buffer_len(),
            ]
            .into_iter()
            .min()
            .unwrap()
        } else {
            self.psg.output_buffer_len()
        };

        for _ in 0..output_buffer_len {
            let psg_sample = self.psg.output_buffer_pop_front().unwrap();

            let cd_da_sample = if self.cd_present {
                self.cd_da.output_buffer_pop_front().unwrap()
            } else {
                [0.0; 2]
            };

            let [adpcm_sample] =
                if self.cd_present { self.adpcm.output_buffer_pop_front().unwrap() } else { [0.0] };

            let mixed_sample: [f64; 2] = array::from_fn(|i| {
                (psg_sample[i] * self.volumes.psg
                    + cd_da_sample[i] * self.volumes.cd_da
                    + adpcm_sample * self.volumes.adpcm)
                    .clamp(-1.0, 1.0)
            });
            audio_output.push_sample(mixed_sample[0], mixed_sample[1])?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.output_frequency = output_frequency;

        self.psg.update_output_frequency(output_frequency as f64);
        self.cd_da.update_output_frequency(output_frequency as f64);
        self.adpcm.update_output_frequency(output_frequency as f64);
    }

    pub fn reload_config(&mut self, config: &PceEmulatorConfig) {
        if config.psg_audio_resampler != self.psg.resampler_impl() {
            self.psg = PsgResampler::new(config.psg_audio_resampler, self.output_frequency);
        }

        self.volumes = VolumeMultipliers::from_config(config);
    }
}
