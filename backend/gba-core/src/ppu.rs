//! GBA PPU (picture processing unit, i.e. the graphics processor)
//!
//! The GBA PPU is descended from the SNES PPU, but it's simplified in ways that largely make sense.
//! It also adds some nifty new features like affine sprites and bitmap display modes.

mod registers;

use crate::control::{ControlRegisters, InterruptType};
use crate::ppu::registers::{
    AffineOverflowBehavior, BgMode, BgScreenSize, BlendMode, ColorDepthBits, ObjTileLayout,
    Registers,
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
    // Internal registers used for affine BG rendering
    // For a reference point (X0, Y0) and affine parameters B and D:
    //   X = X0 + B * line
    //   Y = Y0 + D * line
    affine_bg_x: [i32; 2],
    affine_bg_y: [i32; 2],
}

impl State {
    fn new() -> Self {
        Self { scanline: 0, dot: 0, affine_bg_x: [0, 0], affine_bg_y: [0, 0] }
    }

    // Called whenever BG2X or BG3X is written
    fn handle_bgx_update(&mut self, bg: usize, registers: &Registers) {
        self.affine_bg_x[bg - 2] = registers.bg_affine_point[bg - 2][0];
    }

    // Called whenever BG2Y or BG3Y is written
    fn handle_bgy_update(&mut self, bg: usize, registers: &Registers) {
        self.affine_bg_y[bg - 2] = registers.bg_affine_point[bg - 2][1];
    }

    // Called once per line, at the start of HBlank
    fn increment_internal_affine_registers(&mut self, registers: &Registers) {
        for bg in 0..2 {
            let [_, b, _, d] = registers.bg_affine_parameters[bg];
            self.affine_bg_x[bg] += b;
            self.affine_bg_y[bg] += d;
        }
    }

    // Called once per frame, at the start of VBlank
    fn reset_internal_affine_registers(&mut self, registers: &Registers) {
        for bg in 0..2 {
            self.affine_bg_x[bg] = registers.bg_affine_point[bg][0];
            self.affine_bg_y[bg] = registers.bg_affine_point[bg][1];
        }
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
        if !registers.any_window_enabled() {
            return [true; 4];
        }

        [
            registers.window_in_bg_enabled[0][bg],
            registers.window_in_bg_enabled[1][bg],
            registers.obj_window_bg_enabled[bg],
            registers.window_out_bg_enabled[bg],
        ]
    }

    fn obj_enabled_array(registers: &Registers) -> [bool; 4] {
        if !registers.any_window_enabled() {
            return [true; 4];
        }

        [
            registers.window_in_obj_enabled[0],
            registers.window_in_obj_enabled[1],
            registers.obj_window_obj_enabled,
            registers.window_out_obj_enabled,
        ]
    }

    fn blend_enabled_array(registers: &Registers) -> [bool; 4] {
        if !registers.any_window_enabled() {
            return [true; 4];
        }

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
            Self::Obj | Self::ObjSemiTransparent => registers.obj_1st_target,
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
    obj_mosaic: [bool; SCREEN_WIDTH as usize],
    resolved_colors: [u16; SCREEN_WIDTH as usize],
    resolved_priority: [u8; SCREEN_WIDTH as usize],
    resolved_layers: [Layer; SCREEN_WIDTH as usize],
    second_resolved_colors: [u16; SCREEN_WIDTH as usize],
    second_resolved_priority: [u8; SCREEN_WIDTH as usize],
    second_resolved_layers: [Layer; SCREEN_WIDTH as usize],
}

impl RenderBuffers {
    fn new() -> Self {
        Self {
            windows: array::from_fn(|_| Window::default()),
            bg_colors: array::from_fn(|_| array::from_fn(|_| 0)),
            obj_colors: array::from_fn(|_| 0),
            obj_priority: array::from_fn(|_| 0),
            obj_semi_transparent: array::from_fn(|_| false),
            obj_mosaic: array::from_fn(|_| false),
            resolved_colors: array::from_fn(|_| 0),
            resolved_priority: array::from_fn(|_| 0),
            resolved_layers: array::from_fn(|_| Layer::default()),
            second_resolved_colors: array::from_fn(|_| 0),
            second_resolved_priority: array::from_fn(|_| 0),
            second_resolved_layers: array::from_fn(|_| Layer::default()),
        }
    }

    fn clear(&mut self) {
        self.windows.fill(Window::Outside);
        self.obj_colors.fill(0);
        self.obj_priority.fill(u8::MAX);
        self.obj_semi_transparent.fill(false);
        self.obj_mosaic.fill(false);
        self.resolved_colors.fill(0);
        self.resolved_priority.fill(u8::MAX);
        self.resolved_layers.fill(Layer::Backdrop);
        self.second_resolved_colors.fill(0);
        self.second_resolved_priority.fill(u8::MAX);
        self.second_resolved_layers.fill(Layer::Backdrop);

        // Intentionally don't clear bg_colors; will always be completely populated during rendering
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

        if prev_dot < SCREEN_WIDTH && self.state.dot >= SCREEN_WIDTH {
            // HBlank
            if self.state.scanline < SCREEN_HEIGHT {
                self.render_current_line();
                control.trigger_hblank_dma();
                self.state.increment_internal_affine_registers(&self.registers);
            }

            if self.registers.hblank_irq_enabled {
                // HBlank IRQs trigger on every line, even during VBlank
                control.set_interrupt_flag(InterruptType::HBlank);
            }
        }

        let mut tick_effect = PpuTickEffect::None;
        if self.state.dot >= DOTS_PER_LINE {
            self.state.dot -= DOTS_PER_LINE;
            self.state.scanline += 1;

            match self.state.scanline {
                SCREEN_HEIGHT => {
                    if self.registers.vblank_irq_enabled {
                        control.set_interrupt_flag(InterruptType::VBlank);
                    }
                    control.trigger_vblank_dma();

                    self.state.reset_internal_affine_registers(&self.registers);

                    tick_effect = PpuTickEffect::FrameComplete;
                }
                LINES_PER_FRAME => {
                    self.state.scanline = 0;
                }
                _ => {}
            }

            if self.registers.v_counter_irq_enabled
                && self.state.scanline == self.registers.v_counter_target
            {
                control.set_interrupt_flag(InterruptType::VCounterMatch);
            }
        }

        tick_effect
    }

    fn render_current_line(&mut self) {
        if self.registers.forced_blanking {
            self.frame_buffer.0[(self.state.scanline * SCREEN_WIDTH) as usize
                ..((self.state.scanline + 1) * SCREEN_WIDTH) as usize]
                .fill(Color::rgb(255, 255, 255));
            return;
        }

        self.buffers.clear();

        // Render sprites before windows because OBJ window has lower priority than window 0/1
        self.render_sprites_to_buffer();
        self.render_windows_to_buffer();

        self.render_bgs_to_buffer();

        self.merge_layers();
        self.blend_layers();

        for col in 0..SCREEN_WIDTH {
            let color = self.buffers.resolved_colors[col as usize];
            self.frame_buffer.set(self.state.scanline, col, gba_color_to_rgb888(color));
        }
    }

    fn render_windows_to_buffer(&mut self) {
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

    fn render_sprites_to_buffer(&mut self) {
        if !self.registers.obj_enabled {
            return;
        }

        // TODO sprite limits
        let scanline = self.state.scanline;
        for oam_idx in 0..128 {
            let mut oam_entry = OamEntry::parse(&self.oam[4 * oam_idx..4 * oam_idx + 3]);

            if oam_entry.affine_double_size && !oam_entry.affine {
                // Double size flag means "do not display" for non-affine sprites
                // TODO does this also apply to OBJ window sprites?
                continue;
            }

            let (width, height) = oam_entry.shape.width_and_height(oam_entry.size);
            let y = oam_entry.y;

            // TODO correctly handle edge case for 128px tall affine sprites

            let render_height = if oam_entry.affine_double_size { 2 * height } else { height };
            let sprite_bottom = (y + render_height) & 0xFF;
            if (sprite_bottom < y && scanline >= sprite_bottom)
                || (sprite_bottom >= y && !(y..sprite_bottom).contains(&scanline))
            {
                // Sprite is not on this line
                continue;
            }

            if oam_entry.color_depth == ColorDepthBits::Eight {
                oam_entry.palette = 0;
            }

            let line = if oam_entry.mosaic {
                let mosaic_height = self.registers.obj_mosaic_height + 1;
                (self.state.scanline / mosaic_height) * mosaic_height
            } else {
                self.state.scanline
            };

            if oam_entry.affine {
                self.render_affine_sprite(&oam_entry, line, width, height);
            } else {
                self.render_tile_sprite(&oam_entry, line, width, height);
            }
        }

        // Apply horizontal mosaic
        let mosaic_width = self.registers.obj_mosaic_width + 1;
        if mosaic_width != 1 {
            for x in (0..SCREEN_WIDTH).step_by(mosaic_width as usize) {
                let end = cmp::min(mosaic_width, SCREEN_WIDTH - x);
                for i in 1..end {
                    if !self.buffers.obj_mosaic[(x + i) as usize] {
                        continue;
                    }
                    self.buffers.obj_colors[(x + i) as usize] = self.buffers.obj_colors[x as usize];
                    // TODO update priority?
                }
            }
        }
    }

    fn render_tile_sprite(&mut self, oam_entry: &OamEntry, line: u32, width: u32, height: u32) {
        let mut sprite_row = line.wrapping_sub(oam_entry.y) & 0xFF;
        if oam_entry.v_flip {
            sprite_row = height - 1 - sprite_row;
        }

        for dx in 0..width {
            let col = (oam_entry.x + dx) & 0x1FF;
            if !(0..SCREEN_WIDTH).contains(&col) {
                continue;
            }

            if oam_entry.mode != SpriteMode::ObjWindow && self.buffers.obj_colors[col as usize] != 0
            {
                // Already a non-transparent sprite pixel from a sprite with a lower OAM index
                let existing_priority = self.buffers.obj_priority[col as usize];
                self.buffers.obj_priority[col as usize] =
                    cmp::min(existing_priority, oam_entry.priority);
                if oam_entry.priority >= existing_priority {
                    continue;
                }
            }

            let sprite_col = if oam_entry.h_flip { width - 1 - dx } else { dx };
            self.render_sprite_pixel(oam_entry, col, sprite_col, sprite_row, width);
        }
    }

    #[allow(clippy::many_single_char_names)]
    fn render_affine_sprite(&mut self, oam_entry: &OamEntry, line: u32, width: u32, height: u32) {
        let (render_width, render_height) =
            if oam_entry.affine_double_size { (2 * width, 2 * height) } else { (width, height) };

        // Center coordinates are rounded up; e.g. for an 8x16 sprite, the center is at X=4 Y=8
        let cx = (render_width / 2) as i32;
        let cy = (render_height / 2) as i32;

        // Sprite affine parameters are stored in every 4th word in OAM
        // These 4th words cycle between A/B/C/D, so there is a new parameter group every 16 words
        let parameter_group_base_addr = (4 * 4 * oam_entry.affine_parameter_group + 3) as usize;

        // Parameters are 16-bit fixed-point decimal, 1/7/8; extend to signed 32-bit
        let a: i32 = (self.oam[parameter_group_base_addr] as i16).into();
        let b: i32 = (self.oam[parameter_group_base_addr + 4] as i16).into();
        let c: i32 = (self.oam[parameter_group_base_addr + 8] as i16).into();
        let d: i32 = (self.oam[parameter_group_base_addr + 12] as i16).into();

        let sprite_row = (line.wrapping_sub(oam_entry.y) & 0xFF) as i32;

        // Initialize sampling coordinates for X=-1 in the current sprite row
        // Will be at X=0 after the first increment at the start of the loop
        let mut x = a * (-1 - cx) + b * (sprite_row - cy) + (cx << 8);
        let mut y = c * (-1 - cx) + d * (sprite_row - cy) + (cy << 8);
        if oam_entry.affine_double_size {
            x -= ((render_width / 4) << 8) as i32;
            y -= ((render_height / 4) << 8) as i32;
        }

        for dx in 0..render_width {
            x += a;
            y += c;

            let col = (oam_entry.x + dx) & 0x1FF;
            if !(0..SCREEN_WIDTH).contains(&col) {
                continue;
            }

            if oam_entry.mode != SpriteMode::ObjWindow && self.buffers.obj_colors[col as usize] != 0
            {
                // Already a non-transparent sprite pixel from a sprite with a lower OAM index
                let existing_priority = self.buffers.obj_priority[col as usize];
                if oam_entry.priority >= existing_priority {
                    // Existing pixel is same or higher priority; skip this pixel
                    continue;
                }

                // Hardware quirk: an overlapping sprite with higher OAM index and lower priority
                // _always_ updates the priority and mosaic buffers, even if the lower-priority
                // sprite is transparent
                self.buffers.obj_priority[col as usize] = oam_entry.priority;
                self.buffers.obj_mosaic[col as usize] = oam_entry.mosaic;
            }

            // Convert from 1/256 pixel units to pixel units and check bounds
            let sample_x = x >> 8;
            let sample_y = y >> 8;
            if !(0..width as i32).contains(&sample_x) || !(0..height as i32).contains(&sample_y) {
                continue;
            }

            self.render_sprite_pixel(oam_entry, col, sample_x as u32, sample_y as u32, width);
        }
    }

    fn render_sprite_pixel(
        &mut self,
        oam_entry: &OamEntry,
        col: u32,
        sprite_x: u32,
        sprite_y: u32,
        width: u32,
    ) {
        let tile_x_offset = sprite_x / 8;
        let tile_col = sprite_x % 8;
        let tile_y_offset = sprite_y / 8;
        let tile_row = sprite_y % 8;

        let bpp_shift = u32::from(oam_entry.color_depth == ColorDepthBits::Eight);
        let adjusted_tile_number = match self.registers.obj_tile_layout {
            ObjTileLayout::TwoD => {
                // TODO how should out-of-bounds be handled?
                let x = (oam_entry.tile_number + (tile_x_offset << bpp_shift)) & 0x1F;
                let y = (oam_entry.tile_number + (tile_y_offset << 5)) & (0x1F << 5);
                y | x
            }
            ObjTileLayout::OneD => {
                let offset = tile_y_offset * width / 8 + tile_x_offset;
                (oam_entry.tile_number + (offset << bpp_shift)) & 0x3FF
            }
        };

        if self.registers.bg_mode.is_bitmap() && adjusted_tile_number < 512 {
            // Sprite tiles 0-511 do not render in bitmap modes
            return;
        }

        let tile_data_addr = (0x10000 | (32 * adjusted_tile_number))
            + ((tile_row * 8 + tile_col) >> (1 - bpp_shift));
        let color = match oam_entry.color_depth {
            ColorDepthBits::Four => {
                (self.vram[tile_data_addr as usize] >> (4 * (tile_col & 1))) & 0xF
            }
            ColorDepthBits::Eight => self.vram[tile_data_addr as usize],
        };
        if color == 0 {
            // Pixel is transparent
            return;
        }

        match oam_entry.mode {
            SpriteMode::ObjWindow => {
                // OBJ window; don't render the sprite pixel, only mark this position as
                // being inside the OBJ window
                self.buffers.windows[col as usize] = Window::Obj;
            }
            _ => {
                // Actual pixel
                // Sprites can only use colors from the second half of palette RAM (256-511)
                self.buffers.obj_colors[col as usize] =
                    0x100 | (oam_entry.palette << 4) | u16::from(color);
                self.buffers.obj_priority[col as usize] = oam_entry.priority;
                self.buffers.obj_semi_transparent[col as usize] =
                    oam_entry.mode == SpriteMode::SemiTransparent;

                // TODO should the mosaic flag be set for transparent sprite pixels?
                self.buffers.obj_mosaic[col as usize] = oam_entry.mosaic;
            }
        }
    }

    fn render_bgs_to_buffer(&mut self) {
        match self.registers.bg_mode {
            BgMode::Zero => {
                // BG0-3 all in tile map mode
                for bg in 0..4 {
                    self.render_tile_map_bg(bg);
                }
            }
            BgMode::One => {
                // BG0-1 in tile map mode, BG2 in affine mode
                for bg in 0..2 {
                    self.render_tile_map_bg(bg);
                }

                let affine_dimension_tiles = self.registers.bg_control[2].affine_size_tiles();
                self.render_affine_bg(2, move |ppu, x, y| {
                    ppu.sample_tile_map(2, affine_dimension_tiles, x, y)
                });
            }
            BgMode::Two => {
                // BG2-3 in affine mode
                for bg in 2..4 {
                    let affine_dimension_tiles = self.registers.bg_control[bg].affine_size_tiles();
                    self.render_affine_bg(bg, move |ppu, x, y| {
                        ppu.sample_tile_map(bg, affine_dimension_tiles, x, y)
                    });
                }
            }
            BgMode::Three => {
                // BG2 in bitmap mode, 15bpp 240x160
                self.render_affine_bg(2, Self::sample_bitmap_mode_3);
            }
            BgMode::Four => {
                // BG2 in bitmap mode, 8bpp 240x160 with page flipping
                self.render_affine_bg(2, Self::sample_bitmap_mode_4);
            }
            BgMode::Five => {
                // BG2 in bitmap mode, 15bpp 160x128 with page flipping
                self.render_affine_bg(2, Self::sample_bitmap_mode_5);
            }
        }
    }

    fn render_tile_map_bg(&mut self, bg: usize) {
        if !self.registers.bg_enabled[bg] {
            return;
        }

        let bg_control = &self.registers.bg_control[bg];
        let bg_width_pixels = bg_control.screen_size.tile_map_width_pixels();
        let bg_height_pixels = bg_control.screen_size.tile_map_height_pixels();

        let tile_size_bytes = bg_control.color_depth.tile_size_bytes();
        let tile_row_size_bytes = tile_size_bytes / 8;

        let scanline = if bg_control.mosaic {
            let mosaic_height = self.registers.bg_mosaic_height + 1;
            (self.state.scanline / mosaic_height) * mosaic_height
        } else {
            self.state.scanline
        };

        let bg_y = scanline.wrapping_add(self.registers.bg_v_scroll[bg]) & (bg_height_pixels - 1);
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

                let tile_col = (if h_flip { 7 - i } else { i }) as u32;
                let color = match bg_control.color_depth {
                    ColorDepthBits::Four => {
                        let tile_data_addr = tile_data_row_addr + tile_col / 2;
                        let tile_data_byte = self.vram[tile_data_addr as usize];
                        (tile_data_byte >> (4 * (tile_col & 1))) & 0xF
                    }
                    ColorDepthBits::Eight => {
                        let tile_data_addr = tile_data_row_addr + tile_col;
                        if tile_data_addr <= BG_TILE_MAP_MASK {
                            self.vram[tile_data_addr as usize]
                        } else {
                            0
                        }
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

        self.apply_bg_mosaic(bg);
    }

    fn render_affine_bg(&mut self, bg: usize, sample_fn: impl Fn(&Self, i32, i32) -> u16) {
        if !self.registers.bg_enabled[bg] {
            return;
        }

        let [a, _, c, _] = self.registers.bg_affine_parameters[bg - 2];

        // Given the internal affine BG registers X and Y, and affine parameters A and C:
        //   bg_x = X + A * pixel
        //   bg_y = Y + C * pixel
        // Fully expanded formula (though it would be inaccurate to compute this way):
        //   bg_x = X0 + A * pixel + B * line
        //   bg_y = Y0 + C * pixel + D * line
        let mut bg_x = self.state.affine_bg_x[bg - 2] - a;
        let mut bg_y = self.state.affine_bg_y[bg - 2] - c;
        for x in 0..SCREEN_WIDTH {
            bg_x += a;
            bg_y += c;

            // Convert from 1/256 pixel units to pixels
            let bg_x = bg_x >> 8;
            let bg_y = bg_y >> 8;

            let color = sample_fn(self, bg_x, bg_y);
            self.buffers.bg_colors[bg][x as usize] = color;
        }

        self.apply_bg_mosaic(bg);
    }

    fn sample_tile_map(&self, bg: usize, screen_dimension_tiles: i32, x: i32, y: i32) -> u16 {
        let mut tile_map_x = x >> 3;
        let mut tile_map_y = y >> 3;
        if !(0..screen_dimension_tiles).contains(&tile_map_x)
            || !(0..screen_dimension_tiles).contains(&tile_map_y)
        {
            match self.registers.bg_control[bg].affine_overflow {
                AffineOverflowBehavior::Transparent => {
                    return 0;
                }
                AffineOverflowBehavior::Wrap => {
                    tile_map_x &= screen_dimension_tiles - 1;
                    tile_map_y &= screen_dimension_tiles - 1;
                }
            }
        }

        // Tile map X/Y are now guaranteed to be within the tile map
        let tile_map_addr = self.registers.bg_control[bg].tile_map_base_addr
            + (tile_map_y * screen_dimension_tiles + tile_map_x) as u32;
        if tile_map_addr > 0xFFFF {
            // TODO what should happen when tile map address goes out of bounds?
            return 0;
        }

        let tile_number: u32 = self.vram[tile_map_addr as usize].into();

        let tile_row = (y & 7) as u32;
        let tile_col = (x & 7) as u32;
        let tile_data_addr = self.registers.bg_control[bg].tile_data_base_addr
            + tile_number * ColorDepthBits::Eight.tile_size_bytes()
            + 8 * tile_row
            + tile_col;
        if tile_data_addr > 0xFFFF {
            // TODO what should happen when tile data address goes out of bounds?
            return 0;
        }

        self.vram[tile_data_addr as usize].into()
    }

    fn sample_bitmap_mode_3(&self, x: i32, y: i32) -> u16 {
        if !(0..SCREEN_WIDTH as i32).contains(&x) || !(0..SCREEN_HEIGHT as i32).contains(&y) {
            return 0;
        }

        let x = x as u32;
        let y = y as u32;
        let vram_addr = (2 * (y * SCREEN_WIDTH + x)) as usize;
        u16::from_le_bytes(self.vram[vram_addr..vram_addr + 2].try_into().unwrap())
    }

    fn sample_bitmap_mode_4(&self, x: i32, y: i32) -> u16 {
        if !(0..SCREEN_WIDTH as i32).contains(&x) || !(0..SCREEN_HEIGHT as i32).contains(&y) {
            return 0;
        }

        let x = x as u32;
        let y = y as u32;
        let base_vram_addr = self.registers.page_flipped_bitmap_address();
        let vram_addr = base_vram_addr + y * SCREEN_WIDTH + x;
        self.palette_ram[self.vram[vram_addr as usize] as usize]
    }

    fn sample_bitmap_mode_5(&self, x: i32, y: i32) -> u16 {
        if !(0..MODE_5_BITMAP_WIDTH as i32).contains(&x)
            || !(0..MODE_5_BITMAP_HEIGHT as i32).contains(&y)
        {
            return 0;
        }

        let x = x as u32;
        let y = y as u32;
        let base_vram_addr = self.registers.page_flipped_bitmap_address();
        let vram_addr = (base_vram_addr + 2 * (y * MODE_5_BITMAP_WIDTH + x)) as usize;
        u16::from_le_bytes(self.vram[vram_addr..vram_addr + 2].try_into().unwrap())
    }

    fn apply_bg_mosaic(&mut self, bg: usize) {
        if !self.registers.bg_control[bg].mosaic {
            return;
        }

        let bg_buffer = &mut self.buffers.bg_colors[bg];
        let mosaic_width = self.registers.bg_mosaic_width + 1;
        for x in (0..SCREEN_WIDTH).step_by(mosaic_width as usize) {
            let end = cmp::min(mosaic_width, SCREEN_WIDTH - x);
            for i in 1..end {
                bg_buffer[(x + i) as usize] = bg_buffer[x as usize];
            }
        }
    }

    fn merge_layers(&mut self) {
        let is_bitmap = self.registers.bg_mode.is_bitmap();

        // Process BGs in reverse order because BG0 is highest priority in ties
        for bg in (0..4).rev() {
            if !self.registers.bg_enabled[bg] || !self.registers.bg_mode.bg_enabled(bg) {
                continue;
            }

            let window_bg_enabled = Window::bg_enabled_array(bg, &self.registers);

            let priority = self.registers.bg_control[bg].priority;
            let layer = [Layer::Bg0, Layer::Bg1, Layer::Bg2, Layer::Bg3][bg];

            for col in 0..SCREEN_WIDTH as usize {
                let color = self.buffers.bg_colors[bg][col];
                if color == 0 && !is_bitmap {
                    continue;
                }

                let window = self.buffers.windows[col];
                if !window_bg_enabled[window as usize] {
                    continue;
                }

                self.buffers.push_pixel(col, color, priority, layer);
            }
        }

        if self.registers.obj_enabled {
            let window_obj_enabled = Window::obj_enabled_array(&self.registers);

            for col in 0..SCREEN_WIDTH as usize {
                let obj_priority = self.buffers.obj_priority[col];
                let color = self.buffers.obj_colors[col];
                if color == 0 {
                    continue;
                }

                let window = self.buffers.windows[col];
                if !window_obj_enabled[window as usize] {
                    continue;
                }

                let layer = if self.buffers.obj_semi_transparent[col] {
                    Layer::ObjSemiTransparent
                } else {
                    Layer::Obj
                };
                self.buffers.push_pixel(col, color, obj_priority, layer);
            }
        }

        for col in 0..SCREEN_WIDTH as usize {
            if !(is_bitmap && self.buffers.resolved_layers[col] == Layer::Bg2) {
                let color = self.buffers.resolved_colors[col];
                self.buffers.resolved_colors[col] = self.palette_ram[color as usize];
            }

            if !(is_bitmap && self.buffers.second_resolved_layers[col] == Layer::Bg2) {
                let color = self.buffers.second_resolved_colors[col];
                self.buffers.second_resolved_colors[col] = self.palette_ram[color as usize];
            }
        }
    }

    fn blend_layers(&mut self) {
        let window_blend_enabled = Window::blend_enabled_array(&self.registers);

        for col in 0..SCREEN_WIDTH as usize {
            let window = self.buffers.windows[col];
            if !window_blend_enabled[window as usize] {
                continue;
            }

            let top_layer = self.buffers.resolved_layers[col];
            let second_layer = self.buffers.second_resolved_layers[col];

            if top_layer == Layer::ObjSemiTransparent
                && second_layer.second_target_enabled(&self.registers)
            {
                // Semi-transparent sprites always have 1st target enabled, and they force alpha
                // blending if the 2nd target is valid
                self.buffers.resolved_colors[col] = alpha_blend(
                    self.buffers.resolved_colors[col],
                    self.registers.alpha_1st,
                    self.buffers.second_resolved_colors[col],
                    self.registers.alpha_2nd,
                );
                continue;
            }

            match self.registers.blend_mode {
                // Alpha blending runs if 1st and 2nd target are both valid
                BlendMode::AlphaBlending
                    if top_layer.first_target_enabled(&self.registers)
                        && second_layer.second_target_enabled(&self.registers) =>
                {
                    self.buffers.resolved_colors[col] = alpha_blend(
                        self.buffers.resolved_colors[col],
                        self.registers.alpha_1st,
                        self.buffers.second_resolved_colors[col],
                        self.registers.alpha_2nd,
                    );
                }
                // Brightness increase/decrease run if 1st target is valid
                BlendMode::IncreaseBrightness
                    if top_layer.first_target_enabled(&self.registers) =>
                {
                    self.buffers.resolved_colors[col] = increase_brightness(
                        self.buffers.resolved_colors[col],
                        self.registers.brightness,
                    );
                }
                BlendMode::DecreaseBrightness
                    if top_layer.first_target_enabled(&self.registers) =>
                {
                    self.buffers.resolved_colors[col] = decrease_brightness(
                        self.buffers.resolved_colors[col],
                        self.registers.brightness,
                    );
                }
                _ => {
                    // Do not blend
                }
            }
        }
    }

    pub fn read_vram_byte(&self, address: u32) -> u8 {
        self.vram[vram_address(address)]
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

    pub fn read_oam_halfword(&self, address: u32) -> u16 {
        let oam_addr = ((address as usize) & (OAM_LEN - 1)) >> 1;
        self.oam[oam_addr]
    }

    pub fn read_oam_word(&self, address: u32) -> u32 {
        let oam_addr = ((address as usize) & (OAM_LEN - 1) & !3) >> 1;
        let low: u32 = self.oam[oam_addr].into();
        let high: u32 = self.oam[oam_addr + 1].into();
        low | (high << 16)
    }

    pub fn write_oam_halfword(&mut self, address: u32, value: u16) {
        let oam_addr = ((address as usize) & (OAM_LEN - 1)) >> 1;
        self.oam[oam_addr] = value;
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
            0x50 => self.registers.read_bldcnt(),
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
            0x08..=0x0F => {
                let bg = (address >> 1) & 3;
                self.registers.write_bgcnt(bg as usize, value);
            }
            0x10 | 0x14 | 0x18 | 0x1C => {
                let bg = (address >> 2) & 3;
                self.registers.write_bghofs(bg as usize, value);
            }
            0x12 | 0x16 | 0x1A | 0x1E => {
                let bg = (address >> 2) & 3;
                self.registers.write_bgvofs(bg as usize, value);
            }
            0x20..=0x27 | 0x30..=0x37 => {
                let bg = (address >> 4) & 3;
                let parameter = (address >> 1) & 3;
                self.registers.write_bg_affine_parameter(bg as usize, parameter as usize, value);
            }
            0x28 | 0x38 => {
                let bg = (address >> 4) & 3;
                self.registers.write_bgx_l(bg as usize, value);
                self.state.handle_bgx_update(bg as usize, &self.registers);
            }
            0x2A | 0x3A => {
                let bg = (address >> 4) & 3;
                self.registers.write_bgx_h(bg as usize, value);
                self.state.handle_bgx_update(bg as usize, &self.registers);
            }
            0x2C | 0x3C => {
                let bg = (address >> 4) & 3;
                self.registers.write_bgy_l(bg as usize, value);
                self.state.handle_bgy_update(bg as usize, &self.registers);
            }
            0x2E | 0x3E => {
                let bg = (address >> 4) & 3;
                self.registers.write_bgy_h(bg as usize, value);
                self.state.handle_bgy_update(bg as usize, &self.registers);
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
            0x4C => self.registers.write_mosaic(value),
            0x50 => self.registers.write_bldcnt(value),
            0x52 => self.registers.write_bldalpha(value),
            0x54 => self.registers.write_bldy(value),
            _ => log::error!("PPU I/O register write {address:08X} {value:04X}"),
        }
    }

    // $04000004: DISPSTAT (Display status)
    fn read_dispstat(&self) -> u16 {
        let vblank =
            self.state.scanline >= SCREEN_HEIGHT && self.state.scanline != LINES_PER_FRAME - 1;
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
