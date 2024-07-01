use crate::apu::dsp::tables;

const I16_MIN: f64 = -32768.0;
const I16_MAX: f64 = 32767.0;

pub struct InterpolateArgs {
    pub interpolation_idx: u16,
    pub oldest: i16,
    pub older: i16,
    pub old: i16,
    pub sample: i16,
}

pub fn gaussian(
    InterpolateArgs { interpolation_idx, oldest, older, old, sample }: InterpolateArgs,
) -> i16 {
    // Do math in 32 bits to avoid overflows
    let sample: i32 = sample.into();
    let old: i32 = old.into();
    let older: i32 = older.into();
    let oldest: i32 = oldest.into();

    let interpolation_idx = interpolation_idx as usize;

    // Sum the 3 older samples with 15-bit wrapping
    let mut sum = (tables::GAUSSIAN[0x0FF - interpolation_idx] * oldest) >> 11;
    sum += (tables::GAUSSIAN[0x1FF - interpolation_idx] * older) >> 11;
    sum += (tables::GAUSSIAN[0x100 + interpolation_idx] * old) >> 11;

    // Clip to 15 bits
    sum = (((sum as i16) << 1) >> 1).into();

    // Add in the current sample
    sum += (tables::GAUSSIAN[interpolation_idx] * sample) >> 11;

    // Clamp the final result to signed 15-bit
    sum.clamp((i16::MIN >> 1).into(), (i16::MAX >> 1).into()) as i16
}

// Based on https://yehar.com/blog/wp-content/uploads/2009/08/deip.pdf
pub fn hermite(
    InterpolateArgs { interpolation_idx, oldest, older, old, sample }: InterpolateArgs,
) -> i16 {
    let y3: f64 = sample.into();
    let y2: f64 = old.into();
    let y1: f64 = older.into();
    let y0: f64 = oldest.into();
    let x = f64::from(interpolation_idx) / 256.0;

    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    (((c3 * x + c2) * x + c1) * x + c0).round().clamp(I16_MIN, I16_MAX) as i16
}

// Based on https://yehar.com/blog/wp-content/uploads/2009/08/deip.pdf
pub fn lagrange(
    InterpolateArgs { interpolation_idx, oldest, older, old, sample }: InterpolateArgs,
) -> i16 {
    let y3: f64 = sample.into();
    let y2: f64 = old.into();
    let y1: f64 = older.into();
    let y0: f64 = oldest.into();
    let x = f64::from(interpolation_idx) / 256.0;

    let c0 = y1;
    let c1 = y2 - 1.0 / 3.0 * y0 - 0.5 * y1 - 1.0 / 6.0 * y3;
    let c2 = 0.5 * (y0 + y2) - y1;
    let c3 = 1.0 / 6.0 * (y3 - y0) + 0.5 * (y1 - y2);
    (((c3 * x + c2) * x + c1) * x + c0).round().clamp(I16_MIN, I16_MAX) as i16
}
