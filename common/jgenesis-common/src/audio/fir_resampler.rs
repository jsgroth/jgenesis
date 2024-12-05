use crate::audio::{DEFAULT_OUTPUT_FREQUENCY, RESAMPLE_SCALING_FACTOR};
use bincode::{Decode, Encode};
use std::collections::VecDeque;

// This is different from VecDeque in that the samples are guaranteed to always be contiguous in
// memory, which is important for performance when N is large
#[derive(Debug, Clone, Encode, Decode)]
struct RingBuffer<const N: usize> {
    buffer: Vec<f64>,
    idx: usize,
    len: usize,
}

impl<const N: usize> RingBuffer<N> {
    const fn capacity() -> usize {
        32 * N
    }

    fn new() -> Self {
        Self { buffer: vec![0.0; Self::capacity()], idx: Self::capacity(), len: 0 }
    }

    fn push(&mut self, sample: f64) {
        if self.len < N {
            self.idx -= 1;
            self.buffer[self.idx] = sample;
            self.len += 1;
            return;
        }

        if self.idx == 0 {
            for i in 1..N {
                self.buffer[Self::capacity() - N + i] = self.buffer[i - 1];
            }
            self.idx = Self::capacity() - N;
            self.buffer[self.idx] = sample;
            return;
        }

        self.idx -= 1;
        self.buffer[self.idx] = sample;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct FirResampler<const LPF_TAPS: usize, const ZERO_PADDING: usize> {
    samples_l: RingBuffer<LPF_TAPS>,
    samples_r: RingBuffer<LPF_TAPS>,
    output: VecDeque<(f64, f64)>,
    sample_count_product: u64,
    output_frequency: u64,
    padded_scaled_source_frequency: u64,
    hpf_charge_factor: f64,
    hpf_capacitor_l: f64,
    hpf_capacitor_r: f64,
    lpf_coefficients: [f64; LPF_TAPS],
}

impl<const LPF_TAPS: usize, const ZERO_PADDING: usize> FirResampler<LPF_TAPS, ZERO_PADDING> {
    #[must_use]
    pub fn new(
        source_frequency: f64,
        lpf_coefficients: [f64; LPF_TAPS],
        hpf_charge_factor: f64,
    ) -> Self {
        let padded_scaled_source_frequency = Self::pad_and_scale_frequency(source_frequency);
        Self {
            samples_l: RingBuffer::new(),
            samples_r: RingBuffer::new(),
            output: VecDeque::with_capacity((DEFAULT_OUTPUT_FREQUENCY / 30) as usize),
            sample_count_product: 0,
            output_frequency: DEFAULT_OUTPUT_FREQUENCY,
            padded_scaled_source_frequency,
            hpf_charge_factor,
            hpf_capacitor_l: 0.0,
            hpf_capacitor_r: 0.0,
            lpf_coefficients,
        }
    }

    fn pad_and_scale_frequency(source_frequency: f64) -> u64 {
        (source_frequency * (ZERO_PADDING + 1) as f64 * RESAMPLE_SCALING_FACTOR as f64).round()
            as u64
    }

    fn buffer_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.samples_l.push(sample_l);
        self.samples_r.push(sample_r);

        self.sample_count_product += self.output_frequency * RESAMPLE_SCALING_FACTOR;
        while self.sample_count_product >= self.padded_scaled_source_frequency {
            self.sample_count_product -= self.padded_scaled_source_frequency;

            let sample_l =
                output_sample::<LPF_TAPS, ZERO_PADDING>(&self.samples_l, &self.lpf_coefficients);
            let sample_r =
                output_sample::<LPF_TAPS, ZERO_PADDING>(&self.samples_r, &self.lpf_coefficients);
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

    #[inline]
    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.output_frequency = output_frequency;
    }

    #[inline]
    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.padded_scaled_source_frequency = Self::pad_and_scale_frequency(source_frequency);
    }

    #[inline]
    pub fn update_lpf_coefficients(&mut self, coefficients: [f64; LPF_TAPS]) {
        self.lpf_coefficients = coefficients;
    }
}

fn high_pass_filter(sample: f64, charge_factor: f64, capacitor: &mut f64) -> f64 {
    let filtered_sample = sample - *capacitor;
    *capacitor = sample - charge_factor * filtered_sample;
    filtered_sample
}

fn output_sample<const N: usize, const ZERO_PADDING: usize>(
    samples: &RingBuffer<N>,
    lpf_coefficients: &[f64; N],
) -> f64 {
    let sum = apply_fir_filter(samples, lpf_coefficients);
    (sum * (ZERO_PADDING + 1) as f64).clamp(-1.0, 1.0)
}

#[allow(clippy::needless_range_loop)]
fn apply_fir_filter<const N: usize>(samples: &RingBuffer<N>, coefficients: &[f64; N]) -> f64 {
    if samples.len >= N {
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::is_x86_feature_detected;
            if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
                // SAFETY: This function can only be safely called when running on a CPU that supports
                // AVX and FMA instructions, and we just verified that is the case
                unsafe {
                    return apply_fir_filter_avxfma(samples, coefficients);
                }
            }
        }

        let mut sum = 0.0;
        for (i, coefficient) in coefficients.iter().copied().enumerate() {
            sum += coefficient * samples.buffer[samples.idx + i];
        }
        sum
    } else {
        let mut sum = 0.0;
        for i in N - samples.len..N {
            sum += coefficients[i] * samples.buffer[samples.idx + i - (N - samples.len)];
        }
        sum
    }
}

// SAFETY: This function must only be called when running on a CPU that supports AVX and FMA instructions
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn apply_fir_filter_avxfma<const N: usize>(
    samples: &RingBuffer<N>,
    coefficients: &[f64; N],
) -> f64 {
    #[allow(clippy::wildcard_imports)]
    use std::arch::x86_64::*;
    use std::mem::transmute;

    debug_assert!(samples.len == N);
    let samples = &samples.buffer[samples.idx..samples.idx + N];

    // Sum all chunks of 4 samples using f64x4 vectors
    let mut sumvec = _mm256_setzero_pd();
    for i in (0..N & !3).step_by(4) {
        let a = _mm256_loadu_pd(samples.as_ptr().add(i));
        let b = _mm256_loadu_pd(coefficients.as_ptr().add(i));
        sumvec = _mm256_fmadd_pd(a, b, sumvec);
    }

    // Manual loop unroll to add in the last chunk of 0-3 samples
    // The match should be optimized out at compile time since N is a const generic
    match N & 3 {
        0 => {}
        1 => {
            let a = _mm256_set_pd(samples[N - 1], 0.0, 0.0, 0.0);
            let b = _mm256_set_pd(coefficients[N - 1], 0.0, 0.0, 0.0);
            sumvec = _mm256_fmadd_pd(a, b, sumvec);
        }
        2 => {
            let a = _mm256_set_pd(samples[N - 2], samples[N - 1], 0.0, 0.0);
            let b = _mm256_set_pd(coefficients[N - 2], coefficients[N - 1], 0.0, 0.0);
            sumvec = _mm256_fmadd_pd(a, b, sumvec);
        }
        3 => {
            let a = _mm256_set_pd(samples[N - 3], samples[N - 2], samples[N - 1], 0.0);
            let b =
                _mm256_set_pd(coefficients[N - 3], coefficients[N - 2], coefficients[N - 1], 0.0);
            sumvec = _mm256_fmadd_pd(a, b, sumvec);
        }
        _ => unreachable!("value & 3 is always <= 3"),
    }

    // Compute the final sum using an AVX hadd followed by a single scalar add
    let hsum = _mm256_hadd_pd(sumvec, sumvec);
    let sum_components: [f64; 4] = transmute(hsum);
    sum_components[0] + sum_components[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_basic() {
        let mut buffer = RingBuffer::<3>::new();
        assert_eq!(buffer.idx, buffer.buffer.len());
        assert_eq!(buffer.len, 0);

        buffer.push(3.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - 1);
        assert_eq!(buffer.len, 1);
        assert_eq!(buffer.buffer[buffer.idx], 3.0);

        buffer.push(5.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - 2);
        assert_eq!(buffer.len, 2);
        assert_eq!(&buffer.buffer[buffer.idx..buffer.idx + 2], &[5.0, 3.0]);

        buffer.push(7.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - 3);
        assert_eq!(buffer.len, 3);
        assert_eq!(&buffer.buffer[buffer.idx..buffer.idx + 3], &[7.0, 5.0, 3.0]);

        // Buffer is now full; next push should move the starting point but not increase length
        buffer.push(9.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - 4);
        assert_eq!(buffer.len, 3);
        assert_eq!(&buffer.buffer[buffer.idx..buffer.idx + 3], &[9.0, 7.0, 5.0]);

        // Push one more
        buffer.push(11.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - 5);
        assert_eq!(buffer.len, 3);
        assert_eq!(&buffer.buffer[buffer.idx..buffer.idx + 3], &[11.0, 9.0, 7.0]);
    }

    #[test]
    fn ring_buffer_wrap() {
        const N: usize = 4;

        let mut buffer = RingBuffer::<N>::new();
        for i in 0..buffer.buffer.len() {
            buffer.buffer[i] = (i + 5) as f64;
        }
        buffer.idx = 1;
        buffer.len = N;

        let current: [f64; N] = buffer.buffer[1..N + 1].try_into().unwrap();

        // Last push before buffer is full
        buffer.push(54321.0);
        assert_eq!(buffer.idx, 0);
        assert_eq!(buffer.len, N);
        assert_eq!(&buffer.buffer[0..N], &[54321.0, current[0], current[1], current[2]]);

        // Push while buffer is full should copy contents to the end of the buffer
        buffer.push(56789.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - N);
        assert_eq!(buffer.len, N);
        assert_eq!(&buffer.buffer[buffer.idx..], &[56789.0, 54321.0, current[0], current[1]]);

        buffer.push(12345.0);
        assert_eq!(buffer.idx, buffer.buffer.len() - N - 1);
        assert_eq!(buffer.len, N);
        assert_eq!(&buffer.buffer[buffer.idx..buffer.idx + N], &[
            12345.0, 56789.0, 54321.0, current[0]
        ]);
    }
}
