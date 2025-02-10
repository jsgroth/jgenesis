#![allow(clippy::excessive_precision)]

mod constants;

use crate::api::{NesAudioResampler, NesEmulatorConfig};
use bincode::{Decode, Encode};
use jgenesis_common::audio::fir_resampler::{FirKernel, LpfCoefficients, MonoFirResampler};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::sinc::PerformanceSincResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use jgenesis_proc_macros::MatchEachVariantMacro;

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
    FirstOrderIirFilter::new(
        &[0.9999912235642162, -0.9999912235642162],
        &[1.0, -0.9999824471284324],
    )
}

#[derive(Debug, Clone, Encode, Decode)]
struct NesFirKernel;

impl FirKernel<{ constants::LPF_TAPS }> for NesFirKernel {
    #[inline]
    fn lpf_coefficients() -> &'static LpfCoefficients<{ constants::LPF_TAPS }> {
        constants::LPF_COEFFICIENTS
    }
}

type NesFirResampler = MonoFirResampler<{ constants::LPF_TAPS }, NesFirKernel>;

#[derive(Debug, Clone, Encode, Decode, MatchEachVariantMacro)]
enum ResamplerImpl {
    LowPassNearestNeighbor(NesFirResampler),
    WindowedSinc(PerformanceSincResampler<1>),
}

impl ResamplerImpl {
    fn collect(&mut self, sample: f64) {
        match_each_variant!(self, resampler => resampler.collect([sample]));
    }

    fn output_buffer_pop_front(&mut self) -> Option<[f64; 1]> {
        match_each_variant!(self, resampler => resampler.output_buffer_pop_front())
    }

    fn update_source_frequency(&mut self, source_frequency: f64) {
        match_each_variant!(self, resampler => resampler.update_source_frequency(source_frequency));
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        match_each_variant!(self, resampler => resampler.update_output_frequency(output_frequency as f64));
    }

    fn resampler_type(&self) -> NesAudioResampler {
        match self {
            Self::LowPassNearestNeighbor { .. } => NesAudioResampler::LowPassNearestNeighbor,
            Self::WindowedSinc { .. } => NesAudioResampler::WindowedSinc,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    timing_mode: TimingMode,
    dc_offset_filter: FirstOrderIirFilter,
    resampler: ResamplerImpl,
    output_frequency: u64,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, config: &NesEmulatorConfig) -> Self {
        let source_frequency =
            compute_source_frequency(timing_mode, config.audio_refresh_rate_adjustment);
        let output_frequency = 48000;
        let resampler =
            create_audio_resampler(config.audio_resampler, source_frequency, output_frequency);

        Self { timing_mode, dc_offset_filter: new_dc_offset_filter(), resampler, output_frequency }
    }

    pub fn collect_sample(&mut self, sample: f64) {
        self.resampler.collect(self.dc_offset_filter.filter(sample));
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some([sample]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample, sample)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: &NesEmulatorConfig) {
        let source_frequency =
            compute_source_frequency(self.timing_mode, config.audio_refresh_rate_adjustment);
        if self.resampler.resampler_type() != config.audio_resampler {
            log::info!("Changing resampler type to {:?}", config.audio_resampler);
            self.resampler = create_audio_resampler(
                config.audio_resampler,
                source_frequency,
                self.output_frequency,
            );
        } else {
            self.resampler.update_source_frequency(source_frequency);
        }
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
        self.output_frequency = output_frequency;
    }
}

fn create_audio_resampler(
    resampler_type: NesAudioResampler,
    source_frequency: f64,
    output_frequency: u64,
) -> ResamplerImpl {
    match resampler_type {
        NesAudioResampler::LowPassNearestNeighbor => ResamplerImpl::LowPassNearestNeighbor(
            NesFirResampler::new(source_frequency, output_frequency),
        ),
        NesAudioResampler::WindowedSinc => ResamplerImpl::WindowedSinc(
            PerformanceSincResampler::new(source_frequency, output_frequency as f64),
        ),
    }
}
