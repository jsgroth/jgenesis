mod constants;

use crate::api::Sega32XEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_core::GenesisLowPassFilter;
use genesis_core::audio::{GenesisAudioFilter, Ym2612Resampler};
use jgenesis_common::audio::iir::FirstOrderIirFilter;
use jgenesis_common::audio::{CubicResampler, DEFAULT_OUTPUT_FREQUENCY, FirResampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

// -2 dB (10^(-2 / 20))
const PWM_COEFFICIENT: f64 = 0.7943282347242815;

fn new_pwm_48khz_low_pass() -> FirstOrderIirFilter {
    // First-order Butterworth IIR filter targeting 3390 Hz cutoff with source frequency of 48000 Hz
    FirstOrderIirFilter::new(&[0.18406577751250913, 0.18406577751250913], &[
        1.0,
        -0.6318684449749816,
    ])
}

fn new_pwm_44khz_low_pass() -> FirstOrderIirFilter {
    // First-order Butterworth IIR filter targeting 3390 Hz cutoff with source frequency of 44100 Hz
    FirstOrderIirFilter::new(&[0.1976272152714313, 0.1976272152714313], &[1.0, -0.6047455694571374])
}

// This silliness is necessary to handle dynamic resampling ratio; the frontend doesn't indicate
// whether the output frequency is the original frequency or dynamic-adjusted
fn round_output_frequency(output_frequency: u64) -> u64 {
    let diff_48khz = (output_frequency as i64 - 48000).abs();
    let diff_44khz = (output_frequency as i64 - 44100).abs();

    if diff_48khz <= diff_44khz { 48000 } else { 44100 }
}

fn new_pwm_low_pass(output_frequency: u64) -> FirstOrderIirFilter {
    match output_frequency {
        48000 => new_pwm_48khz_low_pass(),
        44100 => new_pwm_44khz_low_pass(),
        _ => panic!(
            "new_pwm_low_pass(freq) should only be called with 48000 or 44100, was {output_frequency}"
        ),
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct PwmAudioFilter {
    gen_low_pass_setting: GenesisLowPassFilter,
    apply_gen_lpf_to_pwm: bool,
    rounded_output_frequency: u64,
    pwm_lpf_l: FirstOrderIirFilter,
    pwm_lpf_r: FirstOrderIirFilter,
}

impl PwmAudioFilter {
    fn new(config: &Sega32XEmulatorConfig, output_frequency: u64) -> Self {
        let rounded_output_frequency = round_output_frequency(output_frequency);
        Self {
            gen_low_pass_setting: config.genesis.low_pass,
            apply_gen_lpf_to_pwm: config.apply_genesis_lpf_to_pwm,
            rounded_output_frequency,
            pwm_lpf_l: new_pwm_low_pass(rounded_output_frequency),
            pwm_lpf_r: new_pwm_low_pass(rounded_output_frequency),
        }
    }

    fn filter(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.apply_gen_lpf_to_pwm {
            return (sample_l, sample_r);
        }

        (self.pwm_lpf_l.filter(sample_l), self.pwm_lpf_r.filter(sample_r))
    }

    fn reload_config(&mut self, config: &Sega32XEmulatorConfig) {
        if self.gen_low_pass_setting == config.genesis.low_pass
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
        self.pwm_lpf_l = new_pwm_low_pass(output_frequency);
        self.pwm_lpf_r = new_pwm_low_pass(output_frequency);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmResampler {
    filter: PwmAudioFilter,
    resampler: CubicResampler,
    lpf: FirResampler<{ constants::PWM_LPF_TAPS }, 0>,
}

impl PwmResampler {
    pub fn new(config: &Sega32XEmulatorConfig, output_frequency: u64) -> Self {
        Self {
            filter: PwmAudioFilter::new(config, output_frequency),
            resampler: CubicResampler::new(22000.0),
            lpf: FirResampler::new(
                DEFAULT_OUTPUT_FREQUENCY as f64,
                constants::PWM_SHARP_LPF_COEFFICIENTS,
                1.0,
            ),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
        while let Some((output_l, output_r)) = self.resampler.output_buffer_pop_front() {
            let (output_l, output_r) = self.filter.filter((output_l, output_r));
            self.lpf.collect_sample(output_l, output_r);
        }
    }

    pub fn output_buffer_len(&self) -> usize {
        self.lpf.output_buffer_len()
    }

    pub fn output_buffer_pop_front(&mut self) -> Option<(f64, f64)> {
        self.lpf.output_buffer_pop_front()
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
        self.lpf.update_source_frequency(output_frequency as f64);
        self.lpf.update_output_frequency(output_frequency);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XResampler {
    gen_filter: GenesisAudioFilter,
    ym2612_resampler: Ym2612Resampler,
    psg_resampler: PsgResampler,
    pwm_resampler: PwmResampler,
    ym2612_enabled: bool,
    psg_enabled: bool,
    pwm_enabled: bool,
}

impl Sega32XResampler {
    pub fn new(timing_mode: TimingMode, config: Sega32XEmulatorConfig) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        Self {
            gen_filter: GenesisAudioFilter::new(config.genesis.low_pass),
            ym2612_resampler: genesis_core::audio::new_ym2612_resampler(genesis_mclk_frequency),
            psg_resampler: smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency),
            pwm_resampler: PwmResampler::new(&config, 48000),
            ym2612_enabled: config.genesis.ym2612_enabled,
            psg_enabled: config.genesis.psg_enabled,
            pwm_enabled: config.pwm_enabled,
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.gen_filter.filter_ym2612((sample_l, sample_r));
        self.ym2612_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample: f64) {
        let sample = self.gen_filter.filter_psg(sample);
        self.psg_resampler.collect_sample(sample, sample);
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
            let (ym2612_l, ym2612_r) = check_enabled(
                self.ym2612_resampler.output_buffer_pop_front().unwrap(),
                self.ym2612_enabled,
            );
            let (psg_l, psg_r) = check_enabled(
                self.psg_resampler.output_buffer_pop_front().unwrap(),
                self.psg_enabled,
            );
            let (pwm_l, pwm_r) = check_enabled(
                self.pwm_resampler.output_buffer_pop_front().unwrap(),
                self.pwm_enabled,
            );

            let sample_l =
                (ym2612_l + PSG_COEFFICIENT * psg_l + PWM_COEFFICIENT * pwm_l).clamp(-1.0, 1.0);
            let sample_r =
                (ym2612_r + PSG_COEFFICIENT * psg_r + PWM_COEFFICIENT * pwm_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: Sega32XEmulatorConfig) {
        self.ym2612_enabled = config.genesis.ym2612_enabled;
        self.psg_enabled = config.genesis.psg_enabled;
        self.pwm_enabled = config.pwm_enabled;

        self.gen_filter.reload_config(&config.genesis);
        self.pwm_resampler.reload_config(&config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
        self.pwm_resampler.update_output_frequency(output_frequency);
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
