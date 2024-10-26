#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};

// 236.25MHz / 11 / 12
const NTSC_NES_AUDIO_FREQUENCY: f64 = 1789772.7272727272727273;
const NTSC_NES_NATIVE_DISPLAY_RATE: f64 = 60.0988;

// 26.6017125.MHz / 16
const PAL_NES_AUDIO_FREQUENCY: f64 = 1662607.03125;
const PAL_NES_NATIVE_DISPLAY_RATE: f64 = 50.0070;

trait TimingModeAudioExt {
    fn nes_audio_frequency(self) -> f64;

    fn nes_native_display_rate(self) -> f64;

    fn refresh_rate_multiplier(self) -> f64;
}

impl TimingModeAudioExt for TimingMode {
    fn nes_audio_frequency(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_AUDIO_FREQUENCY,
            Self::Pal => PAL_NES_AUDIO_FREQUENCY,
        }
    }

    fn nes_native_display_rate(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_NATIVE_DISPLAY_RATE,
            Self::Pal => PAL_NES_NATIVE_DISPLAY_RATE,
        }
    }

    fn refresh_rate_multiplier(self) -> f64 {
        match self {
            Self::Ntsc => 1.0,
            Self::Pal => 50.0 / 60.0,
        }
    }
}

type NesResampler = SignalResampler<93, 0>;

fn new_nes_resampler(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> NesResampler {
    let source_frequency = compute_source_frequency(timing_mode, apply_refresh_rate_adjustment);
    NesResampler::new(source_frequency, LPF_COEFFICIENT_0, LPF_COEFFICIENTS, HPF_CHARGE_FACTOR)
}

fn compute_source_frequency(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> f64 {
    let refresh_rate_multiplier = if apply_refresh_rate_adjustment {
        timing_mode.refresh_rate_multiplier() * 60.0 / timing_mode.nes_native_display_rate()
    } else {
        1.0
    };

    timing_mode.nes_audio_frequency() * refresh_rate_multiplier
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioResampler {
    timing_mode: TimingMode,
    resampler: NesResampler,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode, apply_refresh_rate_adjustment: bool) -> Self {
        Self {
            timing_mode,
            resampler: new_nes_resampler(timing_mode, apply_refresh_rate_adjustment),
        }
    }

    pub fn collect_sample(&mut self, sample: f64) {
        self.resampler.collect_sample(sample, sample);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn set_apply_refresh_rate_adjustment(&mut self, apply_refresh_rate_adjustment: bool) {
        self.resampler.update_source_frequency(compute_source_frequency(
            self.timing_mode,
            apply_refresh_rate_adjustment,
        ));
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}

const HPF_CHARGE_FACTOR: f64 = 0.9999015765;

// Generated in Octave using `fir1(93, 24000 / (1789772.72727272 / 2), 'low')`
const LPF_COEFFICIENT_0: f64 = -0.0003510245168949023;
const LPF_COEFFICIENTS: [f64; 93] = [
    -0.0003510245168949023,
    -0.0003222072340562825,
    -0.0002961513707638577,
    -0.0002695563975427637,
    -0.0002385943467757409,
    -0.0001989657274612403,
    -0.0001459672935152571,
    -7.457066521649137e-05,
    2.04894249110445e-05,
    0.000144618699031521,
    0.0003032603355412254,
    0.000501787183716972,
    0.000745391095480411,
    0.001038971369845548,
    0.001387024361052764,
    0.001793536319218136,
    0.002261881511753721,
    0.002794727614777842,
    0.003393950266900369,
    0.004060558544526072,
    0.004794632950262171,
    0.005595277306905104,
    0.006460585722197629,
    0.007387625538032744,
    0.008372436906466635,
    0.009410049348628201,
    0.01049451535653609,
    0.01161896079733627,
    0.01277565158005698,
    0.01395607575216478,
    0.01515103991243026,
    0.01635077856311961,
    0.01754507478327531,
    0.01872339039040427,
    0.01987500357435372,
    0.02098915183806726,
    0.02205517796819919,
    0.02306267668646638,
    0.0240016396016543,
    0.02486259609312817,
    0.02563674780952487,
    0.02631609456023096,
    0.02689354951074251,
    0.02736304176377287,
    0.02771960461305106,
    0.02795944799252258,
    0.02808001390594102,
    0.02808001390594102,
    0.02795944799252259,
    0.02771960461305107,
    0.02736304176377287,
    0.02689354951074251,
    0.02631609456023096,
    0.02563674780952487,
    0.02486259609312817,
    0.0240016396016543,
    0.02306267668646638,
    0.02205517796819919,
    0.02098915183806726,
    0.01987500357435372,
    0.01872339039040428,
    0.01754507478327532,
    0.01635077856311961,
    0.01515103991243026,
    0.01395607575216478,
    0.01277565158005698,
    0.01161896079733627,
    0.01049451535653609,
    0.009410049348628198,
    0.008372436906466635,
    0.007387625538032746,
    0.006460585722197629,
    0.005595277306905106,
    0.004794632950262174,
    0.004060558544526072,
    0.003393950266900369,
    0.002794727614777843,
    0.00226188151175372,
    0.001793536319218137,
    0.001387024361052765,
    0.001038971369845548,
    0.000745391095480411,
    0.0005017871837169725,
    0.0003032603355412252,
    0.0001446186990315209,
    2.048942491104479e-05,
    -7.457066521649132e-05,
    -0.0001459672935152572,
    -0.0001989657274612404,
    -0.0002385943467757411,
    -0.0002695563975427639,
    -0.0002961513707638578,
    -0.0003222072340562828,
];
