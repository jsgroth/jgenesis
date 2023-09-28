#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_traits::frontend::{AudioOutput, TimingMode};
use std::cmp;
use std::collections::VecDeque;

const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;
const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;

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
const PSG_COEFFICIENT: f64 = 0.3981071705534972;

const PCM_LPF_COEFFICIENT_0: f64 = -0.001032167331725023;
const PCM_LPF_COEFFICIENTS: [f64; 21] = [
    -0.001032167331725023,
    -0.00337362854293201,
    -0.002300741105977643,
    0.007438828683983638,
    0.01718256624704002,
    0.002040390827841266,
    -0.04030652783842427,
    -0.05506118523737572,
    0.02814357569062969,
    0.2004791993149999,
    0.3467896892919401,
    0.3467896892919402,
    0.2004791993149999,
    0.02814357569062969,
    -0.05506118523737575,
    -0.04030652783842429,
    0.002040390827841267,
    0.01718256624704001,
    0.00743882868398364,
    -0.002300741105977646,
    -0.003373628542932013,
];

const PCM_HPF_CHARGE_FACTOR: f64 = 0.9946028448191855;

// -3 dB (10 ^ -3/20)
const PCM_COEFFICIENT: f64 = 0.7079457843841379;

const CD_LPF_COEFFICIENT_0: f64 = 0.001074119844470324;
const CD_LPF_COEFFICIENTS: [f64; 23] = [
    0.001074119844470324,
    -0.00173597616545656,
    -0.004832665407973518,
    -0.001992823915686409,
    0.0109179929840003,
    0.01955265022534506,
    -0.001029754702410328,
    -0.04519730177754978,
    -0.05443102244415676,
    0.03374428630870474,
    0.2024522203986207,
    0.3414782746520921,
    0.3414782746520921,
    0.2024522203986207,
    0.03374428630870474,
    -0.05443102244415674,
    -0.0451973017775498,
    -0.001029754702410327,
    0.01955265022534507,
    0.01091799298400031,
    -0.001992823915686409,
    -0.004832665407973518,
    -0.001735976165456563,
];

const CD_HPF_CHARGE_FACTOR: f64 = 0.9960133089108504;

// -7 dB (10 ^ -7/20)
const CD_COEFFICIENT: f64 = 0.44668359215096315;

// Arbitrary power of 2 to keep total sample count small-ish for better f64 precision
const SAMPLE_COUNT_MODULO: u64 = 1 << 27;

#[derive(Debug, Clone, Encode, Decode)]
struct SignalDownsampler<const LPF_TAPS: usize, const ZERO_PADDING: usize> {
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

pub const OUTPUT_FREQUENCY: f64 = 48000.0;

impl<const LPF_TAPS: usize, const ZERO_PADDING: usize> SignalDownsampler<LPF_TAPS, ZERO_PADDING> {
    fn new(
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
            self.next_sample = self.next_sample_float.round() as u64;

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

    fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        let sample_l =
            high_pass_filter(sample_l, self.hpf_charge_factor, &mut self.hpf_capacitor_l);
        let sample_r =
            high_pass_filter(sample_r, self.hpf_charge_factor, &mut self.hpf_capacitor_r);

        self.buffer_sample(sample_l, sample_r);
        for _ in 0..ZERO_PADDING {
            self.buffer_sample(0.0, 0.0);
        }
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

type Ym2612Downsampler = SignalDownsampler<25, 2>;
type PsgDownsampler = SignalDownsampler<35, 0>;
type PcmDownsampler = SignalDownsampler<21, 3>;
type CdDownsampler = SignalDownsampler<23, 2>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioDownsampler {
    ym2612_downsampler: Ym2612Downsampler,
    psg_downsampler: PsgDownsampler,
    pcm_downsampler: PcmDownsampler,
    cd_downsampler: CdDownsampler,
}

impl AudioDownsampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        let genesis_mclk_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
            TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
        };
        let ym2612_frequency = genesis_mclk_frequency / 7.0 / 6.0 / 24.0;
        let psg_frequency = genesis_mclk_frequency / 15.0 / 16.0;
        let pcm_frequency = SEGA_CD_MCLK_FREQUENCY / 4.0 / 384.0;

        let ym2612_downsampler = Ym2612Downsampler::new(
            ym2612_frequency,
            YM2612_LPF_COEFFICIENT_0,
            YM2612_LPF_COEFFICIENTS,
            YM2612_HPF_CHARGE_FACTOR,
        );
        let psg_downsampler = PsgDownsampler::new(
            psg_frequency,
            PSG_LPF_COEFFICIENT_0,
            PSG_LPF_COEFFICIENTS,
            PSG_HPF_CHARGE_FACTOR,
        );
        let pcm_downsampler = PcmDownsampler::new(
            pcm_frequency,
            PCM_LPF_COEFFICIENT_0,
            PCM_LPF_COEFFICIENTS,
            PCM_HPF_CHARGE_FACTOR,
        );
        let cd_downsampler = CdDownsampler::new(
            CD_DA_FREQUENCY,
            CD_LPF_COEFFICIENT_0,
            CD_LPF_COEFFICIENTS,
            CD_HPF_CHARGE_FACTOR,
        );

        Self { ym2612_downsampler, psg_downsampler, pcm_downsampler, cd_downsampler }
    }

    pub fn collect_ym2612_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.ym2612_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_psg_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_pcm_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.pcm_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn collect_cd_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.cd_downsampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        let sample_count = cmp::min(
            cmp::min(
                cmp::min(self.ym2612_downsampler.output.len(), self.psg_downsampler.output.len()),
                self.pcm_downsampler.output.len(),
            ),
            self.cd_downsampler.output.len(),
        );
        for _ in 0..sample_count {
            let (ym2612_l, ym2612_r) = self.ym2612_downsampler.output.pop_front().unwrap();
            let (psg_l, psg_r) = self.psg_downsampler.output.pop_front().unwrap();
            let (pcm_l, pcm_r) = self.pcm_downsampler.output.pop_front().unwrap();
            let (cd_l, cd_r) = self.cd_downsampler.output.pop_front().unwrap();

            let sample_l = (ym2612_l
                + PSG_COEFFICIENT * psg_l
                + PCM_COEFFICIENT * pcm_l
                + CD_COEFFICIENT * cd_l)
                .clamp(-1.0, 1.0);
            let sample_r = (ym2612_r
                + PSG_COEFFICIENT * psg_r
                + PCM_COEFFICIENT * pcm_r
                + CD_COEFFICIENT * cd_r)
                .clamp(-1.0, 1.0);

            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}
