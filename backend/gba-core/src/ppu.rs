//! GBA PPU (picture processing unit)

mod registers;

use crate::ppu::registers::{BgMode, BitmapFrameBuffer, Registers};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use std::ops::Range;

const VRAM_LOW_LEN: usize = 64 * 1024;
const VRAM_HIGH_LEN: usize = 32 * 1024;
const VRAM_LEN: usize = VRAM_LOW_LEN + VRAM_HIGH_LEN;
const VRAM_ADDR_MASK: usize = (128 * 1024) - 1;

const PALETTE_RAM_LEN_HALFWORDS: usize = 1024 / 2;

const OAM_LEN_HALFWORDS: usize = 1024 / 2;

pub const SCREEN_HEIGHT: u32 = 160;
pub const SCREEN_WIDTH: u32 = 240;
pub const FRAME_BUFFER_LEN: usize = (SCREEN_HEIGHT as usize) * (SCREEN_WIDTH as usize);
pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

pub const LINES_PER_FRAME: u32 = 228;
pub const DOTS_PER_LINE: u32 = 1232;

// VBlank flag is not set on the last line of the frame because of sprite processing for line 0
const VBLANK_LINES: Range<u32> = 160..227;
const HBLANK_START_DOT: u32 = 1006;

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
    frame_complete: bool,
}

impl State {
    fn new() -> Self {
        Self { scanline: 0, dot: 0, frame_complete: false }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    frame_buffer: FrameBuffer,
    vram: BoxedByteArray<VRAM_LEN>,
    palette_ram: BoxedWordArray<PALETTE_RAM_LEN_HALFWORDS>,
    oam: BoxedWordArray<OAM_LEN_HALFWORDS>,
    registers: Registers,
    state: State,
    cycles: u64,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: BoxedByteArray::new(),
            palette_ram: BoxedWordArray::new(),
            oam: BoxedWordArray::new(),
            registers: Registers::new(),
            state: State::new(),
            cycles: 0,
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        let tick_cycles = cycles - self.cycles;
        self.cycles = cycles;

        for _ in 0..tick_cycles {
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.state.dot += 1;
        if self.state.dot == DOTS_PER_LINE {
            self.state.dot = 0;

            self.state.scanline += 1;
            if self.state.scanline == LINES_PER_FRAME {
                self.state.scanline = 0;
            } else if self.state.scanline == SCREEN_HEIGHT {
                match self.registers.bg_mode {
                    BgMode::Three => self.render_frame_mode_3(),
                    BgMode::Four => self.render_frame_mode_4(),
                    mode => {
                        log::warn!("BG mode {mode:?} not implemented");
                        self.render_frame_mode_3();
                    }
                }

                self.state.frame_complete = true;
            }
        }
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

    fn render_frame_mode_4(&mut self) {
        let base_addr = match self.registers.bitmap_frame_buffer {
            BitmapFrameBuffer::Zero => 0x0000,
            BitmapFrameBuffer::One => 0xA000,
        };

        for line in 0..SCREEN_HEIGHT {
            for pixel in 0..SCREEN_WIDTH {
                let vram_addr = base_addr + (line * SCREEN_WIDTH + pixel) as usize;
                let color_id = self.vram[vram_addr];
                let gba_color = self.palette_ram[color_id as usize];

                let rgb8_color = Color::rgb(
                    RGB_5_TO_8[(gba_color & 0x1F) as usize],
                    RGB_5_TO_8[((gba_color >> 5) & 0x1F) as usize],
                    RGB_5_TO_8[((gba_color >> 10) & 0x1F) as usize],
                );
                self.frame_buffer.set(line, pixel, rgb8_color);
            }
        }
    }

    pub fn frame_complete(&self) -> bool {
        self.state.frame_complete
    }

    pub fn clear_frame_complete(&mut self) {
        self.state.frame_complete = false;
    }

    pub fn frame_buffer(&self) -> &[Color] {
        &self.frame_buffer.0
    }

    pub fn write_vram(&mut self, address: u32, value: u16) {
        let mut vram_addr = (address as usize) & VRAM_ADDR_MASK & !1;
        if vram_addr >= 0x10000 {
            vram_addr = 0x10000 | (vram_addr & 0x7FFF);
        }
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_palette_ram(&mut self, address: u32, value: u16) {
        let palette_ram_addr = ((address >> 1) as usize) & (PALETTE_RAM_LEN_HALFWORDS - 1);
        self.palette_ram[palette_ram_addr] = value;
    }

    pub fn write_oam(&mut self, address: u32, value: u16) {
        let oam_addr = ((address >> 1) as usize) & (OAM_LEN_HALFWORDS - 1);
        self.oam[oam_addr] = value;
    }

    pub fn read_register(&self, address: u32) -> u16 {
        log::trace!("PPU register read {address:08X}");

        match address {
            0x4000000 => self.registers.read_dispcnt(),
            0x4000004 => self.read_dispstat(),
            _ => {
                log::warn!("Unhandled PPU register read {address:08X}");
                0
            }
        }
    }

    fn read_dispstat(&self) -> u16 {
        let in_vblank = VBLANK_LINES.contains(&self.state.scanline);
        let in_hblank = self.state.dot >= HBLANK_START_DOT;
        let v_counter_match = (self.state.scanline as u8) == self.registers.v_counter_match;

        u16::from(in_vblank)
            | (u16::from(in_hblank) << 1)
            | (u16::from(v_counter_match) << 2)
            | (u16::from(self.registers.vblank_irq_enabled) << 3)
            | (u16::from(self.registers.hblank_irq_enabled) << 4)
            | (u16::from(self.registers.v_counter_irq_enabled) << 5)
            | (u16::from(self.registers.v_counter_match) << 8)
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        log::trace!("PPU register write {address:08X} {value:04X}");

        match address {
            0x4000000 => self.registers.write_dispcnt(value),
            0x4000004 => self.registers.write_dispstat(value),
            _ => {
                log::warn!("Unhandled PPU register write {address:08X} {value:04X}");
            }
        }
    }
}
