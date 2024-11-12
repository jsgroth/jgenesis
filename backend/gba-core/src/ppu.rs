mod registers;

use crate::ppu::registers::{BgMode, Registers};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};

const SCREEN_WIDTH: u32 = 240;
const SCREEN_HEIGHT: u32 = 160;
const FRAME_BUFFER_LEN: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

const LINES_PER_FRAME: u32 = 228;
const DOTS_PER_LINE: u32 = 308;

pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

const VRAM_LEN: usize = 96 * 1024;
const PALETTE_RAM_LEN: usize = 1024;

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl Default for FrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
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

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    vram: BoxedByteArray<VRAM_LEN>,
    palette_ram: BoxedByteArray<PALETTE_RAM_LEN>,
    frame_buffer: FrameBuffer,
    registers: Registers,
    state: State,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vram: BoxedByteArray::new(),
            palette_ram: BoxedByteArray::new(),
            frame_buffer: FrameBuffer::default(),
            registers: Registers::new(),
            state: State::new(),
        }
    }

    pub fn tick(&mut self, ppu_cycles: u32) -> PpuTickEffect {
        self.state.dot += ppu_cycles;
        if self.state.dot >= DOTS_PER_LINE {
            self.state.dot -= DOTS_PER_LINE;
            self.state.scanline += 1;

            match self.state.scanline {
                SCREEN_HEIGHT => {
                    self.render_bitmap_frame();
                    return PpuTickEffect::FrameComplete;
                }
                LINES_PER_FRAME => self.state.scanline = 0,
                _ => {}
            }
        }

        PpuTickEffect::None
    }

    fn render_bitmap_frame(&mut self) {
        if self.registers.bg_mode == BgMode::Four {
            self.render_bitmap_4();
            return;
        }

        for row in 0..SCREEN_HEIGHT {
            for col in 0..SCREEN_WIDTH {
                let vram_addr = (2 * (row * SCREEN_WIDTH + col)) as usize;
                let pixel = u16::from_le_bytes([self.vram[vram_addr], self.vram[vram_addr + 1]]);

                let r = pixel & 0x1F;
                let g = (pixel >> 5) & 0x1F;
                let b = (pixel >> 10) & 0x1F;
                self.frame_buffer.0[(row * SCREEN_WIDTH + col) as usize] = Color::rgb(
                    RGB_5_TO_8[r as usize],
                    RGB_5_TO_8[g as usize],
                    RGB_5_TO_8[b as usize],
                );
            }
        }
    }

    fn render_bitmap_4(&mut self) {
        let frame_buffer_addr = if self.registers.bitmap_frame_buffer_1 { 0xA000 } else { 0x0000 };

        for row in 0..SCREEN_HEIGHT {
            for col in 0..SCREEN_WIDTH {
                let vram_addr = (frame_buffer_addr + row * SCREEN_WIDTH + col) as usize;
                let palette_addr = 2 * (self.vram[vram_addr] as usize);
                let pixel = u16::from_le_bytes([
                    self.palette_ram[palette_addr],
                    self.palette_ram[palette_addr + 1],
                ]);

                let r = pixel & 0x1F;
                let g = (pixel >> 5) & 0x1F;
                let b = (pixel >> 10) & 0x1F;
                self.frame_buffer.0[(row * SCREEN_WIDTH + col) as usize] = Color::rgb(
                    RGB_5_TO_8[r as usize],
                    RGB_5_TO_8[g as usize],
                    RGB_5_TO_8[b as usize],
                );
            }
        }
    }

    pub fn write_vram_halfword(&mut self, address: u32, value: u16) {
        let vram_addr = vram_address(address) & !1;
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes())
    }

    pub fn write_palette_halfword(&mut self, address: u32, value: u16) {
        let palette_addr = (address as usize) & (PALETTE_RAM_LEN - 1);
        self.palette_ram[palette_addr..palette_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn read_register(&mut self, address: u32) -> u16 {
        log::trace!(
            "PPU register read (scanline={} dot={}): {address:08X}",
            self.state.scanline,
            self.state.dot
        );

        match address & 0xFF {
            // DISPSTAT
            0x04 => {
                let vblank = self.state.scanline >= SCREEN_HEIGHT;
                let hblank = self.state.dot >= SCREEN_WIDTH;
                u16::from(vblank) | (u16::from(hblank) << 1)
            }
            _ => todo!("PPU register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        log::trace!(
            "PPU register write (scanline={} dot={}): {address:08X} {value:04X}",
            self.state.scanline,
            self.state.dot
        );

        match address & 0xFF {
            0x00 => self.registers.write_dispcnt(value),
            _ => log::error!("PPU I/O register write {address:08X} {value:04X}"),
        }
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.0.as_ref()
    }
}

fn vram_address(address: u32) -> usize {
    // VRAM is 96KB which is not a power of two
    // Mirror it to 128KB by repeating the last 32KB once, then repeatedly mirror that 128KB
    let high_bit = address & 0x10000;
    let vram_addr = high_bit | (address & (0x7FFF | ((high_bit ^ 0x10000) >> 1)));
    vram_addr as usize
}
