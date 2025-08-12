use crate::api::GameBoyEmulatorConfig;
use crate::ppu::PpuFrameBuffer;
use crate::{HardwareMode, ppu};
use gb_config::GbPalette;
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::iter;
use std::ops::{Deref, DerefMut};

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
        ppu_frame_buffer: &PpuFrameBuffer,
        hardware_mode: HardwareMode,
        config: &GameBoyEmulatorConfig,
    ) {
        match hardware_mode {
            HardwareMode::Dmg => {
                self.do_copy(ppu_frame_buffer, dmg_map_color(config));
            }
            HardwareMode::Cgb => {
                self.do_copy(ppu_frame_buffer, cgb_map_color);
            }
        }
    }

    fn do_copy(&mut self, ppu_frame_buffer: &PpuFrameBuffer, map_color: impl Fn(u16) -> Color) {
        for (ppu_color, rgba_color) in iter::zip(ppu_frame_buffer.iter(), self.iter_mut()) {
            *rgba_color = map_color(ppu_color);
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
