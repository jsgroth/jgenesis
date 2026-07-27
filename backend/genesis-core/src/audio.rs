//! Genesis audio resampling, filtering, and mixing code

use crate::api::GenesisHardware;
use crate::bus::GenesisAudioOutput;
use bincode::{Decode, Encode};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use genesis_components::audio::{GenesisAudioFilter, LowPassSettings};
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use segacd_core::api::SegaCdAudioOutput;
use segacd_core::audio::SegaCdAudioFilter;

const PSG_COEFFICIENT: f64 = genesis_components::audio::PSG_COEFFICIENT;

// -6 dB (10 ^ -6/20)
const PCM_COEFFICIENT: f64 = 0.5011872336272722;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

const SEGA_CD_MCLK_FREQUENCY: f64 = segacd_core::audio::SEGA_CD_MCLK_FREQUENCY;
const CD_DA_FREQUENCY: f64 = segacd_core::audio::CD_DA_FREQUENCY;

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
    pcm: f64,
    cd: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &GenesisEmulatorConfig) -> Self {
        use genesis_components::audio::volume_multiplier;

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
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    filter: GenesisAudioFilter,
    sega_cd_filter: SegaCdAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    pcm_resampler: QualitySincResampler<2>,
    cd_resampler: QualitySincResampler<2>,
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
        let pcm_frequency = SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0;

        Self {
            filter: GenesisAudioFilter::new(timing_mode, LowPassSettings::from_config(config)),
            sega_cd_filter: SegaCdAudioFilter::new(config),
            ym2612_resampler: QualitySincResampler::new(
                genesis_components::audio::ym2612_frequency(timing_mode),
                48000.0,
            ),
            psg_resampler: PerformanceSincResampler::new(
                genesis_components::audio::psg_frequency(timing_mode),
                48000.0,
            ),
            pcm_resampler: QualitySincResampler::new(pcm_frequency, 48000.0),
            cd_resampler: QualitySincResampler::new(CD_DA_FREQUENCY, 48000.0),
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

            let sample_l = (ym2612_l + psg + pcm_l + cd_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg + pcm_r + cd_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(config);

        self.filter.reload_config(timing_mode, config);
        self.sega_cd_filter.reload_config(config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        let output_frequency = output_frequency as f64;

        self.ym2612_resampler.update_output_frequency(output_frequency);
        self.psg_resampler.update_output_frequency(output_frequency);
        self.pcm_resampler.update_output_frequency(output_frequency);
        self.cd_resampler.update_output_frequency(output_frequency);
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
