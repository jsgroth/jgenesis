//! GBA PPU (picture processing unit)

mod debug;
mod registers;

use crate::dma::DmaState;
use crate::interrupts::{InterruptRegisters, InterruptType};
use crate::ppu::registers::{
    AffineOverflowBehavior, BgMode, BitsPerPixel, BlendMode, ObjVramMapDimensions, Registers,
    Window, WindowEnabled,
};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::{BoxedByteArray, BoxedWordArray};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::{GetBit, U16Ext};
use std::ops::Range;
use std::{array, cmp, iter, mem};

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
struct GbaFrameBuffer(Box<[u16]>);

impl GbaFrameBuffer {
    fn new() -> Self {
        Self(vec![0; FRAME_BUFFER_LEN].into_boxed_slice())
    }

    fn set(&mut self, line: u32, pixel: u32, color: u16) {
        let frame_buffer_addr = (line * SCREEN_WIDTH + pixel) as usize;
        self.0[frame_buffer_addr] = color;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct RgbaFrameBuffer(Box<[Color]>);

impl RgbaFrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice())
    }

    fn copy_from(&mut self, frame_buffer: &GbaFrameBuffer) {
        let mut address = 0;

        for _ in 0..SCREEN_HEIGHT {
            for _ in 0..SCREEN_WIDTH {
                let gba_color = frame_buffer.0[address];
                self.0[address] = gba_color_to_rgb8(gba_color);
                address += 1;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct BgAffineLatch {
    x: [i32; 2],
    y: [i32; 2],
    x_written: [bool; 2],
    y_written: [bool; 2],
}

impl BgAffineLatch {
    // Called once per frame during VBlank
    fn latch_reference_points(&mut self, registers: &Registers) {
        self.x = registers.bg_affine_parameters.map(|params| params.reference_x);
        self.y = registers.bg_affine_parameters.map(|params| params.reference_y);
    }

    // Called once per line during active display
    fn increment_reference_latches(&mut self, registers: &Registers, bg_enabled_latency: [u8; 4]) {
        for (i, (x, y)) in iter::zip(&mut self.x, &mut self.y).enumerate() {
            // Only increment latched X/Y if they haven't been written within the last scanline
            // e.g. Iridion 3D (game over screen), Star Wars Episode II (text scroll)
            //
            // Also, only increment latches when corresponding BG is enabled (e.g. Pinball Tycoon)
            let bg_enabled = registers.bg_enabled[i + 2] && bg_enabled_latency[i + 2] == 0;

            if mem::take(&mut self.x_written[i]) {
                *x = registers.bg_affine_parameters[i].reference_x;
            } else if bg_enabled {
                *x += registers.bg_affine_parameters[i].b;
            }

            if mem::take(&mut self.y_written[i]) {
                *y = registers.bg_affine_parameters[i].reference_y;
            } else if bg_enabled {
                *y += registers.bg_affine_parameters[i].d;
            }
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct MosaicState {
    bg_v_counter: u8,
    bg_text_line: u32,
    bg_affine: BgAffineLatch,
    obj_v_counter: u8,
    obj_line: u32,
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u32,
    dot: u32,
    frame_complete: bool,
    bg_affine_latch: BgAffineLatch,
    mosaic: MosaicState,
    bg_enabled_latency: [u8; 4],
    obj_enabled_latency: u8,
    forced_blanking_latency: u8,
    window_y_active: [bool; 2],
    video_capture_latch: bool,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            dot: 0,
            frame_complete: false,
            bg_affine_latch: BgAffineLatch::default(),
            mosaic: MosaicState::default(),
            bg_enabled_latency: [0; 4],
            obj_enabled_latency: 0,
            forced_blanking_latency: 0,
            window_y_active: [false; 2],
            video_capture_latch: false,
        }
    }

    // Should be called at the start of each line
    fn update_mosaic_v_state(&mut self, registers: &Registers) {
        // BG V mosaic
        if self.scanline == 0 || self.mosaic.bg_v_counter == registers.bg_mosaic_v_size {
            self.mosaic.bg_v_counter = 0;
            self.mosaic.bg_text_line = self.scanline;
            self.mosaic.bg_affine = self.bg_affine_latch;
        } else {
            self.mosaic.bg_v_counter = (self.mosaic.bg_v_counter + 1) & 0xF;
        }

        // OBJ V mosaic
        if self.scanline == LINES_PER_FRAME - 1 {
            self.mosaic.obj_v_counter = 0;
            self.mosaic.obj_line = 0;
        } else if self.mosaic.obj_v_counter == registers.obj_mosaic_v_size {
            self.mosaic.obj_v_counter = 0;
            self.mosaic.obj_line = self.scanline + 1;
        } else {
            self.mosaic.obj_v_counter = (self.mosaic.obj_v_counter + 1) & 0xF;
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct ObjPixel {
    color: Pixel,
    priority: u8,
    mosaic: bool,
    semi_transparent: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
struct Buffers {
    bg_pixels: [[Pixel; SCREEN_WIDTH as usize]; 4],
    obj_pixels: [ObjPixel; SCREEN_WIDTH as usize],
    obj_window: [bool; SCREEN_WIDTH as usize],
}

impl Buffers {
    fn new() -> Self {
        Self {
            bg_pixels: array::from_fn(|_| array::from_fn(|_| Pixel::default())),
            obj_pixels: array::from_fn(|_| ObjPixel::default()),
            obj_window: array::from_fn(|_| false),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum SpriteMode {
    #[default]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum SpriteSize {
    #[default]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum SpriteShape {
    #[default]
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

#[derive(Debug, Clone, Default, Encode, Decode)]
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
        // First halfword
        // Bit 9 means double size for affine sprites and disabled for non-affine
        let y: u32 = (attributes[0] & 0xFF).into();
        let affine = attributes[0].bit(8);
        let affine_double_size = affine && attributes[0].bit(9);
        let disabled = !affine && attributes[0].bit(9);
        let mode = SpriteMode::from_bits(attributes[0] >> 10);
        let mosaic = attributes[0].bit(12);
        let bpp = BitsPerPixel::from_bit(attributes[0].bit(13));
        let shape = SpriteShape::from_bits(attributes[0] >> 14);

        // Second halfword
        // Bits 9-13 are parameter group for affine sprites and H/V flip for non-affine
        let x: u32 = (attributes[1] & 0x1FF).into();
        let affine_parameter_group = (attributes[1] >> 9) & 0x1F;
        let h_flip = !affine && attributes[1].bit(12);
        let v_flip = !affine && attributes[1].bit(13);
        let size = SpriteSize::from_bits(attributes[1] >> 14);

        // Third halfword
        let tile_number: u32 = (attributes[2] & 0x3FF).into();
        let priority = ((attributes[2] >> 10) & 3) as u8;
        let palette = attributes[2] >> 12;

        Self {
            x,
            y,
            tile_number,
            affine,
            affine_double_size,
            affine_parameter_group,
            disabled,
            mode,
            mosaic,
            bpp,
            shape,
            size,
            h_flip,
            v_flip,
            priority,
            palette,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    frame_buffer: GbaFrameBuffer,
    ready_frame_buffer: RgbaFrameBuffer,
    vram: BoxedByteArray<VRAM_LEN>,
    palette_ram: BoxedWordArray<PALETTE_RAM_LEN_HALFWORDS>,
    oam: BoxedWordArray<OAM_LEN_HALFWORDS>,
    oam_parsed: Box<[OamEntry; 128]>,
    registers: Registers,
    state: State,
    buffers: Box<Buffers>,
    cycles: u64,
    next_event_cycles: u64,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            frame_buffer: GbaFrameBuffer::new(),
            ready_frame_buffer: RgbaFrameBuffer::new(),
            vram: BoxedByteArray::new(),
            palette_ram: BoxedWordArray::new(),
            oam: BoxedWordArray::new(),
            oam_parsed: Box::new(array::from_fn(|_| OamEntry::default())),
            registers: Registers::new(),
            state: State::new(),
            buffers: Box::new(Buffers::new()),
            cycles: 0,
            next_event_cycles: 0,
        }
    }

    pub fn step_to(
        &mut self,
        cycles: u64,
        interrupts: &mut InterruptRegisters,
        dma: &mut DmaState,
    ) {
        if cycles <= self.cycles {
            return;
        }

        if cycles < self.next_event_cycles {
            self.state.dot += (cycles - self.cycles) as u32;
            self.cycles = cycles;
            return;
        }

        self.tick(cycles, interrupts, dma);
    }

    fn tick(&mut self, cycles: u64, interrupts: &mut InterruptRegisters, dma: &mut DmaState) {
        fn render_line(ppu: &mut Ppu, _: &mut InterruptRegisters, _: &mut DmaState, _: u64) {
            ppu.render_current_line();
            ppu.render_next_sprite_line();
        }

        fn hblank_start(
            ppu: &mut Ppu,
            interrupts: &mut InterruptRegisters,
            dma: &mut DmaState,
            cycles: u64,
        ) {
            if ppu.registers.hblank_irq_enabled {
                interrupts.set_flag(InterruptType::HBlank, cycles);
            }

            if ppu.state.scanline < SCREEN_HEIGHT {
                ppu.state
                    .bg_affine_latch
                    .increment_reference_latches(&ppu.registers, ppu.state.bg_enabled_latency);

                dma.notify_hblank_start();
            }
        }

        fn end_of_line(
            ppu: &mut Ppu,
            interrupts: &mut InterruptRegisters,
            dma: &mut DmaState,
            cycles: u64,
        ) {
            ppu.state.dot = 0;

            ppu.state.scanline += 1;
            match ppu.state.scanline {
                LINES_PER_FRAME => {
                    ppu.state.scanline = 0;

                    ppu.state.bg_affine_latch.latch_reference_points(&ppu.registers);
                    ppu.state.bg_affine_latch.x_written = [false; 2];
                    ppu.state.bg_affine_latch.y_written = [false; 2];
                }
                SCREEN_HEIGHT => {
                    if ppu.registers.vblank_irq_enabled {
                        interrupts.set_flag(InterruptType::VBlank, cycles);
                    }

                    dma.notify_vblank_start();

                    ppu.state.frame_complete = true;
                    ppu.ready_frame_buffer.copy_from(&ppu.frame_buffer);
                }
                162 => {
                    if ppu.state.video_capture_latch {
                        dma.end_video_capture();
                    }
                    ppu.state.video_capture_latch = dma.video_capture_active();
                }
                _ => {}
            }

            if ppu.state.video_capture_latch && (2..162).contains(&ppu.state.scanline) {
                dma.notify_video_capture();
            }

            ppu.state.update_mosaic_v_state(&ppu.registers);

            if ppu.registers.v_counter_irq_enabled
                && (ppu.state.scanline as u8) == ppu.registers.v_counter_match
            {
                interrupts.set_flag(InterruptType::VCounter, cycles);
            }

            for latency in &mut ppu.state.bg_enabled_latency {
                *latency = latency.saturating_sub(1);
            }
            ppu.state.obj_enabled_latency = ppu.state.obj_enabled_latency.saturating_sub(1);
            ppu.state.forced_blanking_latency = ppu.state.forced_blanking_latency.saturating_sub(1);

            for (i, window_y_active) in ppu.state.window_y_active.iter_mut().enumerate() {
                *window_y_active |= ppu.state.scanline == ppu.registers.window_y1[i];
                *window_y_active &= ppu.state.scanline != ppu.registers.window_y2[i];
            }
        }

        type EventFn = fn(&mut Ppu, &mut InterruptRegisters, &mut DmaState, u64);

        // Arbitrary dot around the middle of the line
        const RENDER_DOT: u32 = 526;

        const LINE_EVENTS: &[(u32, EventFn)] = &[
            (RENDER_DOT, render_line),
            (HBLANK_START_DOT, hblank_start),
            (DOTS_PER_LINE, end_of_line),
        ];

        if cycles <= self.cycles {
            return;
        }

        let mut elapsed_cycles = (cycles - self.cycles) as u32;
        let mut event_idx = 0;
        while elapsed_cycles != 0 {
            while self.state.dot >= LINE_EVENTS[event_idx].0 {
                event_idx += 1;
            }

            let change = cmp::min(elapsed_cycles, LINE_EVENTS[event_idx].0 - self.state.dot);
            self.state.dot += change;
            elapsed_cycles -= change;
            self.cycles += u64::from(change);

            if self.state.dot == LINE_EVENTS[event_idx].0 {
                (LINE_EVENTS[event_idx].1)(self, interrupts, dma, self.cycles);
                if event_idx == LINE_EVENTS.len() - 1 {
                    event_idx = 0;
                }
            }
        }

        self.next_event_cycles = self.cycles + u64::from(LINE_EVENTS[event_idx].0 - self.state.dot);
    }

    fn render_current_line(&mut self) {
        if self.state.scanline >= SCREEN_HEIGHT {
            return;
        }

        if self.registers.forced_blanking || self.state.forced_blanking_latency != 0 {
            self.clear_current_line();
            return;
        }

        self.render_bg_layers();

        self.merge_layers();
    }

    fn clear_current_line(&mut self) {
        const WHITE: u16 = 0x7FFF;

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

        let bg_control = &self.registers.bg_control[bg];

        let width_tiles = bg_control.size.text_width_tiles();
        let width_screens = width_tiles / 32;
        let height_tiles = bg_control.size.text_height_tiles();

        let h_scroll = self.registers.bg_h_scroll[bg];
        let fine_h_scroll = h_scroll % 8;
        let coarse_h_scroll = h_scroll / 8;

        let scanline =
            if bg_control.mosaic { self.state.mosaic.bg_text_line } else { self.state.scanline };
        let v_scroll = self.registers.bg_v_scroll[bg];
        let scrolled_line = scanline + v_scroll;

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

        self.apply_bg_h_mosaic(bg);
    }

    fn render_affine_bg(&mut self, bg: usize, sample_fn: impl Fn(&Self, i32, i32) -> Pixel) {
        assert!(bg == 2 || bg == 3);

        self.buffers.bg_pixels[bg].fill(Pixel::TRANSPARENT);

        if !self.registers.bg_enabled[bg] {
            return;
        }

        let bg_control = &self.registers.bg_control[bg];

        let dx = self.registers.bg_affine_parameters[bg - 2].a;
        let dy = self.registers.bg_affine_parameters[bg - 2].c;

        let bg_affine_latch = if bg_control.mosaic {
            self.state.mosaic.bg_affine
        } else {
            self.state.bg_affine_latch
        };
        let mut x = bg_affine_latch.x[bg - 2];
        let mut y = bg_affine_latch.y[bg - 2];

        for pixel in 0..SCREEN_WIDTH {
            // Affine coordinates are in 1/256 pixel units - convert to pixel
            let x_pixel = x >> 8;
            let y_pixel = y >> 8;

            self.buffers.bg_pixels[bg][pixel as usize] = sample_fn(self, x_pixel, y_pixel);

            x += dx;
            y += dy;
        }

        self.apply_bg_h_mosaic(bg);
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

    fn apply_bg_h_mosaic(&mut self, bg: usize) {
        if !self.registers.bg_control[bg].mosaic {
            return;
        }

        let mut h_counter = 0;
        let mut color_latch = self.buffers.bg_pixels[bg][0];
        for pixel in 1..SCREEN_WIDTH {
            if h_counter == self.registers.bg_mosaic_h_size {
                h_counter = 0;
                color_latch = self.buffers.bg_pixels[bg][pixel as usize];
            } else {
                h_counter += 1;
                self.buffers.bg_pixels[bg][pixel as usize] = color_latch;
            }
        }
    }

    fn merge_layers(&mut self) {
        #[derive(Debug, Clone, Copy)]
        struct MergePixel {
            color: Pixel,
            layer: Layer,
            priority: u8,
            semi_transparent: bool,
        }

        let backdrop_color = Pixel::new_transparent(self.palette_ram[0]);

        // Alpha blending coefficients
        let eva: u16 = cmp::min(16, self.registers.blend_alpha_a).into();
        let evb: u16 = cmp::min(16, self.registers.blend_alpha_b).into();

        // Brightness increase/decrease coefficient
        let evy: u16 = cmp::min(16, self.registers.blend_brightness).into();

        let bg_enabled: [bool; 4] = array::from_fn(|bg| {
            self.registers.bg_enabled[bg]
                && self.registers.bg_mode.bg_active_in_mode(bg)
                && self.state.bg_enabled_latency[bg] == 0
        });

        let any_window_enabled = self.registers.window_enabled[0]
            || self.registers.window_enabled[1]
            || self.registers.obj_window_enabled;

        let mut window_x_active: [bool; 2] =
            array::from_fn(|i| self.registers.window_x1[i] > self.registers.window_x2[i]);

        let window_y_active: [bool; 2] =
            array::from_fn(|i| self.registers.window_enabled[i] && self.state.window_y_active[i]);

        let obj_enabled = self.registers.obj_enabled && self.state.obj_enabled_latency == 0;

        let mut obj_mosaic_h_counter = self.registers.obj_mosaic_h_size;
        let mut obj_mosaic_latch = ObjPixel::default();

        for pixel in 0..SCREEN_WIDTH {
            for (i, window_x_active) in window_x_active.iter_mut().enumerate() {
                *window_x_active |= pixel == self.registers.window_x1[i];
                *window_x_active &= pixel != self.registers.window_x2[i];
            }

            let window_layers_enabled = if any_window_enabled {
                let window = if window_y_active[0] && window_x_active[0] {
                    Window::Inside0
                } else if window_y_active[1] && window_x_active[1] {
                    Window::Inside1
                } else if self.buffers.obj_window[pixel as usize] {
                    Window::InsideObj
                } else {
                    Window::Outside
                };
                self.registers.window_layers_enabled(window)
            } else {
                WindowEnabled::ALL
            };

            let mut first_pixel = MergePixel {
                color: backdrop_color,
                layer: Layer::Backdrop,
                priority: u8::MAX,
                semi_transparent: false,
            };

            let mut second_pixel = MergePixel {
                color: Pixel::TRANSPARENT,
                layer: Layer::None,
                priority: u8::MAX,
                semi_transparent: false,
            };

            // When priority value is equal, layer priority is OBJ > BG0 > BG1 > BG2 > BG3
            // Process layers in that order

            if obj_enabled {
                let obj_pixel = self.buffers.obj_pixels[pixel as usize];

                if obj_mosaic_h_counter == self.registers.obj_mosaic_h_size {
                    obj_mosaic_h_counter = 0;
                    obj_mosaic_latch = obj_pixel;
                } else {
                    obj_mosaic_h_counter += 1;
                }

                // Update the mosaic latch if the latched pixel or the current pixel is not mosaic-enabled
                // e.g. sprite-hmosaic test ROM
                if !obj_mosaic_latch.mosaic || !obj_pixel.mosaic {
                    obj_mosaic_latch = obj_pixel;
                }

                if window_layers_enabled.obj && !obj_mosaic_latch.color.transparent() {
                    second_pixel = first_pixel;
                    first_pixel = MergePixel {
                        color: obj_mosaic_latch.color,
                        layer: Layer::Obj,
                        priority: obj_pixel.priority,
                        semi_transparent: obj_pixel.semi_transparent,
                    };
                }
            }

            for (bg, enabled) in bg_enabled.into_iter().enumerate() {
                if !enabled || !window_layers_enabled.bg[bg] {
                    continue;
                }

                let bg_pixel = self.buffers.bg_pixels[bg][pixel as usize];
                if bg_pixel.transparent() {
                    continue;
                }

                let priority = self.registers.bg_control[bg].priority;
                if priority < first_pixel.priority {
                    second_pixel = first_pixel;
                    first_pixel = MergePixel {
                        color: bg_pixel,
                        layer: Layer::BG[bg],
                        priority,
                        semi_transparent: false,
                    };
                } else if priority < second_pixel.priority {
                    second_pixel = MergePixel {
                        color: bg_pixel,
                        layer: Layer::BG[bg],
                        priority,
                        semi_transparent: false,
                    };
                }
            }

            let mut blend_color = first_pixel.color;

            if first_pixel.semi_transparent
                || (window_layers_enabled.blend
                    && first_pixel.layer.is_1st_target_enabled(&self.registers))
            {
                let blend_mode = if first_pixel.semi_transparent {
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

            self.frame_buffer.set(self.state.scanline, pixel, blend_color.0);
        }

        if self.registers.green_swap {
            self.green_swap_line();
        }
    }

    fn green_swap_line(&mut self) {
        let scanline_addr = (self.state.scanline * SCREEN_WIDTH) as usize;
        for chunk in self.frame_buffer.0[scanline_addr..scanline_addr + SCREEN_WIDTH as usize]
            .chunks_exact_mut(2)
        {
            let g0 = chunk[0] & (0x1F << 5);
            let g1 = chunk[1] & (0x1F << 5);

            chunk[0] = (chunk[0] & !(0x1F << 5)) | g1;
            chunk[1] = (chunk[1] & !(0x1F << 5)) | g0;
        }
    }

    #[allow(clippy::many_single_char_names)]
    fn render_next_sprite_line(&mut self) {
        if self.registers.forced_blanking
            || (self.state.scanline >= SCREEN_HEIGHT && self.state.scanline != LINES_PER_FRAME - 1)
        {
            return;
        }

        let is_bitmap_mode = self.registers.bg_mode.is_bitmap();

        self.buffers.obj_pixels.fill(ObjPixel::default());
        self.buffers.obj_window.fill(false);

        let target_line =
            if self.state.scanline == LINES_PER_FRAME - 1 { 0 } else { self.state.scanline + 1 };

        // One memory access every 2 cycles
        // When OAM is free during HBlank, sprite rendering runs from dots 40 to 1006 (HBlank start)
        let mut memory_accesses = if self.registers.oam_free_during_hblank {
            // -3 based on Sprite_Last_VRAM_Access_Free test ROM
            (HBLANK_START_DOT - 40) / 2 - 3
        } else {
            DOTS_PER_LINE / 2
        };

        'outer: for oam_idx in 0..128 {
            // 32-bit OAM read of first two attribute words
            memory_accesses -= 1;
            if memory_accesses == 0 {
                break 'outer;
            }

            let oam_entry = &self.oam_parsed[oam_idx as usize];

            if oam_entry.disabled {
                continue;
            }

            let (sprite_width, sprite_height) = oam_entry.shape.size_pixels(oam_entry.size);

            let display_height = sprite_height << u8::from(oam_entry.affine_double_size);
            if oam_entry.y + display_height > 256 && target_line > 128 {
                // 128px tall sprites with Y>128 never display on lines >128
                continue;
            }

            let sprite_y = {
                let mut sprite_y = target_line.wrapping_sub(oam_entry.y) & 0xFF;
                if sprite_y >= display_height {
                    // Sprite does not overlap this scanline
                    continue;
                }

                if oam_entry.mosaic {
                    let mosaic_line = self.state.mosaic.obj_line;
                    sprite_y = mosaic_line.wrapping_sub(oam_entry.y) & 0xFF;
                    if sprite_y >= display_height {
                        // If mosaic moves the Y coordinate out of bounds, clamp to 0
                        // e.g. Castlevania: Aria of Sorrow, Shrek 2, sprite-vmosaic test ROM
                        sprite_y = 0;
                    }
                }

                sprite_y
            };

            // 16-bit OAM read of third attribute word
            memory_accesses -= 1;
            if memory_accesses == 0 {
                break 'outer;
            }

            let oam_entry = oam_entry.clone();

            if oam_entry.affine {
                // 1 idle access cycle plus 4 OAM reads for the affine parameters
                memory_accesses = memory_accesses.saturating_sub(5);
                if memory_accesses == 0 {
                    break 'outer;
                }

                let group_base_addr = 16 * oam_entry.affine_parameter_group as usize;
                let [a, b, c, d] = [
                    self.oam[group_base_addr + 3],
                    self.oam[group_base_addr + 7],
                    self.oam[group_base_addr + 11],
                    self.oam[group_base_addr + 15],
                ]
                .map(|p| i32::from(p as i16));

                let display_width = sprite_width << u8::from(oam_entry.affine_double_size);

                let half_sprite_width = (sprite_width / 2) as i32;
                let half_sprite_height = (sprite_height / 2) as i32;
                let half_display_width = (display_width / 2) as i32;
                let half_display_height = (display_height / 2) as i32;

                let y_offset = (sprite_y as i32) - half_display_height;
                let x_offset = -half_display_width;

                let mut x = a * x_offset + b * y_offset - a;
                let mut y = c * x_offset + d * y_offset - c;

                for sprite_x in 0..display_width {
                    x += a;
                    y += c;

                    let pixel = (oam_entry.x + sprite_x) & 0x1FF;
                    if !(0..SCREEN_WIDTH).contains(&pixel) {
                        // Sprite pixel is offscreen
                        continue;
                    }

                    // 1 VRAM read per pixel for affine sprites
                    memory_accesses -= 1;
                    if memory_accesses == 0 {
                        break 'outer;
                    }

                    let sample_x = (x >> 8) + half_sprite_width;
                    let sample_y = (y >> 8) + half_sprite_height;

                    if !(0..sprite_width as i32).contains(&sample_x)
                        || !(0..sprite_height as i32).contains(&sample_y)
                    {
                        // Sampling point is out of bounds; pixel is transparent
                        continue;
                    }

                    self.render_sprite_pixel(
                        pixel,
                        &oam_entry,
                        sample_x as u32,
                        sample_y as u32,
                        sprite_width,
                        is_bitmap_mode,
                    );
                }
            } else {
                // Non-affine sprite
                let sample_y =
                    if oam_entry.v_flip { sprite_height - 1 - sprite_y } else { sprite_y };

                for sprite_x in 0..sprite_width {
                    let pixel = (oam_entry.x + sprite_x) & 0x1FF;
                    if !(0..SCREEN_WIDTH).contains(&pixel) {
                        // Sprite pixel is offscreen
                        continue;
                    }

                    // 1 VRAM read per 2 pixels for non-affine sprites
                    if sprite_x & 1 == 0 {
                        memory_accesses -= 1;
                        if memory_accesses == 0 {
                            break 'outer;
                        }
                    }

                    let sample_x =
                        if oam_entry.h_flip { sprite_width - 1 - sprite_x } else { sprite_x };

                    self.render_sprite_pixel(
                        pixel,
                        &oam_entry,
                        sample_x,
                        sample_y,
                        sprite_width,
                        is_bitmap_mode,
                    );
                }
            }

            // Next 2 OAM reads overlap with VRAM reads from the previous sprite
            memory_accesses += 2;
        }
    }

    fn render_sprite_pixel(
        &mut self,
        pixel: u32,
        oam_entry: &OamEntry,
        sample_x: u32,
        sample_y: u32,
        sprite_width: u32,
        is_bitmap_mode: bool,
    ) {
        let map_step = match oam_entry.bpp {
            BitsPerPixel::Four => 1,
            BitsPerPixel::Eight => 2,
        };

        let sprite_width_tiles = sprite_width / 8;
        let map_row_width = match self.registers.obj_vram_map_dimensions {
            ObjVramMapDimensions::Two => 32,
            ObjVramMapDimensions::One => map_step * sprite_width_tiles,
        };

        let sprite_tile_x = sample_x / 8;
        let sprite_tile_y = sample_y / 8;

        // TODO how should out-of-bounds tile numbers behave?
        let tile_number =
            (oam_entry.tile_number + sprite_tile_y * map_row_width + sprite_tile_x * map_step)
                & 0x3FF;

        if is_bitmap_mode && tile_number < 512 {
            // Sprite tile numbers 0-511 are not usable in bitmap modes; tiles are fully transparent
            return;
        }

        let tile_col = sample_x % 8;
        let tile_row = sample_y % 8;
        let tile_base_addr = 0x10000 | (tile_number * 32);

        let color_id = match oam_entry.bpp {
            BitsPerPixel::Four => {
                let tile_addr = tile_base_addr + 4 * tile_row + (tile_col >> 1);
                let tile_byte = self.vram[tile_addr as usize];
                (tile_byte >> (4 * (tile_col & 1))) & 0xF
            }
            BitsPerPixel::Eight => {
                let tile_addr = tile_base_addr + 8 * tile_row + tile_col;
                if tile_addr <= 0x17FFF {
                    self.vram[tile_addr as usize]
                } else {
                    // TODO what should this do? can happen when using an odd tile number
                    0
                }
            }
        };

        if oam_entry.mode == SpriteMode::ObjWindow && color_id != 0 {
            // Opaque OBJ window pixel; mark OBJ window and don't update any other buffers
            self.buffers.obj_window[pixel as usize] = true;
            return;
        }

        let buffer_pixel = &mut self.buffers.obj_pixels[pixel as usize];

        if oam_entry.priority >= buffer_pixel.priority && !buffer_pixel.color.transparent() {
            // Existing opaque pixel with the same or lower priority; do nothing
            return;
        }

        // Always update priority and mosaic flags here because of a hardware bug:
        // A transparent pixel that overlaps an opaque pixel from a sprite with lower OAM index and
        // higher priority will overwrite the priority and mosaic flags
        buffer_pixel.priority = oam_entry.priority;
        buffer_pixel.mosaic = oam_entry.mosaic;

        if color_id == 0 {
            // Transparent pixel; don't update color or semi-transparency flag
            return;
        }

        let palette = match oam_entry.bpp {
            BitsPerPixel::Four => oam_entry.palette,
            BitsPerPixel::Eight => 0,
        };
        let palette_ram_addr = 0x100 | (16 * palette + u16::from(color_id));
        let color = self.palette_ram[palette_ram_addr as usize];

        buffer_pixel.color = Pixel::new_opaque(color);
        buffer_pixel.semi_transparent = oam_entry.mode == SpriteMode::SemiTransparent;
    }

    pub fn frame_complete(&self) -> bool {
        self.state.frame_complete
    }

    pub fn clear_frame_complete(&mut self) {
        self.state.frame_complete = false;
    }

    pub fn frame_buffer(&self) -> &[Color] {
        &self.ready_frame_buffer.0
    }

    fn mask_vram_address(address: u32) -> usize {
        let vram_addr = (address as usize) & VRAM_ADDR_MASK & !1;
        if vram_addr & 0x10000 != 0 { 0x10000 | (vram_addr & 0x7FFF) } else { vram_addr }
    }

    fn should_ignore_vram_access(&self, address: u32) -> bool {
        // When the PPU is in a bitmap mode, accesses to mirrored VRAM at $18000-$1C000 do not work (vram-mirror test ROM)
        // Reads always return 0 (or open bus?) and writes are discarded
        self.registers.bg_mode.is_bitmap() && (0x18000..0x1C000).contains(&(address & 0x1FFFF))
    }

    pub fn read_vram(&self, address: u32) -> u16 {
        if self.should_ignore_vram_access(address) {
            return 0;
        }

        let vram_addr = Self::mask_vram_address(address);
        u16::from_le_bytes(self.vram[vram_addr..vram_addr + 2].try_into().unwrap())
    }

    pub fn write_vram(&mut self, address: u32, value: u16) {
        if self.should_ignore_vram_access(address) {
            return;
        }

        let vram_addr = Self::mask_vram_address(address);
        self.vram[vram_addr..vram_addr + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write_vram_byte(&mut self, address: u32, value: u8) {
        let in_obj_vram = if self.registers.bg_mode.is_bitmap() {
            // $14000-$17FFF
            address & 0x10000 != 0 && address & 0x04000 != 0
        } else {
            // $10000-$17FFF
            address & 0x10000 != 0
        };

        if in_obj_vram {
            // 8-bit writes to OBJ VRAM are ignored
            return;
        }

        // 8-bit writes to BG VRAM duplicate the byte
        self.write_vram(address & !1, u16::from_le_bytes([value; 2]));
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

        if address & 3 != 3 {
            let oam_idx = oam_addr >> 2;
            self.oam_parsed[oam_idx] = OamEntry::parse([
                self.oam[4 * oam_idx],
                self.oam[4 * oam_idx + 1],
                self.oam[4 * oam_idx + 2],
            ]);
        }
    }

    // TODO this is not accurate
    // PPU usually performs 1 palette RAM access per 4 cycles, but it will perform a second access
    // if it needs to for alpha blending.
    // In modes 3 and 5, it also skips accesses where the layer is BG2 (direct color bitmap)
    pub fn palette_ram_in_use(&self) -> bool {
        const RENDER_START_DOT: u32 = 46;

        !self.registers.forced_blanking
            && self.state.scanline < SCREEN_HEIGHT
            && (RENDER_START_DOT..HBLANK_START_DOT).contains(&self.state.dot)
            && self.state.dot % 4 == 0
    }

    pub fn vram_in_use(&self, address: u32) -> bool {
        if self.registers.forced_blanking || self.state.forced_blanking_latency != 0 {
            return false;
        }

        let sprite_vram_start = if self.registers.bg_mode.is_bitmap() { 0x14000 } else { 0x10000 };
        if address & 0x1FFFF < sprite_vram_start {
            self.bg_vram_in_use()
        } else {
            self.sprite_vram_in_use()
        }
    }

    fn bg_vram_in_use(&self) -> bool {
        const FETCH_START_DOT: u32 = 30;

        if self.state.scanline >= SCREEN_HEIGHT || self.state.dot >= HBLANK_START_DOT {
            // No BG fetching during VBlank or HBlank
            return false;
        }

        let is_bg_enabled =
            |bg: usize| self.registers.bg_enabled[bg] && self.state.bg_enabled_latency[bg] == 0;

        // Text BG access pattern (32-cycle batches):
        //   4bpp: M--- T--- ---- ---- ---- T--- ---- ----
        //   8bpp: M--- T--- ---- T--- ---- T--- ---- T---
        // In mode 0, slots constantly cycle: BG0, BG1, BG2, BG3, BG0, BG1, etc.
        // In mode 1, the BG2 and BG3 slots are both used for affine BG2
        let check_text_bg = |bg: usize| {
            if !is_bg_enabled(bg) {
                return false;
            }

            // Fetching starts earlier if BG is using fine horizontal scrolling
            let start_dot =
                FETCH_START_DOT + (bg as u32) - 4 * (self.registers.bg_h_scroll[bg] % 8);
            if self.state.dot < start_dot {
                return false;
            }

            let offset = (self.state.dot - start_dot) % 32;
            if offset % 4 != 0 {
                return false;
            }

            // Check slots used for both 4bpp and 8bpp accesses
            let slot = offset / 4;
            if slot == 0 || slot == 1 || slot == 5 {
                return true;
            }

            // If 8bpp, additionally check slots used for only 8bpp accesses
            self.registers.bg_control[bg].bpp == BitsPerPixel::Eight && (slot == 3 || slot == 7)
        };

        // Affine BGs access during every cycle if enabled
        // In mode 1, alternates between 2 cycles of BG0/1 (text), 2 cycles of BG2, 2 cycles of BG0/1, etc.
        // In mode 2, alternates between 2 cycles of BG3, 2 cycles of BG2, 2 cycles of BG3, etc.
        let check_affine_bg = |bg: usize| is_bg_enabled(bg) && self.state.dot >= FETCH_START_DOT;

        let offset = self.state.dot % 4;
        match self.registers.bg_mode {
            BgMode::Zero => check_text_bg(offset as usize),
            BgMode::One => {
                if offset < 2 {
                    check_text_bg(offset as usize)
                } else {
                    check_affine_bg(2)
                }
            }
            BgMode::Two => check_affine_bg(2 + usize::from(offset < 2)),
            _ => {
                // Bitmap modes supposedly never block access to VRAM?
                false
            }
        }
    }

    // TODO this is not entirely accurate - some even cycles only perform an OAM access, and some
    // even cycles don't perform an access at all (e.g. for affine sprites)
    fn sprite_vram_in_use(&self) -> bool {
        const FETCH_START_DOT: u32 = 40;

        if !self.registers.obj_enabled || self.state.obj_enabled_latency != 0 {
            return false;
        }

        if self.state.dot % 2 != 0 {
            // Sprite hardware only accesses VRAM/OAM on even cycles
            return false;
        }

        if (SCREEN_HEIGHT..LINES_PER_FRAME - 1).contains(&self.state.scanline) {
            // VBlank lines; sprite hardware is idle
            return false;
        }

        if (self.state.scanline == SCREEN_HEIGHT - 1 && self.state.dot >= FETCH_START_DOT)
            || (self.state.scanline == LINES_PER_FRAME - 1 && self.state.dot < FETCH_START_DOT)
        {
            // Too late in the last line (159) or too early in the first line (227)
            return false;
        }

        let interval = if self.registers.oam_free_during_hblank {
            FETCH_START_DOT..HBLANK_START_DOT
        } else {
            0..DOTS_PER_LINE
        };

        interval.contains(&self.state.dot)
    }

    pub fn read_register(
        &mut self,
        address: u32,
        cycles: u64,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) -> Option<u16> {
        self.step_to(cycles, interrupts, dma);

        log::trace!("PPU register read {address:08X}");

        let value = match address {
            0x4000000 => self.registers.read_dispcnt(),
            0x4000002 => self.registers.read_green_swap(),
            0x4000004 => self.read_dispstat(),
            0x4000006 => self.v_counter().into(),
            0x4000008..=0x400000E => {
                let bg = (address & 7) >> 1;
                self.registers.read_bgcnt(bg as usize)
            }
            0x4000048 => self.registers.read_winin(),
            0x400004A => self.registers.read_winout(),
            0x4000050 => self.registers.read_bldcnt(),
            0x4000052 => self.registers.read_bldalpha(),
            _ => {
                log::debug!("Unhandled PPU register read {address:08X}");
                return None;
            }
        };

        Some(value)
    }

    // $4000004: DISPSTAT (Display status)
    fn read_dispstat(&self) -> u16 {
        let in_vblank = self.in_vblank();
        let in_hblank = self.in_hblank();
        let v_counter_match = self.v_counter() == self.registers.v_counter_match;

        u16::from(in_vblank)
            | (u16::from(in_hblank) << 1)
            | (u16::from(v_counter_match) << 2)
            | (u16::from(self.registers.vblank_irq_enabled) << 3)
            | (u16::from(self.registers.hblank_irq_enabled) << 4)
            | (u16::from(self.registers.v_counter_irq_enabled) << 5)
            | (u16::from(self.registers.v_counter_match) << 8)
    }

    // $4000004: DISPSTAT (Display status)
    fn write_dispstat(&mut self, value: u16, cycles: u64, interrupts: &mut InterruptRegisters) {
        let prev_v_count_enabled = self.registers.v_counter_irq_enabled;
        let v_counter = self.v_counter();
        let prev_v_count_match = self.registers.v_counter_match == v_counter;

        self.registers.write_dispstat(value);

        // Changing VCOUNT match mid-line can trigger VCOUNT match IRQs
        // e.g. lyc_midline and window_midframe test ROMs
        // TODO is it right that this only happens if VCOUNT enabled status doesn't change?
        if prev_v_count_enabled
            && self.registers.v_counter_irq_enabled
            && !prev_v_count_match
            && self.registers.v_counter_match == v_counter
        {
            interrupts.set_flag(InterruptType::VCounter, cycles + 1);
        }
    }

    fn in_vblank(&self) -> bool {
        VBLANK_LINES.contains(&self.state.scanline)
    }

    fn in_hblank(&self) -> bool {
        (HBLANK_START_DOT..DOTS_PER_LINE - 1).contains(&self.state.dot)
    }

    fn v_counter(&self) -> u8 {
        let mut line = self.state.scanline;
        if self.state.dot == DOTS_PER_LINE - 1 {
            line += 1;
            if line == LINES_PER_FRAME {
                line = 0;
            }
        }
        line as u8
    }

    #[allow(clippy::match_same_arms)]
    pub fn write_register(
        &mut self,
        address: u32,
        value: u16,
        cycles: u64,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        log::debug!(
            "PPU register write {address:08X} {value:04X} (line {} dot {})",
            self.state.scanline,
            self.state.dot
        );

        self.step_to(cycles, interrupts, dma);

        match address {
            0x4000000 => self.registers.write_dispcnt(value, &mut self.state),
            0x4000002 => self.registers.write_green_swap(value),
            0x4000004 => self.write_dispstat(value, cycles, interrupts),
            0x4000006 => {} // High halfword of word-size writes to DISPSTAT
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
            0x400004E => {} // High halfword of word-size writes to MOSAIC
            0x4000050 => self.registers.write_bldcnt(value),
            0x4000052 => self.registers.write_bldalpha(value),
            0x4000054 => self.registers.write_bldy(value),
            0x4000056 => {} // High halfword of word-size writes to BLDY
            _ => {
                log::debug!("Unhandled PPU register write {address:08X} {value:04X}");
            }
        }
    }

    pub fn write_register_byte(
        &mut self,
        address: u32,
        value: u8,
        cycles: u64,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        trait U16Ext {
            fn set_byte(&mut self, i: bool, value: u8);
        }

        impl U16Ext for u16 {
            fn set_byte(&mut self, i: bool, value: u8) {
                if !i {
                    self.set_lsb(value);
                } else {
                    self.set_msb(value);
                }
            }
        }

        self.step_to(cycles, interrupts, dma);

        // TODO BGxHOFS, BGxVOFS, MOSAIC, blend registers
        match address {
            0x4000000..=0x4000005
            | 0x4000008..=0x400000F
            | 0x4000048..=0x400004B
            | 0x4000050..=0x4000053 => {
                // R/W registers: DISPCNT, green swap, DISPSTAT, BGxCNT, WININ, WINOUT, BLDCNT, BLDALPHA
                let Some(mut halfword) = self.read_register(address & !1, cycles, dma, interrupts)
                else {
                    return;
                };
                halfword.set_byte(address.bit(0), value);
                self.write_register(address & !1, halfword, cycles, dma, interrupts);
            }
            0x4000010..=0x400001F => {
                // BGxHOFS / BGxVOFS
                let bg = ((address >> 2) & 3) as usize;
                if !address.bit(1) {
                    let mut hofs = self.registers.bg_h_scroll[bg] as u16;
                    hofs.set_byte(address.bit(0), value);
                    self.registers.write_bghofs(bg, hofs);
                } else {
                    let mut vofs = self.registers.bg_v_scroll[bg] as u16;
                    vofs.set_byte(address.bit(0), value);
                    self.registers.write_bgvofs(bg, vofs);
                }
            }
            0x4000020..=0x400003F => {
                // BG affine registers
                let mut halfword = self.registers.read_bg_affine_register(address & !1);
                halfword.set_byte(address.bit(0), value);
                self.registers.write_bg_affine_register(
                    address & !1,
                    halfword,
                    &mut self.state.bg_affine_latch,
                );
            }
            0x4000040 => self.registers.write_winh_low(0, value),
            0x4000041 => self.registers.write_winh_high(0, value),
            0x4000042 => self.registers.write_winh_low(1, value),
            0x4000043 => self.registers.write_winh_high(1, value),
            0x4000044 => self.registers.write_winv_low(0, value),
            0x4000045 => self.registers.write_winv_high(0, value),
            0x4000046 => self.registers.write_winv_low(1, value),
            0x4000047 => self.registers.write_winv_high(1, value),
            0x400004C => self.registers.write_bg_mosaic(value),
            0x400004D => self.registers.write_obj_mosaic(value),
            0x4000054 => self.registers.write_bldy(value.into()), // BLDY is only a 5-bit register
            _ => {
                log::debug!("Unexpected PPU byte register write {address:08X} {value:02X}");
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

fn gba_color_to_rgb8(gba_color: u16) -> Color {
    const RGB_5_TO_8: &[u8; 32] = &[
        0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165,
        173, 181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
    ];

    let r = gba_color & 0x1F;
    let g = (gba_color >> 5) & 0x1F;
    let b = (gba_color >> 10) & 0x1F;

    Color::rgb(RGB_5_TO_8[r as usize], RGB_5_TO_8[g as usize], RGB_5_TO_8[b as usize])
}
