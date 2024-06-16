//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

#![allow(clippy::excessive_precision)]

use crate::api::SegaCdEmulatorConfig;
use bincode::{Decode, Encode};
use genesis_core::audio::Ym2612Resampler;
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;
use std::cmp;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;

const PCM_LPF_COEFFICIENT_0: f64 = -0.001032167331725023;
const PCM_LPF_COEFFICIENTS: [f64; 21] = [
    -0.001032167331725023,
    -0.00337362854293201,
    -0.002300741105977643,
    0.007438828683983638,
    0.01718256624704002,
    0.002040390827841266,
    -0.04030652783842427,
    -0.05506118523737572,
    0.02814357569062969,
    0.2004791993149999,
    0.3467896892919401,
    0.3467896892919402,
    0.2004791993149999,
    0.02814357569062969,
    -0.05506118523737575,
    -0.04030652783842429,
    0.002040390827841267,
    0.01718256624704001,
    0.00743882868398364,
    -0.002300741105977646,
    -0.003373628542932013,
];

const PCM_HPF_CHARGE_FACTOR: f64 = 0.9946028448191855;

// -3 dB (10 ^ -3/20)
const PCM_COEFFICIENT: f64 = 0.7079457843841379;

const CD_LPF_COEFFICIENT_0: f64 = 0.001074119844470324;
const CD_LPF_COEFFICIENTS: [f64; 23] = [
    0.001074119844470324,
    -0.00173597616545656,
    -0.004832665407973518,
    -0.001992823915686409,
    0.0109179929840003,
    0.01955265022534506,
    -0.001029754702410328,
    -0.04519730177754978,
    -0.05443102244415676,
    0.03374428630870474,
    0.2024522203986207,
    0.3414782746520921,
    0.3414782746520921,
    0.2024522203986207,
    0.03374428630870474,
    -0.05443102244415674,
    -0.0451973017775498,
    -0.001029754702410327,
    0.01955265022534507,
    0.01091799298400031,
    -0.001992823915686409,
    -0.004832665407973518,
    -0.001735976165456563,
];

const CD_HPF_CHARGE_FACTOR: f64 = 0.9960133089108504;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

type PcmResampler = SignalResampler<21, 3>;
type CdResampler = SignalResampler<23, 2>;

fn new_pcm_resampler() -> PcmResampler {
    let pcm_frequency = SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0;
    PcmResampler::new(
        pcm_frequency,
        PCM_LPF_COEFFICIENT_0,
        PCM_LPF_COEFFICIENTS,
        PCM_HPF_CHARGE_FACTOR,
    )
}

fn new_cd_resampler() -> CdResampler {
    CdResampler::new(
        CD_DA_FREQUENCY,
        CD_LPF_COEFFICIENT_0,
        CD_LPF_COEFFICIENTS,
        CD_HPF_CHARGE_FACTOR,
    )
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

        let ym2612_resampler = genesis_core::audio::new_ym2612_resampler(genesis_mclk_frequency);
        let psg_resampler = smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency);
        let pcm_resampler = new_pcm_resampler();
        let cd_resampler = new_cd_resampler();

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
    }
}

fn check_enabled(sample: (f64, f64), enabled: bool) -> (f64, f64) {
    if enabled { sample } else { (0.0, 0.0) }
}
