#![allow(clippy::excessive_precision)]

use crate::TimingMode;
use bincode::{Decode, Encode};
use std::collections::VecDeque;

pub struct LowPassFilter {
    samples: VecDeque<f64>,
}

impl LowPassFilter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(FIR_COEFFICIENTS.len() + 1),
        }
    }

    pub fn collect_sample(&mut self, sample: f64) {
        self.samples.push_back(sample);
        if self.samples.len() > FIR_COEFFICIENTS.len() {
            self.samples.pop_front();
        }
    }

    #[must_use]
    pub fn output_sample(&self) -> f64 {
        FIR_COEFFICIENT_0
            + self
                .samples
                .iter()
                .copied()
                .zip(FIR_COEFFICIENTS.into_iter())
                .map(|(a, b)| a * b)
                .sum::<f64>()
    }
}

impl Default for LowPassFilter {
    fn default() -> Self {
        Self::new()
    }
}

// 236.25MHz / 11 / 12
const NTSC_NES_AUDIO_FREQUENCY: f64 = 1789772.7272727272727273;
const NTSC_NES_NATIVE_DISPLAY_RATE: f64 = 60.0988;

// 26.6017125.MHz / 16
const PAL_NES_AUDIO_FREQUENCY: f64 = 1662607.03125;
const PAL_NES_NATIVE_DISPLAY_RATE: f64 = 50.0070;

impl TimingMode {
    const fn nes_audio_frequency(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_AUDIO_FREQUENCY,
            Self::Pal => PAL_NES_AUDIO_FREQUENCY,
        }
    }

    const fn nes_native_display_rate(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_NES_NATIVE_DISPLAY_RATE,
            Self::Pal => PAL_NES_NATIVE_DISPLAY_RATE,
        }
    }

    fn refresh_rate_multiplier(self) -> f64 {
        match self {
            Self::Ntsc => 1.0,
            Self::Pal => PAL_NES_NATIVE_DISPLAY_RATE / NTSC_NES_NATIVE_DISPLAY_RATE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownsampleAction {
    None,
    OutputSample,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DownsampleCounter {
    sample_count: u64,
    next_output_count: u64,
    next_output_count_float: f64,
    output_count_increment: f64,
    output_frequency: f64,
    display_refresh_rate: f64,
}

impl DownsampleCounter {
    fn compute_output_count_increment(
        output_frequency: f64,
        display_refresh_rate: f64,
        timing_mode: TimingMode,
    ) -> f64 {
        timing_mode.nes_audio_frequency() / output_frequency
            * display_refresh_rate
            * timing_mode.refresh_rate_multiplier()
            / timing_mode.nes_native_display_rate()
    }

    #[must_use]
    pub fn new(output_frequency: f64, display_refresh_rate: f64) -> Self {
        let output_count_increment = Self::compute_output_count_increment(
            output_frequency,
            display_refresh_rate,
            TimingMode::Ntsc,
        );
        Self {
            sample_count: 0,
            next_output_count: output_count_increment.round() as u64,
            next_output_count_float: output_count_increment,
            output_count_increment,
            output_frequency,
            display_refresh_rate,
        }
    }

    #[must_use]
    pub fn increment(&mut self) -> DownsampleAction {
        self.sample_count += 1;

        if self.sample_count == self.next_output_count {
            self.next_output_count_float += self.output_count_increment;
            self.next_output_count = self.next_output_count_float.round() as u64;

            DownsampleAction::OutputSample
        } else {
            DownsampleAction::None
        }
    }

    pub fn set_timing_mode(&mut self, timing_mode: TimingMode) {
        self.output_count_increment = Self::compute_output_count_increment(
            self.output_frequency,
            self.display_refresh_rate,
            timing_mode,
        );
    }
}

// Generated in Octave using `fir1(93, 24000 / (1789772.72727272 / 2), 'low')`
const FIR_COEFFICIENT_0: f64 = -0.0003510245168949023;
const FIR_COEFFICIENTS: [f64; 93] = [
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
