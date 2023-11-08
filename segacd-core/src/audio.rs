//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use genesis_core::audio::{PsgDownsampler, SignalDownsampler, Ym2612Downsampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
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

type PcmDownsampler = SignalDownsampler<21, 3>;
type CdDownsampler = SignalDownsampler<23, 2>;

fn new_pcm_downsampler() -> PcmDownsampler {
    let pcm_frequency = SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0;
    PcmDownsampler::new(
        pcm_frequency,
        PCM_LPF_COEFFICIENT_0,
        PCM_LPF_COEFFICIENTS,
        PCM_HPF_CHARGE_FACTOR,
    )
}

fn new_cd_downsampler() -> CdDownsampler {
    CdDownsampler::new(
        CD_DA_FREQUENCY,
        CD_LPF_COEFFICIENT_0,
        CD_LPF_COEFFICIENTS,
        CD_HPF_CHARGE_FACTOR,
    )
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioDownsampler {
    ym2612_downsampler: Ym2612Downsampler,
    psg_downsampler: PsgDownsampler,
    pcm_downsampler: PcmDownsampler,
    cd_downsampler: CdDownsampler,
}

impl AudioDownsampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        let ym2612_downsampler = Ym2612Downsampler::new_ym2612(genesis_mclk_frequency);
        let psg_downsampler = PsgDownsampler::new_psg(genesis_mclk_frequency);
        let pcm_downsampler = new_pcm_downsampler();
        let cd_downsampler = new_cd_downsampler();

        Self { ym2612_downsampler, psg_downsampler, pcm_downsampler, cd_downsampler }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_pcm_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.pcm_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_cd_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.cd_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            cmp::min(
                cmp::min(
                    self.ym2612_downsampler.output_buffer_len(),
                    self.psg_downsampler.output_buffer_len(),
                ),
                self.pcm_downsampler.output_buffer_len(),
            ),
            self.cd_downsampler.output_buffer_len(),
        );
        for _ in 0..sample_count {
            let (ym2612_l, ym2612_r) = self.ym2612_downsampler.output_buffer_pop_front().unwrap();
            let (psg_l, psg_r) = self.psg_downsampler.output_buffer_pop_front().unwrap();
            let (pcm_l, pcm_r) = self.pcm_downsampler.output_buffer_pop_front().unwrap();
            let (cd_l, cd_r) = self.cd_downsampler.output_buffer_pop_front().unwrap();

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
}
