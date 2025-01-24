mod cubic_resampler;
pub mod iir;
pub mod sinc;

pub use cubic_resampler::CubicResampler;

pub const DEFAULT_OUTPUT_FREQUENCY: u64 = 48000;

// Scale frequencies up by 1e9 to better handle non-integer source frequencies, e.g. the Master System PSG
const RESAMPLE_SCALING_FACTOR: u64 = 1_000_000_000;

// Based on https://yehar.com/blog/wp-content/uploads/2009/08/deip.pdf
#[must_use]
pub fn interpolate_cubic_hermite_4p([ym1, y0, y1, y2]: [f64; 4], x: f64) -> f64 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);

    ((c3 * x + c2) * x + c1) * x + c0
}

// Based on https://yehar.com/blog/wp-content/uploads/2009/08/deip.pdf
// Assuming that Rust/LLVM will optimize these constant floating-point divisions into multiplications,
// which it does seem to do based on experimentation in Compiler Explorer
#[must_use]
pub fn interpolate_cubic_hermite_6p([ym2, ym1, y0, y1, y2, y3]: [f64; 6], x: f64) -> f64 {
    let c0 = y0;
    let c1 = 1.0 / 12.0 * (ym2 - y2) + 2.0 / 3.0 * (y1 - ym1);
    let c2 = 5.0 / 4.0 * ym1 - 7.0 / 3.0 * y0 + 5.0 / 3.0 * y1 - 1.0 / 2.0 * y2 + 1.0 / 12.0 * y3
        - 1.0 / 6.0 * ym2;
    let c3 = 1.0 / 12.0 * (ym2 - y3) + 7.0 / 12.0 * (y2 - ym1) + 4.0 / 3.0 * (y0 - y1);

    ((c3 * x + c2) * x + c1) * x + c0
}

#[derive(Debug, Clone)]
pub struct DynamicResamplingRate {
    base_output_frequency: u32,
    dynamic_output_frequency: u32,
    dynamic_update_counter: u32,
    target_audio_buffer_size: u32,
}

impl DynamicResamplingRate {
    #[must_use]
    pub fn new(base_output_frequency: u32, target_audio_buffer_size: u32) -> Self {
        Self {
            base_output_frequency,
            dynamic_output_frequency: base_output_frequency,
            dynamic_update_counter: 0,
            target_audio_buffer_size,
        }
    }

    pub fn update_config(&mut self, base_output_frequency: u32, target_audio_buffer_size: u32) {
        *self = Self::new(base_output_frequency, target_audio_buffer_size);
    }

    #[must_use]
    pub fn current_output_frequency(&self) -> u32 {
        self.dynamic_output_frequency
    }

    pub fn adjust(&mut self, audio_buffer_len: u32) {
        // Restrict the adjusted ratio to within 0.5% of the expected ratio
        const MAX_DELTA: f64 = 0.005;

        // Only update the ratio every 20 frames
        const UPDATE_PERIOD: u32 = 20;

        self.dynamic_update_counter += 1;
        if self.dynamic_update_counter != UPDATE_PERIOD {
            return;
        }
        self.dynamic_update_counter = 0;

        let target_len: f64 = self.target_audio_buffer_size.into();
        let current_len: f64 = audio_buffer_len.into();
        let difference = ((target_len - current_len) / target_len).clamp(-1.0, 1.0);
        let adjustment = 1.0 + MAX_DELTA * difference;

        // This should _probably_ adjust the current dynamic frequency rather than the audio output
        // stream frequency, but adjusting the latter seems to work much better in practice
        self.dynamic_output_frequency =
            (adjustment * f64::from(self.base_output_frequency)).round() as u32;

        log::debug!(
            "Adjusted dynamic frequency to {}; target={target_len}, current={current_len}, adjustment={adjustment}",
            self.dynamic_output_frequency
        );
    }
}
