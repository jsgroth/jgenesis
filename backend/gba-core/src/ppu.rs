//! GBA PPU (picture processing unit)

mod registers;

use crate::interrupts::{InterruptRegisters, InterruptType};
use crate::ppu::registers::{BgMode, BitsPerPixel, Registers};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::GetBit;
use std::array;
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct Pixel(u16);

impl Pixel {
    const TRANSPARENT: Self = Self(0);

    fn transparent(self) -> bool {
        !self.0.bit(15)
    }

    fn red(self) -> u16 {
        self.0 & 0x1F
    }

    fn green(self) -> u16 {
        (self.0 >> 5) & 0x1F
    }

    fn blue(self) -> u16 {
        (self.0 >> 10) & 0x1F
    }

    fn new_opaque(color: u16) -> Self {
        Self(color | 0x8000)
    }

    fn new_transparent(color: u16) -> Self {
        Self(color & 0x7FFF)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Buffers {
    bg_pixels: [[Pixel; SCREEN_WIDTH as usize]; 4],
    obj_pixels: [Pixel; SCREEN_WIDTH as usize],
    obj_priority: [u8; SCREEN_WIDTH as usize],
    obj_semi_transparent: [bool; SCREEN_WIDTH as usize],
}

impl Buffers {
    fn new() -> Self {
        Self {
            bg_pixels: array::from_fn(|_| array::from_fn(|_| Pixel::default())),
            obj_pixels: array::from_fn(|_| Pixel::default()),
            obj_priority: array::from_fn(|_| u8::MAX),
            obj_semi_transparent: array::from_fn(|_| false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Bg0,
    Bg1,
    Bg2,
    Bg3,
    Obj,
    Backdrop,
    None,
}

impl Layer {
    const BG: [Self; 4] = [Self::Bg0, Self::Bg1, Self::Bg2, Self::Bg3];
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    frame_buffer: FrameBuffer,
    vram: BoxedByteArray<VRAM_LEN>,
    palette_ram: BoxedWordArray<PALETTE_RAM_LEN_HALFWORDS>,
    oam: BoxedWordArray<OAM_LEN_HALFWORDS>,
    registers: Registers,
    state: State,
    buffers: Box<Buffers>,
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
            buffers: Box::new(Buffers::new()),
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
        // Arbitrary dot around the middle of the line
        const RENDER_DOT: u32 = 526;

        self.state.dot += 1;
        if self.state.dot == RENDER_DOT {
            self.render_current_line();
            self.render_next_sprite_line();
        } else if self.state.dot == DOTS_PER_LINE {
            self.state.dot = 0;

            self.state.scanline += 1;
            if self.state.scanline == LINES_PER_FRAME {
                self.state.scanline = 0;
            } else if self.state.scanline == SCREEN_HEIGHT {
                if self.registers.vblank_irq_enabled {
                    interrupts.set_flag(InterruptType::VBlank);
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

    fn render_current_line(&mut self) {
        if self.state.scanline >= SCREEN_HEIGHT {
            return;
        }

        if self.registers.forced_blanking {
            self.clear_current_line();
            return;
        }

        self.render_bg_layers();

        self.merge_layers();
    }

    fn clear_current_line(&mut self) {
        const WHITE: Color = Color::rgb(255, 255, 255);

        for pixel in 0..SCREEN_WIDTH {
            self.frame_buffer.set(self.state.scanline, pixel, WHITE);
        }
    }

    fn render_bg_layers(&mut self) {
        match self.registers.bg_mode {
            BgMode::Zero => {
                // BG0-3 in text mode
                for bg in 0..4 {
                    self.render_text_bg(bg);
                }
            }
            BgMode::One => {
                // BG0-1 in text mode, BG2 in affine mode
                for bg in 0..2 {
                    self.render_text_bg(bg);
                }
                // TODO affine BG2
            }
            BgMode::Two => {
                // BG2-3 in affine mode
                // TODO affine BG2-3
            }
            BgMode::Three => {
                // Bitmap mode: 240x160, 15bpp, single frame buffer
                self.render_bg_mode_3();
            }
            BgMode::Four => {
                // Bitmap mode: 240x160, 8bpp, two frame buffers
                self.render_bg_mode_4();
            }
            BgMode::Five => {
                // Bitmap mode: 160x128, 15bpp, two frame buffers
                self.render_bg_mode_5();
            }
            BgMode::Invalid(_) => {}
        }
    }

    fn render_text_bg(&mut self, bg: usize) {
        self.buffers.bg_pixels[bg].fill(Pixel::TRANSPARENT);

        if !self.registers.bg_enabled[bg] {
            return;
        }

        // TODO mosaic

        let bg_control = &self.registers.bg_control[bg];

        let width_tiles = bg_control.size.text_width_tiles();
        let width_screens = width_tiles / 32;
        let height_tiles = bg_control.size.text_height_tiles();

        let h_scroll = self.registers.bg_h_scroll[bg];
        let fine_h_scroll = h_scroll % 8;
        let coarse_h_scroll = h_scroll / 8;

        let v_scroll = self.registers.bg_v_scroll[bg];
        let scrolled_line = self.state.scanline + v_scroll;
        let (tile_map_row, screen_map_row) = {
            let tile_map_row = (scrolled_line / 8) & (height_tiles - 1);
            let screen_map_row = tile_map_row / 32;
            (tile_map_row % 32, screen_map_row)
        };
        let tile_row = scrolled_line % 8;

        let tile_size_bytes = bg_control.bpp.tile_size_bytes();

        let end_tile = if fine_h_scroll != 0 { SCREEN_WIDTH / 8 + 1 } else { SCREEN_WIDTH / 8 };

        for tile_idx in 0..end_tile {
            let base_pixel = (8 * tile_idx) as i32 - fine_h_scroll as i32;

            let (tile_map_col, screen_map_col) = {
                let tile_map_col = (tile_idx + coarse_h_scroll) & (width_tiles - 1);
                let screen_map_col = tile_map_col / 32;
                (tile_map_col % 32, screen_map_col)
            };

            let screen_idx = screen_map_row * width_screens + screen_map_col;
            let screen_addr = bg_control.tile_map_addr + screen_idx * 2 * 32 * 32;

            let tile_map_addr = screen_addr + 2 * (tile_map_row * 32 + tile_map_col);
            let tile_map_entry = if tile_map_addr <= 0xFFFF {
                u16::from_le_bytes([
                    self.vram[tile_map_addr as usize],
                    self.vram[(tile_map_addr + 1) as usize],
                ])
            } else {
                // TODO should read VRAM open bus?
                0
            };

            let tile_number: u32 = (tile_map_entry & 0x3FF).into();
            let h_flip = tile_map_entry.bit(10);
            let v_flip = tile_map_entry.bit(11);
            let palette = tile_map_entry >> 12;

            let tile_base_addr = bg_control.tile_data_addr + tile_number * tile_size_bytes;
            let tile_row = if v_flip { 7 - tile_row } else { tile_row };

            match bg_control.bpp {
                BitsPerPixel::Four => {
                    let tile_row_addr = tile_base_addr + tile_row * 4;

                    for pixel_idx in 0..8 {
                        let pixel = pixel_idx as i32 + base_pixel;
                        if !(0..SCREEN_WIDTH as i32).contains(&pixel) {
                            continue;
                        }
                        let pixel = pixel as usize;

                        let tile_col = if h_flip { 7 - pixel_idx } else { pixel_idx };
                        let tile_addr = tile_row_addr + (tile_col >> 1);

                        let tile_byte = if tile_addr <= 0xFFFF {
                            self.vram[tile_addr as usize]
                        } else {
                            // TODO should read VRAM open bus?
                            0
                        };

                        let color_id = (tile_byte >> (4 * (tile_col & 1))) & 0xF;
                        if color_id == 0 {
                            // Transparent pixel
                            continue;
                        }

                        let palette_ram_addr = 16 * palette + u16::from(color_id);
                        let color = self.palette_ram[palette_ram_addr as usize];
                        self.buffers.bg_pixels[bg][pixel] = Pixel::new_opaque(color);
                    }
                }
                BitsPerPixel::Eight => {
                    todo!("8bpp BG")
                }
            }
        }
    }

    fn render_bg_mode_3(&mut self) {
        if !self.registers.bg_enabled[2] {
            self.buffers.bg_pixels[2].fill(Pixel::TRANSPARENT);
            return;
        }

        // TODO BG2 can be affine
        // TODO mosaic

        let line_addr = self.state.scanline * 2 * SCREEN_WIDTH;

        for pixel in 0..SCREEN_WIDTH {
            let pixel_addr = (line_addr + 2 * pixel) as usize;
            let color = u16::from_le_bytes([self.vram[pixel_addr], self.vram[pixel_addr + 1]]);
            self.buffers.bg_pixels[2][pixel as usize] = Pixel::new_opaque(color);
        }
    }

    fn render_bg_mode_4(&mut self) {
        self.buffers.bg_pixels[2].fill(Pixel::TRANSPARENT);

        if !self.registers.bg_enabled[2] {
            return;
        }

        // TODO BG2 can be affine
        // TODO mosaic

        let fb_addr = self.registers.bitmap_frame_buffer.vram_address();
        let line_addr = fb_addr + self.state.scanline * SCREEN_WIDTH;

        for pixel in 0..SCREEN_WIDTH {
            let pixel_addr = (line_addr + pixel) as usize;
            let color_id = self.vram[pixel_addr];

            if color_id != 0 {
                let color = self.palette_ram[color_id as usize];
                self.buffers.bg_pixels[2][pixel as usize] = Pixel::new_opaque(color);
            }
        }
    }

    fn render_bg_mode_5(&mut self) {
        const MODE_5_WIDTH: u32 = 160;
        const MODE_5_HEIGHT: u32 = 128;

        self.buffers.bg_pixels[2].fill(Pixel::TRANSPARENT);

        if !self.registers.bg_enabled[2] {
            return;
        }

        // TODO BG2 can be affine
        // TODO mosaic

        if self.state.scanline >= MODE_5_HEIGHT {
            return;
        }

        let fb_addr = self.registers.bitmap_frame_buffer.vram_address();
        let line_addr = fb_addr + self.state.scanline * 2 * MODE_5_WIDTH;

        for pixel in 0..MODE_5_WIDTH {
            let pixel_addr = (line_addr + 2 * pixel) as usize;
            let color = u16::from_le_bytes([self.vram[pixel_addr], self.vram[pixel_addr + 1]]);
            self.buffers.bg_pixels[2][pixel as usize] = Pixel::new_opaque(color);
        }
    }

    fn merge_layers(&mut self) {
        let backdrop_color = Pixel::new_transparent(self.palette_ram[0]);

        // TODO windows

        let bg_enabled: [bool; 4] = array::from_fn(|bg| {
            self.registers.bg_enabled[bg] && self.registers.bg_mode.bg_active_in_mode(bg)
        });

        for pixel in 0..SCREEN_WIDTH {
            let mut first_color = backdrop_color;
            let mut first_layer = Layer::Backdrop;
            let mut first_priority = u8::MAX;

            let mut second_color = Pixel::TRANSPARENT;
            let mut second_layer = Layer::None;
            let mut second_priority = u8::MAX;

            let mut check_pixel = |color: Pixel, layer: Layer, priority: u8| {
                if color.transparent() {
                    return;
                }

                if first_color.transparent() || priority < first_priority {
                    second_color = first_color;
                    second_layer = first_layer;
                    second_priority = first_priority;

                    first_color = color;
                    first_layer = layer;
                    first_priority = priority;

                    return;
                }

                if second_color.transparent() || priority < second_priority {
                    second_color = color;
                    second_layer = layer;
                    second_priority = priority;
                }
            };

            if self.registers.obj_enabled {
                check_pixel(
                    self.buffers.obj_pixels[pixel as usize],
                    Layer::Obj,
                    self.buffers.obj_priority[pixel as usize],
                );
            }

            for (bg, enabled) in bg_enabled.into_iter().enumerate() {
                if !enabled {
                    continue;
                }

                check_pixel(
                    self.buffers.bg_pixels[bg][pixel as usize],
                    Layer::BG[bg],
                    self.registers.bg_control[bg].priority,
                );
            }

            // TODO blending

            self.frame_buffer.set(self.state.scanline, pixel, gba_color_to_rgb8(first_color));
        }
    }

    fn render_next_sprite_line(&mut self) {
        // TODO implement
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
        if vram_addr & 0x10000 != 0 { 0x10000 | (vram_addr & 0x7FFF) } else { vram_addr }
    }

    pub fn read_vram(&self, address: u32) -> u16 {
        let vram_addr = Self::mask_vram_address(address);
        u16::from_le_bytes(self.vram[vram_addr..vram_addr + 2].try_into().unwrap())
    }

    pub fn write_vram(&mut self, address: u32, value: u16) {
        let vram_addr = Self::mask_vram_address(address);
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());

        if !(self.in_vblank() || self.in_hblank() || self.registers.forced_blanking) {
            log::debug!(
                "VRAM write to {address:08X} during active rendering (line {} dot {})",
                self.state.scanline,
                self.state.dot
            );
        }
    }

    pub fn read_palette_ram(&self, address: u32) -> u16 {
        let palette_ram_addr = ((address >> 1) as usize) & (PALETTE_RAM_LEN_HALFWORDS - 1);
        self.palette_ram[palette_ram_addr]
    }

    pub fn write_palette_ram(&mut self, address: u32, value: u16) {
        let palette_ram_addr = ((address >> 1) as usize) & (PALETTE_RAM_LEN_HALFWORDS - 1);
        self.palette_ram[palette_ram_addr] = value;

        if !(self.in_vblank() || self.in_hblank() || self.registers.forced_blanking) {
            log::debug!(
                "Palette RAM write to {address:08X} during active rendering (line {} dot {})",
                self.state.scanline,
                self.state.dot
            );
        }
    }

    pub fn read_oam(&self, address: u32) -> u16 {
        let oam_addr = ((address >> 1) as usize) & (OAM_LEN_HALFWORDS - 1);
        self.oam[oam_addr]
    }

    pub fn write_oam(&mut self, address: u32, value: u16) {
        // Dots when OAM is in use when the "OAM free during HBlank" bit is set (DISPCNT bit 5)
        const OAM_USE_DOTS: Range<u32> = 40..1006;

        let oam_addr = ((address >> 1) as usize) & (OAM_LEN_HALFWORDS - 1);
        self.oam[oam_addr] = value;

        if !(self.in_vblank()
            || (self.registers.oam_free_during_hblank && OAM_USE_DOTS.contains(&self.state.dot))
            || self.registers.forced_blanking)
        {
            log::debug!(
                "OAM write to {address:08X} during active rendering (line {} dot {})",
                self.state.scanline,
                self.state.dot
            );
        }
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
        let in_vblank = self.in_vblank();
        let in_hblank = self.in_hblank();
        let v_counter_match = (self.state.scanline as u8) == self.registers.v_counter_match;

        u16::from(in_vblank)
            | (u16::from(in_hblank) << 1)
            | (u16::from(v_counter_match) << 2)
            | (u16::from(self.registers.vblank_irq_enabled) << 3)
            | (u16::from(self.registers.hblank_irq_enabled) << 4)
            | (u16::from(self.registers.v_counter_irq_enabled) << 5)
            | (u16::from(self.registers.v_counter_match) << 8)
    }

    fn in_vblank(&self) -> bool {
        VBLANK_LINES.contains(&self.state.scanline)
    }

    fn in_hblank(&self) -> bool {
        self.state.dot >= HBLANK_START_DOT
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

fn gba_color_to_rgb8(gba_color: Pixel) -> Color {
    Color::rgb(
        RGB_5_TO_8[gba_color.red() as usize],
        RGB_5_TO_8[gba_color.green() as usize],
        RGB_5_TO_8[gba_color.blue() as usize],
    )
}
