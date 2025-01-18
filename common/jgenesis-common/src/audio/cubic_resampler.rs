use crate::audio::{
    DEFAULT_OUTPUT_FREQUENCY, RESAMPLE_SCALING_FACTOR, interpolate_cubic_hermite_4p,
};
use bincode::{Decode, Encode};
use std::collections::VecDeque;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CubicResampler {
    scaled_source_frequency: u64,
    output_frequency: u64,
    cycle_counter_product: u64,
    scaled_x_counter: u64,
    input_samples_l: [f64; 4],
    input_samples_r: [f64; 4],
    output_samples: VecDeque<(f64, f64)>,
}

impl CubicResampler {
    #[must_use]
    pub fn new(source_frequency: f64) -> Self {
        let scaled_source_frequency = scale_source_frequency(source_frequency);

        Self {
            scaled_source_frequency,
            output_frequency: DEFAULT_OUTPUT_FREQUENCY,
            cycle_counter_product: 0,
            scaled_x_counter: 0,
            input_samples_l: [0.0; 4],
            input_samples_r: [0.0; 4],
            output_samples: VecDeque::with_capacity(48000 / 60 * 2),
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        push_sample(&mut self.input_samples_l, sample_l);
        push_sample(&mut self.input_samples_r, sample_r);

        let scaled_output_frequency = self.output_frequency * RESAMPLE_SCALING_FACTOR;
        self.cycle_counter_product += scaled_output_frequency;
        while self.cycle_counter_product >= self.scaled_source_frequency {
            self.cycle_counter_product -= self.scaled_source_frequency;

            self.scaled_x_counter += self.scaled_source_frequency;
            while self.scaled_x_counter >= scaled_output_frequency {
                self.scaled_x_counter -= scaled_output_frequency;
            }

            let x = (self.scaled_x_counter as f64) / (scaled_output_frequency as f64);
            let output_l = interpolate_cubic_hermite_4p(self.input_samples_l, x).clamp(-1.0, 1.0);
            let output_r = interpolate_cubic_hermite_4p(self.input_samples_r, x).clamp(-1.0, 1.0);
            self.output_samples.push_back((output_l, output_r));
        }
    }

    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output_samples.len()
    }

    #[must_use]
    pub fn output_buffer_pop_front(&mut self) -> Option<(f64, f64)> {
        self.output_samples.pop_front()
    }

    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.scaled_source_frequency = scale_source_frequency(source_frequency);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.output_frequency = output_frequency;
    }
}

fn scale_source_frequency(source_frequency: f64) -> u64 {
    (source_frequency * RESAMPLE_SCALING_FACTOR as f64).round() as u64
}

fn push_sample(samples: &mut [f64; 4], sample: f64) {
    for i in 0..3 {
        samples[i] = samples[i + 1];
    }
    samples[3] = sample;
}
