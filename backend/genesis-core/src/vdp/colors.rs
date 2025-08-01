use crate::vdp::{Cram, DarkenColors, VdpConfig};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::Color;
use std::ops::{Add, AddAssign};

#[derive(Debug, Clone, Encode, Decode)]
pub struct ColorTables {
    pub normal: [u8; 8],
    pub shadow: [u8; 8],
    pub highlight: [u8; 8],
}

impl ColorTables {
    // Values from http://gendev.spritesmind.net/forum/viewtopic.php?f=22&t=2188
    pub const NON_LINEAR: Self = Self {
        normal: [0, 52, 87, 116, 144, 172, 206, 255],
        shadow: [0, 29, 52, 70, 87, 101, 116, 130],
        highlight: [130, 144, 158, 172, 187, 206, 228, 255],
    };

    pub const LINEAR: Self = Self {
        normal: [0, 36, 73, 109, 146, 182, 219, 255], // round(i * 255 / 7)
        shadow: [0, 18, 36, 55, 73, 91, 109, 128],    // round(i * 255 / 7 / 2)
        highlight: [128, 146, 164, 182, 200, 219, 237, 255], // round(255 / 2 + i * 255 / 7 / 2)
    };

    // All values -8 relative to NON_LINEAR
    pub const NON_LINEAR_DARK: Self = Self {
        normal: [0, 44, 79, 108, 136, 164, 198, 247],
        shadow: [0, 21, 44, 62, 79, 93, 108, 122],
        highlight: [122, 136, 150, 164, 179, 198, 220, 247],
    };

    // All values -8 relative to LINEAR
    pub const LINEAR_DARK: Self = Self {
        normal: [0, 28, 65, 101, 138, 174, 211, 247],
        shadow: [0, 10, 28, 47, 65, 83, 101, 120],
        highlight: [120, 138, 156, 174, 192, 211, 229, 247],
    };

    pub fn from_config(config: &VdpConfig) -> Self {
        match (config.non_linear_color_scale, config.color_adjustment) {
            (true, DarkenColors::No) => Self::NON_LINEAR,
            (true, DarkenColors::Yes) => Self::NON_LINEAR_DARK,
            (false, DarkenColors::No) => Self::LINEAR,
            (false, DarkenColors::Yes) => Self::LINEAR_DARK,
        }
    }
}

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
pub fn gen_to_rgba(
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    modifier: ColorModifier,
    color_tables: &ColorTables,
) -> Color {
    let colors = match modifier {
        ColorModifier::None => color_tables.normal,
        ColorModifier::Shadow => color_tables.shadow,
        ColorModifier::Highlight => color_tables.highlight,
    };
    Color::rgba(colors[r as usize], colors[g as usize], colors[b as usize], a)
}

pub fn resolve_color(cram: &Cram, palette: u8, color_id: u8) -> u16 {
    cram[((palette << 4) | color_id) as usize]
}
