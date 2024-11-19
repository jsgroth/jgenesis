mod registers;

use crate::control::{ControlRegisters, InterruptType};
use crate::ppu::registers::{BgMode, BgScreenSize, ColorDepthBits, Registers};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;

const SCREEN_WIDTH: u32 = 240;
const SCREEN_HEIGHT: u32 = 160;
const FRAME_BUFFER_LEN: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

const MODE_5_BITMAP_WIDTH: u32 = 160;
const MODE_5_BITMAP_HEIGHT: u32 = 128;

const LINES_PER_FRAME: u32 = 228;
const DOTS_PER_LINE: u32 = 308;

pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

const VRAM_LEN: usize = 96 * 1024;
const OAM_LEN: usize = 1024;
const PALETTE_RAM_LEN: usize = 1024;
const PALETTE_RAM_LEN_WORDS: usize = PALETTE_RAM_LEN / 2;

// BGs can only use the first 64KB of VRAM in tile map mode
const BG_TILE_MAP_MASK: u32 = 0xFFFF;

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

impl FrameBuffer {
    fn set(&mut self, row: u32, col: u32, color: Color) {
        self.0[(row * SCREEN_WIDTH + col) as usize] = color;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Window {
    Zero = 0,
    One = 1,
    Obj = 2,
    #[default]
    Outside = 3,
}

impl Window {
    fn bg_enabled_array(bg: usize, registers: &Registers) -> [bool; 4] {
        [
            registers.window_in_bg_enabled[0][bg],
            registers.window_in_bg_enabled[1][bg],
            registers.obj_window_bg_enabled[bg],
            registers.window_out_bg_enabled[bg],
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Layer {
    #[default]
    Bg0,
    Bg1,
    Bg2,
    Bg3,
    Obj,
    Backdrop,
}

#[derive(Debug, Clone, Encode, Decode)]
struct RenderBuffers {
    windows: [Window; SCREEN_WIDTH as usize],
    // Palette (0-15) + color (0-15) for 4bpp
    // Color (0-255) for 8bpp
    // BGR555 color for bitmap
    bg_colors: [[u16; SCREEN_WIDTH as usize]; 4],
    resolved_colors: [u16; SCREEN_WIDTH as usize],
    resolved_priority: [u8; SCREEN_WIDTH as usize],
    resolved_layers: [Layer; SCREEN_WIDTH as usize],
}

impl RenderBuffers {
    fn new() -> Self {
        Self {
            windows: array::from_fn(|_| Window::default()),
            bg_colors: array::from_fn(|_| array::from_fn(|_| 0)),
            resolved_colors: array::from_fn(|_| 0),
            resolved_priority: array::from_fn(|_| 0),
            resolved_layers: array::from_fn(|_| Layer::default()),
        }
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
    buffers: Box<RenderBuffers>,
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
            buffers: Box::new(RenderBuffers::new()),
        }
    }

    pub fn tick(&mut self, ppu_cycles: u32, control: &mut ControlRegisters) -> PpuTickEffect {
        self.state.dot += ppu_cycles;
        if self.state.dot >= DOTS_PER_LINE {
            self.state.dot -= DOTS_PER_LINE;

            // TODO render at end of HBlank instead?
            if self.state.scanline < SCREEN_HEIGHT {
                self.render_current_line();
            }

            self.state.scanline += 1;

            match self.state.scanline {
                SCREEN_HEIGHT => {
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

    fn render_current_line(&mut self) {
        // TODO OBJs to buffer first

        self.render_windows_to_buffer();
        self.render_bgs_to_buffer();
        self.merge_layers();

        let is_15bpp_bitmap = self.registers.bg_mode.is_15bpp_bitmap();
        for col in 0..SCREEN_WIDTH {
            let color = self.buffers.resolved_colors[col as usize];
            let pixel =
                if is_15bpp_bitmap && self.buffers.resolved_layers[col as usize] == Layer::Bg2 {
                    color
                } else {
                    self.palette_ram[color as usize]
                };
            self.frame_buffer.set(self.state.scanline, col, gba_color_to_rgb888(pixel));
        }
    }

    fn render_windows_to_buffer(&mut self) {
        if !self.any_window_enabled() {
            return;
        }

        // TODO OBJ window - populate as part of sprite processing before rendering windows/BGs?

        self.buffers.windows.fill(Window::Outside);

        // Process in reverse order so that window 0 "renders" over window 1
        for window in (0..2).rev() {
            if !self.registers.window_enabled[window] {
                // Window is not enabled
                continue;
            }

            if !(self.registers.window_y1[window]..self.registers.window_y2[window])
                .contains(&self.state.scanline)
            {
                // Window does not contain this line
                continue;
            }

            let window_value = [Window::Zero, Window::One][window];
            for x in self.registers.window_x1[window]..self.registers.window_x2[window] {
                self.buffers.windows[x as usize] = window_value;
            }
        }
    }

    fn any_window_enabled(&self) -> bool {
        self.registers.window_enabled[0]
            || self.registers.window_enabled[1]
            || self.registers.obj_window_enabled
    }

    fn render_bgs_to_buffer(&mut self) {
        match self.registers.bg_mode {
            BgMode::Zero => {
                // BG0-3 all in tile map mode
                for bg in 0..4 {
                    self.render_tile_map_bg_to_buffer(bg);
                }
            }
            BgMode::One => {
                // BG0-1 in tile map mode, BG2 in affine mode
                for bg in 0..2 {
                    self.render_tile_map_bg_to_buffer(bg);
                }
                // TODO render BG2 affine
            }
            BgMode::Two => {
                // BG2-3 in affine mode
                // TODO render BG2/BG3 affine
            }
            BgMode::Three | BgMode::Four | BgMode::Five => {
                // BG2 bitmap modes
                self.render_bitmap_to_buffer();
            }
        }
    }

    fn render_tile_map_bg_to_buffer(&mut self, bg: usize) {
        if !self.registers.bg_enabled[bg] {
            return;
        }

        let bg_control = &self.registers.bg_control[bg];
        let bg_width_pixels = bg_control.screen_size.tile_map_width_pixels();
        let bg_width_tiles = bg_width_pixels / 8;
        let bg_height_pixels = bg_control.screen_size.tile_map_height_pixels();
        let bg_height_tiles = bg_height_pixels / 8;
        let total_tiles = bg_width_tiles * bg_height_tiles;

        let tile_size_bytes = bg_control.color_depth.tile_size_bytes();
        let tile_row_size_bytes = tile_size_bytes / 8;

        let bg_window_enabled = if self.any_window_enabled() {
            Window::bg_enabled_array(bg, &self.registers)
        } else {
            [true; 4]
        };

        // TODO mosaic

        let bg_y = self.state.scanline.wrapping_add(self.registers.bg_v_scroll[bg])
            & (bg_height_pixels - 1);
        let screen_y = bg_y / 256;
        let tile_map_y = (bg_y % 256) / 8;

        let screen_y_shift = match bg_control.screen_size {
            BgScreenSize::Zero | BgScreenSize::One | BgScreenSize::Two => 0,
            BgScreenSize::Three => 1,
        };

        let tile_map_row_start = bg_control.tile_map_base_addr
            + (screen_y << screen_y_shift) * total_tiles
            + 2 * tile_map_y * 32;

        let mut bg_x = self.registers.bg_h_scroll[bg] & (bg_width_pixels - 1);

        let bg_buffer = &mut self.buffers.bg_colors[bg];

        let start_col = -((bg_x & 7) as i32);
        bg_x &= !7;

        for tile_start_col in (start_col..SCREEN_WIDTH as i32).step_by(8) {
            let screen_x = bg_x / 256;
            let tile_map_x = (bg_x % 256) / 8;

            let tile_map_addr = ((tile_map_row_start + screen_x * total_tiles + 2 * tile_map_x)
                & BG_TILE_MAP_MASK
                & !1) as usize;
            let tile_map_entry =
                u16::from_le_bytes([self.vram[tile_map_addr], self.vram[tile_map_addr + 1]]);

            let tile_number: u32 = (tile_map_entry & 0x3FF).into();
            let h_flip = tile_map_entry.bit(10);
            let v_flip = tile_map_entry.bit(11);
            let palette = match bg_control.color_depth {
                ColorDepthBits::Four => (tile_map_entry >> 12) as u8,
                ColorDepthBits::Eight => 0,
            };

            let tile_row = if v_flip { 7 - (bg_y % 8) } else { bg_y % 8 };
            let tile_data_row_addr = bg_control.tile_data_base_addr
                + tile_number * tile_size_bytes
                + tile_row * tile_row_size_bytes;

            for i in 0..8 {
                let col = tile_start_col + i;
                if !(0..SCREEN_WIDTH as i32).contains(&col) {
                    continue;
                }

                let window = self.buffers.windows[col as usize];
                if !bg_window_enabled[window as usize] {
                    continue;
                }

                let tile_col = (if h_flip { 7 - i } else { i }) as u32;
                let color = match bg_control.color_depth {
                    ColorDepthBits::Four => {
                        let tile_data_addr = (tile_data_row_addr + tile_col / 2) & BG_TILE_MAP_MASK;
                        let tile_data_byte = self.vram[tile_data_addr as usize];
                        (tile_data_byte >> (4 * (tile_col & 1))) & 0xF
                    }
                    ColorDepthBits::Eight => {
                        let tile_data_addr = (tile_data_row_addr + tile_col) & BG_TILE_MAP_MASK;
                        self.vram[tile_data_addr as usize]
                    }
                };

                if color != 0 {
                    bg_buffer[col as usize] = ((palette << 4) | color).into();
                } else {
                    bg_buffer[col as usize] = 0;
                }
            }

            bg_x = (bg_x + 8) & (bg_width_pixels - 1);
        }
    }

    fn render_bitmap_to_buffer(&mut self) {
        if !self.registers.bg_enabled[2] {
            return;
        }

        match self.registers.bg_mode {
            BgMode::Three => {
                // 15bpp 240x160
                self.render_bitmap_to_buffer_inner(|vram, row, col| {
                    let vram_addr = (2 * (row * SCREEN_WIDTH + col)) as usize;
                    u16::from_le_bytes([vram[vram_addr], vram[vram_addr + 1]])
                });
            }
            BgMode::Four => {
                // 8bpp 240x160 with page flipping
                let base_vram_addr =
                    if self.registers.bitmap_frame_buffer_1 { 40 * 1024 } else { 0 };
                self.render_bitmap_to_buffer_inner(|vram, row, col| {
                    let vram_addr = (base_vram_addr + row * SCREEN_WIDTH + col) as usize;
                    vram[vram_addr].into()
                });
            }
            BgMode::Five => {
                // 15bpp 160x128 with page flipping
                if self.state.scanline >= MODE_5_BITMAP_HEIGHT {
                    self.buffers.bg_colors[2].fill(0);
                    return;
                }

                let base_vram_addr =
                    if self.registers.bitmap_frame_buffer_1 { 40 * 1024 } else { 0 };
                self.render_bitmap_to_buffer_inner(|vram, row, col| {
                    if col >= MODE_5_BITMAP_WIDTH {
                        return 0;
                    }

                    let vram_addr =
                        (base_vram_addr + 2 * (row * MODE_5_BITMAP_WIDTH + col)) as usize;
                    u16::from_le_bytes([vram[vram_addr], vram[vram_addr + 1]])
                });

                self.buffers.bg_colors[2][MODE_5_BITMAP_WIDTH as usize..].fill(0);
            }
            _ => panic!(
                "render_bitmap_to_buffer() called in a non-bitmap mode {:?}",
                self.registers.bg_mode
            ),
        }
    }

    fn render_bitmap_to_buffer_inner(
        &mut self,
        pixel_fn: impl Fn(&[u8; VRAM_LEN], u32, u32) -> u16,
    ) {
        if !self.registers.bg_enabled[2] {
            return;
        }

        let window_enabled = if self.any_window_enabled() {
            Window::bg_enabled_array(2, &self.registers)
        } else {
            [true; 4]
        };

        let row = self.state.scanline;
        for col in 0..SCREEN_WIDTH {
            let window = self.buffers.windows[col as usize];
            if !window_enabled[window as usize] {
                continue;
            }

            let pixel = pixel_fn(self.vram.as_ref(), row, col);
            self.buffers.bg_colors[2][col as usize] = pixel;
        }
    }

    fn merge_layers(&mut self) {
        self.buffers.resolved_colors.fill(0);
        self.buffers.resolved_layers.fill(Layer::Backdrop);
        self.buffers.resolved_priority.fill(u8::MAX);

        // Process BGs in reverse order because BG0 is highest priority in ties
        for bg in (0..4).rev() {
            if !self.registers.bg_enabled[bg] || !self.registers.bg_mode.bg_enabled(bg) {
                continue;
            }

            let priority = self.registers.bg_control[bg].priority;
            let layer = [Layer::Bg0, Layer::Bg1, Layer::Bg2, Layer::Bg3][bg];

            for col in 0..SCREEN_WIDTH {
                if priority > self.buffers.resolved_priority[col as usize] {
                    continue;
                }

                let color = self.buffers.bg_colors[bg][col as usize];
                if color == 0 {
                    continue;
                }

                self.buffers.resolved_colors[col as usize] = color;
                self.buffers.resolved_layers[col as usize] = layer;
                self.buffers.resolved_priority[col as usize] = priority;
            }
        }

        // TODO OBJ
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

fn gba_color_to_rgb888(gba_color: u16) -> Color {
    let r = gba_color & 0x1F;
    let g = (gba_color >> 5) & 0x1F;
    let b = (gba_color >> 10) & 0x1F;
    Color::rgb(RGB_5_TO_8[r as usize], RGB_5_TO_8[g as usize], RGB_5_TO_8[b as usize])
}
