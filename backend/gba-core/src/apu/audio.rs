//! Audio resampling code

use crate::apu::PwmClockShift;
use bincode::{Decode, Encode};
use dsp::design::FilterType;
use dsp::iir::SecondOrderIirFilter;
use dsp::sinc::{PerformanceSincResampler, QualitySincResampler};
use gba_config::GbaAudioInterpolation;
use jgenesis_common::audio::CubicResampler;
use jgenesis_common::frontend::AudioOutput;
use std::array;
use std::cmp::Ordering;

#[derive(Debug, Clone, Encode, Decode)]
pub struct BasicResampler {
    resampler: QualitySincResampler<2>,
}

impl BasicResampler {
    pub fn new(clock_shift: PwmClockShift, output_frequency: u64) -> Self {
        Self {
            resampler: QualitySincResampler::new(
                clock_shift.source_frequency() as f64,
                output_frequency as f64,
            ),
        }
    }

    pub fn push_mixed_sample(&mut self, sample: [f64; 2]) {
        self.resampler.collect(sample);
    }

    pub fn update_source_frequency(&mut self, clock_shift: PwmClockShift) {
        self.resampler.update_source_frequency(clock_shift.source_frequency() as f64);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency as f64);
    }

    pub fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        while let Some([sample_l, sample_r]) = self.resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum EnhancedResampler {
    Cubic(CubicResampler<1>),
    Sinc(QualitySincResampler<1>),
}

impl EnhancedResampler {
    fn new(
        interpolation: GbaAudioInterpolation,
        source_frequency: f64,
        output_frequency: u64,
    ) -> Self {
        match interpolation {
            GbaAudioInterpolation::WindowedSinc => {
                Self::Sinc(QualitySincResampler::new(source_frequency, output_frequency as f64))
            }
            _ => Self::Cubic(CubicResampler::new(source_frequency, output_frequency)),
        }
    }

    fn collect(&mut self, sample: f64) {
        match self {
            Self::Cubic(resampler) => resampler.collect_sample([sample]),
            Self::Sinc(resampler) => resampler.collect([sample]),
        }
    }

    fn update_source_frequency(&mut self, source_frequency: f64) {
        match self {
            Self::Cubic(resampler) => resampler.update_source_frequency(source_frequency),
            Self::Sinc(resampler) => resampler.update_source_frequency(source_frequency),
        }
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        match self {
            Self::Cubic(resampler) => resampler.update_output_frequency(output_frequency),
            Self::Sinc(resampler) => resampler.update_output_frequency(output_frequency as f64),
        }
    }

    fn output_buffer_pop_front(&mut self) -> Option<[f64; 1]> {
        match self {
            Self::Cubic(resampler) => resampler.output_buffer_pop_front(),
            Self::Sinc(resampler) => resampler.output_buffer_pop_front(),
        }
    }
}

// PSG source frequency is actually 2 MHz (2^21), but this implementation only runs it at 1 MHz
const PSG_SOURCE_FREQUENCY: f64 = 1_048_576.0;

#[derive(Debug, Clone, Encode, Decode)]
struct PsgLowPassFilter {
    max_pcm_frequency: Option<f64>,
    psg_low_pass: bool,
    filters: Option<(SecondOrderIirFilter, SecondOrderIirFilter)>,
}

impl PsgLowPassFilter {
    fn new(psg_low_pass: bool, pcm_frequencies: [Option<f64>; 2]) -> Self {
        if !psg_low_pass {
            return Self { max_pcm_frequency: None, psg_low_pass, filters: None };
        }

        let max_pcm_frequency = f64_option_max(pcm_frequencies);
        let filters = max_pcm_frequency.map(|max_pcm_frequency| {
            let lpf_cutoff = max_pcm_frequency * 0.45;
            (Self::new_filter(lpf_cutoff), Self::new_filter(lpf_cutoff))
        });

        Self { max_pcm_frequency, psg_low_pass, filters }
    }

    fn new_filter(cutoff: f64) -> SecondOrderIirFilter {
        dsp::design::butterworth(cutoff, PSG_SOURCE_FREQUENCY, FilterType::LowPass)
    }

    fn filter(&mut self, sample: [f64; 2]) -> [f64; 2] {
        self.filters
            .as_mut()
            .map(|(filter_l, filter_r)| [filter_l.filter(sample[0]), filter_r.filter(sample[1])])
            .unwrap_or(sample)
    }

    fn reload(&mut self, psg_low_pass: bool, pcm_frequencies: [Option<f64>; 2]) {
        if psg_low_pass != self.psg_low_pass
            || f64_option_max(pcm_frequencies) != self.max_pcm_frequency
        {
            *self = Self::new(psg_low_pass, pcm_frequencies);
        }
    }
}

fn f64_option_max(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    // Less is arbitrary here - if there's ever a NaN frequency, there are bigger problems
    values.into_iter().flatten().max_by(|&a, &b| a.partial_cmp(&b).unwrap_or(Ordering::Less))
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterpolatingResampler {
    interpolation: GbaAudioInterpolation,
    psg_low_pass: bool,
    pcm_frequencies: [Option<f64>; 2],
    pcm_resamplers: [Option<EnhancedResampler>; 2],
    pcm_samples: [i8; 2],
    psg_lpf: PsgLowPassFilter,
    psg_resampler: PerformanceSincResampler<2>,
    output_frequency: f64,
}

impl InterpolatingResampler {
    pub fn new(
        interpolation: GbaAudioInterpolation,
        psg_low_pass: bool,
        output_frequency: u64,
        pcm_frequencies: [Option<f64>; 2],
    ) -> Self {
        Self {
            interpolation,
            psg_low_pass,
            pcm_frequencies,
            pcm_resamplers: pcm_frequencies.map(|frequency| {
                frequency.map(|frequency| {
                    EnhancedResampler::new(interpolation, frequency, output_frequency)
                })
            }),
            pcm_samples: array::from_fn(|_| 0),
            psg_lpf: PsgLowPassFilter::new(psg_low_pass, pcm_frequencies),
            psg_resampler: PerformanceSincResampler::new((1 << 20).into(), output_frequency as f64),
            output_frequency: output_frequency as f64,
        }
    }

    pub fn update_psg_low_pass(&mut self, psg_low_pass: bool) {
        self.psg_lpf.reload(psg_low_pass, self.pcm_frequencies);
    }

    pub fn push_pcm_a(&mut self, sample: i8) {
        self.push_pcm(0, sample);
    }

    pub fn push_pcm_b(&mut self, sample: i8) {
        self.push_pcm(1, sample);
    }

    fn push_pcm(&mut self, i: usize, sample: i8) {
        self.pcm_samples[i] = sample;

        if let Some(resampler) = &mut self.pcm_resamplers[i] {
            resampler.collect(f64::from(sample) / 128.0);
        }
    }

    pub fn reset_pcm_a(&mut self) {
        self.pcm_samples[0] = 0;
    }

    pub fn reset_pcm_b(&mut self) {
        self.pcm_samples[1] = 0;
    }

    pub fn push_psg(&mut self, sample: (i16, i16)) {
        let sample =
            self.psg_lpf.filter([f64::from(sample.0) / 512.0, f64::from(sample.1) / 512.0]);

        self.psg_resampler.collect(sample);
    }

    pub fn update_pcm_a_frequency(&mut self, frequency: Option<f64>) {
        self.update_pcm_frequency(0, frequency);
    }

    pub fn update_pcm_b_frequency(&mut self, frequency: Option<f64>) {
        self.update_pcm_frequency(1, frequency);
    }

    fn update_pcm_frequency(&mut self, i: usize, frequency: Option<f64>) {
        let prev_frequency = self.pcm_frequencies[i];
        self.pcm_frequencies[i] = frequency;

        if prev_frequency == frequency {
            return;
        }

        match frequency {
            Some(frequency) => {
                self.pcm_resamplers[i]
                    .get_or_insert_with(|| {
                        EnhancedResampler::new(
                            self.interpolation,
                            frequency,
                            self.output_frequency as u64,
                        )
                    })
                    .update_source_frequency(frequency);
            }
            None => {
                self.pcm_resamplers[i] = None;
            }
        }

        self.psg_lpf.reload(self.psg_low_pass, self.pcm_frequencies);
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.output_frequency = output_frequency as f64;

        self.psg_resampler.update_output_frequency(self.output_frequency);

        for resampler in self.pcm_resamplers.iter_mut().flatten() {
            resampler.update_output_frequency(output_frequency);
        }
    }

    pub fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
        pcm_volume_shifts: [bool; 2],
        psg_volume_shift: u8,
        pcm_a_enabled: [bool; 2],
        pcm_b_enabled: [bool; 2],
    ) -> Result<(), A::Err> {
        while let Some([mut psg_l, mut psg_r]) = self.psg_resampler.output_buffer_pop_front() {
            if psg_volume_shift != 0 {
                psg_l /= f64::from(1 << psg_volume_shift);
                psg_r /= f64::from(1 << psg_volume_shift);
            }

            let mut pcm_a = self.pcm_resamplers[0]
                .as_mut()
                .and_then(EnhancedResampler::output_buffer_pop_front)
                .unwrap_or_else(|| [f64::from(self.pcm_samples[0]) / 128.0])[0];
            let mut pcm_b = self.pcm_resamplers[1]
                .as_mut()
                .and_then(EnhancedResampler::output_buffer_pop_front)
                .unwrap_or_else(|| [f64::from(self.pcm_samples[1]) / 128.0])[0];

            if pcm_volume_shifts[0] {
                pcm_a *= 0.5;
            }
            if pcm_volume_shifts[1] {
                pcm_b *= 0.5;
            }

            let pcm_l = f64::from(pcm_a_enabled[0]) * pcm_a + f64::from(pcm_b_enabled[0]) * pcm_b;
            let pcm_r = f64::from(pcm_a_enabled[1]) * pcm_a + f64::from(pcm_b_enabled[1]) * pcm_b;

            let mixed_l = (psg_l + pcm_l).clamp(-1.0, 1.0);
            let mixed_r = (psg_r + pcm_r).clamp(-1.0, 1.0);

            audio_output.push_sample(mixed_l, mixed_r)?;
        }

        Ok(())
    }
}
