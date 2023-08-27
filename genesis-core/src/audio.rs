use jgenesis_traits::frontend::AudioOutput;
use std::collections::VecDeque;

// 53_693_175 / 7 / 6 / 24 * 3 / 48000
// The *3 is because of zero padding the original audio signal with 2 zeros for every actual sample
const DOWNSAMPLING_RATIO: f64 = 3.329189918154762;

const FIR_COEFFICIENT_0: f64 = -0.001478342773457343;
const FIR_COEFFICIENTS: &[f64] = &[
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

#[derive(Debug, Clone)]
pub struct AudioDownsampler {
    full_buffer_l: VecDeque<f64>,
    full_buffer_r: VecDeque<f64>,
    sample_count: u64,
    next_sample: u64,
    next_sample_float: f64,
}

impl AudioDownsampler {
    pub fn new() -> Self {
        Self {
            full_buffer_l: VecDeque::new(),
            full_buffer_r: VecDeque::new(),
            sample_count: 0,
            next_sample: DOWNSAMPLING_RATIO.round() as u64,
            next_sample_float: DOWNSAMPLING_RATIO,
        }
    }

    fn buffer_sample<A: AudioOutput>(
        &mut self,
        sample_l: f64,
        sample_r: f64,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.full_buffer_l.push_back(sample_l);
        self.full_buffer_r.push_back(sample_r);

        if self.full_buffer_l.len() > FIR_COEFFICIENTS.len() {
            self.full_buffer_l.pop_front();
        }
        if self.full_buffer_r.len() > FIR_COEFFICIENTS.len() {
            self.full_buffer_r.pop_front();
        }

        self.sample_count += 1;
        if self.sample_count == self.next_sample {
            self.next_sample_float += DOWNSAMPLING_RATIO;
            self.next_sample = self.next_sample_float.round() as u64;

            let sample_l = output_sample(&self.full_buffer_l);
            let sample_r = output_sample(&self.full_buffer_r);
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn collect_sample<A: AudioOutput>(
        &mut self,
        sample_l: f64,
        sample_r: f64,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        // Zero pad each actual sample with 2 zeros because otherwise the source sample rate is
        // too close to the target sample rate for downsampling to work well
        self.buffer_sample(sample_l, sample_r, audio_output)?;
        self.buffer_sample(0.0, 0.0, audio_output)?;
        self.buffer_sample(0.0, 0.0, audio_output)?;

        Ok(())
    }
}

fn output_sample(buffer: &VecDeque<f64>) -> f64 {
    let sample = FIR_COEFFICIENT_0
        + FIR_COEFFICIENTS
            .iter()
            .copied()
            .zip(buffer.iter().copied())
            .map(|(a, b)| a * b)
            .sum::<f64>();
    // Multiply amplitude by 3 to somewhat counterbalance the volume drop from zero padding
    (sample * 3.0).clamp(-1.0, 1.0)
}

// -8dB (10 ^ -8/20)
// PSG is too loud if it's given the same volume level as the YM2612
pub const PSG_COEFFICIENT: f64 = 0.3981071705534972;
