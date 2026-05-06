//! HuC6270 VDC (video display controller)

mod debug;
mod registers;

use crate::video::MCLK_CYCLES_PER_SCANLINE;
use crate::video::vce::{DotClockDivider, Vce};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;
use registers::VdcRegisters;
use std::ops::Range;
use std::{array, cmp};

pub const VRAM_LEN_WORDS: usize = 64 * 1024 / 2;
pub const SPRITE_TABLE_LEN: usize = 64;

pub const DOTS_PER_LINE_DIV_4: u64 = MCLK_CYCLES_PER_SCANLINE / 4;
pub const DOTS_PER_LINE_DIV_3: u64 = MCLK_CYCLES_PER_SCANLINE / 3;
pub const DOTS_PER_LINE_DIV_2: u64 = MCLK_CYCLES_PER_SCANLINE / 2;

// Guesses, probably not accurate
pub const OVERSCAN_DOTS_DIV_4: u16 = 13;
pub const OVERSCAN_DOTS_DIV_3: u16 = 13 * 4 / 3; // ~17
pub const OVERSCAN_DOTS_DIV_2: u16 = 13 * 4 / 2; // 26

pub const LEFT_BORDER_DIV_4: u16 = 11;
pub const LEFT_BORDER_DIV_3: u16 = 11 * 4 / 3; // ~14
pub const LEFT_BORDER_DIV_2: u16 = 11 * 4 / 2; // 22

pub const STANDARD_WIDTH_DIV_4: u16 = 256;
pub const STANDARD_WIDTH_DIV_3: u16 = 256 * 4 / 3; // ~341
pub const STANDARD_WIDTH_DIV_2: u16 = 256 * 4 / 2; // 512

pub const MAX_WIDTH_DIV_4: u16 = STANDARD_WIDTH_DIV_4 + 2 * OVERSCAN_DOTS_DIV_4;
pub const MAX_WIDTH_DIV_3: u16 = STANDARD_WIDTH_DIV_3 + 2 * OVERSCAN_DOTS_DIV_3;
pub const MAX_WIDTH_DIV_2: u16 = STANDARD_WIDTH_DIV_2 + 2 * OVERSCAN_DOTS_DIV_2;

pub const LINE_BUFFER_LEN: usize = MAX_WIDTH_DIV_2 as usize;

pub const ACTIVE_DISPLAY_DOTS_DIV_4: Range<u16> =
    LEFT_BORDER_DIV_4..LEFT_BORDER_DIV_4 + STANDARD_WIDTH_DIV_4 + 2 * OVERSCAN_DOTS_DIV_4;
pub const ACTIVE_DISPLAY_DOTS_DIV_3: Range<u16> =
    LEFT_BORDER_DIV_3..LEFT_BORDER_DIV_3 + STANDARD_WIDTH_DIV_3 + 2 * OVERSCAN_DOTS_DIV_3;
pub const ACTIVE_DISPLAY_DOTS_DIV_2: Range<u16> =
    LEFT_BORDER_DIV_2..LEFT_BORDER_DIV_2 + STANDARD_WIDTH_DIV_2 + 2 * OVERSCAN_DOTS_DIV_2;

// Raster compare counter always resets to 64 (0x40) at the beginning of HBlank before the first line of active display
pub const RASTER_COMPARE_DISPLAY_START: u16 = 64;

// 14 lines of top blanking before active display, 4 lines of bottom blanking + 3 lines of VSYNC after
pub const ACTIVE_DISPLAY_LINES: Range<u16> = 14..256;

// Large enough to fit video output at H1365px, after removing overscan
pub const FRAME_BUFFER_WIDTH: usize = (2 * MAX_WIDTH_DIV_2) as usize;
// There are always 242 lines of active display, regardless of vertical display settings
// Some of these lines are usually overscan, where the VDC constantly outputs sprite color 0
pub const FRAME_BUFFER_HEIGHT: usize = 242;

impl DotClockDivider {
    pub fn dots_per_line(self) -> u64 {
        match self {
            Self::Four => DOTS_PER_LINE_DIV_4,
            Self::Three => DOTS_PER_LINE_DIV_3,
            Self::Two => DOTS_PER_LINE_DIV_2,
        }
    }

    pub fn overscan_dots(self) -> u16 {
        match self {
            Self::Four => OVERSCAN_DOTS_DIV_4,
            Self::Three => OVERSCAN_DOTS_DIV_3,
            Self::Two => OVERSCAN_DOTS_DIV_2,
        }
    }

    pub fn standard_width_dots(self) -> u16 {
        match self {
            Self::Four => STANDARD_WIDTH_DIV_4,
            Self::Three => STANDARD_WIDTH_DIV_3,
            Self::Two => STANDARD_WIDTH_DIV_2,
        }
    }

    pub fn max_width_dots(self) -> u16 {
        match self {
            Self::Four => MAX_WIDTH_DIV_4,
            Self::Three => MAX_WIDTH_DIV_3,
            Self::Two => MAX_WIDTH_DIV_2,
        }
    }

    pub fn active_display_dots(self) -> Range<u16> {
        match self {
            Self::Four => ACTIVE_DISPLAY_DOTS_DIV_4,
            Self::Three => ACTIVE_DISPLAY_DOTS_DIV_3,
            Self::Two => ACTIVE_DISPLAY_DOTS_DIV_2,
        }
    }
}

type VdcColorBuffer = [[u16; FRAME_BUFFER_WIDTH]; FRAME_BUFFER_HEIGHT];

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdcFrameBuffer {
    // Contains GRB333 VCE colors
    pub colors: Box<VdcColorBuffer>,
    pub line_dividers: Box<[DotClockDivider; FRAME_BUFFER_HEIGHT]>,
}

impl VdcFrameBuffer {
    fn new() -> Self {
        Self {
            colors: Box::new(array::from_fn(|_| array::from_fn(|_| 0))),
            line_dividers: Box::new(array::from_fn(|_| DotClockDivider::default())),
        }
    }
}

define_bit_enum!(CgMode, [ZeroOne, TwoThree]);

// Single = 16px, Double = 32px
define_bit_enum!(SpriteWidth, [Single, Double]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum SpriteHeight {
    #[default]
    Single, // 16px
    Double, // 32px
    Quad,   // 64px
}

impl SpriteHeight {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Single,
            1 => Self::Double,
            2 | 3 => Self::Quad,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct SpriteTableEntry {
    pub x: u16,
    pub y: u16,
    pub tile_number: u16,
    pub h_flip: bool,
    pub v_flip: bool,
    pub width: SpriteWidth,
    pub height: SpriteHeight,
    pub palette: u16,
    pub priority: bool,
    pub cg_mode: CgMode,
}

impl SpriteTableEntry {
    pub fn write_first_word(&mut self, word: u16) {
        self.y = word & 0x3FF;
    }

    pub fn write_second_word(&mut self, word: u16) {
        self.x = word & 0x3FF;
    }

    pub fn write_third_word(&mut self, word: u16) {
        self.cg_mode = CgMode::from_bit(word.bit(0));
        self.tile_number = (word >> 1) & 0x3FF;
    }

    pub fn write_fourth_word(&mut self, word: u16) {
        self.palette = word & 0xF;
        self.priority = word.bit(7);
        self.width = SpriteWidth::from_bit(word.bit(8));
        self.h_flip = word.bit(11);
        self.height = SpriteHeight::from_bits(word >> 12);
        self.v_flip = word.bit(15);
    }
}

define_bit_enum!(DmaStep, [Increment, Decrement]);

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct LatchedVerticalState {
    pub v_sync_width: u16,
    pub v_display_start: u16,
    pub v_display_width: u16,
    pub v_display_end: u16,
    // The VDC enters "burst mode" when both BG and sprites are disabled at start of frame
    // Burst mode enables DMA and unlimited VRAM access throughout the entire frame, not only during VBlank
    pub burst_mode: bool,
}

impl LatchedVerticalState {
    fn latch(registers: &VdcRegisters) -> Self {
        Self {
            v_sync_width: registers.v_sync_width,
            v_display_start: registers.v_display_start,
            v_display_width: registers.v_display_width,
            v_display_end: registers.v_display_end,
            burst_mode: !registers.bg_enabled && !registers.sprites_enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct LatchedHorizontalState {
    pub h_sync_width: u16,
    pub h_display_start: u16,
    pub h_display_width: u16,
    pub h_display_end: u16,
    pub bg_x_scroll: u16,
    pub bg_enabled: bool,
    pub sprites_enabled: bool,
}

impl LatchedHorizontalState {
    fn latch(registers: &VdcRegisters) -> Self {
        Self {
            h_sync_width: registers.h_sync_width,
            h_display_start: registers.h_display_start,
            h_display_width: registers.h_display_width,
            h_display_end: registers.h_display_end,
            bg_x_scroll: registers.bg_x_scroll,
            bg_enabled: registers.bg_enabled,
            sprites_enabled: registers.sprites_enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum VerticalMode {
    TopBorder,
    ActiveDisplay,
    BottomBorder,
    VSync,
}

impl VerticalMode {
    fn length(self, latch: LatchedVerticalState) -> u16 {
        match self {
            Self::TopBorder => latch.v_display_start,
            Self::ActiveDisplay => latch.v_display_width,
            Self::BottomBorder => latch.v_display_end,
            Self::VSync => latch.v_sync_width,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::TopBorder => Self::ActiveDisplay,
            Self::ActiveDisplay => Self::BottomBorder,
            Self::BottomBorder => Self::VSync,
            Self::VSync => Self::TopBorder,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum HorizontalMode {
    LeftBorder,
    ActiveDisplay,
    RightBorder,
    HSync,
}

impl HorizontalMode {
    fn length(self, latch: LatchedHorizontalState) -> u16 {
        match self {
            Self::LeftBorder => latch.h_display_start,
            Self::ActiveDisplay => latch.h_display_width,
            Self::RightBorder => latch.h_display_end,
            Self::HSync => latch.h_sync_width,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::LeftBorder => Self::ActiveDisplay,
            Self::ActiveDisplay => Self::RightBorder,
            Self::RightBorder => Self::HSync,
            Self::HSync => Self::LeftBorder,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdcIrq {
    VBlank,
    RasterCompare,
    SpriteOverflow,
    SpriteCollision,
    VramDma,
    SatDma,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdcState {
    pub scanline: u16,
    pub scanline_dot: u16,
    pub h_latch: LatchedHorizontalState,
    pub v_latch: LatchedVerticalState,
    pub h_mode: HorizontalMode,
    pub v_mode: VerticalMode,
    pub h_counter: u16,
    pub h_mode_start_dot: u16,
    pub v_counter: u16,
    pub v_mode_start_line: u16,
    pub bg_y_counter: u16,
    pub bg_y_scroll_written: bool,
    // Dot clock divider is not _really_ latched per line, but pretending that it is
    // simplifies a lot of things
    pub line_divider: DotClockDivider,
    pub vblank_irq_pending: bool,
    pub raster_compare_irq_pending: bool,
    pub sprite_overflow_irq_pending: bool,
    pub sprite_collision_irq_pending: bool,
    pub vram_dma_irq_pending: bool,
    pub sat_dma_irq_pending: bool,
    pub any_irq_pending: bool,
    pub vblank_irq_this_frame: bool,
    pub raster_compare_counter: u16,
    pub frame_complete: bool,
}

impl VdcState {
    fn new(registers: &VdcRegisters) -> Self {
        Self {
            scanline: 0,
            scanline_dot: 0,
            h_latch: LatchedHorizontalState::latch(registers),
            v_latch: LatchedVerticalState::latch(registers),
            h_mode: HorizontalMode::LeftBorder,
            v_mode: VerticalMode::TopBorder,
            h_counter: 0,
            h_mode_start_dot: 0,
            v_counter: 0,
            v_mode_start_line: 0,
            bg_y_counter: 0,
            bg_y_scroll_written: false,
            line_divider: DotClockDivider::default(),
            vblank_irq_pending: false,
            raster_compare_irq_pending: false,
            sprite_overflow_irq_pending: false,
            sprite_collision_irq_pending: false,
            vram_dma_irq_pending: false,
            sat_dma_irq_pending: false,
            any_irq_pending: false,
            vblank_irq_this_frame: false,
            raster_compare_counter: RASTER_COMPARE_DISPLAY_START,
            frame_complete: false,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdc {
    vram: BoxedWordArray<VRAM_LEN_WORDS>,
    sprite_table: Box<[SpriteTableEntry; SPRITE_TABLE_LEN]>,
    registers: VdcRegisters,
    state: VdcState,
    frame_buffer: VdcFrameBuffer,
    // Contains 9-bit color indices (0-255 for BG colors, 256-511 for sprite colors)
    line_buffer: Box<[u16; LINE_BUFFER_LEN]>,
    selected_register: u8,
}

impl Vdc {
    pub fn new() -> Self {
        let registers = VdcRegisters::new();
        let state = VdcState::new(&registers);

        Self {
            vram: BoxedWordArray::new(),
            sprite_table: vec![SpriteTableEntry::default(); SPRITE_TABLE_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            registers,
            state,
            frame_buffer: VdcFrameBuffer::new(),
            line_buffer: Box::new(array::from_fn(|_| 0)),
            selected_register: 0x1F,
        }
    }

    pub fn tick_dots(&mut self, dots: u64, vce: &Vce) {
        let active_display_dots = self.state.line_divider.active_display_dots();

        let line_divider = self.state.line_divider as u16;

        let active_line = ACTIVE_DISPLAY_LINES.contains(&self.state.scanline);
        let frame_buffer_row = self.state.scanline.wrapping_sub(ACTIVE_DISPLAY_LINES.start);
        debug_assert!(!active_line || (frame_buffer_row as usize) < self.frame_buffer.colors.len());

        let mut h_mode_length = self.state.h_mode.length(self.state.h_latch);

        let burst_mode = self.state.v_latch.burst_mode;

        // TODO this is very inefficient
        for _ in 0..dots {
            // Render color to frame buffer if inside VCE active display
            if active_line && active_display_dots.contains(&self.state.scanline_dot) {
                let color = if !burst_mode
                    && self.state.v_mode == VerticalMode::ActiveDisplay
                    && self.state.h_mode == HorizontalMode::ActiveDisplay
                {
                    let line_buffer_idx = self.state.scanline_dot - self.state.h_mode_start_dot;
                    vce.read_color(self.line_buffer[line_buffer_idx as usize])
                } else {
                    // Always render overscan color in burst mode and outside of VDC active display
                    vce.overscan_color()
                };

                let frame_buffer_col =
                    line_divider * (self.state.scanline_dot - active_display_dots.start);
                for i in 0..line_divider {
                    self.frame_buffer.colors[frame_buffer_row as usize]
                        [(frame_buffer_col + i) as usize] = color;
                }
            }

            // Increment raster compare counter shortly before the end of horizontal display
            // (Timing is probably not accurate)
            if self.state.h_mode == HorizontalMode::ActiveDisplay
                && self.state.h_counter == self.state.h_latch.h_display_width.wrapping_sub(8)
            {
                if self.state.v_mode == VerticalMode::TopBorder
                    && self.state.v_counter == self.state.v_latch.v_display_start - 1
                {
                    self.state.raster_compare_counter = RASTER_COMPARE_DISPLAY_START;
                } else {
                    self.state.raster_compare_counter += 1;
                }

                if self.state.raster_compare_counter == self.registers.raster_compare {
                    self.set_irq(VdcIrq::RasterCompare);
                }

                // TODO sprite overflow IRQ should trigger here
            }

            self.state.scanline_dot += 1;

            self.state.h_counter += 1;
            if self.state.h_counter >= h_mode_length {
                self.state.h_counter = 0;
                self.state.h_mode = self.state.h_mode.next();
                self.state.h_mode_start_dot = self.state.scanline_dot;

                h_mode_length = self.state.h_mode.length(self.state.h_latch);

                if !burst_mode {
                    match self.state.h_mode {
                        HorizontalMode::ActiveDisplay => {
                            if self.state.v_mode == VerticalMode::ActiveDisplay {
                                self.render_line();
                            }

                            if self.state.v_mode == VerticalMode::ActiveDisplay
                                || (self.state.v_mode == VerticalMode::TopBorder
                                    && self.state.v_counter
                                        == self.state.v_latch.v_display_start - 1)
                            {
                                // TODO sprite evaluation
                            }
                        }
                        HorizontalMode::RightBorder
                            if self.state.v_mode == VerticalMode::ActiveDisplay
                                || (self.state.v_mode == VerticalMode::TopBorder
                                    && self.state.v_counter
                                        == self.state.v_latch.v_display_start - 1) =>
                        {
                            // TODO sprite tile fetching
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn start_new_line(&mut self, scanline: u16, dot_clock_divider: DotClockDivider) {
        self.state.h_latch = LatchedHorizontalState::latch(&self.registers);
        self.state.h_mode = HorizontalMode::LeftBorder;
        self.state.h_counter = 0;
        self.state.h_mode_start_dot = 0;

        self.state.line_divider = dot_clock_divider;

        self.state.scanline = scanline;
        self.state.scanline_dot = 0;

        if self.state.bg_y_scroll_written {
            self.state.bg_y_counter = self.registers.bg_y_scroll;
            self.state.bg_y_scroll_written = false;
        }
        self.state.bg_y_counter = self.state.bg_y_counter.wrapping_add(1);

        if ACTIVE_DISPLAY_LINES.contains(&scanline) {
            let frame_buffer_row = scanline - ACTIVE_DISPLAY_LINES.start;
            self.frame_buffer.line_dividers[frame_buffer_row as usize] = dot_clock_divider;
        }

        self.state.frame_complete |= scanline == ACTIVE_DISPLAY_LINES.end;

        if scanline != 0 {
            self.state.v_counter += 1;
            if self.state.v_counter >= self.state.v_mode.length(self.state.v_latch) {
                self.state.v_counter = 0;
                self.state.v_mode = self.state.v_mode.next();
                self.state.v_mode_start_line = self.state.scanline;

                match self.state.v_mode {
                    VerticalMode::ActiveDisplay => {
                        // TODO end any in-progress DMAs if not in burst mode

                        self.state.bg_y_counter = self.registers.bg_y_scroll;
                    }
                    VerticalMode::BottomBorder => {
                        // TODO start any active DMAs

                        self.set_irq(VdcIrq::VBlank);

                        self.state.vblank_irq_this_frame = true;
                    }
                    _ => {}
                }
            }
        }

        // TODO VBlank IRQ 2 lines before end of frame if display is too large
    }

    pub fn start_new_frame(&mut self) {
        self.state.v_latch = LatchedVerticalState::latch(&self.registers);
        self.state.v_mode = VerticalMode::TopBorder;
        self.state.v_counter = 0;
        self.state.v_mode_start_line = 0;
        self.state.vblank_irq_this_frame = false;
    }

    pub fn frame_complete(&self) -> bool {
        self.state.frame_complete
    }

    pub fn clear_frame_complete(&mut self) {
        self.state.frame_complete = false;
    }

    pub fn frame_buffer(&self) -> &VdcFrameBuffer {
        &self.frame_buffer
    }

    pub fn irq(&self) -> bool {
        self.state.any_irq_pending
    }

    fn render_line(&mut self) {
        const BACKDROP_COLOR: u16 = 0x000;

        // TODO sprites

        self.line_buffer.fill(BACKDROP_COLOR);

        if !self.state.h_latch.bg_enabled {
            return;
        }

        let line_width_dots =
            cmp::min(self.state.line_divider.max_width_dots(), self.state.h_latch.h_display_width);

        let screen_width_tiles = self.registers.virtual_screen_width.to_tiles();
        let screen_height_tiles = self.registers.virtual_screen_height.to_tiles();

        let bg_x_scroll = self.state.h_latch.bg_x_scroll;
        let bg_y_counter = self.state.bg_y_counter;

        let mut bg_tile_x = (bg_x_scroll / 8) & (screen_width_tiles - 1);
        let bg_tile_y = (bg_y_counter / 8) & (screen_height_tiles - 1);

        let tile_row = (bg_y_counter & 7) as usize;

        let start_x = -i32::from(bg_x_scroll & 7);
        let end_x = i32::from(line_width_dots);
        for x in (start_x..end_x).step_by(8) {
            // BAT (BG attribute table) always starts at $0000 in VRAM and has 1 word per tile
            let bat_addr = bg_tile_y * screen_width_tiles + bg_tile_x;
            let bg_attributes = self.vram[bat_addr as usize];
            let tile_number = bg_attributes & 0xFFF;
            let palette = bg_attributes >> 12;

            let tile_addr = (16 * tile_number) as usize;

            // TODO CG mode bit if VRAM access width is 4

            let (cg0, cg1) = if tile_addr < VRAM_LEN_WORDS {
                (self.vram[tile_addr + tile_row], self.vram[tile_addr + tile_row + 8])
            } else {
                // Tiles 2048-4095 are supposedly filled with "garbage"
                (0xFFFF, 0xFFFF)
            };

            for tile_col in 0..8 {
                let pixel = x + tile_col;
                if !(0..end_x).contains(&pixel) {
                    continue;
                }

                let bitplane_shift = 7 - tile_col;
                let color_idx = ((cg0 >> bitplane_shift) & 1)
                    | (((cg0 >> (8 + bitplane_shift)) & 1) << 1)
                    | (((cg1 >> bitplane_shift) & 1) << 2)
                    | (((cg1 >> (8 + bitplane_shift)) & 1) << 3);

                if color_idx != 0 {
                    self.line_buffer[pixel as usize] = (palette << 4) | color_idx;
                }
            }

            bg_tile_x = (bg_tile_x + 1) & (screen_width_tiles - 1);
        }
    }

    fn read_vram(&self, address: u16) -> u16 {
        // Actual hardware usually returns "corrupted" data for out-of-bounds VRAM addresses
        self.vram.get(address as usize).copied().unwrap_or(0xFFFF)
    }

    fn write_vram(&mut self, address: u16, value: u16) {
        let address = address as usize;
        if address < VRAM_LEN_WORDS {
            self.vram[address] = value;
        }
    }

    fn increment_vram_read_address(&mut self) {
        self.registers.vram_read_address =
            self.registers.vram_read_address.wrapping_add(self.registers.vram_address_increment);
    }

    fn increment_vram_write_address(&mut self) {
        self.registers.vram_write_address =
            self.registers.vram_write_address.wrapping_add(self.registers.vram_address_increment);
    }

    fn set_irq(&mut self, irq: VdcIrq) {
        match irq {
            VdcIrq::VBlank => {
                self.state.vblank_irq_pending |= self.registers.vblank_irq_enabled;
            }
            VdcIrq::RasterCompare => {
                self.state.raster_compare_irq_pending |= self.registers.raster_compare_irq_enabled;
            }
            VdcIrq::SpriteOverflow => {
                self.state.sprite_overflow_irq_pending |=
                    self.registers.sprite_overflow_irq_enabled;
            }
            VdcIrq::SpriteCollision => {
                self.state.sprite_collision_irq_pending |=
                    self.registers.sprite_collision_irq_enabled;
            }
            VdcIrq::VramDma => {
                self.state.vram_dma_irq_pending |= self.registers.vram_dma_irq_enabled;
            }
            VdcIrq::SatDma => {
                self.state.sat_dma_irq_pending |= self.registers.sat_dma_irq_enabled;
            }
        }

        self.state.any_irq_pending = self.state.vblank_irq_pending
            || self.state.raster_compare_irq_pending
            || self.state.sprite_collision_irq_pending
            || self.state.sprite_overflow_irq_pending
            || self.state.vram_dma_irq_pending
            || self.state.sat_dma_irq_pending;
    }
}
