mod registers;

use crate::control::{ControlRegisters, InterruptType};
use crate::ppu::registers::{BgMode, Registers};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;

const SCREEN_WIDTH: u32 = 240;
const SCREEN_HEIGHT: u32 = 160;
const FRAME_BUFFER_LEN: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

const LINES_PER_FRAME: u32 = 228;
const DOTS_PER_LINE: u32 = 308;

pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

const VRAM_LEN: usize = 96 * 1024;
const OAM_LEN: usize = 1024;
const PALETTE_RAM_LEN: usize = 1024;
const PALETTE_RAM_LEN_WORDS: usize = PALETTE_RAM_LEN / 2;

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
    oam: BoxedByteArray<OAM_LEN>,
    palette_ram: BoxedWordArray<PALETTE_RAM_LEN_WORDS>,
    frame_buffer: FrameBuffer,
    registers: Registers,
    state: State,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vram: BoxedByteArray::new(),
            oam: BoxedByteArray::new(),
            palette_ram: BoxedWordArray::new(),
            frame_buffer: FrameBuffer::default(),
            registers: Registers::new(),
            state: State::new(),
        }
    }

    pub fn tick(&mut self, ppu_cycles: u32, control: &mut ControlRegisters) -> PpuTickEffect {
        self.state.dot += ppu_cycles;
        if self.state.dot >= DOTS_PER_LINE {
            self.state.dot -= DOTS_PER_LINE;
            self.state.scanline += 1;

            match self.state.scanline {
                SCREEN_HEIGHT => {
                    self.render_frame();

                    if self.registers.vblank_irq_enabled {
                        control.set_interrupt_flag(InterruptType::VBlank);
                    }

                    return PpuTickEffect::FrameComplete;
                }
                LINES_PER_FRAME => {
                    self.state.scanline = 0;
                }
                _ => {}
            }
        }

        PpuTickEffect::None
    }

    fn render_frame(&mut self) {
        if self.registers.forced_blanking {
            self.frame_buffer.0.fill(Color::BLACK);
            return;
        }

        match self.registers.bg_mode {
            BgMode::Zero => self.render_frame_mode_0(),
            BgMode::Three => self.render_frame_mode_3(),
            BgMode::Four => self.render_frame_mode_4(),
            _ => {
                log::error!("Mode {:?} not implemented", self.registers.bg_mode);
                self.render_frame_mode_3();
            }
        }
    }

    fn render_frame_mode_0(&mut self) {
        let backdrop = self.palette_ram[0];
        self.frame_buffer.0.fill(Color::rgb(
            (backdrop & 0x1F) as u8,
            ((backdrop >> 5) & 0x1F) as u8,
            ((backdrop >> 10) & 0x1F) as u8,
        ));

        // TODO actual BG priority
        for bg in (0..4).rev() {
            self.render_tile_bg(bg);
        }
    }

    fn render_tile_bg(&mut self, bg: usize) {
        if !self.registers.bg_enabled[bg] {
            return;
        }

        let any_window_enabled = self.registers.window_enabled[0]
            || self.registers.window_enabled[1]
            || self.registers.obj_window_enabled;

        let bg_control = &self.registers.bg_control[bg];
        for row in 0..SCREEN_HEIGHT {
            for col in 0..SCREEN_WIDTH {
                // TODO actual window handling
                if self.registers.window_contains_pixel(0, col, row)
                    && !self.registers.window_in_bg_enabled[0][bg]
                {
                    continue;
                }

                if any_window_enabled && !self.registers.window_out_bg_enabled[bg] {
                    continue;
                }

                // TODO don't assume 4bpp tiles and 256x256 screen size
                let scrolled_row = (row + self.registers.bg_v_scroll[bg]) % 256;
                let scrolled_col = (col + self.registers.bg_h_scroll[bg]) % 256;

                // TODO scrolling and rotation/scaling
                let tile_map_row = scrolled_row / 8;
                let tile_map_col = scrolled_col / 8;
                let tile_map_addr = (bg_control.tile_map_base_addr
                    + 2 * (32 * tile_map_row + tile_map_col))
                    as usize;

                let tile_map_value = u16::from_le_bytes(
                    self.vram[tile_map_addr..tile_map_addr + 2].try_into().unwrap(),
                );
                let tile_number: u32 = (tile_map_value & 0x3FF).into();
                let h_flip = tile_map_value.bit(10);
                let v_flip = tile_map_value.bit(11);
                let palette = (tile_map_value >> 12) & 0xF;

                let tile_row = if v_flip { 7 - (scrolled_row % 8) } else { scrolled_row % 8 };
                let tile_col = if h_flip { 7 - (scrolled_col % 8) } else { scrolled_col % 8 };
                let tile_addr = bg_control.tile_data_base_addr
                    + 32 * tile_number
                    + (8 * tile_row + tile_col) / 2;

                let tile_byte = self.vram[tile_addr as usize];
                let color = (tile_byte >> (4 * (tile_col & 1))) & 0xF;
                if color == 0 {
                    continue;
                }

                let palette_addr = (palette << 4) | u16::from(color);
                let pixel = self.palette_ram[palette_addr as usize];

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

    fn render_frame_mode_3(&mut self) {
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

    fn render_frame_mode_4(&mut self) {
        let frame_buffer_addr = if self.registers.bitmap_frame_buffer_1 { 0xA000 } else { 0x0000 };

        for row in 0..SCREEN_HEIGHT {
            for col in 0..SCREEN_WIDTH {
                let vram_addr = (frame_buffer_addr + row * SCREEN_WIDTH + col) as usize;
                let palette_addr = self.vram[vram_addr];
                let pixel = self.palette_ram[palette_addr as usize];

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

    pub fn read_vram_halfword(&self, address: u32) -> u16 {
        let vram_addr = vram_address(address) & !1;
        u16::from_le_bytes(array::from_fn(|i| self.vram[vram_addr + i]))
    }

    pub fn read_vram_word(&self, address: u32) -> u32 {
        let vram_addr = vram_address(address) & !3;
        u32::from_le_bytes(array::from_fn(|i| self.vram[vram_addr + i]))
    }

    pub fn write_vram_halfword(&mut self, address: u32, value: u16) {
        let vram_addr = vram_address(address) & !1;
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_vram_word(&mut self, address: u32, value: u32) {
        let vram_addr = vram_address(address) & !3;
        self.vram[vram_addr..vram_addr + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_oam_word(&mut self, address: u32, value: u32) {
        let oam_addr = (address as usize) & (OAM_LEN - 1) & !3;
        self.oam[oam_addr..oam_addr + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_palette_halfword(&mut self, address: u32, value: u16) {
        let palette_addr = (address as usize) & (PALETTE_RAM_LEN - 1);
        self.palette_ram[palette_addr >> 1] = value;
    }

    pub fn write_palette_word(&mut self, address: u32, value: u32) {
        self.write_palette_halfword(address & !2, value as u16);
        self.write_palette_halfword(address | 2, (value >> 16) as u16);
    }

    pub fn read_register(&mut self, address: u32) -> u16 {
        log::trace!(
            "PPU register read (scanline={} dot={}): {address:08X}",
            self.state.scanline,
            self.state.dot
        );

        match address & 0xFF {
            0x00 => self.registers.read_dispcnt(),
            0x04 => self.read_dispstat(),
            0x06 => self.read_vcount(),
            _ => {
                log::error!("PPU register read {address:08X}");
                0
            }
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
            0x04 => self.registers.write_dispstat(value),
            0x10 | 0x14 | 0x18 | 0x1C => {
                let bg = (address >> 2) & 3;
                self.registers.write_bghofs(bg as usize, value);
            }
            0x12 | 0x16 | 0x1A | 0x1E => {
                let bg = (address >> 2) & 3;
                self.registers.write_bgvofs(bg as usize, value);
            }
            0x08..=0x0F => {
                let bg = (address >> 1) & 3;
                self.registers.write_bgcnt(bg as usize, value);
            }
            0x40 | 0x42 => {
                let window = (address >> 1) & 1;
                self.registers.write_winh(window as usize, value);
            }
            0x44 | 0x46 => {
                let window = (address >> 1) & 1;
                self.registers.write_winv(window as usize, value);
            }
            0x48 => self.registers.write_winin(value),
            0x4A => self.registers.write_winout(value),
            _ => log::error!("PPU I/O register write {address:08X} {value:04X}"),
        }
    }

    // $04000004: DISPSTAT (Display status)
    fn read_dispstat(&self) -> u16 {
        let vblank = self.state.scanline >= SCREEN_HEIGHT;
        let hblank = self.state.dot >= SCREEN_WIDTH;
        let v_counter = self.state.scanline;

        self.registers.read_dispstat(vblank, hblank, v_counter)
    }

    // $04000006: VCOUNT (V counter)
    fn read_vcount(&self) -> u16 {
        self.state.scanline as u16
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
