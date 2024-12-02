mod constants;

use bincode::{Decode, Encode};
use jgenesis_common::audio::FirResampler;
use jgenesis_common::frontend::AudioOutput;

type GbApuResampler = FirResampler<{ constants::LPF_TAPS }, 0>;

pub const GB_APU_FREQUENCY: f64 = 1_048_576.0;

fn new_gb_apu_resampler(source_frequency: f64) -> GbApuResampler {
    FirResampler::new(source_frequency, constants::LPF_COEFFICIENTS, constants::HPF_CHARGE_FACTOR)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GameBoyResampler {
    resampler: GbApuResampler,
}

impl GameBoyResampler {
    pub fn new(audio_60hz_hack: bool) -> Self {
        Self { resampler: new_gb_apu_resampler(gb_source_frequency(audio_60hz_hack)) }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_audio_60hz_hack(&mut self, audio_60hz_hack: bool) {
        self.resampler.update_source_frequency(gb_source_frequency(audio_60hz_hack));
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}

fn gb_source_frequency(audio_60hz_hack: bool) -> f64 {
    if audio_60hz_hack {
        // The Game Boy's precise refresh rate is 4.194304 MHz / (154 lines * 456 cycles/line)
        // which is approximately 59.73 Hz.
        // To target 60 FPS, pretend the APU is (60 / ~59.73) faster
        GB_APU_FREQUENCY * 60.0 / (4_194_304.0 / (154.0 * 456.0))
    } else {
        GB_APU_FREQUENCY
    }
}
