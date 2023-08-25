use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{process, thread};

pub struct AudioOutput {
    full_buffer_l: VecDeque<f64>,
    full_buffer_r: VecDeque<f64>,
    audio_buffer: Vec<(f32, f32)>,
    audio_queue: Arc<Mutex<VecDeque<f32>>>,
    sample_count: u64,
    next_sample: u64,
    next_sample_float: f64,
}

impl AudioOutput {
    // 53_693_175 / 7 / 6 / 24 * 3 / 48000
    // The *3 is because of zero padding the original audio signal with 2 zeros for every actual sample
    const DOWNSAMPLING_RATIO: f64 = 3.329189918154762;

    const FIR_COEFFICIENT_0: f64 = -0.001478342773457343;
    const FIR_COEFFICIENTS: &'static [f64] = &[
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

    pub fn new() -> Self {
        Self {
            full_buffer_l: VecDeque::new(),
            full_buffer_r: VecDeque::new(),
            audio_buffer: Vec::new(),
            audio_queue: Arc::new(Mutex::new(VecDeque::new())),
            sample_count: 0,
            next_sample: Self::DOWNSAMPLING_RATIO.round() as u64,
            next_sample_float: Self::DOWNSAMPLING_RATIO,
        }
    }

    pub fn initialize(&self) -> anyhow::Result<impl StreamTrait> {
        let callback_queue = Arc::clone(&self.audio_queue);

        let audio_host = cpal::default_host();
        let audio_device = audio_host.default_output_device().unwrap();
        let audio_stream = audio_device.build_output_stream(
            &StreamConfig {
                channels: 2,
                sample_rate: SampleRate(48000),
                buffer_size: BufferSize::Fixed(1024),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut callback_queue = callback_queue.lock().unwrap();
                for output in data {
                    let Some(sample) = callback_queue.pop_front() else {
                        break;
                    };
                    *output = sample;
                }
            },
            move |err| {
                log::error!("Audio error: {err}");
                process::exit(1);
            },
            None,
        )?;
        audio_stream.play()?;

        Ok(audio_stream)
    }

    fn output_sample(buffer: &VecDeque<f64>) -> f64 {
        Self::FIR_COEFFICIENT_0
            + Self::FIR_COEFFICIENTS
                .iter()
                .copied()
                .zip(buffer.iter().copied())
                .map(|(a, b)| a * b)
                .sum::<f64>()
    }

    fn buffer_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.full_buffer_l.push_back(sample_l);
        self.full_buffer_r.push_back(sample_r);

        if self.full_buffer_l.len() > Self::FIR_COEFFICIENTS.len() {
            self.full_buffer_l.pop_front();
        }
        if self.full_buffer_r.len() > Self::FIR_COEFFICIENTS.len() {
            self.full_buffer_r.pop_front();
        }

        self.sample_count += 1;
        if self.sample_count == self.next_sample {
            self.next_sample_float += Self::DOWNSAMPLING_RATIO;
            self.next_sample = self.next_sample_float.round() as u64;

            let sample_l = Self::output_sample(&self.full_buffer_l);
            let sample_r = Self::output_sample(&self.full_buffer_r);

            self.audio_buffer.push((sample_l as f32, sample_r as f32));
            if self.audio_buffer.len() == 64 {
                loop {
                    {
                        let mut audio_queue = self.audio_queue.lock().unwrap();
                        if audio_queue.len() < 1024 {
                            audio_queue.extend(
                                self.audio_buffer
                                    .drain(..)
                                    .flat_map(|(sample_l, sample_r)| [sample_l, sample_r]),
                            );
                            break;
                        }
                    }

                    thread::sleep(Duration::from_micros(250));
                }
            }
        }
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        // Zero pad each actual sample with 2 zeros because otherwise the source sample rate is
        // too close to the target sample rate for downsampling to work well
        self.buffer_sample(sample_l, sample_r);
        self.buffer_sample(0.0, 0.0);
        self.buffer_sample(0.0, 0.0);
    }
}

// -6dB (10 ^ -6/20)
// PSG is too loud if it's given the same volume level as the YM2612
pub const PSG_COEFFICIENT: f64 = 0.5011872336272722;
