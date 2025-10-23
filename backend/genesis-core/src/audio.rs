//! Genesis audio resampling, filtering, and mixing code

use crate::GenesisEmulatorConfig;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::{FirstOrderIirFilter, IirFilter, SecondOrderIirFilter};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::cmp;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

// -7dB (10 ^ -7/20)
pub const PSG_COEFFICIENT: f64 = 0.44668359215096315;

#[must_use]
pub fn new_ym2612_low_pass<const N: usize>(timing_mode: TimingMode, cutoff: u32) -> IirFilter<N> {
    dsp::design::butterworth(cutoff.into(), ym2612_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_psg_low_pass(timing_mode: TimingMode, cutoff: u32) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff.into(), psg_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_ym2612_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, ym2612_frequency(timing_mode), FilterType::HighPass)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct LowPassSettings {
    pub genesis_enabled: bool,
    pub genesis_cutoff: u32,
    pub ym2612_2nd_enabled: bool,
    pub ym2612_2nd_cutoff: u32,
}

impl LowPassSettings {
    #[must_use]
    pub fn from_config(config: &GenesisEmulatorConfig) -> Self {
        Self {
            genesis_enabled: config.genesis_lpf_enabled,
            genesis_cutoff: config.genesis_lpf_cutoff,
            ym2612_2nd_enabled: config.ym2612_2nd_lpf_enabled,
            ym2612_2nd_cutoff: config.ym2612_2nd_lpf_cutoff,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioFilter {
    ym2612_dc_offset_l: FirstOrderIirFilter,
    ym2612_dc_offset_r: FirstOrderIirFilter,
    psg_dc_offset: FirstOrderIirFilter,
    low_pass_settings: LowPassSettings,
    ym2612_gen_low_pass_l: FirstOrderIirFilter,
    ym2612_gen_low_pass_r: FirstOrderIirFilter,
    ym2612_2nd_low_pass_l: SecondOrderIirFilter,
    ym2612_2nd_low_pass_r: SecondOrderIirFilter,
    psg_low_pass: FirstOrderIirFilter,
}

impl GenesisAudioFilter {
    #[must_use]
    pub fn new(timing_mode: TimingMode, low_pass_settings: LowPassSettings) -> Self {
        Self {
            ym2612_dc_offset_l: new_ym2612_dc_offset(timing_mode),
            ym2612_dc_offset_r: new_ym2612_dc_offset(timing_mode),
            psg_dc_offset: smsgg_core::audio::new_psg_dc_offset(timing_mode),
            low_pass_settings,
            ym2612_gen_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_gen_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_2nd_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            ym2612_2nd_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            psg_low_pass: new_psg_low_pass(timing_mode, low_pass_settings.genesis_cutoff),
        }
    }

    #[must_use]
    pub fn filter_ym2612(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        let (mut sample_l, mut sample_r) =
            (self.ym2612_dc_offset_l.filter(sample_l), self.ym2612_dc_offset_r.filter(sample_r));

        if self.low_pass_settings.ym2612_2nd_enabled {
            sample_l = self.ym2612_2nd_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_2nd_low_pass_r.filter(sample_r);
        }

        if self.low_pass_settings.genesis_enabled {
            sample_l = self.ym2612_gen_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_gen_low_pass_r.filter(sample_r);
        }

        (sample_l, sample_r)
    }

    #[must_use]
    pub fn filter_psg(&mut self, sample: f64) -> f64 {
        let sample = self.psg_dc_offset.filter(sample);

        if !self.low_pass_settings.genesis_enabled {
            return sample;
        }

        self.psg_low_pass.filter(sample)
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        let low_pass_settings = LowPassSettings::from_config(config);
        if self.low_pass_settings == low_pass_settings {
            return;
        }

        *self = Self::new(timing_mode, low_pass_settings);
    }
}

#[must_use]
pub fn ym2612_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 7.0 / 6.0 / 24.0
}

#[must_use]
pub fn psg_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 15.0 / 16.0
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &GenesisEmulatorConfig) -> Self {
        Self {
            ym2612: volume_multiplier(config.ym2612_enabled, config.ym2612_volume_adjustment_db),
            psg: PSG_COEFFICIENT
                * volume_multiplier(config.psg_enabled, config.psg_volume_adjustment_db),
        }
    }
}

#[must_use]
pub fn volume_multiplier(enabled: bool, adjustment_db: f64) -> f64 {
    if !enabled {
        return 0.0;
    }

    // Decibels to linear
    10.0_f64.powf(adjustment_db / 20.0)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    filter: GenesisAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    volumes: VolumeMultipliers,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: GenesisEmulatorConfig) -> Self {
        Self {
            filter: GenesisAudioFilter::new(timing_mode, LowPassSettings::from_config(&config)),
            ym2612_resampler: QualitySincResampler::new(ym2612_frequency(timing_mode), 48000.0),
            psg_resampler: PerformanceSincResampler::new(psg_frequency(timing_mode), 48000.0),
            volumes: VolumeMultipliers::from_config(&config),
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.filter.filter_ym2612((sample_l, sample_r));
        self.ym2612_resampler.collect([sample_l, sample_r]);
    }

    pub fn collect_psg_sample(&mut self, sample: f64) {
        let sample = self.filter.filter_psg(sample);
        self.psg_resampler.collect([sample]);
    }

    /// Push all samples that are ready to the given audio output.
    ///
    /// # Errors
    ///
    /// Will propagate any error returned by the audio output while pushing samples.
    // Should never panic, the unwraps are guarded by length checks
    #[allow(clippy::missing_panics_doc)]
    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
        );

        for _ in 0..sample_count {
            let [ym2612_l, ym2612_r] = self
                .ym2612_resampler
                .output_buffer_pop_front()
                .unwrap()
                .map(|sample| sample * self.volumes.ym2612);
            let [psg] = self
                .psg_resampler
                .output_buffer_pop_front()
                .unwrap()
                .map(|sample| sample * self.volumes.psg);

            let sample_l = (ym2612_l + psg).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: GenesisEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(&config);

        self.filter.reload_config(timing_mode, &config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency as f64);
        self.psg_resampler.update_output_frequency(output_frequency as f64);
    }
}
