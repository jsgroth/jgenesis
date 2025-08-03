//! GBA PPU (picture processing unit)

mod colors;
mod registers;

use crate::api::GbaEmulatorConfig;
use crate::dma::DmaState;
use crate::interrupts::{InterruptRegisters, InterruptType};
use crate::ppu::registers::{
    AffineOverflowBehavior, BgMode, BitsPerPixel, BlendMode, ObjVramMapDimensions, Registers,
    Window, WindowEnabled,
};
use bincode::{Decode, Encode};
use gba_config::GbaColorCorrection;
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::{GetBit, U16Ext};
use std::ops::Range;
use std::{array, cmp, iter};

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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct BgAffineLatch {
    x: [i32; 2],
    y: [i32; 2],
}

impl BgAffineLatch {
    // Called once per frame during VBlank
    fn latch_reference_points(&mut self, registers: &Registers) {
        self.x = registers.bg_affine_parameters.map(|params| params.reference_x);
        self.y = registers.bg_affine_parameters.map(|params| params.reference_y);
    }

    // Called once per line during active display
    fn increment_reference_latches(&mut self, registers: &Registers) {
        for (i, (x, y)) in iter::zip(&mut self.x, &mut self.y).enumerate() {
            *x += registers.bg_affine_parameters[i].b;
            *y += registers.bg_affine_parameters[i].d;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u32,
    dot: u32,
    frame_complete: bool,
    bg_affine_latch: BgAffineLatch,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            dot: 0,
            frame_complete: false,
            bg_affine_latch: BgAffineLatch::default(),
        }
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

    fn new_opaque_rgb(r: u16, g: u16, b: u16) -> Self {
        Self(0x8000 | r | (g << 5) | (b << 10))
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

    fn is_1st_target_enabled(self, registers: &Registers) -> bool {
        match self {
            Self::Bg0 => registers.bg_blend_1st_target[0],
            Self::Bg1 => registers.bg_blend_1st_target[1],
            Self::Bg2 => registers.bg_blend_1st_target[2],
            Self::Bg3 => registers.bg_blend_1st_target[3],
            Self::Obj => registers.obj_blend_1st_target,
            Self::Backdrop => registers.backdrop_blend_1st_target,
            Self::None => false,
        }
    }

    fn is_2nd_target_enabled(self, registers: &Registers) -> bool {
        match self {
            Self::Bg0 => registers.bg_blend_2nd_target[0],
            Self::Bg1 => registers.bg_blend_2nd_target[1],
            Self::Bg2 => registers.bg_blend_2nd_target[2],
            Self::Bg3 => registers.bg_blend_2nd_target[3],
            Self::Obj => registers.obj_blend_2nd_target,
            Self::Backdrop => registers.backdrop_blend_2nd_target,
            Self::None => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteMode {
    Normal,
    SemiTransparent,
    ObjWindow,
    Invalid,
}

impl SpriteMode {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Normal,
            1 => Self::SemiTransparent,
            2 => Self::ObjWindow,
            3 => Self::Invalid,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteSize {
    Zero,
    One,
    Two,
    Three,
}

impl SpriteSize {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteShape {
    Square,
    HorizontalRect,
    VerticalRect,
    Invalid,
}

impl SpriteShape {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Square,
            1 => Self::HorizontalRect,
            2 => Self::VerticalRect,
            3 => Self::Invalid,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn size_pixels(self, size: SpriteSize) -> (u32, u32) {
        use SpriteShape::{HorizontalRect, Invalid, Square, VerticalRect};
        use SpriteSize::{One, Three, Two, Zero};

        match (self, size) {
            (Square, Zero) => (8, 8),
            (Square, One) => (16, 16),
            (Square, Two) => (32, 32),
            (Square, Three) => (64, 64),
            (HorizontalRect, Zero) => (16, 8),
            (HorizontalRect, One) => (32, 8),
            (HorizontalRect, Two) => (32, 16),
            (HorizontalRect, Three) => (64, 32),
            (VerticalRect, Zero) => (8, 16),
            (VerticalRect, One) => (8, 32),
            (VerticalRect, Two) => (16, 32),
            (VerticalRect, Three) => (32, 64),
            (Invalid, _) => {
                // TODO ???
                (8, 8)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct OamEntry {
    x: u32,
    y: u32,
    tile_number: u32,
    affine: bool,
    affine_double_size: bool,
    affine_parameter_group: u16,
    disabled: bool,
    mode: SpriteMode,
    mosaic: bool,
    bpp: BitsPerPixel,
    shape: SpriteShape,
    size: SpriteSize,
    h_flip: bool,
    v_flip: bool,
    priority: u8,
    palette: u16,
}

impl OamEntry {
    fn parse(attributes: [u16; 3]) -> Self {
        let affine = attributes[0].bit(8);

        Self {
            x: (attributes[1] & 0x1FF).into(),
            y: (attributes[0] & 0xFF).into(),
            tile_number: (attributes[2] & 0x3FF).into(),
            affine,
            affine_double_size: affine && attributes[0].bit(9),
            affine_parameter_group: (attributes[1] >> 9) & 0x1F,
            disabled: !affine && attributes[0].bit(9),
            mode: SpriteMode::from_bits(attributes[0] >> 10),
            mosaic: attributes[0].bit(12),
            bpp: BitsPerPixel::from_bit(attributes[0].bit(13)),
            shape: SpriteShape::from_bits(attributes[0] >> 14),
            size: SpriteSize::from_bits(attributes[1] >> 14),
            h_flip: !affine && attributes[1].bit(12),
            v_flip: !affine && attributes[1].bit(13),
            priority: ((attributes[2] >> 10) & 3) as u8,
            palette: attributes[2] >> 12,
        }
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
    buffers: Box<Buffers>,
    cycles: u64,
    color_correction: GbaColorCorrection,
}

impl Ppu {
    pub fn new(config: GbaEmulatorConfig) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: BoxedByteArray::new(),
            palette_ram: BoxedWordArray::new(),
            oam: BoxedWordArray::new(),
            registers: Registers::new(),
            state: State::new(),
            buffers: Box::new(Buffers::new()),
            cycles: 0,
            color_correction: config.color_correction,
        }
    }

    pub fn reload_config(&mut self, config: GbaEmulatorConfig) {
        self.color_correction = config.color_correction;
    }

    pub fn step_to(
        &mut self,
        cycles: u64,
        interrupts: &mut InterruptRegisters,
        dma: &mut DmaState,
    ) {
        let elapsed_cycles = cycles - self.cycles;
        self.cycles = cycles;

        self.tick(elapsed_cycles as u32, interrupts, dma);
    }

    fn tick(
        &mut self,
        mut elapsed_cycles: u32,
        interrupts: &mut InterruptRegisters,
        dma: &mut DmaState,
    ) {
        fn render_line(ppu: &mut Ppu, _: &mut InterruptRegisters, _: &mut DmaState) {
            ppu.render_current_line();
            ppu.render_next_sprite_line();
        }

        fn hblank_start(ppu: &mut Ppu, interrupts: &mut InterruptRegisters, dma: &mut DmaState) {
            if ppu.registers.hblank_irq_enabled {
                interrupts.set_flag(InterruptType::HBlank);
            }

            if ppu.state.scanline < SCREEN_HEIGHT {
                ppu.state.bg_affine_latch.increment_reference_latches(&ppu.registers);

                dma.notify_hblank_start();
            }
        }

        fn end_of_line(ppu: &mut Ppu, interrupts: &mut InterruptRegisters, dma: &mut DmaState) {
            ppu.state.dot = 0;

            ppu.state.scanline += 1;
            if ppu.state.scanline == LINES_PER_FRAME {
                ppu.state.scanline = 0;
            } else if ppu.state.scanline == SCREEN_HEIGHT {
                if ppu.registers.vblank_irq_enabled {
                    interrupts.set_flag(InterruptType::VBlank);
                }

                dma.notify_vblank_start();

                ppu.state.bg_affine_latch.latch_reference_points(&ppu.registers);

                ppu.state.frame_complete = true;
            }

            if ppu.registers.v_counter_irq_enabled
                && (ppu.state.scanline as u8) == ppu.registers.v_counter_match
            {
                interrupts.set_flag(InterruptType::VCounter);
            }
        }

        type EventFn = fn(&mut Ppu, &mut InterruptRegisters, &mut DmaState);

        // Arbitrary dot around the middle of the line
        const RENDER_DOT: u32 = 526;

        const LINE_EVENTS: &[(u32, EventFn)] = &[
            (RENDER_DOT, render_line),
            (HBLANK_START_DOT, hblank_start),
            (DOTS_PER_LINE, end_of_line),
        ];

        while elapsed_cycles != 0 {
            let mut event_idx = 0;
            while self.state.dot >= LINE_EVENTS[event_idx].0 {
                event_idx += 1;
            }

            let change = cmp::min(elapsed_cycles, LINE_EVENTS[event_idx].0 - self.state.dot);
            self.state.dot += change;
            elapsed_cycles -= change;

            if self.state.dot == LINE_EVENTS[event_idx].0 {
                (LINE_EVENTS[event_idx].1)(self, interrupts, dma);
            }
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

    #[allow(clippy::match_same_arms)]
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
                self.render_affine_bg(2, self.affine_sample_tile_map(2));
            }
            BgMode::Two => {
                // BG2-3 in affine mode
                for bg in 2..4 {
                    self.render_affine_bg(bg, self.affine_sample_tile_map(bg));
                }
            }
            BgMode::Three => {
                // Bitmap mode: 240x160, 15bpp, single frame buffer
                self.render_affine_bg(2, Self::affine_sample_mode_3);
            }
            BgMode::Four => {
                // Bitmap mode: 240x160, 8bpp, two frame buffers
                self.render_affine_bg(2, self.affine_sample_mode_4());
            }
            BgMode::Five => {
                // Bitmap mode: 160x128, 15bpp, two frame buffers
                self.render_affine_bg(2, self.affine_sample_mode_5());
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
            let palette = match bg_control.bpp {
                BitsPerPixel::Four => tile_map_entry >> 12,
                BitsPerPixel::Eight => 0,
            };

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
                    let tile_row_addr = tile_base_addr + tile_row * 8;

                    for pixel_idx in 0..8 {
                        let pixel = pixel_idx as i32 + base_pixel;
                        if !(0..SCREEN_WIDTH as i32).contains(&pixel) {
                            continue;
                        }
                        let pixel = pixel as usize;

                        let tile_col = if h_flip { 7 - pixel_idx } else { pixel_idx };
                        let tile_addr = tile_row_addr + tile_col;

                        let color_id = if tile_addr <= 0xFFFF {
                            self.vram[tile_addr as usize]
                        } else {
                            // TODO should read VRAM open bus?
                            0
                        };

                        if color_id == 0 {
                            // Transparent pixel
                            continue;
                        }

                        let color = self.palette_ram[color_id as usize];
                        self.buffers.bg_pixels[bg][pixel] = Pixel::new_opaque(color);
                    }
                }
            }
        }
    }

    fn render_affine_bg(&mut self, bg: usize, sample_fn: impl Fn(&Self, i32, i32) -> Pixel) {
        assert!(bg == 2 || bg == 3);

        self.buffers.bg_pixels[bg].fill(Pixel::TRANSPARENT);

        if !self.registers.bg_enabled[bg] {
            return;
        }

        // TODO mosaic

        let bg_control = &self.registers.bg_control[bg];

        let dimension_tiles = bg_control.size.affine_dimension_tiles();
        let dimension_pixels = (8 * dimension_tiles) as i32;

        let dx = self.registers.bg_affine_parameters[bg - 2].a;
        let dy = self.registers.bg_affine_parameters[bg - 2].c;

        let mut x = self.state.bg_affine_latch.x[bg - 2];
        let mut y = self.state.bg_affine_latch.y[bg - 2];

        for pixel in 0..SCREEN_WIDTH {
            // Affine coordinates are in 1/256 pixel units - convert to pixel
            let x_pixel = x >> 8;
            let y_pixel = y >> 8;

            self.buffers.bg_pixels[bg][pixel as usize] = sample_fn(self, x_pixel, y_pixel);

            x += dx;
            y += dy;
        }
    }

    fn affine_sample_tile_map(&self, bg: usize) -> impl Fn(&Self, i32, i32) -> Pixel + 'static {
        let bg_control = &self.registers.bg_control[bg];

        let dimension_tiles = bg_control.size.affine_dimension_tiles();
        let dimension_pixels = (8 * dimension_tiles) as i32;

        let base_tile_map_addr = bg_control.tile_map_addr;
        let base_tile_data_addr = bg_control.tile_data_addr;
        let affine_overflow = bg_control.affine_overflow;

        move |ppu, mut x, mut y| {
            if !(0..dimension_pixels).contains(&x) || !(0..dimension_pixels).contains(&y) {
                match affine_overflow {
                    AffineOverflowBehavior::Transparent => return Pixel::TRANSPARENT,
                    AffineOverflowBehavior::Wrap => {
                        x &= dimension_pixels - 1;
                        y &= dimension_pixels - 1;
                    }
                }
            }

            let x = x as u32;
            let y = y as u32;

            let tile_map_row = y / 8;
            let tile_row = y % 8;

            let tile_map_col = x / 8;
            let tile_col = x % 8;

            let tile_map_addr = base_tile_map_addr + tile_map_row * dimension_tiles + tile_map_col;
            let tile_number = if tile_map_addr <= 0xFFFF {
                ppu.vram[tile_map_addr as usize]
            } else {
                // TODO should be VRAM open bus?
                0
            };
            let tile_number: u32 = tile_number.into();

            // Affine tiles are always 8bpp
            let tile_base_addr = base_tile_data_addr + 64 * tile_number;

            // Tile data address will never exceed $FFFF because tile numbers are 8-bit and tile
            // data base address is in 16KB steps
            assert!(tile_base_addr <= 0x10000 - 64);

            let tile_row_addr = tile_base_addr + 8 * tile_row;
            let tile_addr = tile_row_addr + tile_col;
            let color_id = ppu.vram[tile_addr as usize];

            if color_id == 0 {
                return Pixel::TRANSPARENT;
            }

            let color = ppu.palette_ram[color_id as usize];
            Pixel::new_opaque(color)
        }
    }

    fn affine_sample_mode_3(&self, x: i32, y: i32) -> Pixel {
        if !(0..SCREEN_WIDTH as i32).contains(&x) || !(0..SCREEN_HEIGHT as i32).contains(&y) {
            return Pixel::TRANSPARENT;
        }

        let x = x as u32;
        let y = y as u32;

        let pixel_addr = (2 * (y * SCREEN_WIDTH + x)) as usize;
        let color = u16::from_le_bytes([self.vram[pixel_addr], self.vram[pixel_addr + 1]]);
        Pixel::new_opaque(color)
    }

    fn affine_sample_mode_4(&self) -> impl Fn(&Self, i32, i32) -> Pixel + 'static {
        let fb_addr = self.registers.bitmap_frame_buffer.vram_address();

        move |ppu, x, y| {
            if !(0..SCREEN_WIDTH as i32).contains(&x) || !(0..SCREEN_HEIGHT as i32).contains(&y) {
                return Pixel::TRANSPARENT;
            }

            let x = x as u32;
            let y = y as u32;

            let pixel_addr = (fb_addr + y * SCREEN_WIDTH + x) as usize;
            let color_id = ppu.vram[pixel_addr];

            if color_id == 0 {
                return Pixel::TRANSPARENT;
            }

            let color = ppu.palette_ram[color_id as usize];
            Pixel::new_opaque(color)
        }
    }

    fn affine_sample_mode_5(&self) -> impl Fn(&Self, i32, i32) -> Pixel + 'static {
        const MODE_5_WIDTH: u32 = 160;
        const MODE_5_HEIGHT: u32 = 128;

        let fb_addr = self.registers.bitmap_frame_buffer.vram_address();

        move |ppu, x, y| {
            if !(0..MODE_5_WIDTH as i32).contains(&x) || !(0..MODE_5_HEIGHT as i32).contains(&y) {
                return Pixel::TRANSPARENT;
            }

            let x = x as u32;
            let y = y as u32;

            let pixel_addr = (fb_addr + 2 * (y * MODE_5_WIDTH + x)) as usize;
            let color = u16::from_le_bytes([ppu.vram[pixel_addr], ppu.vram[pixel_addr + 1]]);
            Pixel::new_opaque(color)
        }
    }

    fn merge_layers(&mut self) {
        #[derive(Debug, Clone, Copy)]
        struct MergePixel {
            color: Pixel,
            layer: Layer,
            priority: u8,
        }

        let scanline = self.state.scanline;

        let backdrop_color = Pixel::new_transparent(self.palette_ram[0]);

        // Alpha blending coefficients
        let eva: u16 = cmp::min(16, self.registers.blend_alpha_a).into();
        let evb: u16 = cmp::min(16, self.registers.blend_alpha_b).into();

        // Brightness increase/decrease coefficient
        let evy: u16 = cmp::min(16, self.registers.blend_brightness).into();

        let bg_enabled: [bool; 4] = array::from_fn(|bg| {
            self.registers.bg_enabled[bg] && self.registers.bg_mode.bg_active_in_mode(bg)
        });

        let any_window_enabled = self.registers.window_enabled[0]
            || self.registers.window_enabled[1]
            || self.registers.obj_window_enabled;

        let window_x = self.registers.window_x_ranges();
        let window_y = self.registers.window_y_ranges();

        let window_y_active =
            [0, 1].map(|i| self.registers.window_enabled[i] && window_y[i].contains(&scanline));

        for pixel in 0..SCREEN_WIDTH {
            let window_active = [0, 1].map(|i| window_y_active[i] && window_x[i].contains(&pixel));

            let window_layers_enabled = if any_window_enabled {
                // TODO OBJ window
                let window = if window_active[0] {
                    Window::Inside0
                } else if window_active[1] {
                    Window::Inside1
                } else {
                    Window::Outside
                };
                self.registers.window_layers_enabled(window)
            } else {
                WindowEnabled::ALL
            };

            let mut first_pixel =
                MergePixel { color: backdrop_color, layer: Layer::Backdrop, priority: u8::MAX };

            let mut second_pixel =
                MergePixel { color: Pixel::TRANSPARENT, layer: Layer::None, priority: u8::MAX };

            let mut check_pixel = |color: Pixel, layer: Layer, priority: u8| {
                if color.transparent() {
                    return;
                }

                if first_pixel.color.transparent() || priority < first_pixel.priority {
                    second_pixel = first_pixel;
                    first_pixel = MergePixel { color, layer, priority };
                    return;
                }

                if second_pixel.color.transparent() || priority < second_pixel.priority {
                    second_pixel = MergePixel { color, layer, priority };
                }
            };

            if self.registers.obj_enabled && window_layers_enabled.obj {
                check_pixel(
                    self.buffers.obj_pixels[pixel as usize],
                    Layer::Obj,
                    self.buffers.obj_priority[pixel as usize],
                );
            }

            for (bg, enabled) in bg_enabled.into_iter().enumerate() {
                if !enabled || !window_layers_enabled.bg[bg] {
                    continue;
                }

                check_pixel(
                    self.buffers.bg_pixels[bg][pixel as usize],
                    Layer::BG[bg],
                    self.registers.bg_control[bg].priority,
                );
            }

            let mut blend_color = first_pixel.color;

            // Semi-transparent OBJs are always 1st target enabled and force blend mode to alpha blending
            let is_semi_transparent_obj = first_pixel.layer == Layer::Obj
                && self.buffers.obj_semi_transparent[pixel as usize];

            if window_layers_enabled.blend
                && (first_pixel.layer.is_1st_target_enabled(&self.registers)
                    || is_semi_transparent_obj)
            {
                let blend_mode = if is_semi_transparent_obj {
                    BlendMode::AlphaBlending
                } else {
                    self.registers.blend_mode
                };

                match blend_mode {
                    BlendMode::AlphaBlending => {
                        if second_pixel.layer.is_2nd_target_enabled(&self.registers) {
                            blend_color =
                                alpha_blend(first_pixel.color, second_pixel.color, eva, evb);
                        }
                    }
                    BlendMode::BrightnessIncrease => {
                        blend_color = adjust_brightness::<true>(first_pixel.color, evy);
                    }
                    BlendMode::BrightnessDecrease => {
                        blend_color = adjust_brightness::<false>(first_pixel.color, evy);
                    }
                    BlendMode::None => {}
                }
            }

            self.frame_buffer.set(
                self.state.scanline,
                pixel,
                gba_color_to_rgb8(blend_color, self.color_correction),
            );
        }
    }

    fn render_next_sprite_line(&mut self) {
        if self.registers.forced_blanking
            || (self.state.scanline >= SCREEN_HEIGHT && self.state.scanline != LINES_PER_FRAME - 1)
        {
            return;
        }

        // TODO mosaic
        // TODO affine
        // TODO OBJ window

        let is_bitmap_mode =
            matches!(self.registers.bg_mode, BgMode::Three | BgMode::Four | BgMode::Five);

        self.buffers.obj_pixels.fill(Pixel::TRANSPARENT);
        self.buffers.obj_priority.fill(u8::MAX);
        self.buffers.obj_semi_transparent.fill(false);

        let target_line =
            if self.state.scanline == LINES_PER_FRAME - 1 { 0 } else { self.state.scanline + 1 };

        // One memory access every 2 cycles
        // When OAM is free during HBlank, sprite rendering runs from dots 40 to 1006 (HBlank start)
        let mut memory_accesses = if self.registers.oam_free_during_hblank {
            (HBLANK_START_DOT - 40) / 2
        } else {
            DOTS_PER_LINE / 2
        };

        'outer: for oam_idx in 0..128 {
            let oam_addr = 4 * oam_idx;
            let oam_attributes =
                [self.oam[oam_addr], self.oam[oam_addr + 1], self.oam[oam_addr + 2]];

            // 32-bit read of first two attribute words
            memory_accesses -= 1;
            if memory_accesses == 0 {
                break 'outer;
            }

            let oam_entry = OamEntry::parse(oam_attributes);

            if oam_entry.disabled {
                continue;
            }

            let (sprite_width, sprite_height) = oam_entry.shape.size_pixels(oam_entry.size);
            let sprite_y = target_line.wrapping_sub(oam_entry.y) & 0xFF;
            if sprite_y >= sprite_height {
                // Sprite does not overlap this scanline
                continue;
            }

            let sprite_width_tiles = sprite_width / 8;

            // 16-bit read of third attribute word
            memory_accesses -= 1;
            if memory_accesses == 0 {
                break 'outer;
            }

            let sprite_row = if oam_entry.v_flip { sprite_height - 1 - sprite_y } else { sprite_y };
            let sprite_tile_row = sprite_row / 8;
            let row_in_tile = sprite_row % 8;

            let map_step = match oam_entry.bpp {
                BitsPerPixel::Four => 1,
                BitsPerPixel::Eight => 2,
            };

            let map_row_width = match self.registers.obj_vram_map_dimensions {
                ObjVramMapDimensions::Two => 32,
                ObjVramMapDimensions::One => map_step * sprite_width_tiles,
            };

            let palette = match oam_entry.bpp {
                BitsPerPixel::Four => oam_entry.palette,
                BitsPerPixel::Eight => 0,
            };

            for sprite_x in 0..sprite_width {
                // One VRAM read for every 2 pixels
                memory_accesses -= (sprite_x & 1) ^ 1;
                if memory_accesses == 0 {
                    break 'outer;
                }

                let pixel = (oam_entry.x + sprite_x) & 0x1FF;
                if !(0..SCREEN_WIDTH).contains(&pixel) {
                    continue;
                }

                let sprite_col =
                    if oam_entry.h_flip { sprite_width - 1 - sprite_x } else { sprite_x };
                let sprite_tile_col = sprite_col / 8;
                let col_in_tile = sprite_col % 8;

                // TODO how should out-of-bounds tile numbers behave?
                let tile_number = (oam_entry.tile_number
                    + sprite_tile_row * map_row_width
                    + sprite_tile_col * map_step)
                    & 0x3FF;

                if is_bitmap_mode && tile_number < 512 {
                    // Sprite tile numbers 0-511 are not usable in bitmap modes; tiles are fully transparent
                    continue;
                }

                let tile_base_addr = 0x10000 | (tile_number * 32);
                let color_id = match oam_entry.bpp {
                    BitsPerPixel::Four => {
                        let tile_addr = tile_base_addr + 4 * row_in_tile + (col_in_tile >> 1);
                        let tile_byte = self.vram[tile_addr as usize];
                        (tile_byte >> (4 * (col_in_tile & 1))) & 0xF
                    }
                    BitsPerPixel::Eight => {
                        let tile_addr = tile_base_addr + 8 * row_in_tile + col_in_tile;
                        if tile_addr <= 0x17FFF {
                            self.vram[tile_addr as usize]
                        } else {
                            // TODO what should this do? can happen when using an odd tile number
                            0
                        }
                    }
                };

                let existing_opaque = !self.buffers.obj_pixels[pixel as usize].transparent();
                let existing_priority = self.buffers.obj_priority[pixel as usize];

                if existing_opaque && oam_entry.priority >= existing_priority {
                    // Existing opaque pixel with the same or lower priority
                    continue;
                }

                if color_id == 0 && !existing_opaque {
                    // Both new pixel and existing pixel are transparent
                    continue;
                }

                // Hardware bug: A transparent pixel that overlaps with an opaque pixel from a sprite
                // with lower OAM index and higher priority will overwrite the priority and semi-transparency
                // flags
                self.buffers.obj_priority[pixel as usize] = oam_entry.priority;
                self.buffers.obj_semi_transparent[pixel as usize] =
                    oam_entry.mode == SpriteMode::SemiTransparent;

                if color_id == 0 {
                    // Transparent pixel
                    continue;
                }

                let palette_ram_addr = 0x100 | (16 * palette + u16::from(color_id));
                let color = self.palette_ram[palette_ram_addr as usize];
                self.buffers.obj_pixels[pixel as usize] = Pixel::new_opaque(color);
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
        log::debug!(
            "PPU register write {address:08X} {value:04X} (line {} dot {})",
            self.state.scanline,
            self.state.dot
        );

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
            0x4000020..=0x400003E => self.registers.write_bg_affine_register(
                address,
                value,
                &mut self.state.bg_affine_latch,
            ),
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

    pub fn write_register_byte(&mut self, address: u32, value: u8) {
        // TODO BGxHOFS, BGxVOFS, MOSAIC, blend registers
        match address {
            0x4000000 | 0x4000004 | 0x4000008 | 0x400000A | 0x400000C | 0x400000E | 0x4000048
            | 0x400004A | 0x4000050 | 0x4000052 => {
                let mut halfword = self.read_register(address);
                halfword.set_lsb(value);
                self.write_register(address, halfword);
            }
            0x4000001 | 0x4000005 | 0x4000009 | 0x400000B | 0x400000D | 0x400000F | 0x4000049
            | 0x400004B | 0x4000051 | 0x4000053 => {
                let mut halfword = self.read_register(address & !1);
                halfword.set_msb(value);
                self.write_register(address & !1, halfword);
            }
            0x4000040 => self.registers.write_winh_low(0, value),
            0x4000041 => self.registers.write_winh_high(0, value),
            0x4000042 => self.registers.write_winh_low(1, value),
            0x4000043 => self.registers.write_winh_high(1, value),
            0x4000044 => self.registers.write_winv_low(0, value),
            0x4000045 => self.registers.write_winv_high(0, value),
            0x4000046 => self.registers.write_winv_low(1, value),
            0x4000047 => self.registers.write_winv_high(1, value),
            _ => {
                log::warn!("Unhandled PPU byte register write {address:08X} {value:02X}");
            }
        }
    }
}

fn alpha_blend(first: Pixel, second: Pixel, eva: u16, evb: u16) -> Pixel {
    let alpha_blend_component =
        |first: u16, second: u16| cmp::min(31, (eva * first + evb * second) >> 4);

    let r = alpha_blend_component(first.red(), second.red());
    let g = alpha_blend_component(first.green(), second.green());
    let b = alpha_blend_component(first.blue(), second.blue());

    Pixel::new_opaque_rgb(r, g, b)
}

fn adjust_brightness<const INCREASE: bool>(color: Pixel, evy: u16) -> Pixel {
    let adjust_component = |component: u16| {
        if INCREASE {
            component + ((evy * (31 - component)) >> 4)
        } else {
            component - ((evy * component) >> 4)
        }
    };

    let r = adjust_component(color.red());
    let g = adjust_component(color.green());
    let b = adjust_component(color.blue());

    Pixel::new_opaque_rgb(r, g, b)
}

fn gba_color_to_rgb8(gba_color: Pixel, color_correction: GbaColorCorrection) -> Color {
    colors::table(color_correction)[(gba_color.0 & 0x7FFF) as usize]
}
