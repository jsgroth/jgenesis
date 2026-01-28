mod constants;

use crate::api::GameBoyEmulatorConfig;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::FirstOrderIirFilter;
use dsp::sinc::PerformanceSincResampler;
use gb_config::GbAudioResampler;
use jgenesis_common::audio::fir_resampler::{FirKernel, LpfCoefficients, StereoFirResampler};
use jgenesis_common::frontend::AudioOutput;
use jgenesis_proc_macros::MatchEachVariantMacro;

pub const GB_APU_FREQUENCY: f64 = (1 << 21) as f64;

fn new_dc_offset_filter() -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, GB_APU_FREQUENCY, FilterType::HighPass)
}

#[derive(Debug, Clone, Encode, Decode)]
struct GbFirKernel;

impl FirKernel<{ constants::LPF_TAPS }> for GbFirKernel {
    #[inline]
    fn lpf_coefficients() -> &'static LpfCoefficients<{ constants::LPF_TAPS }> {
        constants::LPF_COEFFICIENTS
    }
}

type GbFirResampler = StereoFirResampler<{ constants::LPF_TAPS }, GbFirKernel>;

#[derive(Debug, Clone, Encode, Decode, MatchEachVariantMacro)]
enum ResamplerImpl {
    LowPassNearestNeighbor(GbFirResampler),
    WindowedSinc(PerformanceSincResampler<2>),
}

impl ResamplerImpl {
    fn collect(&mut self, samples: [f64; 2]) {
        match_each_variant!(self, resampler => resampler.collect(samples));
    }

    fn output_buffer_pop_front(&mut self) -> Option<[f64; 2]> {
        match_each_variant!(self, resampler => resampler.output_buffer_pop_front())
    }

    fn update_source_frequency(&mut self, source_frequency: f64) {
        match_each_variant!(self, resampler => resampler.update_source_frequency(source_frequency));
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        match_each_variant!(self, resampler => resampler.update_output_frequency(output_frequency as f64));
    }

    fn resampler_type(&self) -> GbAudioResampler {
        match self {
            Self::LowPassNearestNeighbor { .. } => GbAudioResampler::LowPassNearestNeighbor,
            Self::WindowedSinc { .. } => GbAudioResampler::WindowedSinc,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GameBoyResampler {
    dc_offset_l: FirstOrderIirFilter,
    dc_offset_r: FirstOrderIirFilter,
    resampler: ResamplerImpl,
    output_frequency: u64,
}

impl GameBoyResampler {
    pub fn new(config: &GameBoyEmulatorConfig) -> Self {
        let output_frequency = 48000;
        Self {
            dc_offset_l: new_dc_offset_filter(),
            dc_offset_r: new_dc_offset_filter(),
            resampler: create_resampler(config, output_frequency),
            output_frequency,
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler
            .collect([self.dc_offset_l.filter(sample_l), self.dc_offset_r.filter(sample_r)]);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: &GameBoyEmulatorConfig) {
        if config.audio_resampler != self.resampler.resampler_type() {
            log::info!("Changing resampler type to {:?}", config.audio_resampler);
            self.resampler = create_resampler(config, self.output_frequency);
        } else {
            self.resampler.update_source_frequency(gb_source_frequency(config.audio_60hz_hack));
        }
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
        self.output_frequency = output_frequency;
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

fn create_resampler(config: &GameBoyEmulatorConfig, output_frequency: u64) -> ResamplerImpl {
    let source_frequency = gb_source_frequency(config.audio_60hz_hack);

    match config.audio_resampler {
        GbAudioResampler::LowPassNearestNeighbor => ResamplerImpl::LowPassNearestNeighbor(
            GbFirResampler::new(source_frequency, output_frequency),
        ),
        GbAudioResampler::WindowedSinc => ResamplerImpl::WindowedSinc(
            PerformanceSincResampler::new(source_frequency, output_frequency as f64),
        ),
    }
}
