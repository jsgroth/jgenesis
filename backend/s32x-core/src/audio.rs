#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::FirstOrderIirFilter;
use dsp::sinc::QualitySincResampler;
use genesis_config::{GenesisEmulatorConfig, LowPassSettings, S32XPwmResampling};
use jgenesis_common::audio::CubicResampler;
use std::collections::VecDeque;

#[derive(Debug, Clone, Encode, Decode)]
struct PwmAudioFilter {
    gen_low_pass_setting: LowPassSettings,
    apply_gen_lpf_to_pwm: bool,
    pwm_frequency: f64,
    pwm_lpf_l: FirstOrderIirFilter,
    pwm_lpf_r: FirstOrderIirFilter,
}

fn new_pwm_low_pass(pwm_frequency: f64, cutoff_frequency: f64) -> FirstOrderIirFilter {
    let pwm_nyquist = 0.5 * pwm_frequency;
    if cutoff_frequency >= pwm_nyquist {
        // A real low-pass filter will produce garbage; return a fake filter
        return FirstOrderIirFilter::identity();
    }

    dsp::design::butterworth(cutoff_frequency, pwm_frequency, FilterType::LowPass)
}

impl PwmAudioFilter {
    fn new(config: &GenesisEmulatorConfig, pwm_frequency: f64) -> Self {
        let genesis_lpf_cutoff: f64 = config.genesis_lpf_cutoff.into();
        Self {
            gen_low_pass_setting: LowPassSettings::from_config(config),
            apply_gen_lpf_to_pwm: config.sega_32x.apply_genesis_lpf_to_pwm,
            pwm_frequency,
            pwm_lpf_l: new_pwm_low_pass(pwm_frequency, genesis_lpf_cutoff),
            pwm_lpf_r: new_pwm_low_pass(pwm_frequency, genesis_lpf_cutoff),
        }
    }

    fn filter(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.gen_low_pass_setting.genesis_enabled || !self.apply_gen_lpf_to_pwm {
            return (sample_l, sample_r);
        }

        (self.pwm_lpf_l.filter(sample_l), self.pwm_lpf_r.filter(sample_r))
    }

    fn update_pwm_frequency(&mut self, pwm_frequency: f64) {
        // Exact float comparison is fine here because PWM frequency is deterministically derived
        // from the PWM cycle register value
        #[allow(clippy::float_cmp)]
        if pwm_frequency == self.pwm_frequency {
            return;
        }
        self.pwm_frequency = pwm_frequency;

        let genesis_lpf_cutoff: f64 = self.gen_low_pass_setting.genesis_cutoff.into();
        self.pwm_lpf_l = new_pwm_low_pass(self.pwm_frequency, genesis_lpf_cutoff);
        self.pwm_lpf_r = new_pwm_low_pass(self.pwm_frequency, genesis_lpf_cutoff);
    }

    fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        if self.gen_low_pass_setting == LowPassSettings::from_config(config)
            && self.apply_gen_lpf_to_pwm == config.sega_32x.apply_genesis_lpf_to_pwm
        {
            return;
        }

        *self = Self::new(config, self.pwm_frequency);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum PwmResamplerImpl {
    CubicHermite(CubicResampler<2>),
    WindowedSinc(QualitySincResampler<2>),
}

impl PwmResamplerImpl {
    fn new(resampling: S32XPwmResampling, pwm_frequency: f64, output_frequency: u64) -> Self {
        match resampling {
            S32XPwmResampling::CubicHermite => {
                Self::CubicHermite(CubicResampler::new(pwm_frequency, output_frequency))
            }
            S32XPwmResampling::WindowedSinc => Self::WindowedSinc(QualitySincResampler::new(
                pwm_frequency,
                output_frequency as f64,
            )),
        }
    }

    fn resampling(&self) -> S32XPwmResampling {
        match self {
            Self::CubicHermite(_) => S32XPwmResampling::CubicHermite,
            Self::WindowedSinc(_) => S32XPwmResampling::WindowedSinc,
        }
    }

    fn collect_sample(&mut self, sample: [f64; 2]) {
        match self {
            Self::CubicHermite(resampler) => resampler.collect_sample(sample),
            Self::WindowedSinc(resampler) => resampler.collect(sample),
        }
    }

    fn output_buffer_pop_front(&mut self) -> Option<[f64; 2]> {
        match self {
            Self::CubicHermite(resampler) => resampler.output_buffer_pop_front(),
            Self::WindowedSinc(resampler) => resampler.output_buffer_pop_front(),
        }
    }

    fn update_source_frequency(&mut self, source_frequency: f64) {
        match self {
            Self::CubicHermite(resampler) => resampler.update_source_frequency(source_frequency),
            Self::WindowedSinc(resampler) => resampler.update_source_frequency(source_frequency),
        }
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        match self {
            Self::CubicHermite(resampler) => resampler.update_output_frequency(output_frequency),
            Self::WindowedSinc(resampler) => {
                resampler.update_output_frequency(output_frequency as f64);
            }
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmResampler {
    filter: PwmAudioFilter,
    resampler: PwmResamplerImpl,
    output: VecDeque<[f64; 2]>,
    output_frequency: u64,
}

impl PwmResampler {
    #[must_use]
    pub fn new(config: &GenesisEmulatorConfig, output_frequency: u64) -> Self {
        const INITIAL_PWM_FREQUENCY: f64 = 22000.0;

        Self {
            filter: PwmAudioFilter::new(config, INITIAL_PWM_FREQUENCY),
            resampler: PwmResamplerImpl::new(
                config.sega_32x.pwm_resampling,
                INITIAL_PWM_FREQUENCY,
                output_frequency,
            ),
            output: VecDeque::with_capacity(48000 / 30),
            output_frequency,
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.filter.filter((sample_l, sample_r));

        self.resampler.collect_sample([sample_l, sample_r]);
        while let Some(output) = self.resampler.output_buffer_pop_front() {
            self.output.push_back(output);
        }
    }

    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    pub fn output_buffer_pop_front(&mut self) -> Option<[f64; 2]> {
        self.output.pop_front()
    }

    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.filter.update_pwm_frequency(source_frequency);
        self.resampler.update_source_frequency(source_frequency);
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        self.filter.reload_config(config);

        if config.sega_32x.pwm_resampling != self.resampler.resampling() {
            self.resampler = PwmResamplerImpl::new(
                config.sega_32x.pwm_resampling,
                self.filter.pwm_frequency,
                self.output_frequency,
            );
        }
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
        self.output_frequency = output_frequency;
    }
}
