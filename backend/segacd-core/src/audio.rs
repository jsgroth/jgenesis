//! Sega CD audio resampling, filtering, and mixing code
//!
//! Reuses some resampling/filtering code from [`genesis_core::audio`]

use crate::api::SegaCdEmulatorConfig;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::{FirstOrderIirFilter, IirFilter, SecondOrderIirFilter};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use genesis_core::audio::{GenesisAudioFilter, LowPassSettings, volume_multiplier};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::cmp;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;

// -6 dB (10 ^ -6/20)
const PCM_COEFFICIENT: f64 = 0.5011872336272722;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

fn new_pcm_low_pass<const N: usize>(cutoff: f64) -> IirFilter<N> {
    dsp::design::butterworth(cutoff, SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0, FilterType::LowPass)
}

fn new_cd_da_low_pass(cutoff: f64) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff, CD_DA_FREQUENCY, FilterType::LowPass)
}

#[derive(Debug, Clone, Encode, Decode)]
struct SegaCdAudioFilter {
    gen_low_pass_setting: LowPassSettings,
    pcm_lpf_enabled: bool,
    pcm_lpf_cutoff: u32,
    apply_gen_lpf_to_pcm: bool,
    apply_gen_lpf_to_cd_da: bool,
    pcm_lpf_l: SecondOrderIirFilter,
    pcm_lpf_r: SecondOrderIirFilter,
    pcm_gen_lpf_l: FirstOrderIirFilter,
    pcm_gen_lpf_r: FirstOrderIirFilter,
    cd_da_gen_lpf_l: FirstOrderIirFilter,
    cd_da_gen_lpf_r: FirstOrderIirFilter,
}

impl SegaCdAudioFilter {
    fn new(config: &SegaCdEmulatorConfig) -> Self {
        let gen_cutoff: f64 = config.genesis.genesis_lpf_cutoff.into();
        let pcm_cutoff: f64 = config.pcm_lpf_cutoff.into();

        Self {
            gen_low_pass_setting: LowPassSettings::from_config(&config.genesis),
            pcm_lpf_enabled: config.pcm_lpf_enabled,
            pcm_lpf_cutoff: config.pcm_lpf_cutoff,
            apply_gen_lpf_to_pcm: config.apply_genesis_lpf_to_pcm,
            apply_gen_lpf_to_cd_da: config.apply_genesis_lpf_to_cd_da,
            pcm_lpf_l: new_pcm_low_pass(pcm_cutoff),
            pcm_lpf_r: new_pcm_low_pass(pcm_cutoff),
            pcm_gen_lpf_l: new_pcm_low_pass(gen_cutoff),
            pcm_gen_lpf_r: new_pcm_low_pass(gen_cutoff),
            cd_da_gen_lpf_l: new_cd_da_low_pass(gen_cutoff),
            cd_da_gen_lpf_r: new_cd_da_low_pass(gen_cutoff),
        }
    }

    fn filter_pcm(&mut self, (mut sample_l, mut sample_r): (f64, f64)) -> (f64, f64) {
        if self.pcm_lpf_enabled {
            sample_l = self.pcm_lpf_l.filter(sample_l);
            sample_r = self.pcm_lpf_r.filter(sample_r);
        }

        if self.gen_low_pass_setting.genesis_enabled && self.apply_gen_lpf_to_pcm {
            sample_l = self.pcm_gen_lpf_l.filter(sample_l);
            sample_r = self.pcm_gen_lpf_r.filter(sample_r);
        }

        (sample_l, sample_r)
    }

    fn filter_cd_da(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        if !self.gen_low_pass_setting.genesis_enabled || !self.apply_gen_lpf_to_cd_da {
            return (sample_l, sample_r);
        }

        (self.cd_da_gen_lpf_l.filter(sample_l), self.cd_da_gen_lpf_r.filter(sample_r))
    }

    fn reload_config(&mut self, config: &SegaCdEmulatorConfig) {
        if self.gen_low_pass_setting == LowPassSettings::from_config(&config.genesis)
            && self.pcm_lpf_enabled == config.pcm_lpf_enabled
            && self.pcm_lpf_cutoff == config.pcm_lpf_cutoff
            && self.apply_gen_lpf_to_pcm == config.apply_genesis_lpf_to_pcm
            && self.apply_gen_lpf_to_cd_da == config.apply_genesis_lpf_to_cd_da
        {
            return;
        }

        *self = Self::new(config);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
    pcm: f64,
    cd: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &SegaCdEmulatorConfig) -> Self {
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
            pcm: PCM_COEFFICIENT
                * volume_multiplier(config.pcm_enabled, config.pcm_volume_adjustment_db),
            cd: CD_COEFFICIENT
                * volume_multiplier(config.cd_audio_enabled, config.cd_volume_adjustment_db),
        }
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
    volumes: VolumeMultipliers,
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
            gen_filter: GenesisAudioFilter::new(
                timing_mode,
                LowPassSettings::from_config(&config.genesis),
            ),
            scd_filter: SegaCdAudioFilter::new(&config),
            ym2612_resampler,
            psg_resampler,
            pcm_resampler,
            cd_resampler,
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
            let [pcm_l, pcm_r] = self
                .pcm_resampler
                .output_buffer_pop_front()
                .unwrap()
                .map(|sample| sample * self.volumes.pcm);
            let [cd_l, cd_r] = self
                .cd_resampler
                .output_buffer_pop_front()
                .unwrap()
                .map(|sample| sample * self.volumes.cd);

            let sample_l = (ym2612_l + psg + pcm_l + cd_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg + pcm_r + cd_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: SegaCdEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(&config);

        self.gen_filter.reload_config(timing_mode, &config.genesis);
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
