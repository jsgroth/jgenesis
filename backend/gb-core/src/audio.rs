use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::AudioOutput;

type GbApuResampler = SignalResampler<45, 0>;

pub const GB_APU_FREQUENCY: f64 = 1_048_576.0;

fn new_gb_apu_resampler(source_frequency: f64) -> GbApuResampler {
    SignalResampler::new(source_frequency, FIR_COEFFICIENT_0, FIR_COEFFICIENTS, HPF_CHARGE_FACTOR)
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

// High-pass filter capacitor charge factor; 0.999958.powi(4)
const HPF_CHARGE_FACTOR: f64 = 0.999832;

// Generated in Octave using `fir1(45, 24000 / (1048576 / 2), 'low')`
const FIR_COEFFICIENT_0: f64 = -3.34604057969547e-05;

#[allow(clippy::excessive_precision)]
const FIR_COEFFICIENTS: [f64; 45] = [
    -3.346040579695476e-05,
    0.0001699738848263266,
    0.0004537317884742157,
    0.0008908900760124843,
    0.001556924944735581,
    0.00252443316899384,
    0.003857708455430148,
    0.005607524372877115,
    0.007806473251024514,
    0.01046518534367281,
    0.0135697055269241,
    0.01708023853733914,
    0.02093139226830607,
    0.02503395703027764,
    0.02927816278496245,
    0.03353826239060251,
    0.03767820300070183,
    0.04155807563435454,
    0.04504097942654724,
    0.04799990583607647,
    0.05032424136886497,
    0.05192550580621755,
    0.05274198550857523,
    0.05274198550857522,
    0.05192550580621755,
    0.05032424136886497,
    0.04799990583607648,
    0.04504097942654725,
    0.04155807563435453,
    0.03767820300070183,
    0.03353826239060252,
    0.02927816278496245,
    0.02503395703027764,
    0.02093139226830607,
    0.01708023853733914,
    0.01356970552692411,
    0.01046518534367282,
    0.007806473251024512,
    0.005607524372877117,
    0.003857708455430151,
    0.002524433168993842,
    0.001556924944735584,
    0.0008908900760124837,
    0.0004537317884742162,
    0.0001699738848263265,
];
