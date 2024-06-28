use crate::api::Sega32XEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_core::audio::Ym2612Resampler;
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

const PWM_LPF_COEFFICIENT_0: f64 = 0.003281378178835112;
const PWM_LPF_COEFFICIENTS: [f64; 45] = [
    0.003281378178835112,
    0.003464999523333785,
    0.004012289576225322,
    0.004912595959163398,
    0.006148395232487453,
    0.007695633969043074,
    0.009524196926781182,
    0.01159849320768895,
    0.01387814899413554,
    0.0163187933793078,
    0.0188729219964402,
    0.02149082163727849,
    0.02412153786313046,
    0.02671386677505526,
    0.0292173516395127,
    0.03158326497128794,
    0.03376555695856451,
    0.03572175177013042,
    0.03741377429911714,
    0.03880869125164505,
    0.03987935215592773,
    0.04060491781532202,
    0.04097126591958652,
    0.04097126591958652,
    0.04060491781532202,
    0.03987935215592773,
    0.03880869125164505,
    0.03741377429911714,
    0.03572175177013042,
    0.03376555695856451,
    0.03158326497128795,
    0.02921735163951271,
    0.02671386677505526,
    0.02412153786313046,
    0.02149082163727849,
    0.01887292199644021,
    0.01631879337930781,
    0.01387814899413554,
    0.01159849320768896,
    0.009524196926781192,
    0.00769563396904308,
    0.006148395232487462,
    0.004912595959163395,
    0.004012289576225327,
    0.003464999523333785,
];

const PWM_HPF_CHARGE_FACTOR: f64 = 0.9920244986380192;

pub type PwmResampler = SignalResampler<45, 8>;

fn new_pwm_resampler() -> PwmResampler {
    // Source frequency is irrelevant, the PWM chip will change it almost immediately
    PwmResampler::new(22000.0, PWM_LPF_COEFFICIENT_0, PWM_LPF_COEFFICIENTS, PWM_HPF_CHARGE_FACTOR)
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

        Self {
            ym2612_resampler: genesis_core::audio::new_ym2612_resampler(genesis_mclk_frequency),
            psg_resampler: smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency),
            pwm_resampler: new_pwm_resampler(),
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
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
