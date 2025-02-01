//! Genesis audio resampling, filtering, and mixing code

use crate::{GenesisEmulatorConfig, GenesisLowPassFilter};
use bincode::{Decode, Encode};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::sinc::{PerformanceSincResampler, QualitySincResampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::cmp;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

// -7dB (10 ^ -7/20)
pub const PSG_COEFFICIENT: f64 = 0.44668359215096315;

#[must_use]
pub fn new_ym2612_low_pass() -> FirstOrderIirFilter {
    // Filter targets 3390 Hz with a source frequency of 53267 Hz
    FirstOrderIirFilter::new(&[0.1684983368367697, 0.1684983368367697], &[1.0, -0.6630033263264605])
}

#[must_use]
pub fn new_psg_low_pass() -> FirstOrderIirFilter {
    // Filter targets 3390 Hz with a source frequency of 223721 Hz
    FirstOrderIirFilter::new(
        &[0.04547345635121703, 0.04547345635121703],
        &[1.0, -0.9090530872975658],
    )
}

#[must_use]
pub fn new_ym2612_dc_offset() -> FirstOrderIirFilter {
    // Butterworth high-pass with cutoff frequency 5 Hz, source frequency 53267 Hz
    FirstOrderIirFilter::new(
        &[0.9997051960585813, -0.9997051960585813],
        &[1.0, -0.9994103921171625],
    )
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioFilter {
    ym2612_dc_offset_l: FirstOrderIirFilter,
    ym2612_dc_offset_r: FirstOrderIirFilter,
    psg_dc_offset: FirstOrderIirFilter,
    low_pass_setting: GenesisLowPassFilter,
    ym2612_low_pass_l: FirstOrderIirFilter,
    ym2612_low_pass_r: FirstOrderIirFilter,
    psg_low_pass: FirstOrderIirFilter,
}

impl GenesisAudioFilter {
    #[must_use]
    pub fn new(low_pass: GenesisLowPassFilter) -> Self {
        Self {
            ym2612_dc_offset_l: new_ym2612_dc_offset(),
            ym2612_dc_offset_r: new_ym2612_dc_offset(),
            psg_dc_offset: smsgg_core::audio::new_psg_dc_offset(),
            low_pass_setting: low_pass,
            ym2612_low_pass_l: new_ym2612_low_pass(),
            ym2612_low_pass_r: new_ym2612_low_pass(),
            psg_low_pass: new_psg_low_pass(),
        }
    }

    #[must_use]
    pub fn filter_ym2612(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        let (sample_l, sample_r) =
            (self.ym2612_dc_offset_l.filter(sample_l), self.ym2612_dc_offset_r.filter(sample_r));

        if self.low_pass_setting != GenesisLowPassFilter::Model1Va2 {
            return (sample_l, sample_r);
        }

        (self.ym2612_low_pass_l.filter(sample_l), self.ym2612_low_pass_r.filter(sample_r))
    }

    #[must_use]
    pub fn filter_psg(&mut self, sample: f64) -> f64 {
        let sample = self.psg_dc_offset.filter(sample);

        if self.low_pass_setting != GenesisLowPassFilter::Model1Va2 {
            return sample;
        }

        self.psg_low_pass.filter(sample)
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        if self.low_pass_setting == config.low_pass {
            return;
        }

        *self = Self::new(config.low_pass);
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
pub struct GenesisAudioResampler {
    filter: GenesisAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    ym2612_enabled: bool,
    psg_enabled: bool,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: GenesisEmulatorConfig) -> Self {
        Self {
            filter: GenesisAudioFilter::new(config.low_pass),
            ym2612_resampler: QualitySincResampler::new(ym2612_frequency(timing_mode), 48000.0),
            psg_resampler: PerformanceSincResampler::new(psg_frequency(timing_mode), 48000.0),
            ym2612_enabled: config.ym2612_enabled,
            psg_enabled: config.psg_enabled,
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
            let [ym2612_l, ym2612_r] = check_enabled(
                self.ym2612_resampler.output_buffer_pop_front().unwrap(),
                self.ym2612_enabled,
            );
            let [psg] = check_enabled(
                self.psg_resampler.output_buffer_pop_front().unwrap(),
                self.psg_enabled,
            );

            let sample_l = (ym2612_l + PSG_COEFFICIENT * psg).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + PSG_COEFFICIENT * psg).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: GenesisEmulatorConfig) {
        self.ym2612_enabled = config.ym2612_enabled;
        self.psg_enabled = config.psg_enabled;

        self.filter.reload_config(&config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency as f64);
        self.psg_resampler.update_output_frequency(output_frequency as f64);
    }
}

fn check_enabled<T: Default>(sample: T, enabled: bool) -> T {
    if enabled { sample } else { T::default() }
}
