//! Genesis audio resampling, filtering, and mixing code

use crate::api::GenesisHardware;
use crate::bus::GenesisAudioOutput;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::{FirstOrderIirFilter, IirFilter, SecondOrderIirFilter};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use genesis_config::{GenesisEmulatorConfig, LowPassSettings};
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use s32x_core::api::Sega32XAudioOutput;
use s32x_core::audio::PwmResampler;
use segacd_core::api::SegaCdAudioOutput;
use segacd_core::audio::SegaCdAudioFilter;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_components::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_components::PAL_GENESIS_MCLK_FREQUENCY;

const PCM_FREQUENCY: f64 = segacd_core::audio::PCM_FREQUENCY;
const CD_DA_FREQUENCY: f64 = segacd_core::audio::CD_DA_FREQUENCY;

// -7 dB (10 ^ -7/20)
const PSG_COEFFICIENT: f64 = 0.44668359215096315;

// -6 dB (10 ^ -6/20)
const PCM_COEFFICIENT: f64 = 0.5011872336272722;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

// -2 dB (10 ^ -2/20))
const PWM_COEFFICIENT: f64 = 0.7943282347242815;

#[must_use]
pub fn new_ym2612_low_pass<const N: usize>(timing_mode: TimingMode, cutoff: u32) -> IirFilter<N> {
    dsp::design::butterworth(cutoff.into(), ym2612_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_psg_low_pass(timing_mode: TimingMode, cutoff: u32) -> FirstOrderIirFilter {
    dsp::design::butterworth(cutoff.into(), psg_frequency(timing_mode), FilterType::LowPass)
}

#[must_use]
pub fn new_ym2612_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, ym2612_frequency(timing_mode), FilterType::HighPass)
}

#[must_use]
pub fn new_psg_dc_offset(timing_mode: TimingMode) -> FirstOrderIirFilter {
    dsp::design::butterworth(5.0, psg_frequency(timing_mode), FilterType::HighPass)
}

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
    pcm: f64,
    cd: f64,
    pwm: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &GenesisEmulatorConfig) -> Self {
        Self {
            ym2612: volume_multiplier(config.ym2612_enabled, config.ym2612_volume_adjustment_db),
            psg: PSG_COEFFICIENT
                * volume_multiplier(config.psg_enabled, config.psg_volume_adjustment_db),
            pcm: PCM_COEFFICIENT
                * volume_multiplier(
                    config.sega_cd.pcm_enabled,
                    config.sega_cd.pcm_volume_adjustment_db,
                ),
            cd: CD_COEFFICIENT
                * volume_multiplier(
                    config.sega_cd.cd_audio_enabled,
                    config.sega_cd.cd_volume_adjustment_db,
                ),
            pwm: PWM_COEFFICIENT
                * volume_multiplier(
                    config.sega_32x.pwm_enabled,
                    config.sega_32x.pwm_volume_adjustment_db,
                ),
        }
    }
}

#[must_use]
fn volume_multiplier(enabled: bool, adjustment_db: f64) -> f64 {
    if !enabled {
        return 0.0;
    }

    // Decibels to linear
    10.0_f64.powf(adjustment_db / 20.0)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioFilter {
    ym2612_dc_offset_l: FirstOrderIirFilter,
    ym2612_dc_offset_r: FirstOrderIirFilter,
    psg_dc_offset: FirstOrderIirFilter,
    low_pass_settings: LowPassSettings,
    ym2612_gen_low_pass_l: FirstOrderIirFilter,
    ym2612_gen_low_pass_r: FirstOrderIirFilter,
    ym2612_2nd_low_pass_l: SecondOrderIirFilter,
    ym2612_2nd_low_pass_r: SecondOrderIirFilter,
    psg_low_pass: FirstOrderIirFilter,
}

impl GenesisAudioFilter {
    #[must_use]
    pub fn new(timing_mode: TimingMode, low_pass_settings: LowPassSettings) -> Self {
        Self {
            ym2612_dc_offset_l: new_ym2612_dc_offset(timing_mode),
            ym2612_dc_offset_r: new_ym2612_dc_offset(timing_mode),
            psg_dc_offset: new_psg_dc_offset(timing_mode),
            low_pass_settings,
            ym2612_gen_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_gen_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.genesis_cutoff,
            ),
            ym2612_2nd_low_pass_l: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            ym2612_2nd_low_pass_r: new_ym2612_low_pass(
                timing_mode,
                low_pass_settings.ym2612_2nd_cutoff,
            ),
            psg_low_pass: new_psg_low_pass(timing_mode, low_pass_settings.genesis_cutoff),
        }
    }

    #[must_use]
    pub fn filter_ym2612(&mut self, (sample_l, sample_r): (f64, f64)) -> (f64, f64) {
        let (mut sample_l, mut sample_r) =
            (self.ym2612_dc_offset_l.filter(sample_l), self.ym2612_dc_offset_r.filter(sample_r));

        if self.low_pass_settings.ym2612_2nd_enabled {
            sample_l = self.ym2612_2nd_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_2nd_low_pass_r.filter(sample_r);
        }

        if self.low_pass_settings.genesis_enabled {
            sample_l = self.ym2612_gen_low_pass_l.filter(sample_l);
            sample_r = self.ym2612_gen_low_pass_r.filter(sample_r);
        }

        (sample_l, sample_r)
    }

    #[must_use]
    pub fn filter_psg(&mut self, sample: f64) -> f64 {
        let sample = self.psg_dc_offset.filter(sample);

        if !self.low_pass_settings.genesis_enabled {
            return sample;
        }

        self.psg_low_pass.filter(sample)
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        let low_pass_settings = LowPassSettings::from_config(config);
        if self.low_pass_settings == low_pass_settings {
            return;
        }

        *self = Self::new(timing_mode, low_pass_settings);
    }
}

#[must_use]
pub fn ym2612_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 7.0 / 6.0 / 24.0
}

#[must_use]
pub fn psg_frequency(timing_mode: TimingMode) -> f64 {
    let genesis_mclk_frequency = match timing_mode {
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    genesis_mclk_frequency / 15.0 / 16.0
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    filter: GenesisAudioFilter,
    sega_cd_filter: SegaCdAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    pcm_resampler: QualitySincResampler<2>,
    cd_resampler: QualitySincResampler<2>,
    pwm_resampler: PwmResampler,
    volumes: VolumeMultipliers,
    sega_cd_present: bool,
    s32x_present: bool,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(
        hardware: GenesisHardware,
        timing_mode: TimingMode,
        config: &GenesisEmulatorConfig,
    ) -> Self {
        Self {
            filter: GenesisAudioFilter::new(timing_mode, LowPassSettings::from_config(config)),
            sega_cd_filter: SegaCdAudioFilter::new(config),
            ym2612_resampler: QualitySincResampler::new(ym2612_frequency(timing_mode), 48000.0),
            psg_resampler: PerformanceSincResampler::new(psg_frequency(timing_mode), 48000.0),
            pcm_resampler: QualitySincResampler::new(PCM_FREQUENCY, 48000.0),
            cd_resampler: QualitySincResampler::new(CD_DA_FREQUENCY, 48000.0),
            pwm_resampler: PwmResampler::new(config, 48000),
            volumes: VolumeMultipliers::from_config(config),
            sega_cd_present: hardware.has_sega_cd(),
            s32x_present: hardware.has_32x(),
        }
    }

    /// Push all samples that are ready to the given audio output.
    ///
    /// # Errors
    ///
    /// Will propagate any error returned by the audio output while pushing samples.
    // Should never panic, the unwraps are guarded by length checks
    #[allow(clippy::missing_panics_doc)]
    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = [
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
            if self.sega_cd_present { self.pcm_resampler.output_buffer_len() } else { usize::MAX },
            if self.sega_cd_present { self.cd_resampler.output_buffer_len() } else { usize::MAX },
            if self.s32x_present { self.pwm_resampler.output_buffer_len() } else { usize::MAX },
        ]
        .into_iter()
        .min()
        .unwrap();

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

            let [pcm_l, pcm_r] = if self.sega_cd_present {
                self.pcm_resampler
                    .output_buffer_pop_front()
                    .unwrap()
                    .map(|sample| sample * self.volumes.pcm)
            } else {
                [0.0; 2]
            };

            let [cd_l, cd_r] = if self.sega_cd_present {
                self.cd_resampler
                    .output_buffer_pop_front()
                    .unwrap()
                    .map(|sample| sample * self.volumes.cd)
            } else {
                [0.0; 2]
            };

            let [pwm_l, pwm_r] = if self.s32x_present {
                self.pwm_resampler
                    .output_buffer_pop_front()
                    .unwrap()
                    .map(|sample| sample * self.volumes.pwm)
            } else {
                [0.0; 2]
            };

            let sample_l = (ym2612_l + psg + pcm_l + cd_l + pwm_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg + pcm_r + cd_r + pwm_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(config);

        self.filter.reload_config(timing_mode, config);
        self.sega_cd_filter.reload_config(config);
        self.pwm_resampler.reload_config(config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        let output_frequency = output_frequency as f64;

        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
        self.pcm_resampler.update_output_frequency(output_frequency);
        self.cd_resampler.update_output_frequency(output_frequency);
        self.pwm_resampler.update_output_frequency(output_frequency as u64);
    }
}

impl GenesisAudioOutput for GenesisAudioResampler {
    fn collect_ym2612(&mut self, sample: (f64, f64)) {
        let (sample_l, sample_r) = self.filter.filter_ym2612(sample);
        self.ym2612_resampler.collect([sample_l, sample_r])
    }

    fn collect_psg(&mut self, sample: f64) {
        let sample = self.filter.filter_psg(sample);
        self.psg_resampler.collect([sample]);
    }
}

impl SegaCdAudioOutput for GenesisAudioResampler {
    fn collect_pcm(&mut self, sample: (f64, f64)) {
        let (sample_l, sample_r) = self.sega_cd_filter.filter_pcm(sample);
        self.pcm_resampler.collect([sample_l, sample_r]);
    }

    fn collect_cd(&mut self, sample: (f64, f64)) {
        let (sample_l, sample_r) = self.sega_cd_filter.filter_cd_da(sample);
        self.cd_resampler.collect([sample_l, sample_r]);
    }
}

impl Sega32XAudioOutput for GenesisAudioResampler {
    fn collect_pwm(&mut self, (sample_l, sample_r): (f64, f64)) {
        self.pwm_resampler.collect_sample(sample_l, sample_r);
    }

    fn update_pwm_source_frequency(&mut self, frequency: f64) {
        self.pwm_resampler.update_source_frequency(frequency);
    }
}
