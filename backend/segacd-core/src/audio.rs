//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

use crate::api::{PcmLowPassFilter, SegaCdEmulatorConfig};
use bincode::{Decode, Encode};
use genesis_core::GenesisLowPassFilter;
use genesis_core::audio::GenesisAudioFilter;
use jgenesis_common::audio::iir::{FirstOrderIirFilter, SecondOrderIirFilter};
use jgenesis_common::audio::sinc::{PerformanceSincResampler, QualitySincResampler};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::cmp;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;

// -9 dB (10 ^ -9/20)
const PCM_COEFFICIENT: f64 = 0.35481338923357547;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

fn new_pcm_8khz_low_pass() -> SecondOrderIirFilter {
    // Second-order Butterworth IIR filter targeting 7973 Hz cutoff with source frequency of 32552 Hz
    SecondOrderIirFilter::new(&[0.28362508499709993, 0.5672501699941999, 0.28362508499709993], &[
        1.0,
        -0.03731874083716955,
        0.17181908082556915,
    ])
}

fn new_pcm_gen_low_pass() -> FirstOrderIirFilter {
    // First-order Butterworth IIR filter targeting 3390 Hz cutoff with source frequency of 32552 Hz
    FirstOrderIirFilter::new(&[0.2533767724796169, 0.2533767724796169], &[
        1.0,
        -0.49324645504076625,
    ])
}

fn new_cd_da_gen_low_pass() -> FirstOrderIirFilter {
    // First-order Butterworth IIR filter targeting 3390 Hz cutoff with source frequency of 44100 Hz
    FirstOrderIirFilter::new(&[0.1976272152714313, 0.1976272152714313], &[1.0, -0.6047455694571374])
}

#[derive(Debug, Clone, Encode, Decode)]
struct SegaCdAudioFilter {
    gen_low_pass_setting: GenesisLowPassFilter,
    pcm_low_pass_setting: PcmLowPassFilter,
    apply_gen_lpf_to_pcm: bool,
    apply_gen_lpf_to_cd_da: bool,
    pcm_8khz_lpf_l: SecondOrderIirFilter,
    pcm_8khz_lpf_r: SecondOrderIirFilter,
    pcm_gen_lpf_l: FirstOrderIirFilter,
    pcm_gen_lpf_r: FirstOrderIirFilter,
    cd_da_gen_lpf_l: FirstOrderIirFilter,
    cd_da_gen_lpf_r: FirstOrderIirFilter,
}

impl SegaCdAudioFilter {
    fn new(config: &SegaCdEmulatorConfig) -> Self {
        Self {
            gen_low_pass_setting: config.genesis.low_pass,
            pcm_low_pass_setting: config.pcm_low_pass,
            apply_gen_lpf_to_pcm: config.apply_genesis_lpf_to_pcm,
            apply_gen_lpf_to_cd_da: config.apply_genesis_lpf_to_cd_da,
            pcm_8khz_lpf_l: new_pcm_8khz_low_pass(),
            pcm_8khz_lpf_r: new_pcm_8khz_low_pass(),
            pcm_gen_lpf_l: new_pcm_gen_low_pass(),
            pcm_gen_lpf_r: new_pcm_gen_low_pass(),
            cd_da_gen_lpf_l: new_cd_da_gen_low_pass(),
            cd_da_gen_lpf_r: new_cd_da_gen_low_pass(),
        }
    }

    fn filter_pcm(&mut self, (mut sample_l, mut sample_r): (f64, f64)) -> (f64, f64) {
        if self.pcm_low_pass_setting == PcmLowPassFilter::SegaCd {
            sample_l = self.pcm_8khz_lpf_l.filter(sample_l);
            sample_r = self.pcm_8khz_lpf_r.filter(sample_r);
        }

        if self.apply_gen_lpf_to_pcm {
            match self.gen_low_pass_setting {
                GenesisLowPassFilter::Model1Va2 => {
                    sample_l = self.pcm_gen_lpf_l.filter(sample_l);
                    sample_r = self.pcm_gen_lpf_r.filter(sample_r);
                }
                GenesisLowPassFilter::None => {}
            }
        }

        (sample_l, sample_r)
    }

    fn filter_cd_da(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.apply_gen_lpf_to_cd_da {
            return (sample_l, sample_r);
        }

        match self.gen_low_pass_setting {
            GenesisLowPassFilter::Model1Va2 => {
                (self.cd_da_gen_lpf_l.filter(sample_l), self.cd_da_gen_lpf_r.filter(sample_r))
            }
            GenesisLowPassFilter::None => (sample_l, sample_r),
        }
    }

    fn reload_config(&mut self, config: &SegaCdEmulatorConfig) {
        if self.gen_low_pass_setting == config.genesis.low_pass
            && self.pcm_low_pass_setting == config.pcm_low_pass
            && self.apply_gen_lpf_to_pcm == config.apply_genesis_lpf_to_pcm
            && self.apply_gen_lpf_to_cd_da == config.apply_genesis_lpf_to_cd_da
        {
            return;
        }

        *self = Self::new(config);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    gen_filter: GenesisAudioFilter,
    scd_filter: SegaCdAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    pcm_resampler: QualitySincResampler<2>,
    cd_resampler: QualitySincResampler<2>,
    ym2612_enabled: bool,
    psg_enabled: bool,
    pcm_enabled: bool,
    cd_enabled: bool,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, config: SegaCdEmulatorConfig) -> Self {
        let ym2612_resampler =
            QualitySincResampler::new(genesis_core::audio::ym2612_frequency(timing_mode), 48000.0);
        let psg_resampler =
            PerformanceSincResampler::new(genesis_core::audio::psg_frequency(timing_mode), 48000.0);
        let pcm_resampler =
            QualitySincResampler::new(SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0, 48000.0);
        let cd_resampler = QualitySincResampler::new(CD_DA_FREQUENCY, 48000.0);

        Self {
            gen_filter: GenesisAudioFilter::new(config.genesis.low_pass),
            scd_filter: SegaCdAudioFilter::new(&config),
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
        let (sample_l, sample_r) = self.gen_filter.filter_ym2612((sample_l, sample_r));
        self.ym2612_resampler.collect([sample_l, sample_r]);
    }

    pub fn collect_psg_sample(&mut self, sample: f64) {
        let sample = self.gen_filter.filter_psg(sample);
        self.psg_resampler.collect([sample]);
    }

    pub fn collect_pcm_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.scd_filter.filter_pcm((sample_l, sample_r));
        self.pcm_resampler.collect([sample_l, sample_r]);
    }

    pub fn collect_cd_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.scd_filter.filter_cd_da((sample_l, sample_r));
        self.cd_resampler.collect([sample_l, sample_r]);
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
            let [ym2612_l, ym2612_r] = check_enabled(
                self.ym2612_resampler.output_buffer_pop_front().unwrap(),
                self.ym2612_enabled,
            );
            let [psg] = check_enabled(
                self.psg_resampler.output_buffer_pop_front().unwrap(),
                self.psg_enabled,
            );
            let [pcm_l, pcm_r] = check_enabled(
                self.pcm_resampler.output_buffer_pop_front().unwrap(),
                self.pcm_enabled,
            );
            let [cd_l, cd_r] = check_enabled(
                self.cd_resampler.output_buffer_pop_front().unwrap(),
                self.cd_enabled,
            );

            let sample_l = (ym2612_l
                + PSG_COEFFICIENT * psg
                + PCM_COEFFICIENT * pcm_l
                + CD_COEFFICIENT * cd_l)
                .clamp(-1.0, 1.0);
            let sample_r = (ym2612_r
                + PSG_COEFFICIENT * psg
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

        self.gen_filter.reload_config(&config.genesis);
        self.scd_filter.reload_config(&config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        let output_frequency = output_frequency as f64;

        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
        self.pcm_resampler.update_output_frequency(output_frequency);
        self.cd_resampler.update_output_frequency(output_frequency);
    }
}

fn check_enabled<T: Default>(sample: T, enabled: bool) -> T {
    if enabled { sample } else { T::default() }
}
