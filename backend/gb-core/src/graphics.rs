use crate::api::GbPalette;
use crate::ppu::PpuFrameBuffer;
use crate::{ppu, HardwareMode};
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut};

// 0/0/0 = black and 255/255/255 = white, so linearly map [0,3] to [255,0]
const GB_COLOR_TO_RGB_BW: [[u8; 3]; 4] =
    [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];

// Render with a light green tint
const GB_COLOR_TO_RGB_GREEN_TINT: [[u8; 3]; 4] =
    [[0xAE, 0xD2, 0x8D], [0x75, 0x9C, 0x68], [0x40, 0x5E, 0x2D], [0x0C, 0x1E, 0x09]];

// Render with an intense green tint that somewhat mimics the original Game Boy LCD screen
const GB_COLOR_TO_RGB_LIME_GREEN: [[u8; 3]; 4] =
    [[0x80, 0xA6, 0x08], [0x5D, 0x7F, 0x07], [0x25, 0x5C, 0x1A], [0x00, 0x32, 0x00]];

// [round(255 * i / 31) for i in range(32)]
const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct RgbaFrameBuffer(Box<[Color; ppu::FRAME_BUFFER_LEN]>);

impl RgbaFrameBuffer {
    pub fn copy_from(
        &mut self,
        ppu_frame_buffer: &PpuFrameBuffer,
        hardware_mode: HardwareMode,
        gb_palette: GbPalette,
    ) {
        match hardware_mode {
            HardwareMode::Dmg => self.copy_from_dmg(ppu_frame_buffer, gb_palette),
            HardwareMode::Cgb => self.copy_from_cgb(ppu_frame_buffer),
        }
    }

    fn copy_from_dmg(&mut self, ppu_frame_buffer: &PpuFrameBuffer, gb_palette: GbPalette) {
        let color_mapping = match gb_palette {
            GbPalette::BlackAndWhite => GB_COLOR_TO_RGB_BW,
            GbPalette::GreenTint => GB_COLOR_TO_RGB_GREEN_TINT,
            GbPalette::LimeGreen => GB_COLOR_TO_RGB_LIME_GREEN,
        };

        for (ppu_color, rgba_color) in ppu_frame_buffer.iter().zip(self.iter_mut()) {
            let [r, g, b] = color_mapping[ppu_color as usize];
            *rgba_color = Color::rgb(r, g, b);
        }
    }

    fn copy_from_cgb(&mut self, ppu_frame_buffer: &PpuFrameBuffer) {
        for (ppu_color, rgba_color) in ppu_frame_buffer.iter().zip(self.iter_mut()) {
            let r = RGB_5_TO_8[(ppu_color & 0x1F) as usize];
            let g = RGB_5_TO_8[((ppu_color >> 5) & 0x1F) as usize];
            let b = RGB_5_TO_8[((ppu_color >> 10) & 0x1F) as usize];
            *rgba_color = Color::rgb(r, g, b);
        }
    }
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
