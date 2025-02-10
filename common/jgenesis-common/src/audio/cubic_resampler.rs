use crate::audio::{
    DEFAULT_OUTPUT_FREQUENCY, RESAMPLE_SCALING_FACTOR, interpolate_cubic_hermite_6p,
};
use bincode::{Decode, Encode};
use std::collections::VecDeque;

const BUFFER_LEN: usize = 6;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CubicResampler {
    scaled_source_frequency: u64,
    output_frequency: u64,
    cycle_counter_product: u64,
    scaled_x_counter: u64,
    input_samples_l: VecDeque<f64>,
    input_samples_r: VecDeque<f64>,
    output_samples: VecDeque<(f64, f64)>,
}

impl CubicResampler {
    #[must_use]
    pub fn new(source_frequency: f64) -> Self {
        let scaled_source_frequency = scale_source_frequency(source_frequency);

        let mut resampler = Self {
            scaled_source_frequency,
            output_frequency: DEFAULT_OUTPUT_FREQUENCY,
            cycle_counter_product: 0,
            scaled_x_counter: 0,
            input_samples_l: VecDeque::with_capacity(2 * BUFFER_LEN),
            input_samples_r: VecDeque::with_capacity(2 * BUFFER_LEN),
            output_samples: VecDeque::with_capacity(48000 / 60 * 2),
        };

        resampler.input_samples_l.extend([0.0; BUFFER_LEN]);
        resampler.input_samples_r.extend([0.0; BUFFER_LEN]);

        resampler
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.input_samples_l.push_back(sample_l);
        self.input_samples_r.push_back(sample_r);

        let scaled_output_frequency = self.output_frequency * RESAMPLE_SCALING_FACTOR;
        self.cycle_counter_product += scaled_output_frequency;
        while self.cycle_counter_product >= self.scaled_source_frequency {
            self.cycle_counter_product -= self.scaled_source_frequency;

            self.scaled_x_counter += self.scaled_source_frequency;
            while self.scaled_x_counter >= scaled_output_frequency {
                self.scaled_x_counter -= scaled_output_frequency;

                self.input_samples_l.pop_front();
                self.input_samples_r.pop_front();
            }

            // Having fewer than N samples in the buffers _shouldn't_ happen, but don't crash if it does
            while self.input_samples_l.len() < BUFFER_LEN {
                self.input_samples_l.push_back(0.0);
            }
            while self.input_samples_r.len() < BUFFER_LEN {
                self.input_samples_r.push_back(0.0);
            }

            let x = (self.scaled_x_counter as f64) / (scaled_output_frequency as f64);
            let output_l =
                interpolate_cubic_hermite_6p(first_six_samples(&self.input_samples_l), x)
                    .clamp(-1.0, 1.0);
            let output_r =
                interpolate_cubic_hermite_6p(first_six_samples(&self.input_samples_r), x)
                    .clamp(-1.0, 1.0);
            self.output_samples.push_back((output_l, output_r));
        }

        // Having more than N+1 samples in the buffers here also _shouldn't_ happen, but do something reasonable if it does
        while self.input_samples_l.len() > BUFFER_LEN + 1 {
            self.input_samples_l.pop_front();
        }
        while self.input_samples_r.len() > BUFFER_LEN + 1 {
            self.input_samples_r.pop_front();
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
        self.convert_sample_distance_counter(source_frequency);
        self.scaled_source_frequency = scale_source_frequency(source_frequency);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.convert_interpolation_idx_counter(output_frequency);
        self.output_frequency = output_frequency;
    }

    fn convert_sample_distance_counter(&mut self, new_source_frequency: f64) {
        // cycle_counter_product represents the distance towards the next output sample as a percentage
        // of scaled_source_frequency
        let output_distance =
            self.cycle_counter_product as f64 / self.scaled_source_frequency as f64;
        let new_scaled_source_frequency = new_source_frequency * RESAMPLE_SCALING_FACTOR as f64;
        self.cycle_counter_product = (output_distance * new_scaled_source_frequency).floor() as u64;
    }

    fn convert_interpolation_idx_counter(&mut self, new_output_frequency: u64) {
        // scaled_x_counter represents the interpolation index as a percentage of scaled_output_frequency
        let scaled_output_frequency = (self.output_frequency * RESAMPLE_SCALING_FACTOR) as f64;
        let new_scaled_output_frequency = (new_output_frequency * RESAMPLE_SCALING_FACTOR) as f64;
        let interpolation_idx = self.scaled_x_counter as f64 / scaled_output_frequency;
        self.scaled_x_counter = (interpolation_idx * new_scaled_output_frequency).floor() as u64;
    }
}

fn first_six_samples(buffer: &VecDeque<f64>) -> [f64; 6] {
    [buffer[0], buffer[1], buffer[2], buffer[3], buffer[4], buffer[5]]
}

fn scale_source_frequency(source_frequency: f64) -> u64 {
    (source_frequency * RESAMPLE_SCALING_FACTOR as f64).round() as u64
}
