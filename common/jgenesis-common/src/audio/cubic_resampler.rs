use crate::audio::{RESAMPLE_SCALING_FACTOR, interpolate_cubic_hermite_6p};
use bincode::{Decode, Encode};
use std::array;
use std::collections::VecDeque;

const BUFFER_LEN: usize = 6;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CubicResampler<const CHANNELS: usize> {
    scaled_source_frequency: u64,
    output_frequency: u64,
    cycle_counter_product: u64,
    scaled_x_counter: u64,
    input_samples: VecDeque<[f64; CHANNELS]>,
    output_samples: VecDeque<[f64; CHANNELS]>,
}

impl<const CHANNELS: usize> CubicResampler<CHANNELS> {
    #[must_use]
    pub fn new(source_frequency: f64, output_frequency: u64) -> Self {
        let scaled_source_frequency = scale_source_frequency(source_frequency);

        let mut resampler = Self {
            scaled_source_frequency,
            output_frequency,
            cycle_counter_product: 0,
            scaled_x_counter: 0,
            input_samples: VecDeque::with_capacity(2 * BUFFER_LEN),
            output_samples: VecDeque::with_capacity(48000 / 60 * 2),
        };

        resampler.input_samples.extend([[0.0; CHANNELS]; BUFFER_LEN]);

        resampler
    }

    pub fn collect_sample(&mut self, samples: [f64; CHANNELS]) {
        self.input_samples.push_back(samples);

        let scaled_output_frequency = self.output_frequency * RESAMPLE_SCALING_FACTOR;
        self.cycle_counter_product += scaled_output_frequency;
        while self.cycle_counter_product >= self.scaled_source_frequency {
            self.cycle_counter_product -= self.scaled_source_frequency;

            // Having fewer than N samples in the buffers _shouldn't_ happen, but don't crash if it does
            while self.input_samples.len() < BUFFER_LEN {
                self.input_samples
                    .push_front(self.input_samples.front().copied().unwrap_or([0.0; CHANNELS]));
            }

            let x = (self.scaled_x_counter as f64) / (scaled_output_frequency as f64);
            let output: [f64; CHANNELS] = array::from_fn(|channel| {
                let samples: [f64; 6] = array::from_fn(|i| self.input_samples[i][channel]);
                interpolate_cubic_hermite_6p(samples, x).clamp(-1.0, 1.0)
            });

            self.output_samples.push_back(output);

            self.scaled_x_counter += self.scaled_source_frequency;
            while self.scaled_x_counter >= scaled_output_frequency {
                self.scaled_x_counter -= scaled_output_frequency;

                self.input_samples.pop_front();
            }
        }

        // Having more than N+1 samples in the buffers here also _shouldn't_ happen, but do something reasonable if it does
        while self.input_samples.len() > BUFFER_LEN + 1 {
            self.input_samples.pop_front();
        }
    }

    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output_samples.len()
    }

    #[must_use]
    pub fn output_buffer_pop_front(&mut self) -> Option<[f64; CHANNELS]> {
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

fn scale_source_frequency(source_frequency: f64) -> u64 {
    (source_frequency * RESAMPLE_SCALING_FACTOR as f64).round() as u64
}
