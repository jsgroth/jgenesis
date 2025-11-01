#![allow(clippy::excessive_precision)]

use crate::api::Sega32XEmulatorConfig;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::FirstOrderIirFilter;
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use genesis_core::audio::{GenesisAudioFilter, LowPassSettings, volume_multiplier};
use jgenesis_common::audio::CubicResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::collections::VecDeque;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

// -2 dB (10^(-2 / 20))
const PWM_COEFFICIENT: f64 = 0.7943282347242815;

fn new_pwm_48khz_low_pass(cutoff: f64) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff, 48000.0, FilterType::LowPass)
}

fn new_pwm_44khz_low_pass(cutoff: f64) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff, 44100.0, FilterType::LowPass)
}

// This silliness is necessary to handle dynamic resampling ratio; the frontend doesn't indicate
// whether the output frequency is the original frequency or dynamic-adjusted
fn round_output_frequency(output_frequency: u64) -> u64 {
    let diff_48khz = (output_frequency as i64 - 48000).abs();
    let diff_44khz = (output_frequency as i64 - 44100).abs();

    if diff_48khz <= diff_44khz { 48000 } else { 44100 }
}

fn new_pwm_low_pass(output_frequency: u64, cutoff: f64) -> FirstOrderIirFilter {
    match output_frequency {
        48000 => new_pwm_48khz_low_pass(cutoff),
        44100 => new_pwm_44khz_low_pass(cutoff),
        _ => panic!(
            "new_pwm_low_pass(freq) should only be called with 48000 or 44100, was {output_frequency}"
        ),
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct PwmAudioFilter {
    gen_low_pass_setting: LowPassSettings,
    apply_gen_lpf_to_pwm: bool,
    rounded_output_frequency: u64,
    pwm_lpf_l: FirstOrderIirFilter,
    pwm_lpf_r: FirstOrderIirFilter,
}

impl PwmAudioFilter {
    fn new(config: &Sega32XEmulatorConfig, output_frequency: u64) -> Self {
        let rounded_output_frequency = round_output_frequency(output_frequency);
        let genesis_lpf_cutoff: f64 = config.genesis.genesis_lpf_cutoff.into();
        Self {
            gen_low_pass_setting: LowPassSettings::from_config(&config.genesis),
            apply_gen_lpf_to_pwm: config.apply_genesis_lpf_to_pwm,
            rounded_output_frequency,
            pwm_lpf_l: new_pwm_low_pass(rounded_output_frequency, genesis_lpf_cutoff),
            pwm_lpf_r: new_pwm_low_pass(rounded_output_frequency, genesis_lpf_cutoff),
        }
    }

    fn filter(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.gen_low_pass_setting.genesis_enabled || !self.apply_gen_lpf_to_pwm {
            return (sample_l, sample_r);
        }

        (self.pwm_lpf_l.filter(sample_l), self.pwm_lpf_r.filter(sample_r))
    }

    fn reload_config(&mut self, config: &Sega32XEmulatorConfig) {
        if self.gen_low_pass_setting == LowPassSettings::from_config(&config.genesis)
            && self.apply_gen_lpf_to_pwm == config.apply_genesis_lpf_to_pwm
        {
            return;
        }

        *self = Self::new(config, self.rounded_output_frequency);
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        let rounded_output_frequency = round_output_frequency(output_frequency);
        if self.rounded_output_frequency == rounded_output_frequency {
            return;
        }

        self.rounded_output_frequency = rounded_output_frequency;

        let genesis_lpf_cutoff: f64 = self.gen_low_pass_setting.genesis_cutoff.into();
        self.pwm_lpf_l = new_pwm_low_pass(output_frequency, genesis_lpf_cutoff);
        self.pwm_lpf_r = new_pwm_low_pass(output_frequency, genesis_lpf_cutoff);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmResampler {
    filter: PwmAudioFilter,
    resampler: CubicResampler<2>,
    output: VecDeque<(f64, f64)>,
}

impl PwmResampler {
    pub fn new(config: &Sega32XEmulatorConfig, output_frequency: u64) -> Self {
        Self {
            filter: PwmAudioFilter::new(config, output_frequency),
            resampler: CubicResampler::new(22000.0, output_frequency),
            output: VecDeque::with_capacity(48000 / 30),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample([sample_l, sample_r]);
        while let Some([output_l, output_r]) = self.resampler.output_buffer_pop_front() {
            let (output_l, output_r) = self.filter.filter((output_l, output_r));
            self.output.push_back((output_l, output_r));
        }
    }

    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    pub fn output_buffer_pop_front(&mut self) -> Option<(f64, f64)> {
        self.output.pop_front()
    }

    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.resampler.update_source_frequency(source_frequency);
    }

    fn reload_config(&mut self, config: &Sega32XEmulatorConfig) {
        self.filter.reload_config(config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.filter.update_output_frequency(output_frequency);
        self.resampler.update_output_frequency(output_frequency);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
    pwm: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &Sega32XEmulatorConfig) -> Self {
        Self {
            ym2612: volume_multiplier(
                config.genesis.ym2612_enabled,
                config.genesis.ym2612_volume_adjustment_db,
            ),
            psg: PSG_COEFFICIENT
                * volume_multiplier(
                    config.genesis.psg_enabled,
                    config.genesis.psg_volume_adjustment_db,
                ),
            pwm: PWM_COEFFICIENT
                * volume_multiplier(config.pwm_enabled, config.pwm_volume_adjustment_db),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XResampler {
    gen_filter: GenesisAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    pwm_resampler: PwmResampler,
    volumes: VolumeMultipliers,
}

impl Sega32XResampler {
    pub fn new(timing_mode: TimingMode, config: Sega32XEmulatorConfig) -> Self {
        Self {
            gen_filter: GenesisAudioFilter::new(
                timing_mode,
                LowPassSettings::from_config(&config.genesis),
            ),
            ym2612_resampler: QualitySincResampler::new(
                genesis_core::audio::ym2612_frequency(timing_mode),
                48000.0,
            ),
            psg_resampler: PerformanceSincResampler::new(
                genesis_core::audio::psg_frequency(timing_mode),
                48000.0,
            ),
            pwm_resampler: PwmResampler::new(&config, 48000),
            volumes: VolumeMultipliers::from_config(&config),
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.gen_filter.filter_ym2612((sample_l, sample_r));
        self.ym2612_resampler.collect([sample_l, sample_r]);
    }

    pub fn collect_psg_sample(&mut self, sample: f64) {
        let sample = self.gen_filter.filter_psg(sample);
        self.psg_resampler.collect([sample]);
    }

    pub fn pwm_resampler_mut(&mut self) -> &mut PwmResampler {
        &mut self.pwm_resampler
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let samples_ready = [
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
            self.pwm_resampler_mut().output_buffer_len(),
        ]
        .into_iter()
        .min()
        .unwrap();
        for _ in 0..samples_ready {
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

            let (mut pwm_l, mut pwm_r) = self.pwm_resampler.output_buffer_pop_front().unwrap();
            pwm_l *= self.volumes.pwm;
            pwm_r *= self.volumes.pwm;

            let sample_l = (ym2612_l + psg + pwm_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg + pwm_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: Sega32XEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(&config);

        self.gen_filter.reload_config(timing_mode, &config.genesis);
        self.pwm_resampler.reload_config(&config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency as f64);
        self.psg_resampler.update_output_frequency(output_frequency as f64);
        self.pwm_resampler.update_output_frequency(output_frequency);
    }
}
