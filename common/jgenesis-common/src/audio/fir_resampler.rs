use crate::audio::{DEFAULT_OUTPUT_FREQUENCY, RESAMPLE_SCALING_FACTOR};
use bincode::{Decode, Encode};
use multiversion::multiversion;
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

// Force coefficients to be aligned to a 64-byte boundary in order to support AVX512 aligned loads
#[derive(Debug, Clone)]
#[repr(C, align(64))]
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
                apply_fir_filter(array::from_fn(|ch| &self.input[ch]), Kernel::lpf_coefficients());
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

#[multiversion(targets("x86_64+sse4.2", "x86_64+avx2+fma", "x86_64+avx512f"))]
fn apply_fir_filter<const N: usize, const CHANNELS: usize>(
    samples: [&RingBuffer<N>; CHANNELS],
    coefficients: &LpfCoefficients<N>,
) -> [f64; CHANNELS] {
    if samples[0].len >= N {
        let mut sums = [0.0_f64; CHANNELS];
        for (i, coefficient) in coefficients.iter().copied().enumerate() {
            let input_idx = samples[0].idx + i;
            for (ch, sum) in sums.iter_mut().enumerate() {
                *sum = sum.algebraic_add(coefficient.algebraic_mul(samples[ch].buffer[input_idx]));
            }
        }
        sums
    } else {
        let mut sums = [0.0_f64; CHANNELS];
        for i in N - samples[0].len..N {
            let input_idx = samples[0].idx + i - (N - samples[0].len);
            for (ch, sum) in sums.iter_mut().enumerate() {
                *sum =
                    sum.algebraic_add(coefficients[i].algebraic_mul(samples[ch].buffer[input_idx]));
            }
        }
        sums
    }
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

        let current: [f64; N] = buffer.buffer[1..=N].try_into().unwrap();

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
        assert_eq!(
            &buffer.buffer[buffer.idx..buffer.idx + N],
            &[12345.0, 56789.0, 54321.0, current[0]]
        );
    }
}
