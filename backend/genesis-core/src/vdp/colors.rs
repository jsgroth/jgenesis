use crate::vdp::Cram;
use jgenesis_common::frontend::Color;
use std::ops::{Add, AddAssign};

// i * 255 / 7
const NORMAL_RGB_COLORS_LINEAR: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];

// i * 255 / 7 / 2
const SHADOWED_RGB_COLORS_LINEAR: [u8; 8] = [0, 18, 36, 55, 73, 91, 109, 128];

// 255 / 2 + i * 255 / 7 / 2
const HIGHLIGHTED_RGB_COLORS_LINEAR: [u8; 8] = [128, 146, 164, 182, 200, 219, 237, 255];

// Values from http://gendev.spritesmind.net/forum/viewtopic.php?f=22&t=2188
const NORMAL_RGB_COLORS_NON_LINEAR: [u8; 8] = [0, 52, 87, 116, 144, 172, 206, 255];
const SHADOWED_RGB_COLORS_NON_LINEAR: [u8; 8] = [0, 29, 52, 70, 87, 101, 116, 130];
const HIGHLIGHTED_RGB_COLORS_NON_LINEAR: [u8; 8] = [130, 144, 158, 172, 187, 206, 228, 255];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModifier {
    None,
    Shadow,
    Highlight,
}

impl Add for ColorModifier {
    type Output = Self;

    #[allow(clippy::unnested_or_patterns)]
    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::None, Self::None)
            | (Self::Shadow, Self::Highlight)
            | (Self::Highlight, Self::Shadow) => Self::None,
            (Self::None, Self::Shadow)
            | (Self::Shadow, Self::None)
            | (Self::Shadow, Self::Shadow) => Self::Shadow,
            (Self::None, Self::Highlight)
            | (Self::Highlight, Self::None)
            | (Self::Highlight, Self::Highlight) => Self::Highlight,
        }
    }
}

impl AddAssign for ColorModifier {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[inline]
pub fn gen_to_rgb(
    r: u8,
    g: u8,
    b: u8,
    modifier: ColorModifier,
    emulate_non_linear_dac: bool,
) -> Color {
    let colors = match (modifier, emulate_non_linear_dac) {
        (ColorModifier::None, false) => NORMAL_RGB_COLORS_LINEAR,
        (ColorModifier::Shadow, false) => SHADOWED_RGB_COLORS_LINEAR,
        (ColorModifier::Highlight, false) => HIGHLIGHTED_RGB_COLORS_LINEAR,
        (ColorModifier::None, true) => NORMAL_RGB_COLORS_NON_LINEAR,
        (ColorModifier::Shadow, true) => SHADOWED_RGB_COLORS_NON_LINEAR,
        (ColorModifier::Highlight, true) => HIGHLIGHTED_RGB_COLORS_NON_LINEAR,
    };
    Color::rgb(colors[r as usize], colors[g as usize], colors[b as usize])
}

pub fn resolve_color(cram: &Cram, palette: u8, color_id: u8) -> u16 {
    cram[((palette << 4) | color_id) as usize]
}
