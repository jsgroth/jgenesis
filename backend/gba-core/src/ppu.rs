//! GBA PPU (picture processing unit)

mod registers;

use crate::interrupts::{InterruptRegisters, InterruptType};
use crate::ppu::registers::{BgMode, BitmapFrameBuffer, Registers, ScreenSize};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::GetBit;
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

    pub fn step_to(&mut self, cycles: u64, interrupts: &mut InterruptRegisters) {
        let tick_cycles = cycles - self.cycles;
        self.cycles = cycles;

        for _ in 0..tick_cycles {
            self.tick(interrupts);
        }
    }

    fn tick(&mut self, interrupts: &mut InterruptRegisters) {
        self.state.dot += 1;
        if self.state.dot == DOTS_PER_LINE {
            self.state.dot = 0;

            self.state.scanline += 1;
            if self.state.scanline == LINES_PER_FRAME {
                self.state.scanline = 0;
            } else if self.state.scanline == SCREEN_HEIGHT {
                if self.registers.vblank_irq_enabled {
                    interrupts.set_flag(InterruptType::VBlank);
                }

                match self.registers.bg_mode {
                    BgMode::Zero => self.render_frame_mode_0(),
                    BgMode::Three => self.render_frame_mode_3(),
                    BgMode::Four => self.render_frame_mode_4(),
                    mode => {
                        log::warn!("BG mode {mode:?} not implemented");
                        self.render_frame_mode_3();
                    }
                }

                self.state.frame_complete = true;
            }

            if self.registers.v_counter_irq_enabled
                && (self.state.scanline as u8) == self.registers.v_counter_match
            {
                interrupts.set_flag(InterruptType::VCounter);
            }
        } else if self.registers.hblank_irq_enabled && self.state.dot == HBLANK_START_DOT {
            interrupts.set_flag(InterruptType::HBlank);
        }
    }

    fn render_frame_mode_0(&mut self) {
        let backdrop = gba_color_to_rgb8(self.palette_ram[0]);

        for line in 0..SCREEN_HEIGHT {
            for pixel in 0..SCREEN_WIDTH {
                let mut min_priority = 4;
                let mut color = backdrop;

                for bg in 0..4 {
                    if !self.registers.bg_enabled[bg] {
                        continue;
                    }

                    let bg_control = &self.registers.bg_control[bg];
                    if bg_control.priority >= min_priority {
                        continue;
                    }

                    let y = line + u32::from(self.registers.bg_v_scroll[bg]);
                    let x = pixel + u32::from(self.registers.bg_h_scroll[bg]);

                    let tile_map_width = bg_control.size.tile_map_width_pixels() / 8;
                    let tile_map_height = bg_control.size.tile_map_height_pixels() / 8;

                    let mut tile_map_row = (y / 8) & (tile_map_height - 1);
                    let mut tile_map_col = (x / 8) & (tile_map_width - 1);

                    let mut base_tile_map_addr = bg_control.tile_map_addr;
                    if tile_map_row >= 32 {
                        match bg_control.size {
                            ScreenSize::Two => {
                                base_tile_map_addr = base_tile_map_addr.wrapping_add(2 * 32 * 32);
                            }
                            _ => {
                                base_tile_map_addr =
                                    base_tile_map_addr.wrapping_add(2 * 2 * 32 * 32);
                            }
                        }
                        tile_map_row %= 32;
                    }

                    if tile_map_col >= 32 {
                        base_tile_map_addr = base_tile_map_addr.wrapping_add(2 * 32 * 32);
                        tile_map_col %= 32;
                    }

                    let tile_map_offset = 32 * tile_map_row + tile_map_col;
                    let tile_map_addr =
                        base_tile_map_addr.wrapping_add((2 * tile_map_offset) as u16) as usize;
                    let tile = u16::from_le_bytes([
                        self.vram[tile_map_addr],
                        self.vram[tile_map_addr + 1],
                    ]);

                    let tile_number = tile & 0x3FF;
                    let h_flip = tile.bit(10);
                    let v_flip = tile.bit(11);
                    let palette = (tile >> 12) as u8;

                    let base_tile_addr =
                        bg_control.tile_data_addr.wrapping_add(32 * tile_number) as usize;

                    let tile_row = if v_flip { 7 - (y & 7) } else { y & 7 };
                    let tile_col = if h_flip { 7 - (x & 7) } else { x & 7 };
                    let tile_offset = (8 * tile_row + tile_col) as usize;
                    let tile_addr = base_tile_addr + (tile_offset >> 1);

                    let color_id = (self.vram[tile_addr] >> (4 * (tile_offset & 1))) & 0xF;
                    if color_id == 0 {
                        continue;
                    }

                    min_priority = bg_control.priority;
                    color = gba_color_to_rgb8(self.palette_ram[(16 * palette + color_id) as usize]);
                }

                self.frame_buffer.set(line, pixel, color);
            }
        }
    }

    fn render_frame_mode_3(&mut self) {
        for line in 0..SCREEN_HEIGHT {
            for pixel in 0..SCREEN_WIDTH {
                let vram_addr = (2 * (line * SCREEN_WIDTH + pixel)) as usize;
                let gba_color =
                    u16::from_le_bytes([self.vram[vram_addr], self.vram[vram_addr + 1]]);
                let rgb8_color = gba_color_to_rgb8(gba_color);
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
                let rgb8_color = gba_color_to_rgb8(gba_color);
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

    fn mask_vram_address(address: u32) -> usize {
        let vram_addr = (address as usize) & VRAM_ADDR_MASK & !1;
        if vram_addr >= 0x10000 { 0x10000 | (vram_addr & 0x7FFF) } else { vram_addr }
    }

    pub fn read_vram(&self, address: u32) -> u16 {
        let vram_addr = Self::mask_vram_address(address);
        u16::from_le_bytes(self.vram[vram_addr..vram_addr + 2].try_into().unwrap())
    }

    pub fn write_vram(&mut self, address: u32, value: u16) {
        let vram_addr = Self::mask_vram_address(address);
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn read_palette_ram(&self, address: u32) -> u16 {
        let palette_ram_addr = ((address >> 1) as usize) & (PALETTE_RAM_LEN_HALFWORDS - 1);
        self.palette_ram[palette_ram_addr]
    }

    pub fn write_palette_ram(&mut self, address: u32, value: u16) {
        let palette_ram_addr = ((address >> 1) as usize) & (PALETTE_RAM_LEN_HALFWORDS - 1);
        self.palette_ram[palette_ram_addr] = value;
    }

    pub fn read_oam(&self, address: u32) -> u16 {
        let oam_addr = ((address >> 1) as usize) & (OAM_LEN_HALFWORDS - 1);
        self.oam[oam_addr]
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
            0x4000006 => self.state.scanline as u16,
            0x4000008..=0x400000E => {
                let bg = (address & 7) >> 1;
                self.registers.read_bgcnt(bg as usize)
            }
            0x4000048 => self.registers.read_winin(),
            0x400004A => self.registers.read_winout(),
            0x4000050 => self.registers.read_bldcnt(),
            0x4000052 => self.registers.read_bldalpha(),
            _ => {
                log::warn!("Unhandled PPU register read {address:08X}");
                0
            }
        }
    }

    // $4000004: DISPSTAT (Display status)
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
            0x4000008..=0x400000E => {
                // BGxCNT
                let bg = (address & 7) >> 1;
                self.registers.write_bgcnt(bg as usize, value);
            }
            0x4000010..=0x400001E => {
                // BGxHOFS / BGxVOFS
                let bg = (address & 0xF) >> 2;
                if !address.bit(1) {
                    self.registers.write_bghofs(bg as usize, value);
                } else {
                    self.registers.write_bgvofs(bg as usize, value);
                }
            }
            0x4000020..=0x400003E => self.registers.write_bg_affine_register(address, value),
            0x4000040 => self.registers.write_winh(0, value),
            0x4000042 => self.registers.write_winh(1, value),
            0x4000044 => self.registers.write_winv(0, value),
            0x4000046 => self.registers.write_winv(1, value),
            0x4000048 => self.registers.write_winin(value),
            0x400004A => self.registers.write_winout(value),
            0x400004C => self.registers.write_mosaic(value),
            0x4000050 => self.registers.write_bldcnt(value),
            0x4000052 => self.registers.write_bldalpha(value),
            0x4000054 => self.registers.write_bldy(value),
            _ => {
                log::warn!("Unhandled PPU register write {address:08X} {value:04X}");
            }
        }
    }
}

fn gba_color_to_rgb8(gba_color: u16) -> Color {
    Color::rgb(
        RGB_5_TO_8[(gba_color & 0x1F) as usize],
        RGB_5_TO_8[((gba_color >> 5) & 0x1F) as usize],
        RGB_5_TO_8[((gba_color >> 10) & 0x1F) as usize],
    )
}
