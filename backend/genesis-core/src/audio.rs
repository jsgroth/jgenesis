//! Genesis audio resampling, filtering, and mixing code

use bincode::{Decode, Encode};
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use genesis_components::audio::{GenesisAudioFilter, LowPassSettings};
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use std::cmp;

const PSG_COEFFICIENT: f64 = genesis_components::audio::PSG_COEFFICIENT;

#[derive(Debug, Clone, Encode, Decode)]
struct VolumeMultipliers {
    ym2612: f64,
    psg: f64,
}

impl VolumeMultipliers {
    fn from_config(config: &GenesisEmulatorConfig) -> Self {
        use genesis_components::audio::volume_multiplier;

        Self {
            ym2612: volume_multiplier(config.ym2612_enabled, config.ym2612_volume_adjustment_db),
            psg: PSG_COEFFICIENT
                * volume_multiplier(config.psg_enabled, config.psg_volume_adjustment_db),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioResampler {
    filter: GenesisAudioFilter,
    ym2612_resampler: QualitySincResampler<2>,
    psg_resampler: PerformanceSincResampler<1>,
    volumes: VolumeMultipliers,
}

impl GenesisAudioResampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: &GenesisEmulatorConfig) -> Self {
        Self {
            filter: GenesisAudioFilter::new(timing_mode, LowPassSettings::from_config(config)),
            ym2612_resampler: QualitySincResampler::new(
                genesis_components::audio::ym2612_frequency(timing_mode),
                48000.0,
            ),
            psg_resampler: PerformanceSincResampler::new(
                genesis_components::audio::psg_frequency(timing_mode),
                48000.0,
            ),
            volumes: VolumeMultipliers::from_config(config),
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        let (sample_l, sample_r) = self.filter.filter_ym2612((sample_l, sample_r));
        self.ym2612_resampler.collect([sample_l, sample_r]);
    }

    pub fn collect_psg_sample(&mut self, sample: f64) {
        let sample = self.filter.filter_psg(sample);
        self.psg_resampler.collect([sample]);
    }

    /// Push all samples that are ready to the given audio output.
    ///
    /// # Errors
    ///
    /// Will propagate any error returned by the audio output while pushing samples.
    // Should never panic, the unwraps are guarded by length checks
    #[allow(clippy::missing_panics_doc)]
    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
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

            let sample_l = (ym2612_l + psg).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + psg).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn reload_config(&mut self, timing_mode: TimingMode, config: &GenesisEmulatorConfig) {
        self.volumes = VolumeMultipliers::from_config(config);

        self.filter.reload_config(timing_mode, config);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.ym2612_resampler.update_output_frequency(output_frequency as f64);
        self.psg_resampler.update_output_frequency(output_frequency as f64);
    }
}
