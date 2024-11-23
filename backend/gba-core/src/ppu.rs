mod registers;

use crate::control::{ControlRegisters, InterruptType};
use crate::ppu::registers::{
    BgMode, BgScreenSize, BlendMode, ColorDepthBits, ObjTileLayout, Registers,
};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::{array, cmp};

const SCREEN_WIDTH: u32 = 240;
const SCREEN_HEIGHT: u32 = 160;
const FRAME_BUFFER_LEN: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

const MODE_5_BITMAP_WIDTH: u32 = 160;
const MODE_5_BITMAP_HEIGHT: u32 = 128;

pub const LINES_PER_FRAME: u32 = 228;
pub const DOTS_PER_LINE: u32 = 308;

pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

const VRAM_LEN: usize = 96 * 1024;
const OAM_LEN: usize = 1024;
const OAM_LEN_WORDS: usize = OAM_LEN / 2;
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
        Self(
            vec![Color::rgb(255, 255, 255); FRAME_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        )
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

    fn obj_enabled_array(registers: &Registers) -> [bool; 4] {
        [
            registers.window_in_obj_enabled[0],
            registers.window_in_obj_enabled[1],
            registers.obj_window_obj_enabled,
            registers.window_out_obj_enabled,
        ]
    }

    fn blend_enabled_array(registers: &Registers) -> [bool; 4] {
        [
            registers.window_in_color_enabled[0],
            registers.window_in_color_enabled[1],
            registers.obj_window_color_enabled,
            registers.window_out_color_enabled,
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
    ObjSemiTransparent,
    Backdrop,
}

impl Layer {
    fn first_target_enabled(self, registers: &Registers) -> bool {
        match self {
            Self::Bg0 => registers.bg_1st_target[0],
            Self::Bg1 => registers.bg_1st_target[1],
            Self::Bg2 => registers.bg_1st_target[2],
            Self::Bg3 => registers.bg_1st_target[3],
            Self::Obj => registers.obj_1st_target,
            Self::ObjSemiTransparent => true,
            Self::Backdrop => registers.backdrop_1st_target,
        }
    }

    fn second_target_enabled(self, registers: &Registers) -> bool {
        match self {
            Self::Bg0 => registers.bg_2nd_target[0],
            Self::Bg1 => registers.bg_2nd_target[1],
            Self::Bg2 => registers.bg_2nd_target[2],
            Self::Bg3 => registers.bg_2nd_target[3],
            Self::Obj | Self::ObjSemiTransparent => registers.obj_2nd_target,
            Self::Backdrop => registers.backdrop_2nd_target,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteMode {
    Normal = 0,
    SemiTransparent = 1,
    ObjWindow = 2,
    Prohibited = 3,
}

impl SpriteMode {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Normal,
            1 => Self::SemiTransparent,
            2 => Self::ObjWindow,
            3 => Self::Prohibited,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteShape {
    Square = 0,
    HorizontalRect = 1,
    VerticalRect = 2,
    Prohibited = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteSize {
    // 8x8 / 16x8 / 8x16
    Smallest = 0,
    // 16x16 / 32x8 / 8x32
    Small = 1,
    // 32x32 / 32x16 / 16x32
    Large = 2,
    // 64x64 / 64x32 / 32x64
    Largest = 3,
}

impl SpriteSize {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Smallest,
            1 => Self::Small,
            2 => Self::Large,
            3 => Self::Largest,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

impl SpriteShape {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Square,
            1 => Self::HorizontalRect,
            2 => Self::VerticalRect,
            3 => Self::Prohibited,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn width_and_height(self, size: SpriteSize) -> (u32, u32) {
        match (self, size) {
            (Self::Prohibited, _) => panic!("Prohibited sprite shape used"),
            (Self::Square, SpriteSize::Smallest) => (8, 8),
            (Self::HorizontalRect, SpriteSize::Smallest) => (16, 8),
            (Self::VerticalRect, SpriteSize::Smallest) => (8, 16),
            (Self::Square, SpriteSize::Small) => (16, 16),
            (Self::HorizontalRect, SpriteSize::Small) => (32, 8),
            (Self::VerticalRect, SpriteSize::Small) => (8, 32),
            (Self::Square, SpriteSize::Large) => (32, 32),
            (Self::HorizontalRect, SpriteSize::Large) => (32, 16),
            (Self::VerticalRect, SpriteSize::Large) => (16, 32),
            (Self::Square, SpriteSize::Largest) => (64, 64),
            (Self::HorizontalRect, SpriteSize::Largest) => (64, 32),
            (Self::VerticalRect, SpriteSize::Largest) => (32, 64),
        }
    }
}

#[derive(Debug, Clone)]
struct OamEntry {
    // Attribute 0 (bytes 0-1)
    y: u32,
    affine: bool,
    affine_double_size: bool,
    mode: SpriteMode,
    mosaic: bool,
    color_depth: ColorDepthBits,
    shape: SpriteShape,
    // Attribute 1 (bytes 2-3)
    x: u32,
    affine_parameter_group: u16,
    h_flip: bool,
    v_flip: bool,
    size: SpriteSize,
    // Attribute 2 (bytes 4-5)
    tile_number: u32,
    priority: u8,
    palette: u16,
}

impl OamEntry {
    fn parse(attributes: &[u16]) -> Self {
        Self {
            y: (attributes[0] & 0xFF).into(),
            affine: attributes[0].bit(8),
            affine_double_size: attributes[0].bit(9),
            mode: SpriteMode::from_bits(attributes[0] >> 10),
            mosaic: attributes[0].bit(12),
            color_depth: ColorDepthBits::from_bit(attributes[0].bit(13)),
            shape: SpriteShape::from_bits(attributes[0] >> 14),
            x: (attributes[1] & 0x1FF).into(),
            affine_parameter_group: (attributes[1] >> 9) & 0x1F,
            h_flip: attributes[1].bit(12),
            v_flip: attributes[1].bit(13),
            size: SpriteSize::from_bits(attributes[1] >> 14),
            tile_number: (attributes[2] & 0x3FF).into(),
            priority: ((attributes[2] >> 10) & 3) as u8,
            palette: attributes[2] >> 12,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct RenderBuffers {
    windows: [Window; SCREEN_WIDTH as usize],
    // Palette (0-15) + color (0-15) for 4bpp
    // Color (0-255) for 8bpp
    // BGR555 color for bitmap
    bg_colors: [[u16; SCREEN_WIDTH as usize]; 4],
    obj_colors: [u16; SCREEN_WIDTH as usize],
    obj_priority: [u8; SCREEN_WIDTH as usize],
    obj_semi_transparent: [bool; SCREEN_WIDTH as usize],
    resolved_colors: [u16; SCREEN_WIDTH as usize],
    resolved_priority: [u8; SCREEN_WIDTH as usize],
    resolved_layers: [Layer; SCREEN_WIDTH as usize],
    second_resolved_colors: [u16; SCREEN_WIDTH as usize],
    second_resolved_priority: [u8; SCREEN_WIDTH as usize],
    second_resolved_layers: [Layer; SCREEN_WIDTH as usize],
    final_colors: [u16; SCREEN_WIDTH as usize],
    final_layers: [Layer; SCREEN_WIDTH as usize],
}

impl RenderBuffers {
    fn new() -> Self {
        Self {
            windows: array::from_fn(|_| Window::default()),
            bg_colors: array::from_fn(|_| array::from_fn(|_| 0)),
            obj_colors: array::from_fn(|_| 0),
            obj_priority: array::from_fn(|_| 0),
            obj_semi_transparent: array::from_fn(|_| false),
            resolved_colors: array::from_fn(|_| 0),
            resolved_priority: array::from_fn(|_| 0),
            resolved_layers: array::from_fn(|_| Layer::default()),
            second_resolved_colors: array::from_fn(|_| 0),
            second_resolved_priority: array::from_fn(|_| 0),
            second_resolved_layers: array::from_fn(|_| Layer::default()),
            final_colors: array::from_fn(|_| 0),
            final_layers: array::from_fn(|_| Layer::default()),
        }
    }

    fn push_pixel(&mut self, col: usize, color: u16, priority: u8, layer: Layer) {
        if priority <= self.resolved_priority[col] {
            if self.resolved_priority[col] <= self.second_resolved_priority[col] {
                self.second_resolved_colors[col] = self.resolved_colors[col];
                self.second_resolved_priority[col] = self.resolved_priority[col];
                self.second_resolved_layers[col] = self.resolved_layers[col];
            }

            self.resolved_colors[col] = color;
            self.resolved_priority[col] = priority;
            self.resolved_layers[col] = layer;
        } else if priority <= self.second_resolved_priority[col] {
            self.second_resolved_colors[col] = color;
            self.second_resolved_priority[col] = priority;
            self.second_resolved_layers[col] = layer;
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
    oam: BoxedWordArray<OAM_LEN_WORDS>,
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
            oam: BoxedWordArray::new(),
            palette_ram: BoxedWordArray::new(),
            frame_buffer: FrameBuffer::default(),
            registers: Registers::new(),
            state: State::new(),
            buffers: Box::new(RenderBuffers::new()),
        }
    }

    pub fn tick(&mut self, ppu_cycles: u32, control: &mut ControlRegisters) -> PpuTickEffect {
        let prev_dot = self.state.dot;
        self.state.dot += ppu_cycles;

        if self.state.scanline < SCREEN_HEIGHT
            && prev_dot < SCREEN_WIDTH
            && self.state.dot >= SCREEN_WIDTH
        {
            // HBlank
            // TODO should the PPU generate an HBlank interrupt before line 0?
            self.render_current_line();
            if self.registers.hblank_irq_enabled {
                control.notify_hblank();
            }
        }

        if self.state.dot >= DOTS_PER_LINE {
            self.state.dot -= DOTS_PER_LINE;
            self.state.scanline += 1;

            if self.registers.v_counter_irq_enabled
                && self.state.scanline == self.registers.v_counter_target
            {
                control.set_interrupt_flag(InterruptType::VCounterMatch);
            }

            match self.state.scanline {
                SCREEN_HEIGHT => {
                    if self.registers.vblank_irq_enabled {
                        control.notify_vblank();
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
        if self.registers.forced_blanking {
            self.frame_buffer.0[(self.state.scanline * SCREEN_WIDTH) as usize
                ..((self.state.scanline + 1) * SCREEN_WIDTH) as usize]
                .fill(Color::rgb(255, 255, 255));
            return;
        }

        // TODO OBJ window

        self.render_windows_to_buffer();
        self.render_sprites_to_buffer();
        self.render_bgs_to_buffer();
        self.merge_layers();
        self.blend_layers();

        for col in 0..SCREEN_WIDTH {
            let color = self.buffers.final_colors[col as usize];
            self.frame_buffer.set(self.state.scanline, col, gba_color_to_rgb888(color));
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

    fn render_sprites_to_buffer(&mut self) {
        if !self.registers.obj_enabled {
            return;
        }

        self.buffers.obj_colors.fill(0);

        let is_bitmap_mode = self.registers.bg_mode.is_bitmap();
        let window_enabled = if self.any_window_enabled() {
            Window::obj_enabled_array(&self.registers)
        } else {
            [true; 4]
        };

        // TODO sprite limits
        // TODO sprite mosaic
        // TODO affine sprites
        let scanline = self.state.scanline;
        for oam_idx in 0..128 {
            let mut oam_entry = OamEntry::parse(&self.oam[4 * oam_idx..4 * oam_idx + 3]);

            if !oam_entry.affine && oam_entry.affine_double_size {
                // Double size flag means "do not display" for non-affine sprites
                continue;
            }

            let y = oam_entry.y;
            let (width, height) = oam_entry.shape.width_and_height(oam_entry.size);

            let sprite_bottom = (y + height) & 0xFF;
            if (sprite_bottom < y && scanline >= sprite_bottom)
                || (sprite_bottom >= y && !(y..sprite_bottom).contains(&scanline))
            {
                // Sprite is not on this line
                continue;
            }

            let is_8bpp = oam_entry.color_depth == ColorDepthBits::Eight;
            if is_8bpp {
                oam_entry.palette = 0;
                // TODO should this actually be masked out?
                oam_entry.tile_number &= !1;
            }

            if is_bitmap_mode && oam_entry.tile_number < 512 {
                // Sprite tiles 0-511 do not render in bitmap modes
                continue;
            }

            let mut sprite_row = scanline.wrapping_sub(y) & 0xFF;
            if oam_entry.v_flip {
                sprite_row = height - 1 - sprite_row;
            }

            let tile_y_offset = sprite_row / 8;
            let tile_row = sprite_row % 8;

            for dx in 0..width {
                let col = (oam_entry.x + dx) & 0x1FF;
                if !(0..SCREEN_WIDTH).contains(&col) {
                    continue;
                }

                let window = self.buffers.windows[col as usize];
                if !window_enabled[window as usize] {
                    // TODO check window while merging layers, not here
                    continue;
                }

                if self.buffers.obj_colors[col as usize] != 0 {
                    // Already a non-transparent sprite pixel from a sprite with a lower OAM index
                    continue;
                }

                let sprite_col = if oam_entry.h_flip { width - 1 - dx } else { dx };

                let tile_x_offset = sprite_col / 8;
                let tile_col = sprite_col % 8;

                // TODO handle 8bpp sprites correctly
                let adjusted_tile_number = match self.registers.obj_tile_layout {
                    ObjTileLayout::TwoD => {
                        let x = (oam_entry.tile_number + tile_x_offset) & 0x1F;
                        let y = (oam_entry.tile_number + (tile_y_offset << 5)) & (0x1F << 5);
                        y | x
                    }
                    ObjTileLayout::OneD => {
                        let offset = tile_y_offset * width / 8 + tile_x_offset;
                        (oam_entry.tile_number + offset) & 0x3FF
                    }
                };

                let tile_data_addr =
                    (0x10000 | (32 * adjusted_tile_number)) + tile_row * 4 + tile_col / 2;
                let color = match oam_entry.color_depth {
                    ColorDepthBits::Four => {
                        (self.vram[tile_data_addr as usize] >> (4 * (tile_col & 1))) & 0xF
                    }
                    ColorDepthBits::Eight => self.vram[tile_data_addr as usize],
                };
                if color == 0 {
                    // Pixel is transparent
                    continue;
                }

                self.buffers.obj_colors[col as usize] =
                    0x100 | (oam_entry.palette << 4) | u16::from(color);
                self.buffers.obj_priority[col as usize] = oam_entry.priority;
                self.buffers.obj_semi_transparent[col as usize] =
                    oam_entry.mode == SpriteMode::SemiTransparent;
            }
        }
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
            + (screen_y << screen_y_shift) * 2 * 32 * 32
            + tile_map_y * 2 * 32;

        let mut bg_x = self.registers.bg_h_scroll[bg] & (bg_width_pixels - 1);

        let bg_buffer = &mut self.buffers.bg_colors[bg];

        let start_col = -((bg_x & 7) as i32);
        bg_x &= !7;

        for tile_start_col in (start_col..SCREEN_WIDTH as i32).step_by(8) {
            let screen_x = bg_x / 256;
            let tile_map_x = (bg_x % 256) / 8;

            let tile_map_addr = ((tile_map_row_start + screen_x * 2 * 32 * 32 + 2 * tile_map_x)
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
                    // TODO do window checks while merging layers
                    bg_buffer[col as usize] = 0;
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
        self.buffers.second_resolved_colors.fill(0);
        self.buffers.resolved_layers.fill(Layer::Backdrop);
        self.buffers.second_resolved_layers.fill(Layer::Backdrop);
        self.buffers.resolved_priority.fill(u8::MAX);
        self.buffers.second_resolved_priority.fill(u8::MAX);

        // Process BGs in reverse order because BG0 is highest priority in ties
        for bg in (0..4).rev() {
            if !self.registers.bg_enabled[bg] || !self.registers.bg_mode.bg_enabled(bg) {
                continue;
            }

            let priority = self.registers.bg_control[bg].priority;
            let layer = [Layer::Bg0, Layer::Bg1, Layer::Bg2, Layer::Bg3][bg];

            for col in 0..SCREEN_WIDTH {
                let color = self.buffers.bg_colors[bg][col as usize];
                if color == 0 {
                    continue;
                }

                self.buffers.push_pixel(col as usize, color, priority, layer);
            }
        }

        if self.registers.obj_enabled {
            for col in 0..SCREEN_WIDTH {
                let obj_priority = self.buffers.obj_priority[col as usize];
                let color = self.buffers.obj_colors[col as usize];
                if color == 0 {
                    continue;
                }

                let layer = if self.buffers.obj_semi_transparent[col as usize] {
                    Layer::ObjSemiTransparent
                } else {
                    Layer::Obj
                };
                self.buffers.push_pixel(col as usize, color, obj_priority, layer);
            }
        }

        let is_15bpp_bitmap = self.registers.bg_mode.is_15bpp_bitmap();
        for col in 0..SCREEN_WIDTH as usize {
            if !(is_15bpp_bitmap && self.buffers.resolved_layers[col] == Layer::Bg2) {
                let color = self.buffers.resolved_colors[col];
                self.buffers.resolved_colors[col] = self.palette_ram[color as usize];
            }

            if !(is_15bpp_bitmap && self.buffers.second_resolved_layers[col] == Layer::Bg2) {
                let color = self.buffers.second_resolved_colors[col];
                self.buffers.second_resolved_colors[col] = self.palette_ram[color as usize];
            }
        }
    }

    fn blend_layers(&mut self) {
        // TODO semi-transparent sprites
        for col in 0..SCREEN_WIDTH as usize {
            self.buffers.final_layers[col] = self.buffers.resolved_layers[col];

            // TODO clean up
            if self.any_window_enabled() {
                let window = self.buffers.windows[col];
                if !Window::blend_enabled_array(&self.registers)[window as usize] {
                    self.buffers.final_colors[col] = self.buffers.resolved_colors[col];
                    continue;
                }
            }

            // TODO clean up
            if self.buffers.resolved_layers[col] == Layer::ObjSemiTransparent
                && self.buffers.second_resolved_layers[col].second_target_enabled(&self.registers)
            {
                self.buffers.final_colors[col] = alpha_blend(
                    self.buffers.resolved_colors[col],
                    self.registers.alpha_1st,
                    self.buffers.second_resolved_colors[col],
                    self.registers.alpha_2nd,
                );
                continue;
            }

            match self.registers.blend_mode {
                BlendMode::AlphaBlending
                    if self.buffers.resolved_layers[col].first_target_enabled(&self.registers)
                        && self.buffers.second_resolved_layers[col]
                            .second_target_enabled(&self.registers) =>
                {
                    self.buffers.final_colors[col] = alpha_blend(
                        self.buffers.resolved_colors[col],
                        self.registers.alpha_1st,
                        self.buffers.second_resolved_colors[col],
                        self.registers.alpha_2nd,
                    );
                }
                BlendMode::IncreaseBrightness
                    if self.buffers.resolved_layers[col].first_target_enabled(&self.registers) =>
                {
                    self.buffers.final_colors[col] = increase_brightness(
                        self.buffers.resolved_colors[col],
                        self.registers.brightness,
                    );
                }
                BlendMode::DecreaseBrightness
                    if self.buffers.resolved_layers[col].first_target_enabled(&self.registers) =>
                {
                    self.buffers.final_colors[col] = decrease_brightness(
                        self.buffers.resolved_colors[col],
                        self.registers.brightness,
                    );
                }
                _ => {
                    self.buffers.final_colors[col] = self.buffers.resolved_colors[col];
                }
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
        log::trace!("VRAM write on line {} dot {}", self.state.scanline, self.state.dot);
        let vram_addr = vram_address(address) & !1;
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_vram_word(&mut self, address: u32, value: u32) {
        log::trace!("VRAM write on line {} dot {}", self.state.scanline, self.state.dot);
        let vram_addr = vram_address(address) & !3;
        self.vram[vram_addr..vram_addr + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_oam_word(&mut self, address: u32, value: u32) {
        let oam_addr = ((address as usize) & (OAM_LEN - 1) & !3) >> 1;
        self.oam[oam_addr] = value as u16;
        self.oam[oam_addr + 1] = (value >> 16) as u16;
    }

    pub fn read_palette_byte(&self, address: u32) -> u8 {
        let palette_addr = (address as usize) & (PALETTE_RAM_LEN - 1);
        let halfword = self.palette_ram[palette_addr >> 1];
        (halfword >> (8 * (palette_addr & 1))) as u8
    }

    pub fn read_palette_halfword(&self, address: u32) -> u16 {
        let palette_addr = (address as usize) & (PALETTE_RAM_LEN - 1);
        self.palette_ram[palette_addr >> 1]
    }

    pub fn read_palette_word(&self, address: u32) -> u32 {
        let palette_addr = (address as usize) & (PALETTE_RAM_LEN - 1) & !3;
        let low: u32 = self.palette_ram[palette_addr >> 1].into();
        let high: u32 = self.palette_ram[(palette_addr >> 1) + 1].into();
        low | (high << 16)
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
            0x08..=0x0F => {
                let bg = (address >> 1) & 3;
                self.registers.read_bgcnt(bg as usize)
            }
            0x48 => self.registers.read_winin(),
            0x4A => self.registers.read_winout(),
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
            0x50 => self.registers.write_bldcnt(value),
            0x52 => self.registers.write_bldalpha(value),
            0x54 => self.registers.write_bldy(value),
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

fn alpha_blend(color1: u16, alpha1: u16, color2: u16, alpha2: u16) -> u16 {
    let alpha1 = cmp::min(16, alpha1);
    let alpha2 = cmp::min(16, alpha2);

    let r1 = color1 & 0x1F;
    let g1 = (color1 >> 5) & 0x1F;
    let b1 = (color1 >> 10) & 0x1F;

    let r2 = color2 & 0x1F;
    let g2 = (color2 >> 5) & 0x1F;
    let b2 = (color2 >> 10) & 0x1F;

    let r = cmp::min(0x1F, (r1 * alpha1 + r2 * alpha2) >> 4);
    let g = cmp::min(0x1F, (g1 * alpha1 + g2 * alpha2) >> 4);
    let b = cmp::min(0x1F, (b1 * alpha1 + b2 * alpha2) >> 4);

    r | (g << 5) | (b << 10)
}

fn increase_brightness(color: u16, brightness: u16) -> u16 {
    let r = color & 0x1F;
    let g = (color >> 5) & 0x1F;
    let b = (color >> 10) & 0x1F;

    let r = cmp::min(0x1F, r + (((31 - r) * brightness) >> 4));
    let g = cmp::min(0x1F, g + (((31 - g) * brightness) >> 4));
    let b = cmp::min(0x1F, b + (((31 - b) * brightness) >> 4));

    r | (g << 5) | (b << 10)
}

fn decrease_brightness(color: u16, brightness: u16) -> u16 {
    let r = color & 0x1F;
    let g = (color >> 5) & 0x1F;
    let b = (color >> 10) & 0x1F;

    let r = r.saturating_sub((r * brightness) >> 4);
    let g = g.saturating_sub((g * brightness) >> 4);
    let b = b.saturating_sub((b * brightness) >> 4);

    r | (g << 5) | (b << 10)
}
