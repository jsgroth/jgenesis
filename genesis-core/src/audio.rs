//! Genesis audio resampling, filtering, and mixing code

#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_traits::frontend::{AudioOutput, TimingMode};
use std::cmp;
use std::collections::VecDeque;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

const YM2612_LPF_COEFFICIENT_0: f64 = -0.001478342773457343;
const YM2612_LPF_COEFFICIENTS: [f64; 25] = [
    -0.001478342773457343,
    -0.002579939173264984,
    -0.001815391014296705,
    0.003232249258559727,
    0.010914665789461,
    0.01180369689254257,
    -0.00423226347744078,
    -0.03255778315532309,
    -0.04631404301025462,
    -0.01139190330985419,
    0.08276070429927576,
    0.2033479308228996,
    0.2883104188511529,
    0.2883104188511529,
    0.2033479308228996,
    0.08276070429927578,
    -0.01139190330985419,
    -0.04631404301025461,
    -0.03255778315532309,
    -0.004232263477440783,
    0.01180369689254257,
    0.01091466578946099,
    0.00323224925855973,
    -0.001815391014296708,
    -0.002579939173264985,
];

const YM2612_HPF_CHARGE_FACTOR: f64 = 0.9966982656608827;

const PSG_LPF_COEFFICIENT_0: f64 = -0.001070923693774405;
const PSG_LPF_COEFFICIENTS: [f64; 35] = [
    -0.001070923693774405,
    -0.001685350075225726,
    -0.001863413379276681,
    -0.0009568415472585112,
    0.001640749268572869,
    0.005567205576795833,
    0.00885183238540499,
    0.008348379350607549,
    0.00146034668504318,
    -0.0115355816923585,
    -0.02594911574769958,
    -0.03323373392428908,
    -0.02401612130459429,
    0.007419221555475174,
    0.05927546269924155,
    0.1211627893325687,
    0.1768103438836072,
    0.2097747506271597,
    0.2097747506271597,
    0.1768103438836073,
    0.1211627893325687,
    0.05927546269924156,
    0.007419221555475176,
    -0.02401612130459429,
    -0.03323373392428909,
    -0.02594911574769959,
    -0.0115355816923585,
    0.001460346685043181,
    0.008348379350607551,
    0.00885183238540499,
    0.005567205576795835,
    0.00164074926857287,
    -0.0009568415472585116,
    -0.001863413379276682,
    -0.001685350075225726,
];

const PSG_HPF_CHARGE_FACTOR: f64 = 0.999212882632514;

// -8dB (10 ^ -8/20)
pub const PSG_COEFFICIENT: f64 = 0.3981071705534972;

// Arbitrary power of 2 to keep total sample count small-ish for better f64 precision
const SAMPLE_COUNT_MODULO: u64 = 1 << 27;

pub const OUTPUT_FREQUENCY: f64 = 48000.0;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SignalDownsampler<const LPF_TAPS: usize, const ZERO_PADDING: usize> {
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

impl<const LPF_TAPS: usize, const ZERO_PADDING: usize> SignalDownsampler<LPF_TAPS, ZERO_PADDING> {
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

#[inline]
fn high_pass_filter(sample: f64, charge_factor: f64, capacitor: &mut f64) -> f64 {
    let filtered_sample = sample - *capacitor;
    *capacitor = sample - charge_factor * filtered_sample;
    filtered_sample
}

#[inline]
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

pub type Ym2612Downsampler = SignalDownsampler<25, 2>;
pub type PsgDownsampler = SignalDownsampler<35, 0>;

impl Ym2612Downsampler {
    #[must_use]
    pub fn new_ym2612(genesis_mclk_frequency: f64) -> Self {
        let ym2612_frequency = genesis_mclk_frequency / 7.0 / 6.0 / 24.0;
        Self::new(
            ym2612_frequency,
            YM2612_LPF_COEFFICIENT_0,
            YM2612_LPF_COEFFICIENTS,
            YM2612_HPF_CHARGE_FACTOR,
        )
    }
}

impl PsgDownsampler {
    #[must_use]
    pub fn new_psg(genesis_mclk_frequency: f64) -> Self {
        let psg_frequency = genesis_mclk_frequency / 15.0 / 16.0;
        Self::new(psg_frequency, PSG_LPF_COEFFICIENT_0, PSG_LPF_COEFFICIENTS, PSG_HPF_CHARGE_FACTOR)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisAudioDownsampler {
    ym2612_downsampler: Ym2612Downsampler,
    psg_downsampler: PsgDownsampler,
}

impl GenesisAudioDownsampler {
    #[must_use]
    pub fn new(timing_mode: TimingMode) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };

        let ym2612_downsampler = Ym2612Downsampler::new_ym2612(genesis_mclk_frequency);
        let psg_downsampler = PsgDownsampler::new_psg(genesis_mclk_frequency);

        Self { ym2612_downsampler, psg_downsampler }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_downsampler.collect_sample(sample_l, sample_r);
    }

    /// Push all samples that are ready to the given audio output.
    ///
    /// # Errors
    ///
    /// Will propagate any error returned by the audio output while pushing samples.
    // Should never panic, the unwraps are guarded by length checks
    #[allow(clippy::missing_panics_doc)]
    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count =
            cmp::min(self.ym2612_downsampler.output.len(), self.psg_downsampler.output.len());

        for _ in 0..sample_count {
            let (ym2612_l, ym2612_r) = self.ym2612_downsampler.output.pop_front().unwrap();
            let (psg_l, psg_r) = self.psg_downsampler.output.pop_front().unwrap();

            let sample_l = (ym2612_l + PSG_COEFFICIENT * psg_l).clamp(-1.0, 1.0);
            let sample_r = (ym2612_r + PSG_COEFFICIENT * psg_r).clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
