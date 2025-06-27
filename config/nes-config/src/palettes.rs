//! Implements the NES-to-NTSC-to-YUV-to-RGB algorithm described here:
//! <https://www.nesdev.org/wiki/NTSC_video>
//!
//! Generates 12 NTSC samples for each NES color, converts those to a single YUV sample, then
//! converts from YUV to RGB

#![allow(clippy::many_single_char_names)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColorEmphasis {
    r: bool,
    g: bool,
    b: bool,
}

impl ColorEmphasis {
    const NONE: Self = Self { r: false, g: false, b: false };
}

use crate::NesPalette;
use std::array;

#[derive(Debug, Clone, Copy)]
pub struct PaletteGenerationArgs {
    pub brightness: f64,
    pub saturation: f64,
    pub contrast: f64,
    pub gamma: f64,
    pub hue_offset: f64,
}

impl Default for PaletteGenerationArgs {
    fn default() -> Self {
        Self { brightness: 1.0, saturation: 1.0, contrast: 1.0, gamma: 2.0, hue_offset: 0.0 }
    }
}

fn generate_normalized_ntsc_signal(nes_color: u8, emphasis: ColorEmphasis, phase: u8) -> f64 {
    const BLACK: f64 = 0.312;
    const WHITE: f64 = 1.100;

    const LOW: [f64; 4] = [0.228, 0.312, 0.552, 0.880];
    const HIGH: [f64; 4] = [0.616, 0.840, 1.100, 1.100];
    const ATTENUATED_LOW: [f64; 4] = [0.192, 0.256, 0.448, 0.712];
    const ATTENUATED_HIGH: [f64; 4] = [0.500, 0.676, 0.896, 0.896];

    let hue = nes_color & 0xF;

    // Luma is forced to 1 for colors $xE and $xF
    let luma = if hue < 0xE { (nes_color >> 4) & 3 } else { 1 };

    let in_color_phase = |color| (color + phase) % 12 < 6;

    // Color emphasis does not apply to colors $xE and $xF
    let attenuate = hue < 0xE
        && ((emphasis.r && in_color_phase(0))
            || (emphasis.g && in_color_phase(4))
            || (emphasis.b && in_color_phase(8)));

    let (low, high) = if attenuate {
        (ATTENUATED_LOW[luma as usize], ATTENUATED_HIGH[luma as usize])
    } else {
        (LOW[luma as usize], HIGH[luma as usize])
    };

    // Signal is always high for colors $x0 and always low for colors $xD, $xE, $xF
    let signal = match hue {
        0 => high,
        1..=12 => {
            if in_color_phase(hue) {
                high
            } else {
                low
            }
        }
        13..=15 => low,
        _ => unreachable!("value & 0xF is always <= 15"),
    };

    // Normalize signal
    (signal - BLACK) / (WHITE - BLACK)
}

// From <https://www.nesdev.org/wiki/NTSC_video#Chroma_saturation_correction>
const CHROMA_SATURATION_CORRECTION: f64 = 2.0 * (40.0 / 140.0) / (0.524 - 0.148);

const SAMPLES: u8 = 12;
const WEIGHT: f64 = 1.0 / (SAMPLES as f64);

fn nes_to_yuv(nes_color: u8, emphasis: ColorEmphasis, hue_offset: f64) -> (f64, f64, f64) {
    use std::f64::consts::PI;

    let mut y = 0.0;
    let mut u = 0.0;
    let mut v = 0.0;

    for phase in 0..SAMPLES {
        let ntsc_signal = WEIGHT * generate_normalized_ntsc_signal(nes_color, emphasis, phase);
        let wave_phase = f64::from(phase) + 3.0 + hue_offset;

        y += ntsc_signal;
        u += ntsc_signal * (wave_phase / 12.0 * 2.0 * PI).sin() * CHROMA_SATURATION_CORRECTION;
        v += ntsc_signal * (wave_phase / 12.0 * 2.0 * PI).cos() * CHROMA_SATURATION_CORRECTION;
    }

    (y, u, v)
}

fn yuv_to_rgb(y: f64, u: f64, v: f64, gamma: f64) -> (u8, u8, u8) {
    let apply_gamma = |c: f64| if c >= 0.0 { c.powf(2.2 / gamma) } else { 0.0 };
    let clamp_to_u8 = |c: f64| (255.0 * c).clamp(0.0, 255.0).round() as u8;

    let r = clamp_to_u8(apply_gamma(y + 1.139883 * v));
    let g = clamp_to_u8(apply_gamma(y - 0.394642 * u - 0.580622 * v));
    let b = clamp_to_u8(apply_gamma(y + 2.032062 * u));

    (r, g, b)
}

fn emphasis_from_index(index: usize) -> ColorEmphasis {
    ColorEmphasis { r: index & 0x040 != 0, g: index & 0x080 != 0, b: index & 0x100 != 0 }
}

#[must_use]
pub fn generate(args: PaletteGenerationArgs) -> NesPalette {
    NesPalette(array::from_fn(|color| {
        let nes_color = (color & 0x03F) as u8;
        let emphasis = emphasis_from_index(color);

        let (mut y, mut u, mut v) = nes_to_yuv(nes_color, emphasis, args.hue_offset);

        // Apply contrast
        y = (y - 0.5) * args.contrast + 0.5;

        // Apply brightness and saturation
        y *= args.brightness;
        u *= args.brightness * args.saturation;
        v *= args.brightness * args.saturation;

        yuv_to_rgb(y, u, v, args.gamma)
    }))
}

fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let u8_to_f64 = |c: u8| f64::from(c) / 255.0;

    let r: f64 = u8_to_f64(r);
    let g: f64 = u8_to_f64(g);
    let b: f64 = u8_to_f64(b);

    let y = r * 0.299 + g * 0.587 + b * 0.114;
    let u = 0.492111 * (b - y);
    let v = 0.877283 * (r - y);

    (y, u, v)
}

#[must_use]
pub fn extrapolate_64_to_512(palette: &[(u8, u8, u8); 64]) -> NesPalette {
    use std::f64::consts::PI;

    NesPalette(array::from_fn(|color| {
        if color < 64 {
            return palette[color];
        }

        let nes_color = (color & 0x03F) as u8;
        let (r, g, b) = palette[nes_color as usize];
        let emphasis = emphasis_from_index(color);

        let (mut y, mut u, mut v) = rgb_to_yuv(r, g, b);

        for phase in 0..SAMPLES {
            let ntsc_without_emphasis =
                generate_normalized_ntsc_signal(nes_color, ColorEmphasis::NONE, phase);
            let ntsc_with_emphasis = generate_normalized_ntsc_signal(nes_color, emphasis, phase);
            let difference = WEIGHT * (ntsc_without_emphasis - ntsc_with_emphasis);
            debug_assert!(difference >= 0.0);

            if difference < 1e-6 {
                continue;
            }

            let wave_phase = (f64::from(phase) + 3.0) / 12.0;

            y -= difference;
            u -= difference * (wave_phase * 2.0 * PI).sin() * CHROMA_SATURATION_CORRECTION;
            v -= difference * (wave_phase * 2.0 * PI).cos() * CHROMA_SATURATION_CORRECTION;
        }

        // YUV to RGB conversion uses 2.2 as "base" gamma
        yuv_to_rgb(y, u, v, 2.2)
    }))
}
