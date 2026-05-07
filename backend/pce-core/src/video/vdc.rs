//! HuC6270 VDC (video display controller)

mod debug;
mod registers;

use crate::video::MCLK_CYCLES_PER_SCANLINE;
use crate::video::vce::{DotClockDivider, Vce};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{Boxed2DWordArray, BoxedWordArray};
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

pub const DMA_DOTS_PER_WORD: u8 = 4;

pub const MAX_SPRITES_PER_LINE: usize = 16;

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

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdcFrameBuffer {
    // Contains GRB333 VCE colors
    pub colors: Boxed2DWordArray<FRAME_BUFFER_HEIGHT, FRAME_BUFFER_WIDTH>,
    pub line_dividers: Box<[DotClockDivider; FRAME_BUFFER_HEIGHT]>,
}

impl VdcFrameBuffer {
    fn new() -> Self {
        Self {
            colors: Boxed2DWordArray::new(),
            line_dividers: Box::new(array::from_fn(|_| DotClockDivider::default())),
        }
    }
}

define_bit_enum!(CgMode, [ZeroOne, TwoThree]);

// Single = 16px, Double = 32px
define_bit_enum!(SpriteWidth, [Single, Double]);

impl SpriteWidth {
    pub fn to_pixels(self) -> u16 {
        match self {
            Self::Single => 16,
            Self::Double => 32,
        }
    }

    // 16px tiles
    pub fn to_sprite_tiles(self) -> u16 {
        match self {
            Self::Single => 1,
            Self::Double => 2,
        }
    }

    pub fn tile_number_mask(self) -> u16 {
        match self {
            Self::Single => !0,
            Self::Double => !1,
        }
    }
}

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

    pub fn to_pixels(self) -> u16 {
        match self {
            Self::Single => 16,
            Self::Double => 32,
            Self::Quad => 64,
        }
    }

    pub fn tile_number_mask(self) -> u16 {
        match self {
            Self::Single => !0,
            Self::Double => !0b010,
            Self::Quad => !0b110,
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
    pub fn write_word(&mut self, i: u16, word: u16) {
        match i & 3 {
            0 => self.write_first_word(word),
            1 => self.write_second_word(word),
            2 => self.write_third_word(word),
            3 => self.write_fourth_word(word),
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

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

#[derive(Debug, Clone, Encode, Decode)]
pub struct EvaluatedSpriteEntry {
    pub sprite_idx: u8,
    pub x: u16,
    pub tile_number: u16,
    pub tile_row: u16,
    pub h_flip: bool,
    pub priority: bool,
    pub palette: u16,
    pub cg_mode: CgMode,
}

#[derive(Debug, Clone, Copy)]
pub struct BgTileRow {
    pub cg0: u16,
    pub cg1: u16,
    pub palette: u16,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SpritePixel {
    pub sprite_idx: u8,
    pub priority: bool,
    pub palette: u16,
    pub color_idx: u16,
}

impl SpritePixel {
    pub const TRANSPARENT: Self = Self { sprite_idx: 0, priority: false, palette: 0, color_idx: 0 };

    pub fn transparent(self) -> bool {
        self.color_idx == 0
    }
}

define_bit_enum!(DmaStep, [Increment, Decrement]);

impl DmaStep {
    fn apply(self, address: &mut u16) {
        match self {
            Self::Increment => {
                *address = address.wrapping_add(1);
            }
            Self::Decrement => {
                *address = address.wrapping_sub(1);
            }
        }
    }
}

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

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaState {
    pub vram_triggered: bool,
    pub vram_active: bool,
    pub sat_triggered: bool,
    pub sat_active: bool,
    pub sat_address: u16,
    pub dots_till_next_word: u8,
}

impl DmaState {
    fn new() -> Self {
        Self {
            vram_triggered: false,
            vram_active: false,
            sat_triggered: false,
            sat_active: false,
            sat_address: 0,
            dots_till_next_word: DMA_DOTS_PER_WORD,
        }
    }

    fn start_vram(&mut self) {
        self.vram_active = true;

        // Don't interrupt an in-progress VRAM-to-SAT DMA read
        if !self.sat_active {
            self.dots_till_next_word = DMA_DOTS_PER_WORD;
        }
    }

    fn start_sat(&mut self) {
        self.sat_active = true;
        self.sat_address = 0;
        self.dots_till_next_word = DMA_DOTS_PER_WORD;
    }

    fn halt(&mut self) {
        self.vram_active = false;
        self.sat_active = false;
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
    pub dma: DmaState,
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
            dma: DmaState::new(),
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

    fn can_start_vram_dma(&self) -> bool {
        if self.v_latch.burst_mode {
            // Can always run during burst mode
            return true;
        }

        if self.v_mode == VerticalMode::ActiveDisplay {
            // Can never run during active display
            return false;
        }

        if self.v_mode == VerticalMode::TopBorder
            && self.v_counter == self.v_latch.v_display_start - 1
            && matches!(self.h_mode, HorizontalMode::RightBorder | HorizontalMode::HSync)
        {
            // Can't run during HBlank on the last line before active display
            return false;
        }

        true
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdc {
    vram: BoxedWordArray<VRAM_LEN_WORDS>,
    sprite_table: Box<[SpriteTableEntry; SPRITE_TABLE_LEN]>,
    registers: VdcRegisters,
    state: VdcState,
    sprite_evaluation_buffer: Vec<EvaluatedSpriteEntry>,
    sprite_line_buffer: Box<[SpritePixel; LINE_BUFFER_LEN]>,
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
            vram: BoxedWordArray::new_random(),
            sprite_table: vec![SpriteTableEntry::default(); SPRITE_TABLE_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            registers,
            state,
            sprite_evaluation_buffer: Vec::with_capacity(MAX_SPRITES_PER_LINE),
            sprite_line_buffer: vec![SpritePixel::TRANSPARENT; LINE_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
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
            if self.state.dma.sat_active {
                self.progress_sat_dma();
            } else if self.state.dma.vram_active {
                self.progress_vram_dma();
            }

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

                let is_sprite_line = self.state.v_mode == VerticalMode::ActiveDisplay
                    || (self.state.v_mode == VerticalMode::TopBorder
                        && self.state.v_counter == self.state.v_latch.v_display_start - 1);

                match self.state.h_mode {
                    HorizontalMode::ActiveDisplay => {
                        if self.state.v_mode == VerticalMode::ActiveDisplay {
                            self.render_line();
                        }

                        if is_sprite_line && !burst_mode {
                            self.run_sprite_evaluation();
                        }
                    }
                    HorizontalMode::RightBorder => {
                        if self.state.v_mode == VerticalMode::TopBorder
                            && self.state.v_counter == self.state.v_latch.v_display_start - 1
                        {
                            // DMAs cannot run once sprite tile fetching for the first line of active display begins
                            self.state.dma.halt();
                        }

                        if is_sprite_line && !burst_mode {
                            self.fetch_sprite_tiles();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn start_new_line(
        &mut self,
        scanline: u16,
        dot_clock_divider: DotClockDivider,
        lines_per_frame: u16,
    ) {
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
                        self.state.bg_y_counter = self.registers.bg_y_scroll;

                        if !self.state.v_latch.burst_mode {
                            // DMAs cannot run during active display when not in burst mode
                            self.state.dma.halt();
                        }
                    }
                    VerticalMode::BottomBorder => {
                        if self.state.dma.sat_triggered || self.registers.sat_dma_repeat {
                            self.state.dma.start_sat();
                            self.state.dma.sat_triggered = false;

                            log::trace!("Starting VRAM-to-SAT DMA on line {scanline}");
                        }

                        if self.state.dma.vram_triggered {
                            self.state.dma.start_vram();

                            log::trace!("Starting VRAM-to-VRAM DMA on line {scanline}");
                        }

                        self.set_irq(VdcIrq::VBlank);

                        self.state.vblank_irq_this_frame = true;
                    }
                    _ => {}
                }
            }
        }

        if !self.state.vblank_irq_this_frame && scanline == lines_per_frame - 2 {
            // VDC supposedly always generates a VBlank IRQ when the VCE asserts VSYNC if it didn't
            // already generate one earlier in the frame
            self.set_irq(VdcIrq::VBlank);
        }
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

        self.line_buffer.fill(BACKDROP_COLOR);

        if self.state.v_latch.burst_mode
            || (!self.state.h_latch.bg_enabled && !self.state.h_latch.sprites_enabled)
        {
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

            let BgTileRow { cg0, cg1, palette: bg_palette } = if self.state.h_latch.bg_enabled {
                self.read_bg_tile_row(bat_addr, tile_row)
            } else {
                BgTileRow { cg0: 0, cg1: 0, palette: 0 }
            };

            for tile_col in 0..8 {
                let pixel = x + tile_col;
                if !(0..end_x).contains(&pixel) {
                    continue;
                }

                let bitplane_shift = 7 - tile_col;
                let bg_color_idx = ((cg0 >> bitplane_shift) & 1)
                    | (((cg0 >> (8 + bitplane_shift)) & 1) << 1)
                    | (((cg1 >> bitplane_shift) & 1) << 2)
                    | (((cg1 >> (8 + bitplane_shift)) & 1) << 3);

                let sprite_pixel = if self.state.h_latch.sprites_enabled {
                    self.sprite_line_buffer[pixel as usize]
                } else {
                    SpritePixel::TRANSPARENT
                };

                let rendered_color = if !sprite_pixel.transparent()
                    && (sprite_pixel.priority || bg_color_idx == 0)
                {
                    0x100 | (sprite_pixel.palette << 4) | sprite_pixel.color_idx
                } else if bg_color_idx != 0 {
                    (bg_palette << 4) | bg_color_idx
                } else {
                    BACKDROP_COLOR
                };

                self.line_buffer[pixel as usize] = rendered_color;
            }

            bg_tile_x = (bg_tile_x + 1) & (screen_width_tiles - 1);
        }
    }

    fn read_bg_tile_row(&self, bat_addr: u16, tile_row: usize) -> BgTileRow {
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

        BgTileRow { cg0, cg1, palette }
    }

    fn progress_sat_dma(&mut self) {
        self.state.dma.dots_till_next_word -= 1;
        if self.state.dma.dots_till_next_word != 0 {
            return;
        }

        self.state.dma.dots_till_next_word = DMA_DOTS_PER_WORD;

        let sat_address = self.state.dma.sat_address;
        let word = self.read_vram(self.registers.sat_dma_source_address.wrapping_add(sat_address));

        let sprite_idx = sat_address / 4;
        self.sprite_table[sprite_idx as usize].write_word(sat_address % 4, word);

        self.state.dma.sat_address = sat_address.wrapping_add(1);
        if self.state.dma.sat_address == (4 * SPRITE_TABLE_LEN) as u16 {
            self.state.dma.sat_active = false;
            self.set_irq(VdcIrq::SatDma);

            log::trace!("Finished SAT DMA on line {}", self.state.scanline);
        }
    }

    fn progress_vram_dma(&mut self) {
        self.state.dma.dots_till_next_word -= 1;
        if self.state.dma.dots_till_next_word != 0 {
            return;
        }

        self.state.dma.dots_till_next_word = DMA_DOTS_PER_WORD;

        let word = self.read_vram(self.registers.vram_dma_source_address);
        self.registers.vram_dma_source_step.apply(&mut self.registers.vram_dma_source_address);

        self.write_vram(self.registers.vram_dma_destination_address, word);
        self.registers
            .vram_dma_destination_step
            .apply(&mut self.registers.vram_dma_destination_address);

        let overflowed;
        (self.registers.vram_dma_length, overflowed) =
            self.registers.vram_dma_length.overflowing_sub(1);

        if overflowed {
            self.state.dma.vram_triggered = false;
            self.state.dma.vram_active = false;
            self.set_irq(VdcIrq::VramDma);

            log::trace!("Finished VRAM DMA on line {}", self.state.scanline);
        }
    }

    fn run_sprite_evaluation(&mut self) {
        // In sprite coordinates, Y=64 is the first line of active display
        const SCREEN_TOP: u16 = 64;

        // TODO sprite limits if display width is less than 256px (or 248px?)

        self.sprite_evaluation_buffer.clear();

        let sprite_line = match self.state.v_mode {
            VerticalMode::ActiveDisplay => SCREEN_TOP + self.state.v_counter + 1,
            _ => SCREEN_TOP, // Last line of top border; evaluate for first line of active display
        };

        for (sprite_idx, sprite) in self.sprite_table.iter().enumerate() {
            let sprite_height_pixels = sprite.height.to_pixels();
            let sprite_y_range = sprite.y..sprite.y + sprite_height_pixels;
            if !sprite_y_range.contains(&sprite_line) {
                continue;
            }

            let mut sprite_row = sprite_line - sprite.y;
            if sprite.v_flip {
                sprite_row = sprite_height_pixels - 1 - sprite_row;
            }

            let mut base_tile_number = sprite.tile_number
                & sprite.width.tile_number_mask()
                & sprite.height.tile_number_mask();
            base_tile_number += 2 * (sprite_row / 16);
            sprite_row %= 16;

            let width_tiles = sprite.width.to_sprite_tiles();
            for i in 0..width_tiles {
                let x_tile = match sprite.width {
                    SpriteWidth::Single => 0,
                    SpriteWidth::Double => i ^ u16::from(sprite.h_flip),
                };

                let x = sprite.x + 16 * i;
                let tile_number = base_tile_number + x_tile;

                if self.sprite_evaluation_buffer.len() == MAX_SPRITES_PER_LINE {
                    // TODO sprite overflow IRQ; should get set around the same time as raster compare IRQ?
                    return;
                }

                self.sprite_evaluation_buffer.push(EvaluatedSpriteEntry {
                    sprite_idx: sprite_idx as u8,
                    x,
                    tile_number,
                    tile_row: sprite_row,
                    h_flip: sprite.h_flip,
                    priority: sprite.priority,
                    palette: sprite.palette,
                    cg_mode: sprite.cg_mode,
                });
            }
        }
    }

    fn fetch_sprite_tiles(&mut self) {
        // In sprite coordinates, X=32 is the leftmost column of active display
        const SCREEN_LEFT: u16 = 32;

        // TODO check for sprite 0 collision
        // TODO sprite limits if not enough dots to fetch 16 tiles

        self.sprite_line_buffer.fill(SpritePixel::TRANSPARENT);

        if !self.state.h_latch.sprites_enabled {
            return;
        }

        for sprite in &self.sprite_evaluation_buffer {
            let tile_addr = (64 * sprite.tile_number) as usize;
            let tile_data = if tile_addr < VRAM_LEN_WORDS {
                &self.vram[tile_addr..tile_addr + 64]
            } else {
                // Tiles 512-1023 supposedly contain "garbage"
                &[0xFFFF; 64]
            };

            let tile_row = sprite.tile_row as usize;
            let cg0 = tile_data[tile_row];
            let cg1 = tile_data[tile_row + 16];
            let cg2 = tile_data[tile_row + 32];
            let cg3 = tile_data[tile_row + 48];

            let mut color_indices: [u16; 16] = array::from_fn(|i| {
                ((cg0 >> (15 - i)) & 1)
                    | (((cg1 >> (15 - i)) & 1) << 1)
                    | (((cg2 >> (15 - i)) & 1) << 2)
                    | (((cg3 >> (15 - i)) & 1) << 3)
            });
            if sprite.h_flip {
                color_indices.reverse();
            }

            for (i, color_idx) in color_indices.into_iter().enumerate() {
                if color_idx == 0 {
                    // Transparent
                    continue;
                }

                let x = sprite.x + i as u16;
                if x.wrapping_sub(SCREEN_LEFT) >= LINE_BUFFER_LEN as u16 {
                    // Horizontally out of bounds
                    continue;
                }

                let line_buffer_idx = (x - SCREEN_LEFT) as usize;
                if !self.sprite_line_buffer[line_buffer_idx].transparent() {
                    // Already an opaque sprite pixel in this position
                    // TODO check for sprite 0 collision
                    continue;
                }

                self.sprite_line_buffer[line_buffer_idx] = SpritePixel {
                    sprite_idx: sprite.sprite_idx,
                    priority: sprite.priority,
                    palette: sprite.palette,
                    color_idx,
                };
            }
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
            log::trace!("  VRAM WRITE: {address:04X} = {value:04X}");
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
