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
pub struct RgbaFrameBuffer(Box<[Color; ppu::FRAME_BUFFER_LEN]>);

impl RgbaFrameBuffer {
    pub fn copy_from(
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

fn cgb_map_color_gbc_correction(ppu_color: u16) -> Color {
    // Based on this public domain shader:
    // https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gbc-color.cg
    static COLOR_TABLE: LazyLock<Box<[Color; 32768]>> = LazyLock::new(|| {
        Box::new(array::from_fn(|ppu_color| {
            let (r, g, b) = parse_cgb_color(ppu_color as u16);
            let r: f64 = r.into();
            let g: f64 = g.into();
            let b: f64 = b.into();

            let corrected_r = ((0.78824 * r + 0.12157 * g) * 255.0 / 31.0).round() as u8;
            let corrected_g = ((0.025 * r + 0.72941 * g + 0.275 * b) * 255.0 / 31.0).round() as u8;
            let corrected_b = ((0.12039 * r + 0.12157 * g + 0.82 * b) * 255.0 / 31.0).round() as u8;

            Color::rgb(corrected_r, corrected_g, corrected_b)
        }))
    });

    COLOR_TABLE[(ppu_color & 0x7FFF) as usize]
}

fn cgb_map_color_gba_correction(ppu_color: u16) -> Color {
    // Based on this public domain shader:
    // https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gba-color.cg
    static COLOR_TABLE: LazyLock<Box<[Color; 32768]>> = LazyLock::new(|| {
        Box::new(array::from_fn(|ppu_color| {
            let (r, g, b) = parse_cgb_color(ppu_color as u16);
            let r: f64 = r.into();
            let g: f64 = g.into();
            let b: f64 = b.into();

            let corrected_r =
                ((0.845 * r + 0.17 * g - 0.015 * b) * 255.0 / 31.0).powf(2.2 / 2.7).round() as u8;
            let corrected_g =
                ((0.09 * r + 0.68 * g + 0.23 * b) * 255.0 / 31.0).powf(2.2 / 2.7).round() as u8;
            let corrected_b =
                ((0.16 * r + 0.085 * g + 0.755 * b) * 255.0 / 31.0).powf(2.2 / 2.7).round() as u8;

            Color::rgb(corrected_r, corrected_g, corrected_b)
        }))
    });

    COLOR_TABLE[(ppu_color & 0x7FFF) as usize]
}

fn blend(a: Color, b: Color) -> Color {
    fn blend_component(a: u8, b: u8) -> u8 {
        ((u16::from(a) + u16::from(b) + 1) >> 1) as u8
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
