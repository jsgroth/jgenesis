use bincode::{Decode, Encode};
use std::collections::VecDeque;

pub const OUTPUT_FREQUENCY: u64 = 48000;

// Scale frequencies up by 1e9 to better handle non-integer source frequencies, e.g. the Master System PSG
const RESAMPLE_SCALING_FACTOR: u64 = 1_000_000_000;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SignalResampler<const LPF_TAPS: usize, const ZERO_PADDING: usize> {
    samples_l: VecDeque<f64>,
    samples_r: VecDeque<f64>,
    output: VecDeque<(f64, f64)>,
    sample_count_product: u64,
    padded_scaled_source_frequency: u64,
    hpf_charge_factor: f64,
    hpf_capacitor_l: f64,
    hpf_capacitor_r: f64,
    lpf_coefficient_0: f64,
    lpf_coefficients: [f64; LPF_TAPS],
}

impl<const LPF_TAPS: usize, const ZERO_PADDING: usize> SignalResampler<LPF_TAPS, ZERO_PADDING> {
    #[must_use]
    pub fn new(
        source_frequency: f64,
        lpf_coefficient_0: f64,
        lpf_coefficients: [f64; LPF_TAPS],
        hpf_charge_factor: f64,
    ) -> Self {
        let padded_scaled_source_frequency = Self::pad_and_scale_frequency(source_frequency);
        Self {
            samples_l: VecDeque::with_capacity(lpf_coefficients.len() + 1),
            samples_r: VecDeque::with_capacity(lpf_coefficients.len() + 1),
            output: VecDeque::with_capacity((OUTPUT_FREQUENCY / 30) as usize),
            sample_count_product: 0,
            padded_scaled_source_frequency,
            hpf_charge_factor,
            hpf_capacitor_l: 0.0,
            hpf_capacitor_r: 0.0,
            lpf_coefficient_0,
            lpf_coefficients,
        }
    }

    fn pad_and_scale_frequency(source_frequency: f64) -> u64 {
        (source_frequency * (ZERO_PADDING + 1) as f64 * RESAMPLE_SCALING_FACTOR as f64).round()
            as u64
    }

    fn buffer_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.samples_l.push_back(sample_l);
        self.samples_r.push_back(sample_r);

        if self.samples_l.len() > self.lpf_coefficients.len() {
            self.samples_l.pop_front();
        }
        if self.samples_r.len() > self.lpf_coefficients.len() {
            self.samples_r.pop_front();
        }

        self.sample_count_product += OUTPUT_FREQUENCY * RESAMPLE_SCALING_FACTOR;
        while self.sample_count_product >= self.padded_scaled_source_frequency {
            self.sample_count_product -= self.padded_scaled_source_frequency;

            let sample_l = output_sample(
                &self.samples_l,
                self.lpf_coefficient_0,
                &self.lpf_coefficients,
                ZERO_PADDING,
            );
            let sample_r = output_sample(
                &self.samples_r,
                self.lpf_coefficient_0,
                &self.lpf_coefficients,
                ZERO_PADDING,
            );
            self.output.push_back((sample_l, sample_r));
        }
    }

    #[inline]
    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l =
            high_pass_filter(sample_l, self.hpf_charge_factor, &mut self.hpf_capacitor_l);
        let sample_r =
            high_pass_filter(sample_r, self.hpf_charge_factor, &mut self.hpf_capacitor_r);

        self.buffer_sample(sample_l, sample_r);
        for _ in 0..ZERO_PADDING {
            self.buffer_sample(0.0, 0.0);
        }
    }

    #[inline]
    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    #[inline]
    pub fn output_buffer_pop_front(&mut self) -> Option<(f64, f64)> {
        self.output.pop_front()
    }

    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.padded_scaled_source_frequency = Self::pad_and_scale_frequency(source_frequency);
    }
}

fn high_pass_filter(sample: f64, charge_factor: f64, capacitor: &mut f64) -> f64 {
    let filtered_sample = sample - *capacitor;
    *capacitor = sample - charge_factor * filtered_sample;
    filtered_sample
}

fn output_sample<const N: usize>(
    buffer: &VecDeque<f64>,
    lpf_coefficient_0: f64,
    lpf_coefficients: &[f64; N],
    zero_padding: usize,
) -> f64 {
    let sample = lpf_coefficient_0
        + lpf_coefficients
            .iter()
            .copied()
            .zip(buffer.iter().copied())
            .map(|(a, b)| a * b)
            .sum::<f64>();
    (sample * (zero_padding + 1) as f64).clamp(-1.0, 1.0)
}
