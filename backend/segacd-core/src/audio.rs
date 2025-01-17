//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

mod constants;

use crate::api::SegaCdEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_core::audio::{LowPassFilter, Ym2612Resampler};
use jgenesis_common::audio::FirResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;
use std::cmp;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;

// -9 dB (10 ^ -9/20)
const PCM_COEFFICIENT: f64 = 0.35481338923357547;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

type PcmResampler = FirResampler<{ constants::PCM_LPF_TAPS }, { constants::PCM_ZERO_PADDING }>;
type CdResampler = FirResampler<{ constants::CD_LPF_TAPS }, { constants::CD_ZERO_PADDING }>;

trait LpfExt {
    fn pcm_coefficients(self) -> &'static [f64; constants::PCM_LPF_TAPS];

    fn cd_coefficients(self, low_pass_cd_da: bool) -> &'static [f64; constants::CD_LPF_TAPS];
}

impl LpfExt for LowPassFilter {
    fn pcm_coefficients(self) -> &'static [f64; constants::PCM_LPF_TAPS] {
        match self {
            Self::Sharp => &constants::PCM_SHARP_LPF_COEFFICIENTS,
            Self::Moderate => &constants::PCM_MID_LPF_COEFFICIENTS,
            Self::Soft => &constants::PCM_SOFT_LPF_COEFFICIENTS,
            Self::VerySoft => &constants::PCM_VSOFT_LPF_COEFFICIENTS,
        }
    }

    fn cd_coefficients(self, low_pass_cd_da: bool) -> &'static [f64; constants::CD_LPF_TAPS] {
        if !low_pass_cd_da {
            return &constants::CD_SHARP_LPF_COEFFICIENTS;
        }

        match self {
            Self::Sharp => &constants::CD_SHARP_LPF_COEFFICIENTS,
            Self::Moderate => &constants::CD_MID_LPF_COEFFICIENTS,
            Self::Soft => &constants::CD_SOFT_LPF_COEFFICIENTS,
            Self::VerySoft => &constants::CD_VSOFT_LPF_COEFFICIENTS,
        }
    }
}

fn new_pcm_resampler(lpf_coefficients: &[f64; constants::PCM_LPF_TAPS]) -> PcmResampler {
    let pcm_frequency = SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0;
    PcmResampler::new(pcm_frequency, *lpf_coefficients, constants::PCM_HPF_CHARGE_FACTOR)
}

fn new_cd_resampler(lpf_coefficients: &[f64; constants::CD_LPF_TAPS]) -> CdResampler {
    CdResampler::new(CD_DA_FREQUENCY, *lpf_coefficients, constants::CD_HPF_CHARGE_FACTOR)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    ym2612_resampler: Ym2612Resampler,
    psg_resampler: PsgResampler,
    pcm_resampler: PcmResampler,
    cd_resampler: CdResampler,
    ym2612_enabled: bool,
    psg_enabled: bool,
    pcm_enabled: bool,
    cd_enabled: bool,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, config: SegaCdEmulatorConfig) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        let lpf = config.genesis.low_pass_filter;
        let ym2612_resampler =
            genesis_core::audio::new_ym2612_resampler(genesis_mclk_frequency, lpf);
        let psg_resampler =
            smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency, *lpf.psg_coefficients());
        let pcm_resampler = new_pcm_resampler(lpf.pcm_coefficients());
        let cd_resampler = new_cd_resampler(lpf.cd_coefficients(config.low_pass_cd_da));

        Self {
            ym2612_resampler,
            psg_resampler,
            pcm_resampler,
            cd_resampler,
            ym2612_enabled: config.genesis.ym2612_enabled,
            psg_enabled: config.genesis.psg_enabled,
            pcm_enabled: config.pcm_enabled,
            cd_enabled: config.cd_audio_enabled,
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_pcm_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.pcm_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_cd_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.cd_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            cmp::min(
                cmp::min(
                    self.ym2612_resampler.output_buffer_len(),
                    self.psg_resampler.output_buffer_len(),
                ),
                self.pcm_resampler.output_buffer_len(),
            ),
            self.cd_resampler.output_buffer_len(),
        );
        for _ in 0..sample_count {
            let (ym2612_l, ym2612_r) = check_enabled(
                self.ym2612_resampler.output_buffer_pop_front().unwrap(),
                self.ym2612_enabled,
            );
            let (psg_l, psg_r) = check_enabled(
                self.psg_resampler.output_buffer_pop_front().unwrap(),
                self.psg_enabled,
            );
            let (pcm_l, pcm_r) = check_enabled(
                self.pcm_resampler.output_buffer_pop_front().unwrap(),
                self.pcm_enabled,
            );
            let (cd_l, cd_r) = check_enabled(
                self.cd_resampler.output_buffer_pop_front().unwrap(),
                self.cd_enabled,
            );

            let sample_l = (ym2612_l
                + PSG_COEFFICIENT * psg_l
                + PCM_COEFFICIENT * pcm_l
                + CD_COEFFICIENT * cd_l)
                .clamp(-1.0, 1.0);
            let sample_r = (ym2612_r
                + PSG_COEFFICIENT * psg_r
                + PCM_COEFFICIENT * pcm_r
                + CD_COEFFICIENT * cd_r)
                .clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: SegaCdEmulatorConfig) {
        self.ym2612_enabled = config.genesis.ym2612_enabled;
        self.psg_enabled = config.genesis.psg_enabled;
        self.pcm_enabled = config.pcm_enabled;
        self.cd_enabled = config.cd_audio_enabled;

        let lpf = config.genesis.low_pass_filter;
        self.ym2612_resampler.update_lpf_coefficients(*lpf.ym2612_coefficients());
        self.psg_resampler.update_lpf_coefficients(*lpf.psg_coefficients());
        self.pcm_resampler.update_lpf_coefficients(*lpf.pcm_coefficients());
        self.cd_resampler.update_lpf_coefficients(*lpf.cd_coefficients(config.low_pass_cd_da));
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
        self.pcm_resampler.update_output_frequency(output_frequency);
        self.cd_resampler.update_output_frequency(output_frequency);
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
