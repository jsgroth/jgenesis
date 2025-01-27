use crate::audio::{DEFAULT_OUTPUT_FREQUENCY, RESAMPLE_SCALING_FACTOR};
use bincode::{Decode, Encode};
use std::array;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::ops::Deref;

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

// Force coefficients to be aligned to a 32-byte boundary in order to support AVX aligned loads
#[derive(Debug, Clone)]
#[repr(C, align(32))]
pub struct LpfCoefficients<const LPF_TAPS: usize>(pub [f64; LPF_TAPS]);

impl<const LPF_TAPS: usize> Deref for LpfCoefficients<LPF_TAPS> {
    type Target = [f64; LPF_TAPS];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Would be nicer if `LPF_TAPS` was an associated const, but `[f64; Self::LPF_TAPS]` doesn't compile
// without nightly features related to const generic expressions
pub trait FirKernel<const LPF_TAPS: usize> {
    fn lpf_coefficients() -> &'static LpfCoefficients<LPF_TAPS>;
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct FirResampler<const CHANNELS: usize, const LPF_TAPS: usize, Kernel: FirKernel<LPF_TAPS>> {
    input: [RingBuffer<LPF_TAPS>; CHANNELS],
    output: VecDeque<[f64; CHANNELS]>,
    sample_count_product: u64,
    scaled_output_frequency: u64,
    scaled_source_frequency: u64,
    // Required for this struct to compile with the generic Kernel type
    _marker: PhantomData<Kernel>,
}

impl<const CHANNELS: usize, const LPF_TAPS: usize, Kernel: FirKernel<LPF_TAPS>>
    FirResampler<CHANNELS, LPF_TAPS, Kernel>
{
    #[must_use]
    pub fn new(source_frequency: f64, output_frequency: u64) -> Self {
        Self {
            input: array::from_fn(|_| RingBuffer::new()),
            output: VecDeque::with_capacity((DEFAULT_OUTPUT_FREQUENCY / 30) as usize),
            sample_count_product: 0,
            scaled_output_frequency: output_frequency * RESAMPLE_SCALING_FACTOR,
            scaled_source_frequency: Self::scale_frequency(source_frequency),
            _marker: PhantomData,
        }
    }

    fn scale_frequency(source_frequency: f64) -> u64 {
        (source_frequency * RESAMPLE_SCALING_FACTOR as f64).round() as u64
    }

    #[inline]
    pub fn collect(&mut self, samples: [f64; CHANNELS]) {
        for (ch, sample) in samples.into_iter().enumerate() {
            self.input[ch].push(sample);
        }

        self.sample_count_product += self.scaled_output_frequency;
        while self.sample_count_product >= self.scaled_source_frequency {
            self.sample_count_product -= self.scaled_source_frequency;

            let output_samples =
                array::from_fn(|ch| apply_fir_filter(&self.input[ch], Kernel::lpf_coefficients()));
            self.output.push_back(output_samples);
        }
    }

    #[inline]
    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    #[inline]
    pub fn output_buffer_pop_front(&mut self) -> Option<[f64; CHANNELS]> {
        self.output.pop_front()
    }

    #[inline]
    pub fn update_output_frequency(&mut self, output_frequency: f64) {
        self.scaled_output_frequency = Self::scale_frequency(output_frequency);
    }

    #[inline]
    pub fn update_source_frequency(&mut self, source_frequency: f64) {
        self.scaled_source_frequency = Self::scale_frequency(source_frequency);
    }
}

#[allow(clippy::needless_range_loop)]
fn apply_fir_filter<const N: usize>(
    samples: &RingBuffer<N>,
    coefficients: &LpfCoefficients<N>,
) -> f64 {
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
    coefficients: &LpfCoefficients<N>,
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
        let b = _mm256_load_pd(coefficients.as_ptr().add(i));
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

    let components: [f64; 4] = transmute(sumvec);
    components.into_iter().sum()
}

pub type MonoFirResampler<const LPF_TAPS: usize, Kernel> = FirResampler<1, LPF_TAPS, Kernel>;
pub type StereoFirResampler<const LPF_TAPS: usize, Kernel> = FirResampler<2, LPF_TAPS, Kernel>;

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
