use bincode::{Decode, Encode};
use std::collections::VecDeque;

// Arbitrary power of 2 to keep total sample count small-ish for better f64 precision
const SAMPLE_COUNT_MODULO: u64 = 1 << 27;

pub const OUTPUT_FREQUENCY: f64 = 48000.0;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SignalResampler<const LPF_TAPS: usize, const ZERO_PADDING: usize> {
    samples_l: VecDeque<f64>,
    samples_r: VecDeque<f64>,
    output: VecDeque<(f64, f64)>,
    sample_count: u64,
    next_sample: u64,
    next_sample_float: f64,
    downsampling_ratio: f64,
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
        let downsampling_ratio = source_frequency * (ZERO_PADDING + 1) as f64 / OUTPUT_FREQUENCY;
        Self {
            samples_l: VecDeque::new(),
            samples_r: VecDeque::new(),
            output: VecDeque::new(),
            sample_count: 0,
            next_sample: downsampling_ratio.round() as u64,
            next_sample_float: downsampling_ratio,
            downsampling_ratio,
            hpf_charge_factor,
            hpf_capacitor_l: 0.0,
            hpf_capacitor_r: 0.0,
            lpf_coefficient_0,
            lpf_coefficients,
        }
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

        self.sample_count = (self.sample_count + 1) % SAMPLE_COUNT_MODULO;
        if self.sample_count == self.next_sample {
            self.next_sample_float =
                (self.next_sample_float + self.downsampling_ratio) % SAMPLE_COUNT_MODULO as f64;
            self.next_sample = (self.next_sample_float.round() as u64) % SAMPLE_COUNT_MODULO;

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
