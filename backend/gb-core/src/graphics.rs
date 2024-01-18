use crate::ppu;
use crate::ppu::PpuFrameBuffer;
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut};

// 0/0/0 = black and 255/255/255 = white, so linearly map [0,3] to [255,0]
const GB_COLOR_TO_RGB_BW: [[u8; 3]; 4] =
    [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct RgbaFrameBuffer(Box<[Color; ppu::FRAME_BUFFER_LEN]>);

impl RgbaFrameBuffer {
    pub fn copy_from(&mut self, ppu_frame_buffer: &PpuFrameBuffer) {
        for (ppu_color, rgba_color) in ppu_frame_buffer.iter().zip(self.iter_mut()) {
            let [r, g, b] = GB_COLOR_TO_RGB_BW[ppu_color as usize];
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
