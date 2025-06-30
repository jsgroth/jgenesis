//! GBA PPU (picture processing unit)

use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::frontend::{Color, FrameSize};
use std::ops::BitOrAssign;

const VRAM_LEN: usize = 96 * 1024;
const VRAM_ADDR_MASK: usize = (128 * 1024) - 1;

pub const SCREEN_HEIGHT: u32 = 160;
pub const SCREEN_WIDTH: u32 = 240;
pub const FRAME_BUFFER_LEN: usize = (SCREEN_HEIGHT as usize) * (SCREEN_WIDTH as usize);
pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

pub const LINES_PER_FRAME: u32 = 228;
pub const DOTS_PER_LINE: u32 = 1232;

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

#[derive(Debug, Clone, Encode, Decode)]
struct FrameBuffer(Box<[Color]>);

impl FrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice())
    }

    fn set(&mut self, line: u32, pixel: u32, color: Color) {
        let frame_buffer_addr = (line * SCREEN_WIDTH + pixel) as usize;
        self.0[frame_buffer_addr] = color;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u32,
    dot: u32,
}

impl State {
    fn new() -> Self {
        Self { scanline: 0, dot: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuTickEffect {
    None,
    FrameComplete,
}

impl BitOrAssign for PpuTickEffect {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = match (*self, rhs) {
            (Self::FrameComplete, _) | (_, Self::FrameComplete) => Self::FrameComplete,
            (Self::None, Self::None) => Self::None,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    frame_buffer: FrameBuffer,
    vram: BoxedByteArray<VRAM_LEN>,
    state: State,
    cycles: u64,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: BoxedByteArray::new(),
            state: State::new(),
            cycles: 0,
        }
    }

    pub fn step_to(&mut self, cycles: u64) -> PpuTickEffect {
        let tick_cycles = cycles - self.cycles;
        self.cycles = cycles;

        let mut tick_effect = PpuTickEffect::None;
        for _ in 0..tick_cycles {
            tick_effect |= self.tick();
        }
        tick_effect
    }

    fn tick(&mut self) -> PpuTickEffect {
        let mut tick_effect = PpuTickEffect::None;

        self.state.dot += 1;
        if self.state.dot == DOTS_PER_LINE {
            self.state.dot = 0;

            self.state.scanline += 1;
            if self.state.scanline == LINES_PER_FRAME {
                self.state.scanline = 0;
            } else if self.state.scanline == SCREEN_HEIGHT {
                self.render_frame_mode_3();
                tick_effect = PpuTickEffect::FrameComplete;
            }
        }

        tick_effect
    }

    pub fn frame_buffer(&self) -> &[Color] {
        &self.frame_buffer.0
    }

    fn render_frame_mode_3(&mut self) {
        for line in 0..SCREEN_HEIGHT {
            for pixel in 0..SCREEN_WIDTH {
                let vram_addr = (2 * (line * SCREEN_WIDTH + pixel)) as usize;
                let gba_color =
                    u16::from_le_bytes([self.vram[vram_addr], self.vram[vram_addr + 1]]);

                let rgb8_color = Color::rgb(
                    RGB_5_TO_8[(gba_color & 0x1F) as usize],
                    RGB_5_TO_8[((gba_color >> 5) & 0x1F) as usize],
                    RGB_5_TO_8[((gba_color >> 10) & 0x1F) as usize],
                );
                self.frame_buffer.set(line, pixel, rgb8_color);
            }
        }
    }

    pub fn write_vram(&mut self, address: u32, value: u16) {
        let vram_addr = (address as usize) & VRAM_ADDR_MASK & !1;
        if vram_addr >= VRAM_LEN {
            return;
        }

        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }
}
