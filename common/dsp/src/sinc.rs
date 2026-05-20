//! Based on this windowed sinc interpolation algorithm by Julius O. Smith III:
//!   <https://ccrma.stanford.edu/~jos/resample/resample.html>

mod performance;
mod quality;

use bincode::{Decode, Encode};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::{array, iter};

const LINEAR_INTERPOLATION_BITS: u32 = 20;

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
struct SampleRingBuffer {
    buffer: Vec<f64>,
    idx: usize,
    len: usize,
}

impl SampleRingBuffer {
    // Always leave space at the beginning and end so AVX512 loads can read the first and last
    // samples without going out of bounds
    const EXTRA_SPACE: usize = 8;

    // For an N-sample buffer, allows roughly 64*N^2 samples before needing to copy back to the
    // beginning
    const CAPACITY_MULTIPLIER: usize = 64;

    fn new(required_samples: usize) -> Self {
        assert_ne!(required_samples, 0);

        let buffer_len = Self::CAPACITY_MULTIPLIER * required_samples;

        Self { buffer: vec![0.0; buffer_len], idx: Self::EXTRA_SPACE, len: 0 }
    }

    fn ensure_capacity(&mut self, required_samples: usize) {
        let buffer_len = Self::CAPACITY_MULTIPLIER * required_samples;
        let additional = buffer_len.saturating_sub(self.buffer.len());
        if additional != 0 {
            self.buffer.extend(iter::repeat_n(0.0, additional));
        }
    }

    fn push(&mut self, sample: f64) {
        if self.idx + self.len == self.buffer.len() - Self::EXTRA_SPACE {
            // Copy from end of buffer to start
            let left_len = Self::EXTRA_SPACE + self.len;
            let (left, right) = self.buffer.split_at_mut(left_len);

            let copy_start = self.idx - left_len;
            left[Self::EXTRA_SPACE..].copy_from_slice(&right[copy_start..copy_start + self.len]);

            self.idx = Self::EXTRA_SPACE;
        }

        self.buffer[self.idx + self.len] = sample;
        self.len += 1;
    }

    fn pop(&mut self) {
        if self.len != 0 {
            self.idx += 1;
            self.len -= 1;
        }
    }

    #[cfg(test)]
    fn as_slice(&self) -> &[f64] {
        &self.buffer[self.idx..self.idx + self.len]
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SincResampler<const CHANNELS: usize, Kernel: SincKernel> {
    input_counter: f64,
    source_rate: f64,
    target_rate: f64,
    ratio: f64,
    required_samples: usize,
    input: [SampleRingBuffer; CHANNELS],
    output: VecDeque<[f64; CHANNELS]>,
    // Required for this to compile with the generic Kernel type
    _marker: PhantomData<Kernel>,
}

impl<const CHANNELS: usize, Kernel: SincKernel> SincResampler<CHANNELS, Kernel> {
    #[must_use]
    pub fn new(source_rate: f64, target_rate: f64) -> Self {
        let ratio = target_rate / source_rate;
        let required_samples = estimate_required_samples::<Kernel>(ratio);

        let resampler = Self {
            input_counter: 0.0,
            source_rate,
            target_rate,
            ratio,
            required_samples,
            input: array::from_fn(|_| SampleRingBuffer::new(required_samples)),
            output: VecDeque::with_capacity(48000 / 30),
            _marker: PhantomData,
        };

        resampler.log_debug_output();

        resampler
    }

    #[inline]
    pub fn collect(&mut self, samples: [f64; CHANNELS]) {
        for (sample, input) in iter::zip(samples, &mut self.input) {
            input.push(sample);
        }

        while self.input[0].len >= self.required_samples {
            self.generate_output_sample();

            self.input_counter += 1.0 / self.ratio;
            while self.input_counter >= 1.0 {
                self.input_counter -= 1.0;
                for input in &mut self.input {
                    input.pop();
                }
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

        let input_slices: [_; CHANNELS] = array::from_fn(|ch| self.input[ch].buffer.as_slice());

        // Sum the left wing of the input window / right wing of the windowed sinc
        let interpolation_idx = {
            let idx_float = scale * self.input_counter.fract();
            interpolation_idx_float_to_fixed_point(idx_float, oversample_factor)
        };
        let l_sum =
            sum_wing::<true, _>(fir, interpolation_idx, step, input_slices, n + self.input[0].idx);

        // Sum the right wing of the input window / left wing of the windowed sinc
        let interpolation_idx = {
            let idx_float = scale * (1.0 - self.input_counter.fract());
            interpolation_idx_float_to_fixed_point(idx_float, oversample_factor)
        };
        let r_sum =
            sum_wing::<false, _>(fir, interpolation_idx, step, input_slices, n + self.input[0].idx);

        let volume = scale * f64::from(oversample_factor);
        self.output.push_back(array::from_fn(|ch| volume * (l_sum[ch] + r_sum[ch])));
    }

    #[inline]
    #[must_use]
    pub fn output_buffer_len(&self) -> usize {
        self.output.len()
    }

    #[inline]
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

        for input in &mut self.input {
            input.ensure_capacity(self.required_samples);
        }

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

fn sum_wing<const REVERSE: bool, const CHANNELS: usize>(
    fir: &[f32],
    interpolation_idx: u64,
    step: u64,
    input: [&[f64]; CHANNELS],
    n: usize,
) -> [f64; CHANNELS] {
    #[cfg(target_arch = "x86_64")]
    {
        use std::sync::LazyLock;

        static AVX512_SUPPORTED: LazyLock<bool> = LazyLock::new(|| {
            crate::AVX512_ENABLED
                && is_x86_feature_detected!("avx512f")
                && is_x86_feature_detected!("avx512vl")
                && is_x86_feature_detected!("avx512dq")
        });

        static AVX2_SUPPORTED: LazyLock<bool> = LazyLock::new(|| {
            crate::AVX2_ENABLED
                && is_x86_feature_detected!("avx2")
                && is_x86_feature_detected!("fma")
        });

        if *AVX512_SUPPORTED {
            // SAFETY: This CPU supports AVX512 (F + VL + DQ)
            unsafe {
                return sum_wing_avx512::<REVERSE, _>(fir, interpolation_idx, step, input, n);
            }
        }

        if *AVX2_SUPPORTED {
            // SAFETY: This CPU supports AVX2 and FMA
            unsafe {
                return sum_wing_avx2::<REVERSE, _>(fir, interpolation_idx, step, input, n);
            }
        }
    }

    sum_wing_no_avx::<REVERSE, _>(fir, interpolation_idx, step, input, n)
}

fn sum_wing_no_avx<const REVERSE: bool, const CHANNELS: usize>(
    fir: &[f32],
    mut interpolation_idx: u64,
    step: u64,
    input: [&[f64]; CHANNELS],
    n: usize,
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

        let input_idx = if REVERSE { n - i } else { n + i + 1 };

        for ch in 0..CHANNELS {
            let in_sample = input[ch][input_idx];
            sum[ch] += multiplier * in_sample;
        }

        interpolation_idx += step;
    }

    sum
}

// SAFETY: Can only be called on a CPU that supports AVX2 and FMA instructions
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
fn sum_wing_avx2<const REVERSE: bool, const CHANNELS: usize>(
    fir: &[f32],
    interpolation_idx: u64,
    step: u64,
    input: [&[f64]; CHANNELS],
    n: usize,
) -> [f64; CHANNELS] {
    #[allow(clippy::wildcard_imports)]
    use std::arch::x86_64::*;
    use std::hint::cold_path;
    use std::mem::transmute;

    const LINEAR_FACTOR_MASK: i64 = (1 << LINEAR_INTERPOLATION_BITS) - 1;
    const LINEAR_FACTOR_MULTIPLIER: f64 = 1.0 / (1 << LINEAR_INTERPOLATION_BITS) as f64;

    // SAFETY: Later code assumes all input slices are the same length and that n < input.len()
    assert!(n < input[0].len());
    if CHANNELS > 1 {
        assert!(input[1..].iter().all(|channel_input| channel_input.len() == input[0].len()));
    }

    let mut sums = [_mm256_setzero_pd(); CHANNELS];

    let initial_steps = if REVERSE {
        _mm256_set_epi64x(0, step as i64, (2 * step) as i64, (3 * step) as i64)
    } else {
        _mm256_setr_epi64x(0, step as i64, (2 * step) as i64, (3 * step) as i64)
    };

    let mut interpolation_idxs =
        _mm256_add_epi64(_mm256_set1_epi64x(interpolation_idx as i64), initial_steps);

    for i in (0..).step_by(4) {
        let fir_idxs =
            _mm256_srli_epi64::<{ LINEAR_INTERPOLATION_BITS as i32 }>(interpolation_idxs);

        // SAFETY: Compare to len-1 instead of len because FIR is read using 64-bit loads
        let in_bounds = _mm256_xor_si256(
            _mm256_set1_epi32(!0),
            _mm256_cmpgt_epi64(fir_idxs, _mm256_set1_epi64x((fir.len() - 2) as i64)),
        );
        if _mm256_testz_si256(in_bounds, _mm256_set1_epi32(!0)) != 0 {
            break;
        }

        let mut linear_factor_numerators =
            _mm256_and_si256(interpolation_idxs, _mm256_set1_epi64x(LINEAR_FACTOR_MASK));

        // _mm256_cvtepi64_* intrinsics are AVX512-only :(
        // Shuffle/permute to an i32x4 vector then convert to f64x4

        // 0 x 1 x  2 x 3 x  ->  0 1 0 1  2 3 2 3
        linear_factor_numerators = _mm256_shuffle_epi32::<0b10_00_10_00>(linear_factor_numerators);

        // 0 1 0 1  2 3 2 3  ->  0 1 2 3  0 1 2 3
        linear_factor_numerators =
            _mm256_permute4x64_epi64::<0b10_00_10_00>(linear_factor_numerators);

        let linear_factor_numerators =
            _mm256_cvtepi32_pd(_mm256_castsi256_si128(linear_factor_numerators));

        let linear_factors =
            _mm256_mul_pd(linear_factor_numerators, _mm256_set1_pd(LINEAR_FACTOR_MULTIPLIER));

        // SAFETY: Mask is used to prevent out-of-bounds loads
        // Load as f64s to pull in each coefficient followed by the next coefficient
        // Pointer cast is fine because gather instructions don't require an aligned pointer
        #[allow(clippy::cast_ptr_alignment)]
        let all_coefficients = unsafe {
            _mm256_mask_i64gather_pd::<4>(
                _mm256_setzero_pd(),
                fir.as_ptr().cast::<f64>(),
                fir_idxs,
                _mm256_castsi256_pd(in_bounds),
            )
        };

        // 0 n0 1 n1  2 n2 3 n3 -> 0 1 n0 n1  2 3 n2 n3
        let all_coefficients =
            _mm256_permute_ps::<0b11_01_10_00>(_mm256_castpd_ps(all_coefficients));

        // 0 1 n0 n1  2 3 n2 n3 -> 0 1 2 3  n0 n1 n2 n3
        let all_coefficients = _mm256_castpd_ps(_mm256_permute4x64_pd::<0b11_01_10_00>(
            _mm256_castps_pd(all_coefficients),
        ));

        let coefficients = _mm256_cvtps_pd(_mm256_castps256_ps128(all_coefficients));
        let next_coefficients = _mm256_cvtps_pd(_mm256_extractf128_ps::<1>(all_coefficients));

        let multipliers = _mm256_fmadd_pd(
            linear_factors,
            _mm256_sub_pd(next_coefficients, coefficients),
            coefficients,
        );

        let input_idx = if REVERSE {
            let (idx, overflowed) = n.overflowing_sub(i + 3);
            if overflowed {
                // Should never happen, but don't go out of bounds if it does
                cold_path();
                panic!("input array index out of bounds; idx={idx}, len={}", input[0].len());
            }
            idx
        } else {
            let idx = n + i + 1;
            if idx + 3 >= input[0].len() {
                // Should never happen, but don't go out of bounds if it does
                cold_path();
                panic!("input array index out of bounds; idx={}, len={}", idx + 3, input[0].len());
            }
            idx
        };

        for ch in 0..CHANNELS {
            // SAFETY: Checked that idx >= 0 and idx + 3 < input.len()
            let in_samples = unsafe { _mm256_loadu_pd(input[ch].as_ptr().add(input_idx)) };
            sums[ch] = _mm256_fmadd_pd(multipliers, in_samples, sums[ch]);
        }

        interpolation_idxs =
            _mm256_add_epi64(interpolation_idxs, _mm256_set1_epi64x((4 * step) as i64));
    }

    sums.map(|sum| {
        let hsum = _mm256_hadd_pd(sum, sum);
        let components: [f64; 4] = unsafe { transmute(hsum) };
        components[0] + components[2]
    })
}

// SAFETY: Can only be called on a CPU that supports AVX512 (F + VL + DQ)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512vl,avx512dq")]
fn sum_wing_avx512<const REVERSE: bool, const CHANNELS: usize>(
    fir: &[f32],
    interpolation_idx: u64,
    step: u64,
    input: [&[f64]; CHANNELS],
    n: usize,
) -> [f64; CHANNELS] {
    #[allow(clippy::wildcard_imports)]
    use std::arch::x86_64::*;
    use std::hint::cold_path;

    const LINEAR_FACTOR_MASK: i64 = (1 << LINEAR_INTERPOLATION_BITS) - 1;
    const LINEAR_FACTOR_MULTIPLIER: f64 = 1.0 / (1 << LINEAR_INTERPOLATION_BITS) as f64;

    // SAFETY: Later code assumes all input slices are the same length and that n < input.len()
    assert!(n < input[0].len());
    if CHANNELS > 1 {
        assert!(input[1..].iter().all(|channel_input| channel_input.len() == input[0].len()));
    }

    let mut sums = [_mm512_setzero_pd(); CHANNELS];

    let initial_step_multipliers = if REVERSE {
        _mm512_set_epi64(0, 1, 2, 3, 4, 5, 6, 7)
    } else {
        _mm512_setr_epi64(0, 1, 2, 3, 4, 5, 6, 7)
    };

    let mut interpolation_idxs = _mm512_add_epi64(
        _mm512_set1_epi64(interpolation_idx as i64),
        _mm512_mullo_epi64(_mm512_set1_epi64(step as i64), initial_step_multipliers),
    );

    for i in (0..).step_by(8) {
        let fir_idxs = _mm512_srli_epi64::<{ LINEAR_INTERPOLATION_BITS }>(interpolation_idxs);

        // SAFETY: Compare to len-1 instead of len because FIR is read using 64-bit loads
        let in_bounds =
            _mm512_cmplt_epi64_mask(fir_idxs, _mm512_set1_epi64((fir.len() - 1) as i64));
        if in_bounds == 0 {
            break;
        }

        let linear_factor_numerators =
            _mm512_and_si512(interpolation_idxs, _mm512_set1_epi64(LINEAR_FACTOR_MASK));
        let linear_factors = _mm512_mul_pd(
            _mm512_cvtepi64_pd(linear_factor_numerators),
            _mm512_set1_pd(LINEAR_FACTOR_MULTIPLIER),
        );

        // SAFETY: Mask is used to prevent out-of-bounds loads
        // Load as f64s to pull in each coefficient followed by the next coefficient
        // Pointer cast is fine because gather instructions don't require an aligned pointer
        #[allow(clippy::cast_ptr_alignment)]
        let all_coefficients = unsafe {
            _mm512_mask_i64gather_pd::<4>(
                _mm512_setzero_pd(),
                in_bounds,
                fir_idxs,
                fir.as_ptr().cast::<f64>(),
            )
        };

        // 0 n0 1 n1 2 n2 3 n3 4 n4 5 n5 6 n6 7 n7 -> 0 1 2 3 4 5 6 7 n0 n1 n2 n3 n4 n5 n6 n7
        let all_coefficients = _mm512_permutexvar_ps(
            _mm512_setr_epi32(0, 2, 4, 6, 8, 10, 12, 14, 1, 3, 5, 7, 9, 11, 13, 15),
            _mm512_castpd_ps(all_coefficients),
        );

        let coefficients = _mm512_cvtps_pd(_mm512_castps512_ps256(all_coefficients));
        let next_coefficients = _mm512_cvtps_pd(_mm512_extractf32x8_ps::<1>(all_coefficients));

        let multipliers = _mm512_fmadd_pd(
            linear_factors,
            _mm512_sub_pd(next_coefficients, coefficients),
            coefficients,
        );

        let input_idx = if REVERSE {
            let (idx, overflowed) = n.overflowing_sub(i + 7);
            if overflowed {
                // Should never happen, but don't go out of bounds if it does
                cold_path();
                panic!("input array index out of bounds; idx={idx}, len={}", input[0].len());
            }
            idx
        } else {
            let idx = n + i + 1;
            if idx + 7 >= input[0].len() {
                // Should never happen, but don't go out of bounds if it does
                cold_path();
                panic!("input array index out of bounds; idx={}, len={}", idx + 7, input[0].len());
            }
            idx
        };

        for ch in 0..CHANNELS {
            // SAFETY: Checked that idx >= 0 and idx + 7 < input.len()
            let in_samples = unsafe { _mm512_loadu_pd(input[ch].as_ptr().add(input_idx)) };
            sums[ch] = _mm512_fmadd_pd(multipliers, in_samples, sums[ch]);
        }

        interpolation_idxs =
            _mm512_add_epi64(interpolation_idxs, _mm512_set1_epi64((8 * step) as i64));
    }

    sums.map(|sum| _mm512_reduce_add_pd(sum))
}

pub type QualitySincResampler<const CHANNELS: usize> = SincResampler<CHANNELS, Quality>;
pub type PerformanceSincResampler<const CHANNELS: usize> = SincResampler<CHANNELS, Performance>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_basic() {
        let mut buffer = SampleRingBuffer::new(10);

        buffer.push(1.0);
        buffer.push(2.0);
        buffer.push(3.0);
        buffer.push(4.0);
        buffer.push(5.0);

        assert_eq!(buffer.as_slice(), &[1.0, 2.0, 3.0, 4.0, 5.0]);

        buffer.pop();
        assert_eq!(buffer.as_slice(), &[2.0, 3.0, 4.0, 5.0]);
        buffer.pop();
        assert_eq!(buffer.as_slice(), &[3.0, 4.0, 5.0]);
        buffer.pop();
        assert_eq!(buffer.as_slice(), &[4.0, 5.0]);
        buffer.pop();
        assert_eq!(buffer.as_slice(), &[5.0]);
        buffer.pop();
        assert_eq!(buffer.as_slice(), &[]);
        buffer.pop();
        assert_eq!(buffer.as_slice(), &[]);
    }

    #[test]
    fn ring_buffer_copy() {
        let mut buffer = SampleRingBuffer::new(10);

        buffer.buffer.fill_with(|| rand::random_range(-1.0..=1.0));

        let len = 15;
        let end_start = buffer.buffer.len() - 8 - len;

        buffer.idx = end_start;
        buffer.len = len;

        // Should trigger a copy from end to start
        buffer.push(1.0);

        // Validate that the index moved to start
        assert_eq!(buffer.idx, 8);
        assert_eq!(buffer.len, len + 1);

        // Validate that samples were copied
        let mut expected = buffer.buffer[end_start..end_start + len].to_vec();
        expected.push(1.0);
        assert_eq!(buffer.as_slice(), expected);
    }

    // Validates that sum_wing_avx2() and sum_wing_avx512() produce the same results as sum_wing_no_avx()
    #[cfg(target_arch = "x86_64")]
    fn sum_wing_avx_kernel<Kernel: SincKernel>(source_rate: f64) {
        if !is_x86_feature_detected!("avx2") || !is_x86_feature_detected!("fma") {
            // Don't bother doing anything if not even AVX2 is supported
            return;
        }

        let target_rate = 48000.0;
        let ratio = target_rate / source_rate;
        let required_samples = estimate_required_samples::<Kernel>(ratio);

        let scale = if ratio < 1.0 { ratio } else { 1.0 };
        let step_float = scale
            * f64::from(Kernel::oversample_factor())
            * f64::from(1 << LINEAR_INTERPOLATION_BITS);
        let step = step_float.round() as u64;

        let fir = Kernel::fir();

        let mut buffer = SampleRingBuffer::new(required_samples);
        for _ in 0..2 * required_samples {
            buffer.push(rand::random());
        }

        // SAFETY: AVX2 and FMA are supported on this CPU, as checked above
        unsafe {
            for _ in 0..required_samples / 2 {
                let n = required_samples / 2 + buffer.idx;

                let interpolation_idx = (rand::random_range(0.0..1.0) * step_float).round() as u64;

                let non_avx_reverse = sum_wing_no_avx::<true, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );
                let non_avx_forward = sum_wing_no_avx::<false, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );

                let avx2_reverse = sum_wing_avx2::<true, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );
                let avx2_forward = sum_wing_avx2::<false, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );

                assert!(
                    (non_avx_reverse[0] - avx2_reverse[0]).abs() < 1e-9,
                    "{non_avx_reverse:?} == {avx2_reverse:?} (source rate {source_rate})"
                );
                assert!(
                    (non_avx_forward[0] - avx2_forward[0]).abs() < 1e-9,
                    "{non_avx_forward:?} == {avx2_forward:?} (source rate {source_rate})",
                );

                buffer.pop();
            }
        }

        // Skip rest of test if AVX512 is not supported
        if !is_x86_feature_detected!("avx512f")
            || !is_x86_feature_detected!("avx512vl")
            || !is_x86_feature_detected!("avx512dq")
        {
            return;
        }

        let mut buffer = SampleRingBuffer::new(required_samples);
        for _ in 0..2 * required_samples {
            buffer.push(rand::random());
        }

        // SAFETY: AVX512 is supported on this CPU, as checked above
        unsafe {
            for _ in 0..required_samples / 2 {
                let n = required_samples / 2 + buffer.idx;

                let interpolation_idx = (rand::random_range(0.0..1.0) * step_float).round() as u64;

                let non_avx_reverse = sum_wing_no_avx::<true, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );
                let non_avx_forward = sum_wing_no_avx::<false, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );

                let avx512_reverse = sum_wing_avx512::<true, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );
                let avx512_forward = sum_wing_avx512::<false, _>(
                    fir,
                    interpolation_idx,
                    step,
                    [buffer.buffer.as_slice()],
                    n,
                );

                assert!(
                    (non_avx_reverse[0] - avx512_reverse[0]).abs() < 1e-9,
                    "{non_avx_reverse:?} == {avx512_reverse:?} (source rate {source_rate})"
                );
                assert!(
                    (non_avx_forward[0] - avx512_forward[0]).abs() < 1e-9,
                    "{non_avx_forward:?} == {avx512_forward:?} (source rate {source_rate})"
                );

                buffer.pop();
            }
        }
    }

    const TEST_SOURCE_RATES: &[f64] = &[4000000.0, 55000.0, 48000.0, 20000.0];

    // These tests should ideally be run using miri to verify no out-of-bounds memory reads:
    //   $ cargo +nightly miri test -p dsp
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sum_wing_avx_quality_kernel() {
        for &source_rate in TEST_SOURCE_RATES {
            sum_wing_avx_kernel::<Quality>(source_rate);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sum_wing_avx_performance_kernel() {
        for &source_rate in TEST_SOURCE_RATES {
            sum_wing_avx_kernel::<Performance>(source_rate);
        }
    }
}
