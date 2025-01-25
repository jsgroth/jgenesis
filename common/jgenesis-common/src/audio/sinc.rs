//! Based on this windowed sinc interpolation algorithm by Julius O. Smith III:
//!   <https://ccrma.stanford.edu/~jos/resample/resample.html>

mod performance;
mod quality;

use bincode::{Decode, Encode};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::{array, iter};

const LINEAR_INTERPOLATION_BITS: u32 = 10;

pub trait SincKernel {
    fn fir() -> &'static [f32];

    fn oversample_factor() -> u32;
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct Quality;

impl SincKernel for Quality {
    fn fir() -> &'static [f32] {
        quality::SINC_KERNEL
    }

    fn oversample_factor() -> u32 {
        quality::SINC_OVERSAMPLE_FACTOR
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct Performance;

impl SincKernel for Performance {
    fn fir() -> &'static [f32] {
        performance::SINC_KERNEL
    }

    fn oversample_factor() -> u32 {
        performance::SINC_OVERSAMPLE_FACTOR
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SincResampler<const CHANNELS: usize, Kernel> {
    input_counter: f64,
    source_rate: f64,
    target_rate: f64,
    ratio: f64,
    required_samples: usize,
    input: VecDeque<[f64; CHANNELS]>,
    output: VecDeque<[f64; CHANNELS]>,
    // Required for this to compile with the generic Kernel type
    _marker: PhantomData<Kernel>,
}

impl<const CHANNELS: usize, Kernel: SincKernel> SincResampler<CHANNELS, Kernel> {
    #[must_use]
    pub fn new(source_rate: f64, target_rate: f64) -> Self {
        let ratio = target_rate / source_rate;
        let required_samples = estimate_required_samples::<Kernel>(ratio);

        // Fill with 0s to start
        let mut input = VecDeque::with_capacity(48000 / 30);
        input.extend(iter::repeat_n([0.0; CHANNELS], required_samples));

        let resampler = Self {
            input_counter: 0.0,
            source_rate,
            target_rate,
            ratio,
            required_samples,
            input,
            output: VecDeque::with_capacity(48000 / 30),
            _marker: PhantomData,
        };

        resampler.log_debug_output();

        resampler
    }

    pub fn collect(&mut self, samples: [f64; CHANNELS]) {
        self.input.push_back(samples);

        while self.input.len() >= self.required_samples {
            self.generate_output_sample();

            self.input_counter += 1.0 / self.ratio;
            while self.input_counter >= 1.0 {
                self.input_counter -= 1.0;
                self.input.pop_front();
            }
        }
    }

    fn generate_output_sample(&mut self) {
        fn interpolation_idx_float_to_fixed_point(float: f64, oversample_factor: u32) -> u64 {
            (float * f64::from(1 << LINEAR_INTERPOLATION_BITS) * f64::from(oversample_factor))
                .round() as u64
        }

        let fir = Kernel::fir();
        let oversample_factor = Kernel::oversample_factor();

        let n = self.required_samples / 2;

        // Steps are smaller when downsampling (ratio < 1.0) to lower the low-pass filter's cutoff
        let scale = if self.ratio < 1.0 { self.ratio } else { 1.0 };

        let step = {
            let step_float = scale * f64::from(oversample_factor);
            (step_float * f64::from(1 << LINEAR_INTERPOLATION_BITS)).round() as u64
        };

        // Sum the left wing of the input window / right wing of the windowed sinc
        let interpolation_idx = {
            let idx_float = scale * self.input_counter.fract();
            interpolation_idx_float_to_fixed_point(idx_float, oversample_factor)
        };
        let l_sum = sum_wing(fir, interpolation_idx, step, |i| self.input[n - i]);

        // Sum the right wing of the input window / left wing of the windowed sinc
        let interpolation_idx = {
            let idx_float = scale * (1.0 - self.input_counter.fract());
            interpolation_idx_float_to_fixed_point(idx_float, oversample_factor)
        };
        let r_sum = sum_wing(fir, interpolation_idx, step, |i| self.input[n + 1 + i]);

        let volume = scale * f64::from(oversample_factor);
        self.output.push_back(array::from_fn(|ch| volume * (l_sum[ch] + r_sum[ch])));
    }

    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    #[must_use]
    pub fn output_buffer_pop_front(&mut self) -> Option<[f64; CHANNELS]> {
        self.output.pop_front()
    }

    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.source_rate = source_frequency;
        self.handle_frequency_update();
    }

    pub fn update_output_frequency(&mut self, output_frequency: f64) {
        self.target_rate = output_frequency;
        self.handle_frequency_update();
    }

    fn handle_frequency_update(&mut self) {
        // TODO adjust input counter?
        self.ratio = self.target_rate / self.source_rate;
        self.required_samples = estimate_required_samples::<Kernel>(self.ratio);

        self.log_debug_output();
    }

    fn log_debug_output(&self) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }

        log::debug!("Source frequency: {}", self.source_rate);
        log::debug!("Target frequency: {}", self.target_rate);
        log::debug!("Ratio: {}", self.ratio);
        log::debug!("FIR half-length: {}", Kernel::fir().len());
        log::debug!("Oversampling factor: {}", Kernel::oversample_factor());
        log::debug!("Required input samples: {}", self.required_samples);
    }
}

fn estimate_required_samples<Kernel: SincKernel>(ratio: f64) -> usize {
    let fir = Kernel::fir();
    let oversample_factor = Kernel::oversample_factor();

    let mut step: f64 = oversample_factor.into();
    if ratio < 1.0 {
        step *= ratio;
    }

    let required_half = (fir.len() as f64 / step).ceil() as usize;
    2 * required_half + 1
}

fn sum_wing<const CHANNELS: usize>(
    fir: &[f32],
    mut interpolation_idx: u64,
    step: u64,
    input_access: impl Fn(usize) -> [f64; CHANNELS],
) -> [f64; CHANNELS] {
    let mut sum = [0.0; CHANNELS];
    for i in 0.. {
        let fir_idx = (interpolation_idx >> LINEAR_INTERPOLATION_BITS) as usize;

        // Check len-1 because last entry is always 0
        if fir_idx >= fir.len() - 1 {
            break;
        }

        // Apply linear interpolation
        // Coefficient diffs are not cached as the algorithm describes because RAM speed is likely
        // going to be the bottleneck here, not calculation throughput
        let linear_factor = (interpolation_idx & ((1 << LINEAR_INTERPOLATION_BITS) - 1)) as f64
            / f64::from(1 << LINEAR_INTERPOLATION_BITS);

        let coefficient: f64 = fir[fir_idx].into();
        let next_coeff: f64 = fir[fir_idx + 1].into();
        let multiplier = coefficient + linear_factor * (next_coeff - coefficient);

        let in_samples = input_access(i);
        for ch in 0..CHANNELS {
            sum[ch] += multiplier * in_samples[ch];
        }

        interpolation_idx += step;
    }

    sum
}

pub type QualitySincResampler<const CHANNELS: usize> = SincResampler<CHANNELS, Quality>;
pub type PerformanceSincResampler<const CHANNELS: usize> = SincResampler<CHANNELS, Performance>;
