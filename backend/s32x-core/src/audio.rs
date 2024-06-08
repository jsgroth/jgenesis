use bincode::{Decode, Encode};
use genesis_core::audio::Ym2612Resampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};
use smsgg_core::audio::PsgResampler;
use std::cmp;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::NTSC_GENESIS_MCLK_FREQUENCY;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = genesis_core::audio::PAL_GENESIS_MCLK_FREQUENCY;

const PSG_COEFFICIENT: f64 = genesis_core::audio::PSG_COEFFICIENT;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XResampler {
    ym2612_resampler: Ym2612Resampler,
    psg_resampler: PsgResampler,
}

impl Sega32XResampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        Self {
            ym2612_resampler: genesis_core::audio::new_ym2612_resampler(genesis_mclk_frequency),
            psg_resampler: smsgg_core::audio::new_psg_resampler(genesis_mclk_frequency),
        }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let samples_ready = cmp::min(
            self.ym2612_resampler.output_buffer_len(),
            self.psg_resampler.output_buffer_len(),
        );
        for _ in 0..samples_ready {
            let (ym2612_l, ym2612_r) = self.ym2612_resampler.output_buffer_pop_front().unwrap();
            let (psg_l, psg_r) = self.psg_resampler.output_buffer_pop_front().unwrap();

            let sample_l = ym2612_l + PSG_COEFFICIENT * psg_l;
            let sample_r = ym2612_r + PSG_COEFFICIENT * psg_r;
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
