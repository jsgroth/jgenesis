mod constants;

use crate::api::Sega32XEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_core::audio::{LowPassFilter, Ym2612Resampler};
use jgenesis_common::audio::{CubicResampler, DEFAULT_OUTPUT_FREQUENCY, FirResampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

trait LowPassFilterExt {
    fn pwm_coefficients(self) -> &'static [f64; constants::PWM_LPF_TAPS];
}

impl LowPassFilterExt for LowPassFilter {
    fn pwm_coefficients(self) -> &'static [f64; constants::PWM_LPF_TAPS] {
        match self {
            Self::Sharp => &constants::PWM_SHARP_LPF_COEFFICIENTS,
            Self::Moderate => &constants::PWM_MID_LPF_COEFFICIENTS,
            Self::Soft => &constants::PWM_SOFT_LPF_COEFFICIENTS,
            Self::VerySoft => &constants::PWM_VSOFT_LPF_COEFFICIENTS,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmResampler {
    resampler: CubicResampler,
    lpf: FirResampler<{ constants::PWM_LPF_TAPS }>,
}

impl PwmResampler {
    pub fn new(lpf: LowPassFilter) -> Self {
        Self {
            resampler: CubicResampler::new(22000.0),
            lpf: FirResampler::new(
                DEFAULT_OUTPUT_FREQUENCY as f64,
                *lpf.pwm_coefficients(),
                1.0,
                0,
            ),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
        while let Some((output_l, output_r)) = self.resampler.output_buffer_pop_front() {
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

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
        self.lpf.update_source_frequency(output_frequency as f64);
        self.lpf.update_output_frequency(output_frequency);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XResampler {
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

        let lpf = config.genesis.low_pass_filter;
        Self {
            ym2612_resampler: genesis_core::audio::new_ym2612_resampler(
                genesis_mclk_frequency,
                lpf,
            ),
            psg_resampler: smsgg_core::audio::new_psg_resampler(
                genesis_mclk_frequency,
                *lpf.psg_coefficients(),
            ),
            pwm_resampler: PwmResampler::new(lpf),
            ym2612_enabled: config.genesis.ym2612_enabled,
            psg_enabled: config.genesis.psg_enabled,
            pwm_enabled: config.pwm_enabled,
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
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

            let sample_l = (ym2612_l + PSG_COEFFICIENT * psg_l + pwm_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + PSG_COEFFICIENT * psg_r + pwm_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: Sega32XEmulatorConfig) {
        self.ym2612_enabled = config.genesis.ym2612_enabled;
        self.psg_enabled = config.genesis.psg_enabled;
        self.pwm_enabled = config.pwm_enabled;

        let lpf = config.genesis.low_pass_filter;
        self.ym2612_resampler.update_lpf_coefficients(*lpf.ym2612_coefficients());
        self.psg_resampler.update_lpf_coefficients(*lpf.psg_coefficients());
        self.pwm_resampler.lpf.update_lpf_coefficients(*lpf.pwm_coefficients());
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
