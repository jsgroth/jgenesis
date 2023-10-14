//! FIR low-pass filter for resampling Sega Master System / Game Gear PSG output to 48 KHz

use bincode::{Decode, Encode};
use std::collections::VecDeque;

const FIR_COEFFICIENT_0: f64 = -0.003259956981271349;
const FIR_COEFFICIENTS: [f64; 15] = [
    -0.003259956981271349,
    -0.005570469222224621,
    -0.007089953629792726,
    0.003470776399922447,
    0.03855014192816923,
    0.09860213748863883,
    0.1654284215535321,
    0.209868902463026,
    0.2098689024630259,
    0.1654284215535322,
    0.09860213748863887,
    0.03855014192816925,
    0.003470776399922449,
    -0.007089953629792724,
    -0.005570469222224619,
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct LowPassFilter {
    samples_l: VecDeque<f64>,
    samples_r: VecDeque<f64>,
}

impl LowPassFilter {
    pub fn new() -> Self {
        Self { samples_l: VecDeque::new(), samples_r: VecDeque::new() }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.samples_l.push_back(sample_l);
        self.samples_r.push_back(sample_r);

        if self.samples_l.len() > FIR_COEFFICIENTS.len() {
            self.samples_l.pop_front();
            self.samples_r.pop_front();
        }
    }

    pub fn output_sample(&self) -> (f64, f64) {
        let sample_l = filter_sample(&self.samples_l);
        let sample_r = filter_sample(&self.samples_r);
        (sample_l, sample_r)
    }
}

fn filter_sample(samples: &VecDeque<f64>) -> f64 {
    FIR_COEFFICIENT_0
        + FIR_COEFFICIENTS
            .iter()
            .copied()
            .zip(samples.iter().copied())
            .map(|(a, b)| a * b)
            .sum::<f64>()
}
