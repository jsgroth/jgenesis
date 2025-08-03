use crate::api::GameBoyEmulatorConfig;
use crate::ppu::PpuFrameBuffers;
use crate::{HardwareMode, ppu};
use gb_config::{GbPalette, GbcColorCorrection};
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use std::{array, iter};

// 0/0/0 = black and 255/255/255 = white, so linearly map [0,3] to [255,0]
const GB_COLOR_TO_RGB_BW: [[u8; 3]; 4] =
    [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];

// Render with a light green tint
pub(crate) const GB_COLOR_TO_RGB_GREEN_TINT: [[u8; 3]; 4] =
    [[0xAE, 0xD2, 0x8D], [0x75, 0x9C, 0x68], [0x40, 0x5E, 0x2D], [0x0C, 0x1E, 0x09]];

// Render with an intense green tint that somewhat mimics the original Game Boy LCD screen
const GB_COLOR_TO_RGB_LIME_GREEN: [[u8; 3]; 4] =
    [[0x80, 0xA6, 0x08], [0x5D, 0x7F, 0x07], [0x25, 0x5C, 0x1A], [0x00, 0x32, 0x00]];

// [round(255 * i / 31) for i in range(32)]
pub(crate) const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub(crate) struct RgbaFrameBuffer(Box<[Color; ppu::FRAME_BUFFER_LEN]>);

impl RgbaFrameBuffer {
    pub(crate) fn copy_from(
        &mut self,
        ppu_frame_buffers: PpuFrameBuffers<'_>,
        hardware_mode: HardwareMode,
        config: &GameBoyEmulatorConfig,
    ) {
        match hardware_mode {
            HardwareMode::Dmg => {
                self.do_copy(ppu_frame_buffers, config.frame_blending, dmg_map_color(config));
            }
            HardwareMode::Cgb => match config.gbc_color_correction {
                GbcColorCorrection::None => {
                    self.do_copy(ppu_frame_buffers, config.frame_blending, cgb_map_color);
                }
                GbcColorCorrection::GbcLcd => self.do_copy(
                    ppu_frame_buffers,
                    config.frame_blending,
                    cgb_map_color_gbc_correction,
                ),
                GbcColorCorrection::GbaLcd => self.do_copy(
                    ppu_frame_buffers,
                    config.frame_blending,
                    cgb_map_color_gba_correction,
                ),
            },
        }
    }

    fn do_copy(
        &mut self,
        ppu_frame_buffers: PpuFrameBuffers<'_>,
        frame_blending: bool,
        map_color: impl Fn(u16) -> Color,
    ) {
        if frame_blending {
            for ((ppu_color_current, ppu_color_prev), rgba_color) in iter::zip(
                iter::zip(ppu_frame_buffers.current.iter(), ppu_frame_buffers.previous.iter()),
                self.iter_mut(),
            ) {
                let rgba_current = map_color(ppu_color_current);
                let rgba_prev = map_color(ppu_color_prev);
                *rgba_color = blend(rgba_current, rgba_prev);
            }
        } else {
            for (ppu_color, rgba_color) in
                iter::zip(ppu_frame_buffers.current.iter(), self.iter_mut())
            {
                *rgba_color = map_color(ppu_color);
            }
        }
    }
}

fn dmg_map_color(config: &GameBoyEmulatorConfig) -> impl Fn(u16) -> Color {
    let color_mapping = match config.gb_palette {
        GbPalette::BlackAndWhite => GB_COLOR_TO_RGB_BW,
        GbPalette::GreenTint => GB_COLOR_TO_RGB_GREEN_TINT,
        GbPalette::LimeGreen => GB_COLOR_TO_RGB_LIME_GREEN,
        GbPalette::Custom => config.gb_custom_palette.map(|(r, g, b)| [r, g, b]),
    };

    move |ppu_color| {
        let [r, g, b] = color_mapping[ppu_color as usize];
        Color::rgb(r, g, b)
    }
}

fn cgb_map_color(ppu_color: u16) -> Color {
    let (r, g, b) = parse_cgb_color(ppu_color);
    Color::rgb(RGB_5_TO_8[r as usize], RGB_5_TO_8[g as usize], RGB_5_TO_8[b as usize])
}

fn correct_gbc_color(
    color: usize,
    screen_gamma: f64,
    display_gamma: f64,
    mat: &[[f64; 3]; 3],
) -> Color {
    fn u8_to_f64(component: u8, gamma: f64) -> f64 {
        (f64::from(component) / 31.0).powf(gamma)
    }

    fn f64_to_u8(component: f64, gamma: f64) -> u8 {
        (255.0 * component.powf(1.0 / gamma)).clamp(0.0, 255.0).round() as u8
    }

    let (r, g, b) = parse_cgb_color(color as u16);
    let [r, g, b] = [r, g, b].map(|c| u8_to_f64(c, screen_gamma));

    let [r, g, b] = [
        mat[0][0] * r + mat[0][1] * g + mat[0][2] * b,
        mat[1][0] * r + mat[1][1] * g + mat[1][2] * b,
        mat[2][0] * r + mat[2][1] * g + mat[2][2] * b,
    ];

    let [r, g, b] = [r, g, b].map(|c| f64_to_u8(c, display_gamma));
    Color::rgb(r, g, b)
}

#[rustfmt::skip]
fn gbc_lcd_correction(color: usize) -> Color {
    // Based on this public domain shader:
    // https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gbc-color.cg
    correct_gbc_color(
        color,
        2.0, // Slightly brighten
        2.2,
        &[
            [0.78824, 0.12157, 0.0  ],
            [0.025  , 0.72941, 0.275],
            [0.12039, 0.12157, 0.82 ],
        ],
    )
}

#[must_use]
#[rustfmt::skip]
pub fn gba_lcd_correction(color: usize) -> Color {
    // Based on this public domain shader:
    // https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gba-color.cg
    correct_gbc_color(
        color,
        3.2, // Significantly darken
        2.2,
        &[
            [0.845, 0.17 , 0.015],
            [0.09 , 0.68 , 0.23 ],
            [0.16 , 0.085, 0.755],
        ],
    )
}

fn cgb_map_color_gbc_correction(ppu_color: u16) -> Color {
    static COLOR_TABLE: LazyLock<Box<[Color; 32768]>> =
        LazyLock::new(|| Box::new(array::from_fn(gbc_lcd_correction)));

    COLOR_TABLE[(ppu_color & 0x7FFF) as usize]
}

fn cgb_map_color_gba_correction(ppu_color: u16) -> Color {
    static COLOR_TABLE: LazyLock<Box<[Color; 32768]>> =
        LazyLock::new(|| Box::new(array::from_fn(gba_lcd_correction)));

    COLOR_TABLE[(ppu_color & 0x7FFF) as usize]
}

fn blend(a: Color, b: Color) -> Color {
    // TODO this should really be done on the GPU, not the CPU
    // a GPU implementation would also make this easier to use for other systems (e.g. GBA)
    const GAMMA: f64 = 2.2;

    // Using a 64K lookup table for this seems to be significantly faster than not, from limited testing
    static BLEND_TABLE: LazyLock<Box<[u8; 65536]>> = LazyLock::new(|| {
        Box::new(array::from_fn(|i| {
            let a = i >> 8;
            let b = i & 0xFF;

            // Convert to linear color space
            let a = (a as f64 / 255.0).powf(GAMMA);
            let b = (b as f64 / 255.0).powf(GAMMA);

            // Average
            let c = 0.5 * (a + b);

            // Convert back to sRGB 0-255 scale
            (c.powf(1.0 / GAMMA) * 255.0).clamp(0.0, 255.0).round() as u8
        }))
    });

    fn blend_component(a: u8, b: u8) -> u8 {
        let table_idx = (usize::from(a) << 8) | usize::from(b);
        BLEND_TABLE[table_idx]
    }

    Color::rgb(blend_component(a.r, b.r), blend_component(a.g, b.g), blend_component(a.b, b.b))
}

pub(crate) fn parse_cgb_color(ppu_color: u16) -> (u8, u8, u8) {
    let r = ppu_color & 0x1F;
    let g = (ppu_color >> 5) & 0x1F;
    let b = (ppu_color >> 10) & 0x1F;

    (r as u8, g as u8, b as u8)
}

impl Default for RgbaFrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); ppu::FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Deref for RgbaFrameBuffer {
    type Target = [Color; ppu::FRAME_BUFFER_LEN];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RgbaFrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
