//! SNES PPU (picture processing unit)

mod colortable;
mod debug;
mod registers;
mod sprites;

use crate::api::SnesEmulatorConfig;
use crate::ppu::registers::{
    AccessFlipflop, BgMode, BgScreenSize, BitsPerPixel, MidScanlineUpdate, Mode7OobBehavior,
    Registers, TileSize, VramIncrementMode,
};
use crate::ppu::sprites::{SpriteProcessor, SpriteState};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;
use std::ops::{Deref, DerefMut};

const MAX_BRIGHTNESS: u8 = 15;

const NORMAL_SCREEN_WIDTH: usize = 256;
const HIRES_SCREEN_WIDTH: usize = 512;
const MAX_SCREEN_HEIGHT: usize = 478;
const FRAME_BUFFER_LEN: usize = HIRES_SCREEN_WIDTH * MAX_SCREEN_HEIGHT;

const MAX_SPRITES_PER_LINE: usize = 32;
const MAX_SPRITE_TILES_PER_LINE: usize = 34;

const VRAM_LEN_WORDS: usize = 64 * 1024 / 2;
const OAM_LOW_LEN_WORDS: usize = 512 / 2;
const OAM_HIGH_LEN_BYTES: usize = 32;
const CGRAM_LEN_WORDS: usize = 256;

const VRAM_ADDRESS_MASK: u16 = (1 << 15) - 1;
const OAM_ADDRESS_MASK: u16 = (1 << 9) - 1;

const MCLKS_PER_NORMAL_SCANLINE: u64 = 1364;
const MCLKS_PER_SHORT_SCANLINE: u64 = 1360;
const MCLKS_PER_LONG_SCANLINE: u64 = 1368;

type Vram = [u16; VRAM_LEN_WORDS];
type OamLow = [u16; OAM_LOW_LEN_WORDS];
type OamHigh = [u8; OAM_HIGH_LEN_BYTES];
type Cgram = [u16; CGRAM_LEN_WORDS];

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u16,
    scanline_master_cycles: u64,
    current_line_length: ScanlineLength,
    prev_line_length: ScanlineLength,
    v_mosaic_counter: u8,
    mosaic_size_latch: u8,
    dot_event_idx: u8,
    odd_frame: bool,
    pending_sprite_pixel_overflow: bool,
    ppu1_open_bus: u8,
    ppu2_open_bus: u8,
    last_rendered_scanline: Option<u16>,
    // Tracks if Mode 5/6 or pseudo-hi-res was enabled at any point during active display
    h_hi_res_frame: bool,
    // Tracks if interlacing was enabled at the start of the frame
    v_hi_res_frame: bool,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            scanline_master_cycles: 0,
            current_line_length: ScanlineLength::Normal,
            prev_line_length: ScanlineLength::Normal,
            v_mosaic_counter: 0,
            mosaic_size_latch: 0,
            dot_event_idx: 0,
            odd_frame: false,
            pending_sprite_pixel_overflow: false,
            ppu1_open_bus: 0,
            ppu2_open_bus: 0,
            last_rendered_scanline: None,
            h_hi_res_frame: false,
            v_hi_res_frame: false,
        }
    }

    fn frame_screen_width(&self) -> u32 {
        if self.h_hi_res_frame { HIRES_SCREEN_WIDTH as u32 } else { NORMAL_SCREEN_WIDTH as u32 }
    }

    fn update_v_mosaic(&mut self, mosaic_size: u8) {
        if self.scanline == 1 || self.v_mosaic_counter == 0 {
            self.v_mosaic_counter = mosaic_size;
            self.mosaic_size_latch = mosaic_size;
        } else {
            self.v_mosaic_counter -= 1;
        }
    }

    fn v_mosaic_filter(&self, scanline: u16, hi_res_mode: HiResMode) -> u16 {
        let mut offset: u16 = (self.mosaic_size_latch - self.v_mosaic_counter).into();
        if hi_res_mode == HiResMode::True && self.v_hi_res_frame {
            offset *= 2;
        }
        scanline - offset
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct CachedBgMapEntry {
    map_x: u16,
    map_y: u16,
    tile_number: u16,
    palette: u8,
    priority: bool,
    x_flip: bool,
    y_flip: bool,
}

impl Default for CachedBgMapEntry {
    fn default() -> Self {
        Self {
            map_x: u16::MAX,
            map_y: u16::MAX,
            tile_number: 0,
            palette: 0,
            priority: false,
            x_flip: false,
            y_flip: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct Pixel {
    palette: u8,
    color: u8,
    priority: u8,
}

impl Pixel {
    const TRANSPARENT: Self = Self { palette: 0, color: 0, priority: 0 };

    fn is_transparent(self) -> bool {
        self.color == 0
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct RenderedPixel {
    color: u16,
    palette: u8,
    layer: Layer,
}

impl Default for RenderedPixel {
    fn default() -> Self {
        Self { color: 0, palette: 0, layer: Layer::Backdrop }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Buffers {
    bg_pixels: [[Pixel; HIRES_SCREEN_WIDTH]; 4],
    obj_pixels: [Pixel; NORMAL_SCREEN_WIDTH],
    offset_per_tile_h_scroll: [[u16; NORMAL_SCREEN_WIDTH]; 2],
    offset_per_tile_v_scroll: [[u16; NORMAL_SCREEN_WIDTH]; 2],
    main_screen_pixels: [PriorityResolver; NORMAL_SCREEN_WIDTH],
    main_screen_rendered_pixels: [RenderedPixel; NORMAL_SCREEN_WIDTH],
    sub_screen_pixels: [PriorityResolver; NORMAL_SCREEN_WIDTH],
    sub_screen_rendered_pixels: [RenderedPixel; NORMAL_SCREEN_WIDTH],
}

impl Buffers {
    fn new() -> Self {
        Self {
            bg_pixels: array::from_fn(|_| array::from_fn(|_| Pixel::TRANSPARENT)),
            obj_pixels: array::from_fn(|_| Pixel::TRANSPARENT),
            offset_per_tile_h_scroll: array::from_fn(|_| array::from_fn(|_| 0)),
            offset_per_tile_v_scroll: array::from_fn(|_| array::from_fn(|_| 0)),
            main_screen_pixels: array::from_fn(|_| PriorityResolver::new()),
            main_screen_rendered_pixels: array::from_fn(|_| RenderedPixel::default()),
            sub_screen_pixels: array::from_fn(|_| PriorityResolver::new()),
            sub_screen_rendered_pixels: array::from_fn(|_| RenderedPixel::default()),
        }
    }
}

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl FrameBuffer {
    fn new() -> Self {
        Self::default()
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Deref for FrameBuffer {
    type Target = Box<[Color; FRAME_BUFFER_LEN]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuTickEffect {
    None,
    FrameComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Layer {
    Bg1,
    Bg2,
    Bg3,
    Bg4,
    Obj,
    Backdrop,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct PriorityResolver {
    min_priority: u8,
    min_pixel: Pixel,
    min_layer: Layer,
}

impl PriorityResolver {
    // Mode 0-1 priorities:
    //   OBJ.3 > BG1.1 > BG2.1 > OBJ.2 > BG1.0 > BG2.0 > OBJ.1 > BG3.1 > BG4.1 > OBJ.0 > BG3.0 > BG4.0
    //     0  <    1   <   2   <   3   <   4   <   5   <   6   <   7   <   8   <   9   <   10  <   11
    //   (unless in Mode 1 and the BG3 high priority flag is set, which moves BG3.1 to highest priority)
    // Mode 2-7 priorities:
    //   OBJ.3 > BG1.1 > OBJ.2 > BG2.1 > OBJ.1 > BG1.0 > OBJ.0 > BG2.0
    //     0   <   1   <   2   <   3   <   4   <   5   <   6   <   7
    //   (BG3 and BG4 are never rendered in these modes)
    const OBJ3: u8 = 0;
    const BG1_HIGH: u8 = 1;
    const MODE_01_BG2_HIGH: u8 = 2;
    const MODE_01_OBJ2: u8 = 3;
    const MODE_01_BG1_LOW: u8 = 4;
    const MODE_01_BG2_LOW: u8 = 5;
    const MODE_01_OBJ1: u8 = 6;
    const BG3_HIGH: u8 = 7;
    const BG4_HIGH: u8 = 8;
    const MODE_01_OBJ0: u8 = 9;
    const BG3_LOW: u8 = 10;
    const BG4_LOW: u8 = 11;

    // OBJ.3 and BG1.1 have the same priority in modes 2-7 as in modes 0-1
    const MODE_27_OBJ2: u8 = 2;
    const MODE_27_BG2_HIGH: u8 = 3;
    const MODE_27_OBJ1: u8 = 4;
    const MODE_27_BG1_LOW: u8 = 5;
    const MODE_27_OBJ0: u8 = 6;
    const MODE_27_BG2_LOW: u8 = 7;

    fn new() -> Self {
        Self { min_priority: u8::MAX, min_pixel: Pixel::TRANSPARENT, min_layer: Layer::Backdrop }
    }

    fn add_bg1(&mut self, pixel: Pixel, is_mode_0_or_1: bool) {
        let priority = match (is_mode_0_or_1, pixel.priority) {
            (true, 0) => Self::MODE_01_BG1_LOW,
            (false, 0) => Self::MODE_27_BG1_LOW,
            (_, 1) => Self::BG1_HIGH,
            _ => panic!("Invalid BG1 pixel priority: {}", pixel.priority),
        };
        self.add_pixel(pixel, Layer::Bg1, priority);
    }

    fn add_bg2(&mut self, pixel: Pixel, is_mode_0_or_1: bool) {
        let priority = match (is_mode_0_or_1, pixel.priority) {
            (true, 0) => Self::MODE_01_BG2_LOW,
            (true, 1) => Self::MODE_01_BG2_HIGH,
            (false, 0) => Self::MODE_27_BG2_LOW,
            (false, 1) => Self::MODE_27_BG2_HIGH,
            _ => panic!("Invalid BG2 pixel priority: {}", pixel.priority),
        };
        self.add_pixel(pixel, Layer::Bg2, priority);
    }

    fn add_bg3(&mut self, pixel: Pixel, bg3_high_priority: bool) {
        if bg3_high_priority && pixel.priority == 1 {
            // In mode 1, non-transparent high-priority BG3 pixels display over all other layers
            self.min_priority = 0;
            self.min_pixel = pixel;
            self.min_layer = Layer::Bg3;
            return;
        }

        let priority = if pixel.priority == 1 { Self::BG3_HIGH } else { Self::BG3_LOW };
        self.add_pixel(pixel, Layer::Bg3, priority);
    }

    fn add_bg4(&mut self, pixel: Pixel) {
        let priority = if pixel.priority == 1 { Self::BG4_HIGH } else { Self::BG4_LOW };
        self.add_pixel(pixel, Layer::Bg4, priority);
    }

    fn add_obj(&mut self, pixel: Pixel, is_mode_0_or_1: bool) {
        let priority = match (is_mode_0_or_1, pixel.priority) {
            (true, 0) => Self::MODE_01_OBJ0,
            (true, 1) => Self::MODE_01_OBJ1,
            (true, 2) => Self::MODE_01_OBJ2,
            (_, 3) => Self::OBJ3,
            (false, 0) => Self::MODE_27_OBJ0,
            (false, 1) => Self::MODE_27_OBJ1,
            (false, 2) => Self::MODE_27_OBJ2,
            _ => panic!("Invalid OBJ pixel priority: {}", pixel.priority),
        };
        self.add_pixel(pixel, Layer::Obj, priority);
    }

    #[inline(always)]
    fn add_pixel(&mut self, pixel: Pixel, layer: Layer, layer_priority: u8) {
        if layer_priority < self.min_priority {
            self.min_priority = layer_priority;
            self.min_pixel = pixel;
            self.min_layer = layer;
        }
    }

    fn get(self) -> Option<(Pixel, Layer)> {
        (self.min_priority != u8::MAX).then_some((self.min_pixel, self.min_layer))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Sub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HiResMode {
    None,
    Pseudo,
    True,
}

impl HiResMode {
    fn is_hi_res(self) -> bool {
        matches!(self, Self::Pseudo | Self::True)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    timing_mode: TimingMode,
    registers: Registers,
    state: State,
    buffers: Box<Buffers>,
    vram: Box<Vram>,
    oam_low: Box<OamLow>,
    oam_high: Box<OamHigh>,
    cgram: Box<Cgram>,
    frame_buffer: FrameBuffer,
    sprites: SpriteProcessor,
    deinterlace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ScanlineLength {
    Normal,
    Short,
    Long,
}

impl ScanlineLength {
    fn scanline_mclks(self) -> u64 {
        match self {
            Self::Normal => MCLKS_PER_NORMAL_SCANLINE,
            Self::Short => MCLKS_PER_SHORT_SCANLINE,
            Self::Long => MCLKS_PER_LONG_SCANLINE,
        }
    }
}

// In actual hardware, PPU starts rendering pixels at H=22 / mclk=88
// Some games depend on this 88-cycle delay to finish HDMA before rendering starts, e.g. Final Fantasy 6
//
// Here, start rendering at H=23 / mclk=92 to account for CPU writes occurring at the end of the
// CPU cycle rather than in the middle of it; starting at mclk=88 causes flickering in Lemmings
const RENDER_LINE_MCLK: u64 = 92;

// Used as the threshold for applying mid-scanline register writes.
// Ignore writes that occur during the last few pixels of active display; this fixes minor flickering
// in Wild Guns
const END_RENDER_LINE_MCLK: u64 = RENDER_LINE_MCLK + 256 * 4 - 3 * 4;

// Latch Mode 7 registers a bit before rendering starts
// Battle Clash depends on this to avoid a glitchy line where the screen transitions from Mode 1 to Mode 7
const MODE_7_LATCH_MCLK: u64 = 40;

impl Ppu {
    pub fn new(timing_mode: TimingMode, config: SnesEmulatorConfig) -> Self {
        Self {
            timing_mode,
            registers: Registers::new(),
            state: State::new(),
            buffers: Box::new(Buffers::new()),
            vram: vec![0; VRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            oam_low: vec![0; OAM_LOW_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            oam_high: vec![0; OAM_HIGH_LEN_BYTES].into_boxed_slice().try_into().unwrap(),
            cgram: vec![0; CGRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            frame_buffer: FrameBuffer::new(),
            sprites: SpriteProcessor::new(),
            deinterlace: config.deinterlace,
        }
    }

    #[must_use]
    pub fn tick(&mut self, master_cycles: u64) -> PpuTickEffect {
        debug_assert!(master_cycles <= MCLKS_PER_SHORT_SCANLINE);

        let new_scanline_mclks = self.state.scanline_master_cycles + master_cycles;
        self.state.scanline_master_cycles = new_scanline_mclks;

        self.advance_to_dot((new_scanline_mclks / 4) as u16);

        let v_display_size = self.registers.v_display_size.to_lines();

        let mclks_per_scanline = self.state.current_line_length.scanline_mclks();

        let mut tick_effect = PpuTickEffect::None;
        if new_scanline_mclks >= mclks_per_scanline {
            self.state.scanline += 1;
            self.state.scanline_master_cycles = new_scanline_mclks - mclks_per_scanline;

            if self.state.pending_sprite_pixel_overflow {
                self.state.pending_sprite_pixel_overflow = false;
                self.registers.sprite_pixel_overflow = true;
            }

            // Interlaced mode adds an extra scanline every other frame
            let scanlines_per_frame = self.scanlines_per_frame();
            if (self.state.scanline == scanlines_per_frame
                && (!self.registers.interlaced || self.state.odd_frame))
                || self.state.scanline == scanlines_per_frame + 1
            {
                // Start new frame
                self.state.scanline = 0;

                if !self.state.v_hi_res_frame && self.registers.interlaced && !self.deinterlace {
                    self.fix_interlaced_frame_buffer();
                }

                // TODO wait until H=1?
                self.state.odd_frame = !self.state.odd_frame;
                self.state.last_rendered_scanline = None;
                self.state.h_hi_res_frame = self.registers.in_hi_res_mode();
                self.state.v_hi_res_frame = self.registers.interlaced;

                if !self.registers.forced_blanking {
                    self.registers.sprite_overflow = false;
                    self.registers.sprite_pixel_overflow = false;
                }
            }

            self.state.prev_line_length = self.state.current_line_length;
            self.state.current_line_length = self.scanline_length(self.state.scanline);

            self.state.update_v_mosaic(self.registers.mosaic_size);

            self.sprites_finish_line();

            let sprites_interlaced_odd =
                !self.deinterlace && self.state.v_hi_res_frame && self.state.odd_frame;
            self.sprites_start_new_line(self.state.scanline, sprites_interlaced_odd);

            self.state.dot_event_idx = 0;
            self.advance_to_dot((self.state.scanline_master_cycles / 4) as u16);

            if self.state.scanline == v_display_size + 1 {
                // Reload OAM data port address at start of VBlank if not in forced blanking
                if !self.registers.forced_blanking {
                    self.registers.oam_address = self.registers.oam_address_reload_value << 1;
                }

                tick_effect = PpuTickEffect::FrameComplete;
            }
        }

        if self.registers.mid_line_update.is_some()
            && (1..=v_display_size).contains(&self.state.scanline)
            && (RENDER_LINE_MCLK..END_RENDER_LINE_MCLK).contains(&new_scanline_mclks)
        {
            // Between H=22 and H=276 and INIDISP or one of the scroll registers was just modified;
            // partially render current line
            let mid_line_update = self.registers.mid_line_update.unwrap();

            // Scroll register writes don't seem to apply immediately - see the "Good Luck"
            // animation in Air Strike Patrol
            let pixel_offset = match mid_line_update {
                MidScanlineUpdate::Inidisp => 0,
                MidScanlineUpdate::Scroll => 15,
            };

            let from_pixel = (new_scanline_mclks - RENDER_LINE_MCLK) / 4 + pixel_offset;
            if from_pixel < NORMAL_SCREEN_WIDTH as u64 {
                self.render_current_line(from_pixel as u16);
            }
        }

        self.registers.mid_line_update = None;

        tick_effect
    }

    fn advance_to_dot(&mut self, dot: u16) {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum PpuEvent {
            LatchMode7Registers,
            RenderLine,
            FinishSpriteEvaluation,
            StartSpriteTileFetch,
            None,
        }

        const EVENT_DOTS: &[u16; 5] = &[
            (MODE_7_LATCH_MCLK / 4) as u16,
            (RENDER_LINE_MCLK / 4) as u16,
            sprites::SPRITE_EVALUATION_END_DOT,
            sprites::SPRITE_FETCH_START_DOT,
            u16::MAX,
        ];

        const EVENTS: &[PpuEvent; 5] = &[
            PpuEvent::LatchMode7Registers,
            PpuEvent::RenderLine,
            PpuEvent::FinishSpriteEvaluation,
            PpuEvent::StartSpriteTileFetch,
            PpuEvent::None,
        ];

        debug_assert!(EVENT_DOTS.is_sorted());

        while dot >= EVENT_DOTS[self.state.dot_event_idx as usize] {
            match EVENTS[self.state.dot_event_idx as usize] {
                PpuEvent::LatchMode7Registers => self.registers.latch_mode_7(),
                PpuEvent::RenderLine => {
                    let v_display_size = self.registers.v_display_size.to_lines();
                    let is_active_scanline = (1..=v_display_size).contains(&self.state.scanline);
                    if is_active_scanline {
                        self.render_current_line(0);
                    }
                }
                PpuEvent::FinishSpriteEvaluation => {
                    self.progress_sprite_evaluation(sprites::SPRITE_EVALUATION_END_DOT);
                }
                PpuEvent::StartSpriteTileFetch => self.begin_sprite_tile_fetch(),
                PpuEvent::None => {}
            }

            self.state.dot_event_idx += 1;
        }
    }

    fn render_current_line(&mut self, from_pixel: u16) {
        let scanline = self.state.scanline;
        self.state.last_rendered_scanline = Some(scanline);

        if self.registers.forced_blanking {
            // Forced blanking always draws black
            let from_pixel = if self.state.h_hi_res_frame { 2 * from_pixel } else { from_pixel };
            let screen_width = self.state.frame_screen_width();

            if self.state.v_hi_res_frame {
                for y in [2 * scanline - 1, 2 * scanline] {
                    for pixel in from_pixel..screen_width as u16 {
                        self.set_in_frame_buffer(y, pixel, Color::BLACK);
                    }
                }
            } else {
                for pixel in from_pixel..screen_width as u16 {
                    self.set_in_frame_buffer(scanline, pixel, Color::BLACK);
                }
            }

            return;
        }

        let hi_res_mode = if self.registers.bg_mode.is_hi_res() {
            HiResMode::True
        } else if self.registers.pseudo_h_hi_res {
            HiResMode::Pseudo
        } else {
            HiResMode::None
        };

        let bg_from_pixel =
            if hi_res_mode == HiResMode::True { 2 * from_pixel } else { from_pixel };
        let screen_from_pixel = if hi_res_mode.is_hi_res() { 2 * from_pixel } else { from_pixel };
        let v_hi_res = hi_res_mode == HiResMode::True && self.registers.interlaced;

        if self.state.v_hi_res_frame && v_hi_res {
            // Vertical hi-res, 448px
            if self.deinterlace || !self.state.odd_frame {
                // Render even line
                self.render_bg_layers_to_buffer(2 * scanline - 1, hi_res_mode, bg_from_pixel);
                self.render_scanline(2 * scanline - 1, hi_res_mode, screen_from_pixel);
            }

            if self.deinterlace || self.state.odd_frame {
                // Render odd line
                if self.deinterlace && self.registers.pseudo_obj_hi_res {
                    self.render_sprite_tiles();
                }

                self.render_bg_layers_to_buffer(2 * scanline, hi_res_mode, bg_from_pixel);
                self.render_scanline(2 * scanline, hi_res_mode, screen_from_pixel);
            }
        } else if !self.state.v_hi_res_frame && v_hi_res {
            // Probably should never happen - PPU is in 448px mode but interlacing was disabled at
            // start of frame
            let y = if self.state.odd_frame { 2 * scanline } else { 2 * scanline - 1 };

            self.render_bg_layers_to_buffer(y, hi_res_mode, bg_from_pixel);
            self.render_scanline(scanline, hi_res_mode, screen_from_pixel);
        } else if self.state.v_hi_res_frame {
            // Interlacing was enabled at start of frame - duplicate lines
            if !self.deinterlace {
                let odd_frame: u16 = self.state.odd_frame.into();
                self.render_bg_layers_to_buffer(scanline, hi_res_mode, screen_from_pixel);
                self.render_scanline(2 * scanline - 1 + odd_frame, hi_res_mode, screen_from_pixel);
            } else {
                // Render even line
                self.render_bg_layers_to_buffer(scanline, hi_res_mode, bg_from_pixel);
                self.render_scanline(2 * scanline - 1, hi_res_mode, screen_from_pixel);

                // Duplicate to odd line, re-rendering OBJs if necessary
                if self.registers.pseudo_obj_hi_res {
                    self.render_sprite_tiles();
                    self.render_scanline(2 * scanline, hi_res_mode, screen_from_pixel);
                } else {
                    self.duplicate_line(
                        (2 * scanline - 1).into(),
                        (2 * scanline).into(),
                        screen_from_pixel.into(),
                    );
                }
            }
        } else {
            // Interlacing is disabled, render normally
            self.render_bg_layers_to_buffer(scanline, hi_res_mode, bg_from_pixel);
            self.render_scanline(scanline, hi_res_mode, screen_from_pixel);
        }
    }

    fn render_bg_layers_to_buffer(
        &mut self,
        scanline: u16,
        hi_res_mode: HiResMode,
        from_pixel: u16,
    ) {
        let mode = self.registers.bg_mode;

        if mode == BgMode::Seven {
            self.render_mode_7_to_buffer(scanline, from_pixel);
            return;
        }

        let bg1_enabled = self.registers.main_bg_enabled[0] || self.registers.sub_bg_enabled[0];
        let bg2_enabled = mode.bg2_enabled(false)
            && (self.registers.main_bg_enabled[1] || self.registers.sub_bg_enabled[1]);
        let bg3_enabled = mode.bg3_enabled()
            && (self.registers.main_bg_enabled[2] || self.registers.sub_bg_enabled[2]);
        let bg4_enabled = mode.bg4_enabled()
            && (self.registers.main_bg_enabled[3] || self.registers.sub_bg_enabled[3]);

        if mode.is_offset_per_tile() && (bg1_enabled || bg2_enabled) {
            // Populate offset-per-tile buffers before rendering BG1 or BG2
            self.populate_offset_per_tile_buffers();
        }

        if bg1_enabled {
            self.render_bg_to_buffer(0, scanline, hi_res_mode, from_pixel);
        }

        if bg2_enabled {
            self.render_bg_to_buffer(1, scanline, hi_res_mode, from_pixel);
        }

        if bg3_enabled {
            self.render_bg_to_buffer(2, scanline, hi_res_mode, from_pixel);
        }

        if bg4_enabled {
            self.render_bg_to_buffer(3, scanline, hi_res_mode, from_pixel);
        }
    }

    fn render_bg_to_buffer(
        &mut self,
        bg: usize,
        scanline: u16,
        hi_res_mode: HiResMode,
        from_pixel: u16,
    ) {
        let screen_width =
            if hi_res_mode == HiResMode::True { HIRES_SCREEN_WIDTH } else { NORMAL_SCREEN_WIDTH };

        let mode = self.registers.bg_mode;
        let bpp = match bg {
            0 => mode.bg1_bpp(),
            1 => mode.bg2_bpp(),
            2 => BitsPerPixel::BG3,
            3 => BitsPerPixel::BG4,
            _ => panic!("invalid BG layer: {bg}"),
        };

        let bg_h_scroll = self.registers.bg_h_scroll[bg];
        let bg_v_scroll = self.registers.bg_v_scroll[bg];

        let mut bg_map_entry = CachedBgMapEntry::default();

        for pixel_idx in from_pixel..screen_width as u16 {
            // Apply mosaic if enabled
            let (base_y, mosaic_x) = self.apply_mosaic(bg, scanline, pixel_idx, hi_res_mode);
            if mosaic_x != pixel_idx {
                // Mosaic is enabled and this is not the far left pixel; copy last pixel and move on
                self.buffers.bg_pixels[bg][pixel_idx as usize] =
                    self.buffers.bg_pixels[bg][(pixel_idx - 1) as usize];
                continue;
            }

            // Account for offset-per-tile if in mode 2/4/6
            let (mut h_scroll, v_scroll) = if mode.is_offset_per_tile() {
                let buffer_idx = match hi_res_mode {
                    HiResMode::True => (pixel_idx / 2) as usize,
                    HiResMode::None | HiResMode::Pseudo => pixel_idx as usize,
                };
                (
                    self.buffers.offset_per_tile_h_scroll[bg][buffer_idx],
                    self.buffers.offset_per_tile_v_scroll[bg][buffer_idx],
                )
            } else {
                (bg_h_scroll, bg_v_scroll)
            };

            if hi_res_mode == HiResMode::True {
                // H scroll values are effectively doubled in mode 5/6
                h_scroll <<= 1;
            }

            // Apply scroll values
            let x = pixel_idx.wrapping_add(h_scroll);
            let y = base_y.wrapping_add(v_scroll);

            // Retrieve background map entry (if different from the last pixel's)
            // BG tiles can be larger than 8x8 but that doesn't matter here since this is just to
            // avoid doing a BG map lookup on every pixel
            if x / 8 != bg_map_entry.map_x || y / 8 != bg_map_entry.map_y {
                let raw_entry = get_bg_map_entry(&self.vram, &self.registers, bg, x, y);
                bg_map_entry = CachedBgMapEntry {
                    map_x: x / 8,
                    map_y: y / 8,
                    tile_number: raw_entry & 0x3FF,
                    palette: ((raw_entry >> 10) & 0x07) as u8,
                    priority: raw_entry.bit(13),
                    x_flip: raw_entry.bit(14),
                    y_flip: raw_entry.bit(15),
                };
            }

            // Retrieve tile bytes from VRAM
            let tile_data = get_bg_tile(
                &self.vram,
                &self.registers,
                bg,
                x,
                y,
                bpp,
                bg_map_entry.tile_number,
                bg_map_entry.x_flip,
                bg_map_entry.y_flip,
            );

            let tile_row = if bg_map_entry.y_flip { 7 - (y % 8) } else { y % 8 };
            let tile_col = if bg_map_entry.x_flip { 7 - (x % 8) } else { x % 8 };
            let bit_index = (7 - tile_col) as u8;

            // Parse color bits out of bitplane tile data
            let mut color = 0_u8;
            for plane in (0..bpp.bitplanes()).step_by(2) {
                let word_index = tile_row as usize + 4 * plane;
                let word = tile_data[word_index];

                color |= u8::from(word.bit(bit_index)) << plane;
                color |= u8::from(word.bit(bit_index + 8)) << (plane + 1);
            }

            let pixel = Pixel {
                palette: bg_map_entry.palette,
                color,
                priority: bg_map_entry.priority.into(),
            };
            self.buffers.bg_pixels[bg][pixel_idx as usize] = pixel;
        }
    }

    fn render_mode_7_to_buffer(&mut self, scanline: u16, from_pixel: u16) {
        // Mode 7 tile map is always 128x128
        const TILE_MAP_SIZE_PIXELS: i32 = 128 * 8;

        fn clip_i11(value: i32) -> i32 {
            // This is intentionally not equivalent to `(value << (32 - 11)) >> (32 - 11)`
            //
            // Some games depend on using the magnitude from the lowest 10 bits but using the sign
            // of the full i14 value instead of clipping to i11.
            // e.g. Kaite Tsukutte Asoberu Dezaemon, title screen animation
            // e.g. Tiny Toon Adventures: Wacky Sports Challenge, the birdman event
            let magnitude = value & 0x03FF;
            let sign = (value >> 31) & !0x03FF;
            magnitude | sign
        }

        fn sign_extend_i13(value: u16) -> i32 {
            (((value as i16) << 3) >> 3).into()
        }

        // Several Mode 7 intermediate results truncate the lowest 6 bits
        fn truncate_intermediate(value: i32) -> i32 {
            value & !0x3F
        }

        // Affine transformation parameters (fixed point, 1/256 pixel units)
        let m7a: i32 = (self.registers.mode_7_latched.parameter_a as i16).into();
        let m7b: i32 = (self.registers.mode_7_latched.parameter_b as i16).into();
        let m7c: i32 = (self.registers.mode_7_latched.parameter_c as i16).into();
        let m7d: i32 = (self.registers.mode_7_latched.parameter_d as i16).into();

        // Center of rotation
        let m7x = sign_extend_i13(self.registers.mode_7_latched.center_x);
        let m7y = sign_extend_i13(self.registers.mode_7_latched.center_y);

        let h_scroll = sign_extend_i13(self.registers.mode_7_latched.h_scroll);
        let v_scroll = sign_extend_i13(self.registers.mode_7_latched.v_scroll);

        let h_flip = self.registers.mode_7_latched.h_flip;
        let v_flip = self.registers.mode_7_latched.v_flip;

        let oob_behavior = self.registers.mode_7_latched.oob_behavior;

        for pixel in from_pixel..NORMAL_SCREEN_WIDTH as u16 {
            let (base_y, mosaic_x) = self.apply_mosaic(0, scanline, pixel, HiResMode::None);
            if mosaic_x != pixel {
                // Copy last pixel and move on
                self.buffers.bg_pixels[0][pixel as usize] =
                    self.buffers.bg_pixels[0][(pixel - 1) as usize];
                continue;
            }

            let screen_x: i32 = (if h_flip { 255 - pixel } else { pixel }).into();
            let screen_y: i32 = (if v_flip { 255 - base_y } else { base_y }).into();

            // Perform the following matrix transformation (logically):
            //   [ vram_x ] = [ m7a  m7b ] * [ screen_x + m7hofs - m7x ] + [ m7x ]
            //   [ vram_y ]   [ m7c  m7d ]   [ screen_y + m7vofs - m7y ]   [ m7y ]
            // m7a/m7b/m7c/m7d are in 1/256 pixel units, so the multiplication result is also in
            // 1/256 pixel units, and m7x/m7y need to be converted for the addition
            let scrolled_center_x = clip_i11(h_scroll - m7x);
            let scrolled_center_y = clip_i11(v_scroll - m7y);

            let mut tile_map_x = truncate_intermediate(m7a * scrolled_center_x)
                + m7a * screen_x
                + truncate_intermediate(m7b * scrolled_center_y)
                + truncate_intermediate(m7b * screen_y)
                + (m7x << 8);
            let mut tile_map_y = truncate_intermediate(m7c * scrolled_center_x)
                + m7c * screen_x
                + truncate_intermediate(m7d * scrolled_center_y)
                + truncate_intermediate(m7d * screen_y)
                + (m7y << 8);

            // Convert back from 1/256 pixel units to pixel units
            tile_map_x >>= 8;
            tile_map_y >>= 8;

            let mut force_tile_0 = false;
            if tile_map_x < 0
                || tile_map_y < 0
                || tile_map_x >= TILE_MAP_SIZE_PIXELS
                || tile_map_y >= TILE_MAP_SIZE_PIXELS
            {
                match oob_behavior {
                    Mode7OobBehavior::Wrap => {
                        tile_map_x &= TILE_MAP_SIZE_PIXELS - 1;
                        tile_map_y &= TILE_MAP_SIZE_PIXELS - 1;
                    }
                    Mode7OobBehavior::Transparent => {
                        self.buffers.bg_pixels[0][pixel as usize] = Pixel::TRANSPARENT;
                        continue;
                    }
                    Mode7OobBehavior::Tile0 => {
                        tile_map_x &= 0x07;
                        tile_map_y &= 0x07;
                        force_tile_0 = true;
                    }
                }
            }

            let tile_number = if force_tile_0 {
                0
            } else {
                // Mode 7 tile map is always located at $0000
                let tile_map_row = tile_map_y / 8;
                let tile_map_col = tile_map_x / 8;
                let tile_map_addr = tile_map_row * TILE_MAP_SIZE_PIXELS / 8 + tile_map_col;
                self.vram[tile_map_addr as usize] & 0x00FF
            };

            let tile_row = (tile_map_y % 8) as u16;
            let tile_col = (tile_map_x % 8) as u16;
            let pixel_addr = 64 * tile_number + 8 * tile_row + tile_col;
            let color = self.vram[pixel_addr as usize].msb();

            self.buffers.bg_pixels[0][pixel as usize] = Pixel { palette: 0, color, priority: 0 };
        }
    }

    fn populate_offset_per_tile_buffers(&mut self) {
        // Offset-per-tile does not apply to first visible BG1/BG2 tile (1-8 pixels)
        for bg in 0..2 {
            let bg_h_scroll = self.registers.bg_h_scroll[bg];
            let bg_v_scroll = self.registers.bg_v_scroll[bg];

            for pixel in 0..(8 - (bg_h_scroll & 7)) {
                self.buffers.offset_per_tile_h_scroll[bg][pixel as usize] = bg_h_scroll;
                self.buffers.offset_per_tile_v_scroll[bg][pixel as usize] = bg_v_scroll;
            }
        }

        let mode = self.registers.bg_mode;
        let bg3_h_scroll = self.registers.bg_h_scroll[2];
        let bg3_v_scroll = self.registers.bg_v_scroll[2];

        // Up to 33 8x8 tiles can be visible when fine H scrolling is used
        for tile_idx in 0..33_u16 {
            // Lowest 3 bits of BG3 H scroll do not apply in offset-per-tile
            let bg3_x = (8 * tile_idx).wrapping_add(bg3_h_scroll & !7);

            let (h_offset_entry, v_offset_entry) = match mode {
                BgMode::Four => {
                    // In Mode 4, instead of loading both map entries, the PPU uses the highest bit
                    // of the first entry to determine whether to apply offset to H or V
                    let bg3_entry =
                        get_bg_map_entry(&self.vram, &self.registers, 2, bg3_x, bg3_v_scroll);
                    if bg3_entry.bit(15) {
                        // Apply to V scroll
                        (0, bg3_entry)
                    } else {
                        // Apply to H scroll
                        (bg3_entry, 0)
                    }
                }
                _ => {
                    let h_offset_entry =
                        get_bg_map_entry(&self.vram, &self.registers, 2, bg3_x, bg3_v_scroll);
                    let v_offset_entry = get_bg_map_entry(
                        &self.vram,
                        &self.registers,
                        2,
                        bg3_x,
                        bg3_v_scroll.wrapping_add(8),
                    );
                    (h_offset_entry, v_offset_entry)
                }
            };

            for bg in 0..2 {
                let bg_h_scroll = self.registers.bg_h_scroll[bg];
                let bg_v_scroll = self.registers.bg_v_scroll[bg];

                let use_offset_bit = [13, 14][bg];
                let h_scroll = if h_offset_entry.bit(use_offset_bit) {
                    // TODO is this right for fine H scroll?
                    (h_offset_entry & 0x03FF & !7) | (bg_h_scroll & 7)
                } else {
                    bg_h_scroll
                };
                let v_scroll = if v_offset_entry.bit(use_offset_bit) {
                    v_offset_entry & 0x03FF
                } else {
                    bg_v_scroll
                };

                // BG3 tile N is used as offsets for BG1/BG2 visible tile N+1
                let base_x = (8 * (tile_idx + 1)).wrapping_sub(bg_h_scroll & 7);
                for dx in 0..8 {
                    let pixel = base_x + dx;
                    if pixel >= NORMAL_SCREEN_WIDTH as u16 {
                        break;
                    }

                    self.buffers.offset_per_tile_h_scroll[bg][pixel as usize] = h_scroll;
                    self.buffers.offset_per_tile_v_scroll[bg][pixel as usize] = v_scroll;
                }
            }
        }
    }

    fn render_scanline(&mut self, scanline: u16, hi_res_mode: HiResMode, screen_from_pixel: u16) {
        // Main screen is always rendered
        self.render_screen_pixels(Screen::Main, hi_res_mode);

        // Sub screen is rendered if in a hi-res mode (which causes even pixels to display the sub screen) OR color math
        // is enabled for at least one layer and the sub screen is not forced to the subbackdrop color.
        if hi_res_mode.is_hi_res()
            || (self.registers.sub_bg_obj_enabled
                && self.registers.color_math_enabled_for_any_layer())
        {
            self.render_screen_pixels(Screen::Sub, hi_res_mode);
        }

        let screen_width =
            if hi_res_mode.is_hi_res() { HIRES_SCREEN_WIDTH } else { NORMAL_SCREEN_WIDTH };

        let brightness = self.registers.brightness;
        let main_backdrop_pixel =
            RenderedPixel { palette: 0, color: self.cgram[0], layer: Layer::Backdrop };
        let sub_backdrop_color = self.registers.sub_backdrop_color;

        for pixel in screen_from_pixel..screen_width as u16 {
            let screen_x = match hi_res_mode {
                HiResMode::None => pixel,
                HiResMode::Pseudo | HiResMode::True => pixel / 2,
            };

            let mut main_screen_pixel = if hi_res_mode.is_hi_res() && !pixel.bit(0) {
                // Even pixels draw the sub screen in hi-res mode
                // If all sub screen pixels are transparent, draw the main backdrop color
                let sub_pixel = self.buffers.sub_screen_rendered_pixels[screen_x as usize];
                if sub_pixel.layer == Layer::Backdrop { main_backdrop_pixel } else { sub_pixel }
            } else {
                self.buffers.main_screen_rendered_pixels[screen_x as usize]
            };

            // Check if inside the color window (used for clipping and color math)
            let in_color_window = self.registers.in_math_window(screen_x);

            let force_main_screen_black =
                self.registers.force_main_screen_black.enabled(in_color_window);
            if force_main_screen_black {
                // Pixel is clipped; force color to 0 (black)
                main_screen_pixel.color = 0;
            }

            // Check if color math is enabled globally and for this layer
            let color_math_enabled_global =
                self.registers.color_math_enabled.enabled(in_color_window);

            let color_math_enabled_layer = match main_screen_pixel.layer {
                Layer::Bg1 => self.registers.bg_color_math_enabled[0],
                Layer::Bg2 => self.registers.bg_color_math_enabled[1],
                Layer::Bg3 => self.registers.bg_color_math_enabled[2],
                Layer::Bg4 => self.registers.bg_color_math_enabled[3],
                Layer::Obj => {
                    self.registers.obj_color_math_enabled && main_screen_pixel.palette >= 4
                }
                Layer::Backdrop => self.registers.backdrop_color_math_enabled,
            };

            let snes_color = if color_math_enabled_global && color_math_enabled_layer {
                // Find the frontmost sub screen pixel
                let (sub_screen_color, sub_transparent) = if self.registers.sub_bg_obj_enabled {
                    let pixel = self.buffers.sub_screen_rendered_pixels[screen_x as usize];
                    (pixel.color, pixel.layer == Layer::Backdrop)
                } else {
                    (sub_backdrop_color, false)
                };

                // Apply color math to the main and sub screen pixels
                // Division only applies if the main pixel was not clipped and the sub pixel is not
                // transparent
                let divide = self.registers.color_math_divide_enabled
                    && !force_main_screen_black
                    && !sub_transparent;
                self.registers.color_math_operation.apply(
                    main_screen_pixel.color,
                    sub_screen_color,
                    divide,
                )
            } else {
                main_screen_pixel.color
            };

            let final_color = convert_snes_color(snes_color, brightness);

            if self.state.h_hi_res_frame && !hi_res_mode.is_hi_res() {
                // Hi-res mode is not currently enabled, but it was enabled earlier in the frame;
                // draw in 512px
                self.set_in_frame_buffer(scanline, 2 * pixel, final_color);
                self.set_in_frame_buffer(scanline, 2 * pixel + 1, final_color);
            } else {
                self.set_in_frame_buffer(scanline, pixel, final_color);
            }
        }
    }

    fn render_screen_pixels(&mut self, screen: Screen, hi_res_mode: HiResMode) {
        #[inline(always)]
        fn apply_screen_shift(x: usize, shift: i32, offset: usize) -> u16 {
            ((x << shift) | offset) as u16
        }

        let (
            screen_pixels,
            screen_rendered_pixels,
            bg_enabled,
            bg_disabled_in_window,
            obj_enabled,
            obj_disabled_in_window,
            backdrop_color,
        ) = match screen {
            Screen::Main => (
                &mut self.buffers.main_screen_pixels,
                &mut self.buffers.main_screen_rendered_pixels,
                self.registers.main_bg_enabled,
                self.registers.main_bg_disabled_in_window,
                self.registers.main_obj_enabled,
                self.registers.main_obj_disabled_in_window,
                self.cgram[0],
            ),
            Screen::Sub => (
                &mut self.buffers.sub_screen_pixels,
                &mut self.buffers.sub_screen_rendered_pixels,
                self.registers.sub_bg_enabled,
                self.registers.sub_bg_disabled_in_window,
                self.registers.sub_obj_enabled,
                self.registers.sub_obj_disabled_in_window,
                self.registers.sub_backdrop_color,
            ),
        };

        screen_pixels.fill(PriorityResolver::new());

        let screen_x_shift = match hi_res_mode {
            HiResMode::True => 1,
            HiResMode::None | HiResMode::Pseudo => 0,
        };
        let screen_x_offset = match (hi_res_mode, screen) {
            (HiResMode::None | HiResMode::Pseudo, _) | (HiResMode::True, Screen::Sub) => 0,
            (HiResMode::True, Screen::Main) => 1,
        };

        let mode = self.registers.bg_mode;
        let is_mode_0_or_1 = matches!(mode, BgMode::Zero | BgMode::One);

        // OBJ layer (enabled in all modes)
        if obj_enabled {
            for (x, priority_resolver) in screen_pixels.iter_mut().enumerate() {
                if obj_disabled_in_window {
                    let screen_x = apply_screen_shift(x, screen_x_shift, screen_x_offset);
                    let obj_in_window = self.registers.obj_in_window(screen_x);
                    if obj_in_window {
                        continue;
                    }
                }

                let obj_pixel = self.buffers.obj_pixels[x];
                if !obj_pixel.is_transparent() {
                    priority_resolver.add_obj(obj_pixel, is_mode_0_or_1);
                }
            }
        }

        // BG1 layer (enabled in all modes)
        if bg_enabled[0] {
            for (x, priority_resolver) in screen_pixels.iter_mut().enumerate() {
                let screen_x = apply_screen_shift(x, screen_x_shift, screen_x_offset);

                if bg_disabled_in_window[0] {
                    let bg1_in_window = self.registers.bg_in_window(0, screen_x);
                    if bg1_in_window {
                        continue;
                    }
                }

                let bg1_pixel = self.buffers.bg_pixels[0][screen_x as usize];
                if !bg1_pixel.is_transparent() {
                    priority_resolver.add_bg1(bg1_pixel, is_mode_0_or_1);
                }
            }
        }

        // BG2 layer (enabled in all modes except 6 and 7)
        if mode.bg2_enabled(self.registers.extbg_enabled) && bg_enabled[1] {
            for (x, priority_resolver) in screen_pixels.iter_mut().enumerate() {
                let screen_x = apply_screen_shift(x, screen_x_shift, screen_x_offset);

                if bg_disabled_in_window[1] {
                    let bg2_in_window = self.registers.bg_in_window(1, screen_x);
                    if bg2_in_window {
                        continue;
                    }
                }

                match mode {
                    BgMode::Seven => {
                        // When EXTBG is enabled in Mode 7, BG1 pixels are duplicated into BG2 but
                        // use the highest color bit as priority
                        let bg1_pixel = self.buffers.bg_pixels[0][screen_x as usize];
                        let bg2_pixel_color = bg1_pixel.color & 0x7F;
                        if bg2_pixel_color == 0 {
                            // Transparent
                            continue;
                        }

                        let bg2_priority = bg1_pixel.color >> 7;
                        priority_resolver.add_bg2(
                            Pixel { priority: bg2_priority, color: bg2_pixel_color, palette: 0 },
                            false,
                        );
                    }
                    _ => {
                        let bg2_pixel = self.buffers.bg_pixels[1][screen_x as usize];
                        if !bg2_pixel.is_transparent() {
                            priority_resolver.add_bg2(bg2_pixel, is_mode_0_or_1);
                        }
                    }
                }
            }
        }

        // BG3 layer (enabled in modes 0 and 1)
        if mode.bg3_enabled() && bg_enabled[2] {
            let bg3_high_priority = mode == BgMode::One && self.registers.mode_1_bg3_priority;

            for (x, priority_resolver) in screen_pixels.iter_mut().enumerate() {
                let screen_x = apply_screen_shift(x, screen_x_shift, screen_x_offset);

                if bg_disabled_in_window[2] {
                    let bg3_in_window = self.registers.bg_in_window(2, screen_x);
                    if bg3_in_window {
                        continue;
                    }
                }

                let bg3_pixel = self.buffers.bg_pixels[2][screen_x as usize];
                if !bg3_pixel.is_transparent() {
                    priority_resolver.add_bg3(bg3_pixel, bg3_high_priority);
                }
            }
        }

        // BG4 layer (enabled in mode 0 only)
        if mode.bg4_enabled() && bg_enabled[3] {
            for (x, priority_resolver) in screen_pixels.iter_mut().enumerate() {
                let screen_x = apply_screen_shift(x, screen_x_shift, screen_x_offset);

                if bg_disabled_in_window[3] {
                    let bg4_in_window = self.registers.bg_in_window(3, screen_x);
                    if bg4_in_window {
                        continue;
                    }
                }

                let bg4_pixel = self.buffers.bg_pixels[3][screen_x as usize];
                if !bg4_pixel.is_transparent() {
                    priority_resolver.add_bg4(bg4_pixel);
                }
            }
        }

        let backdrop_pixel =
            RenderedPixel { color: backdrop_color, palette: 0, layer: Layer::Backdrop };
        for (priority_resolver, rendered_pixel) in
            screen_pixels.iter_mut().zip(screen_rendered_pixels)
        {
            *rendered_pixel = priority_resolver.get().map_or(backdrop_pixel, |(pixel, layer)| {
                let color = resolve_pixel_color(
                    &self.cgram,
                    layer,
                    mode,
                    self.registers.direct_color_mode_enabled,
                    pixel.palette,
                    pixel.color,
                );
                RenderedPixel { color, palette: pixel.palette, layer }
            });
        }
    }

    fn apply_mosaic(
        &self,
        bg: usize,
        scanline: u16,
        pixel: u16,
        hi_res_mode: HiResMode,
    ) -> (u16, u16) {
        let mosaic_size = self.registers.mosaic_size;
        let mosaic_enabled = self.registers.bg_mosaic_enabled[bg];
        if !mosaic_enabled {
            return (scanline, pixel);
        }

        let mosaic_line = self.state.v_mosaic_filter(scanline, hi_res_mode);

        // Mosaic size of N fills each (N+1)x(N+1) square with the pixel in the top-left corner
        // Mosaic sizes are doubled in true hi-res mode
        let mosaic_size: u16 = (mosaic_size + 1).into();
        let mosaic_width = match hi_res_mode {
            HiResMode::True => 2 * mosaic_size,
            _ => mosaic_size,
        };

        (mosaic_line, pixel / mosaic_width * mosaic_width)
    }

    fn enter_hi_res_mode(&mut self) {
        if !self.vblank_flag() && !self.state.h_hi_res_frame {
            // Hi-res mode enabled mid-frame; redraw previously rendered scanlines to 512x224 in-place
            if let Some(last_rendered_scanline) = self.state.last_rendered_scanline {
                let last_copy_line = if self.state.v_hi_res_frame {
                    2 * last_rendered_scanline
                } else {
                    last_rendered_scanline
                };
                self.frame_buffer_h256_to_h512(last_copy_line);
            }
        }

        self.state.h_hi_res_frame = true;
    }

    fn frame_buffer_h256_to_h512(&mut self, to_scanline: u16) {
        for scanline in (1..=to_scanline).rev() {
            let src_line_addr = 256 * u32::from(scanline - 1);
            let dest_line_addr = 512 * u32::from(scanline - 1);
            for pixel in (0..256).rev() {
                let color = self.frame_buffer[(src_line_addr + pixel) as usize];
                self.frame_buffer[(dest_line_addr + 2 * pixel) as usize] = color;
                self.frame_buffer[(dest_line_addr + 2 * pixel + 1) as usize] = color;
            }
        }
    }

    fn set_in_frame_buffer(&mut self, scanline: u16, pixel: u16, color: Color) {
        let screen_width = self.state.frame_screen_width();
        let index = u32::from(scanline - 1) * screen_width + u32::from(pixel);
        self.frame_buffer[index as usize] = color;
    }

    fn duplicate_line(&mut self, from_line: u32, to_line: u32, from_pixel: u32) {
        let screen_width = self.state.frame_screen_width();
        let from_row_addr = screen_width * (from_line - 1);
        let to_row_addr = screen_width * (to_line - 1);
        for pixel in from_pixel..screen_width {
            self.frame_buffer[(to_row_addr + pixel) as usize] =
                self.frame_buffer[(from_row_addr + pixel) as usize];
        }
    }

    fn fix_interlaced_frame_buffer(&mut self) {
        log::debug!("Just entered interlaced mode; rewriting frame buffer");

        let v_display_size = self.registers.v_display_size.to_lines();

        // Check if changed from H256px to H512px
        let next_frame_h_hi_res =
            self.registers.bg_mode.is_hi_res() || self.registers.pseudo_h_hi_res;
        if !self.state.h_hi_res_frame && next_frame_h_hi_res {
            log::debug!("Expanding previous frame from H256px to H512px");
            self.frame_buffer_h256_to_h512(v_display_size);
        }

        log::debug!(
            "Expanding previous frame from V{}px to V{}px; screen width H{}px",
            v_display_size,
            2 * v_display_size,
            if next_frame_h_hi_res { HIRES_SCREEN_WIDTH } else { NORMAL_SCREEN_WIDTH }
        );

        // Duplicate lines to expand frame buffer from V224px to V448px (or V239px to V478px)
        let screen_width = if next_frame_h_hi_res {
            HIRES_SCREEN_WIDTH as u32
        } else {
            NORMAL_SCREEN_WIDTH as u32
        };
        for scanline in (1..=u32::from(v_display_size)).rev() {
            let even_line = 2 * scanline - 1;
            let odd_line = 2 * scanline;
            for pixel in 0..screen_width {
                let color = self.frame_buffer[(scanline * screen_width + pixel) as usize];
                self.frame_buffer[(even_line * screen_width + pixel) as usize] = color;
                self.frame_buffer[(odd_line * screen_width + pixel) as usize] = color;
            }
        }
    }

    pub fn scanlines_per_frame(&self) -> u16 {
        match self.timing_mode {
            TimingMode::Ntsc => 262,
            TimingMode::Pal => 312,
        }
    }

    fn scanline_length(&self, scanline: u16) -> ScanlineLength {
        // In NTSC progressive mode, line 240 is short every other frame (1360 mclks)
        // In PAL interlaced mode, line 311 is long every other frame (1368 mclks)
        // All other lines are 1364 mclks
        if !self.state.odd_frame {
            return ScanlineLength::Normal;
        }

        match (scanline, self.timing_mode, self.registers.interlaced) {
            (240, TimingMode::Ntsc, false) => ScanlineLength::Short,
            (311, TimingMode::Pal, true) => ScanlineLength::Long,
            _ => ScanlineLength::Normal,
        }
    }

    fn in_active_display(&self, scanline: u16) -> bool {
        !self.registers.forced_blanking && scanline <= self.registers.v_display_size.to_lines()
    }

    pub fn vblank_flag(&self) -> bool {
        self.state.scanline > self.registers.v_display_size.to_lines()
    }

    pub fn hblank_flag(&self) -> bool {
        self.state.scanline_master_cycles < 4 || self.state.scanline_master_cycles >= 1096
    }

    pub fn scanline(&self) -> u16 {
        self.state.scanline
    }

    #[cfg(test)]
    pub fn set_scanline(&mut self, scanline: u16) {
        assert!(
            scanline < self.scanlines_per_frame(),
            "{scanline} must be less than {}",
            self.scanlines_per_frame()
        );
        self.state.scanline = scanline;
    }

    pub fn is_first_vblank_scanline(&self) -> bool {
        self.state.scanline == self.registers.v_display_size.to_lines() + 1
    }

    pub fn scanline_master_cycles(&self) -> u64 {
        self.state.scanline_master_cycles
    }

    #[cfg(test)]
    pub fn set_scanline_master_cycles(&mut self, scanline_master_cycles: u64) {
        assert!(
            scanline_master_cycles < self.state.current_line_length.scanline_mclks(),
            "{scanline_master_cycles} must be less than {}",
            self.state.current_line_length.scanline_mclks()
        );
        self.state.scanline_master_cycles = scanline_master_cycles;
    }

    // There is a 10 mclk / 2.5 dot delay on raising IRQ in response to VTIME/HTIME matches
    const IRQ_OFFSET_MCLKS: u64 = 10;

    pub fn vtime_for_irq(&self) -> u16 {
        if self.state.scanline_master_cycles >= Self::IRQ_OFFSET_MCLKS {
            self.state.scanline
        } else if self.state.scanline == 0 {
            self.scanlines_per_frame() - 1
        } else {
            self.state.scanline - 1
        }
    }

    pub fn htime_for_irq(&self) -> u16 {
        let scanline_mclks = if self.state.scanline_master_cycles >= Self::IRQ_OFFSET_MCLKS {
            self.state.scanline_master_cycles - Self::IRQ_OFFSET_MCLKS
        } else {
            self.state.prev_line_length.scanline_mclks()
                - (Self::IRQ_OFFSET_MCLKS - self.state.scanline_master_cycles)
        };

        // CPU HTIME counter does not account for some dots being shorter/longer; assumes all dots
        // are 4 mclks
        (scanline_mclks / 4) as u16
    }

    pub fn previous_line_max_htime(&self) -> u16 {
        (self.state.prev_line_length.scanline_mclks() / 4) as u16
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.as_ref()
    }

    pub fn frame_size(&self) -> FrameSize {
        let screen_width = self.state.frame_screen_width();

        let mut screen_height = self.registers.v_display_size.to_lines();
        if self.state.v_hi_res_frame {
            screen_height *= 2;
        }

        FrameSize { width: screen_width, height: screen_height.into() }
    }

    pub fn read_port(&mut self, address: u32) -> Option<u8> {
        log::trace!("Read PPU register: {address:06X}");

        let address_lsb = address & 0xFF;
        let value = match address_lsb {
            0x34 => self.registers.read_mpyl(),
            0x35 => self.registers.read_mpym(),
            0x36 => self.registers.read_mpyh(),
            0x37 => {
                // SLHV: Latch H/V counter
                let h_counter = (self.state.scanline_master_cycles >> 2) as u16;
                let v_counter = self.state.scanline;
                self.registers.read_slhv(h_counter, v_counter);

                // Reading from this address returns CPU open bus
                return None;
            }
            0x38 => {
                // RDOAM: OAM data port, read
                self.read_oam_data_port()
            }
            0x39 => {
                // RDVRAML: VRAM data port, read, low byte
                self.read_vram_data_port_low()
            }
            0x3A => {
                // RDVRAMH: VRAM data port, read, high byte
                self.read_vram_data_port_high()
            }
            0x3B => {
                // RDCGRAM: CGRAM data port, read
                self.read_cgram_data_port()
            }
            0x3C => self.registers.read_ophct(self.state.ppu2_open_bus),
            0x3D => self.registers.read_opvct(self.state.ppu2_open_bus),
            0x3E => {
                // STAT77: PPU1 status and version number
                // Version number hardcoded to 1
                // Bit 4 is PPU1 open bus
                (u8::from(self.registers.sprite_pixel_overflow) << 7)
                    | (u8::from(self.registers.sprite_overflow) << 6)
                    | (self.state.ppu1_open_bus & 0x10)
                    | 0x01
            }
            0x3F => {
                // STAT78: PPU2 status and version number
                // Version number hardcoded to 1
                // Bit 5 is PPU2 open bus
                let value = (u8::from(self.state.odd_frame) << 7)
                    | (u8::from(self.registers.new_hv_latched) << 6)
                    | (self.state.ppu2_open_bus & 0x20)
                    | (u8::from(self.timing_mode == TimingMode::Pal) << 4)
                    | 0x01;

                self.registers.new_hv_latched = false;
                self.registers.reset_hv_counter_flipflops();

                value
            }
            0x04 | 0x05 | 0x06 | 0x08 | 0x09 | 0x0A | 0x14 | 0x15 | 0x16 | 0x18 | 0x19 | 0x1A
            | 0x24 | 0x25 | 0x26 | 0x28 | 0x29 | 0x2A => {
                // PPU1 open bus (all 8 bits)
                self.state.ppu1_open_bus
            }
            _ => {
                // CPU open bus
                return None;
            }
        };

        if (0x34..0x37).contains(&address_lsb)
            || (0x38..0x3B).contains(&address_lsb)
            || address_lsb == 0x3E
        {
            // Reading $2134-$2136, $2138-$213A, or $213E sets PPU1 open bus
            self.state.ppu1_open_bus = value;
        } else if (0x3B..0x3E).contains(&address_lsb) || address_lsb == 0x3F {
            // Reading $213B-$213D or $213F sets PPU2 open bus
            self.state.ppu2_open_bus = value;
        }

        Some(value)
    }

    pub fn write_port(&mut self, address: u32, value: u8) {
        if log::log_enabled!(log::Level::Trace) {
            // Don't log data port writes
            let address = address & 0xFF;
            if address != 0x04 && address != 0x18 && address != 0x19 && address != 0x22 {
                log::trace!(
                    "PPU register write: 21{address:02X} {value:02X} (scanline {} mclk {})",
                    self.state.scanline,
                    self.state.scanline_master_cycles,
                );
            }
        }

        let scanline = self.state.scanline;
        match address & 0xFF {
            0x00 => {
                let new_forced_blanking = value.bit(7);
                if self.registers.forced_blanking != new_forced_blanking {
                    self.sprites_forced_blanking_change(new_forced_blanking);
                }

                self.registers.write_inidisp(value, self.is_first_vblank_scanline());
            }
            0x01 => self.registers.write_obsel(value),
            0x02 => self.registers.write_oamaddl(value, self.in_active_display(scanline)),
            0x03 => self.registers.write_oamaddh(value, self.in_active_display(scanline)),
            0x04 => {
                // OAMDATA: OAM data port (write)
                self.write_oam_data_port(value);
            }
            0x05 => {
                self.registers.write_bgmode(value);
                if self.registers.bg_mode.is_hi_res() {
                    self.enter_hi_res_mode();
                }
            }
            0x06 => self.registers.write_mosaic(value),
            0x07..=0x0A => {
                let bg = ((address + 1) & 0x3) as usize;
                self.registers.write_bg1234sc(bg, value);
            }
            0x0B => self.registers.write_bg1234nba(0, value),
            0x0C => self.registers.write_bg1234nba(2, value),
            0x0D => self.registers.write_bg1hofs(value),
            0x0E => self.registers.write_bg1vofs(value),
            address @ (0x0F | 0x11 | 0x13) => {
                // BG2HOFS/BG3HOFS/BG4HOFS: BG2-4 horizontal scroll
                let bg = (((address - 0x0F) >> 1) + 1) as usize;
                self.registers.write_bg_h_scroll(bg, value);
            }
            address @ (0x10 | 0x12 | 0x14) => {
                // BG2VOFS/BG3VOFS/BG4VOFS: BG2-4 vertical scroll
                let bg = (((address & 0x0F) >> 1) + 1) as usize;
                self.registers.write_bg_v_scroll(bg, value);
            }
            0x15 => self.registers.write_vmain(value),
            0x16 => self.registers.write_vmaddl(value, &self.vram),
            0x17 => self.registers.write_vmaddh(value, &self.vram),
            0x18 => {
                // VMDATAL: VRAM data port (write), low byte
                self.write_vram_data_port_low(value);
            }
            0x19 => {
                // VMDATAH: VRAM data port (write), high byte
                self.write_vram_data_port_high(value);
            }
            0x1A => self.registers.write_m7sel(value),
            0x1B => self.registers.write_m7a(value),
            0x1C => self.registers.write_m7b(value),
            0x1D => self.registers.write_m7c(value),
            0x1E => self.registers.write_m7d(value),
            0x1F => self.registers.write_m7x(value),
            0x20 => self.registers.write_m7y(value),
            0x21 => self.registers.write_cgadd(value),
            0x22 => {
                // CGDATA: CGRAM data port (write)
                self.write_cgram_data_port(value);
            }
            0x23 => self.registers.write_w1234sel(0, value),
            0x24 => self.registers.write_w1234sel(2, value),
            0x25 => self.registers.write_wobjsel(value),
            0x26 => self.registers.write_wh0(value),
            0x27 => self.registers.write_wh1(value),
            0x28 => self.registers.write_wh2(value),
            0x29 => self.registers.write_wh3(value),
            0x2A => self.registers.write_wbglog(value),
            0x2B => self.registers.write_wobjlog(value),
            0x2C => self.registers.write_tm(value),
            0x2D => self.registers.write_ts(value),
            0x2E => self.registers.write_tmw(value),
            0x2F => self.registers.write_tsw(value),
            0x30 => self.registers.write_cgwsel(value),
            0x31 => self.registers.write_cgadsub(value),
            0x32 => self.registers.write_coldata(value),
            0x33 => {
                self.registers.write_setini(value);
                if self.registers.pseudo_h_hi_res {
                    self.enter_hi_res_mode();
                }
            }
            _ => {
                // No other mappings are valid; do nothing
            }
        }
    }

    fn write_vram_data_port_low(&mut self, value: u8) {
        if self.vblank_flag() || self.registers.forced_blanking {
            // VRAM writes only allowed during VBlank and forced blanking
            let vram_addr =
                (self.registers.vram_address_translation.apply(self.registers.vram_address)
                    & VRAM_ADDRESS_MASK) as usize;
            self.vram[vram_addr].set_lsb(value);
        }

        if self.registers.vram_address_increment_mode == VramIncrementMode::Low {
            self.increment_vram_address();
        }
    }

    fn write_vram_data_port_high(&mut self, value: u8) {
        if self.vblank_flag() || self.registers.forced_blanking {
            // VRAM writes only allowed during VBlank and forced blanking
            let vram_addr =
                (self.registers.vram_address_translation.apply(self.registers.vram_address)
                    & VRAM_ADDRESS_MASK) as usize;
            self.vram[vram_addr].set_msb(value);
        }

        if self.registers.vram_address_increment_mode == VramIncrementMode::High {
            self.increment_vram_address();
        }
    }

    fn read_vram_data_port_low(&mut self) -> u8 {
        let vram_byte = self.registers.vram_prefetch_buffer.lsb();

        if self.registers.vram_address_increment_mode == VramIncrementMode::Low {
            // Fill prefetch buffer *before* address increment
            self.fill_vram_prefetch_buffer();
            self.increment_vram_address();
        }

        vram_byte
    }

    fn read_vram_data_port_high(&mut self) -> u8 {
        let vram_byte = self.registers.vram_prefetch_buffer.msb();

        if self.registers.vram_address_increment_mode == VramIncrementMode::High {
            // Fill prefetch buffer *before* address increment
            self.fill_vram_prefetch_buffer();
            self.increment_vram_address();
        }

        vram_byte
    }

    fn increment_vram_address(&mut self) {
        self.registers.vram_address =
            self.registers.vram_address.wrapping_add(self.registers.vram_address_increment_step);
    }

    fn fill_vram_prefetch_buffer(&mut self) {
        let vram_addr = self.registers.vram_address_translation.apply(self.registers.vram_address)
            & VRAM_ADDRESS_MASK;
        self.registers.vram_prefetch_buffer = self.vram[vram_addr as usize];
    }

    fn write_oam_data_port(&mut self, value: u8) {
        if self.in_active_display(self.state.scanline) {
            self.handle_active_display_oam_write(value);
            return;
        }

        if self.registers.oam_address >= 0x100 {
            // Writes to $100 or higher immediately go through to high OAM at (address << 1) & 0x1F
            // $220-$3FF are mirrors of $200-$21F
            let second_write = self.registers.oam_data_flipflop == AccessFlipflop::Second;
            let oam_high_addr = (self.registers.oam_address << 1) | u16::from(second_write);
            self.oam_high[(oam_high_addr & 0x1F) as usize] = value;

            self.registers.oam_data_flipflop = self.registers.oam_data_flipflop.toggle();
            if second_write {
                self.registers.oam_address = (self.registers.oam_address + 1) & OAM_ADDRESS_MASK;
            }
        } else {
            // Writes to $000-$1FF go to low OAM; requires two writes to persist a word
            match self.registers.oam_data_flipflop {
                AccessFlipflop::First => {
                    // First write: Latch LSB
                    self.registers.oam_write_buffer = value;
                }
                AccessFlipflop::Second => {
                    // Second write: Write word to OAM
                    self.oam_low[self.registers.oam_address as usize] =
                        u16::from_le_bytes([self.registers.oam_write_buffer, value]);
                    self.registers.oam_address =
                        (self.registers.oam_address + 1) & OAM_ADDRESS_MASK;
                }
            }
            self.registers.oam_data_flipflop = self.registers.oam_data_flipflop.toggle();
        }
    }

    fn handle_active_display_oam_write(&mut self, value: u8) {
        self.progress_for_mid_scanline_write((self.state.scanline_master_cycles / 4) as u16);

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Active display OAM write at line {} dot {}, state is {:?}",
                self.state.scanline,
                self.state.scanline_master_cycles / 4,
                self.sprites.state
            );

            if let SpriteState::TileFetch { oam_buffer_idx, .. } = self.sprites.state {
                log::debug!(
                    "  Current OAM idx = {}",
                    self.sprites.scanned_oam_idxs[oam_buffer_idx as usize]
                );
            }
        }

        let oam_idx = self.sprites.state.oam_idx(&self.sprites.scanned_oam_idxs);

        let oam_high_addr = (oam_idx >> 2) as usize;
        self.oam_high[oam_high_addr] = value;

        log::debug!("Set OAMHigh[${oam_high_addr:02X}] = 0x{value:02X}");

        match self.registers.oam_data_flipflop {
            AccessFlipflop::First => {
                self.registers.oam_write_buffer = value;
            }
            AccessFlipflop::Second => {
                let word = u16::from_le_bytes([self.registers.oam_write_buffer, value]);
                let oam_low_addr = usize::from(oam_idx << 1);
                self.oam_low[oam_low_addr] = word;

                log::debug!("Set OAMLow[${oam_low_addr:02X}] = 0x{value:04X}");
            }
        }

        self.registers.oam_data_flipflop = self.registers.oam_data_flipflop.toggle();
    }

    fn read_oam_data_port(&mut self) -> u8 {
        if self.in_active_display(self.state.scanline) {
            return self.handle_active_display_oam_read();
        }

        let second_read = self.registers.oam_data_flipflop == AccessFlipflop::Second;

        let oam_byte = if self.registers.oam_address >= 0x100 {
            // High OAM; $220-$3FF mirrors $200-$21F
            let oam_high_addr = (self.registers.oam_address << 1) | u16::from(second_read);
            self.oam_high[(oam_high_addr & 0x1F) as usize]
        } else {
            // Low OAM
            let word_bytes = self.oam_low[self.registers.oam_address as usize].to_le_bytes();
            word_bytes[usize::from(second_read)]
        };

        self.registers.oam_data_flipflop = self.registers.oam_data_flipflop.toggle();
        if second_read {
            self.registers.oam_address = (self.registers.oam_address + 1) & OAM_ADDRESS_MASK;
        }

        oam_byte
    }

    fn handle_active_display_oam_read(&self) -> u8 {
        // TODO is this right? should active display reads always return from high OAM instead of low OAM?
        let oam_idx = self.sprites.state.oam_idx(&self.sprites.scanned_oam_idxs);
        let oam_high_addr = (oam_idx >> 2) & 0x1F;
        self.oam_high[oam_high_addr as usize]
    }

    fn write_cgram_data_port(&mut self, value: u8) {
        match self.registers.cgram_flipflop {
            AccessFlipflop::First => {
                self.registers.cgram_write_buffer = value;
                self.registers.cgram_flipflop = AccessFlipflop::Second;
            }
            AccessFlipflop::Second => {
                // Only bits 6-0 of high byte are persisted
                self.cgram[self.registers.cgram_address as usize] =
                    u16::from_le_bytes([self.registers.cgram_write_buffer, value & 0x7F]);
                self.registers.cgram_flipflop = AccessFlipflop::First;

                self.registers.cgram_address = self.registers.cgram_address.wrapping_add(1);
            }
        }
    }

    fn read_cgram_data_port(&mut self) -> u8 {
        let word = self.cgram[self.registers.cgram_address as usize];

        match self.registers.cgram_flipflop {
            AccessFlipflop::First => {
                // Low byte
                self.registers.cgram_flipflop = AccessFlipflop::Second;

                word.lsb()
            }
            AccessFlipflop::Second => {
                // High byte; bit 7 is PPU2 open bus
                self.registers.cgram_flipflop = AccessFlipflop::First;
                self.registers.cgram_address = self.registers.cgram_address.wrapping_add(1);

                (self.state.ppu2_open_bus & 0x80) | word.msb()
            }
        }
    }

    pub fn update_wrio(&mut self, wrio: u8) {
        if wrio != self.registers.programmable_joypad_port {
            let h_counter = (self.state.scanline_master_cycles >> 2) as u16;
            let v_counter = self.state.scanline;
            self.registers.update_wrio(wrio, h_counter, v_counter);
        }
    }

    pub fn update_controller_hv_latch(&mut self, h: u16, v: u16, master_cycles_elapsed: u64) {
        if v == self.state.scanline
            && h > (self.state.scanline_master_cycles / 4) as u16
            && h <= ((self.state.scanline_master_cycles + master_cycles_elapsed) / 4) as u16
        {
            self.registers.latched_h_counter = h;
            self.registers.latched_v_counter = v;
            self.registers.new_hv_latched = true;
        }
    }

    pub fn update_config(&mut self, config: SnesEmulatorConfig) {
        self.deinterlace = config.deinterlace;
    }

    pub fn reset(&mut self) {
        // Enable forced blanking
        self.registers.write_inidisp(0x80, self.is_first_vblank_scanline());

        // Return to default rendering mode (224-line, non-interlaced, no pseudo-hi-res or smaller OBJs)
        self.registers.write_setini(0x00);
    }
}

#[allow(clippy::too_many_arguments)]
fn get_bg_tile<'vram>(
    vram: &'vram Vram,
    registers: &Registers,
    bg: usize,
    x: u16,
    y: u16,
    bpp: BitsPerPixel,
    raw_tile_number: u16,
    x_flip: bool,
    y_flip: bool,
) -> &'vram [u16] {
    let bg_mode = registers.bg_mode;
    let bg_tile_size = registers.bg_tile_size[bg];
    let (bg_tile_width_pixels, bg_tile_height_pixels) = get_bg_tile_size(bg_mode, bg_tile_size);

    let tile_number = {
        let x_shift = bg_tile_width_pixels == 16 && (if x_flip { x % 16 < 8 } else { x % 16 >= 8 });
        let y_shift =
            bg_tile_height_pixels == 16 && (if y_flip { y % 16 < 8 } else { y % 16 >= 8 });
        match (x_shift, y_shift) {
            (false, false) => raw_tile_number,
            (true, false) => raw_tile_number + 1,
            (false, true) => raw_tile_number + 16,
            (true, true) => raw_tile_number + 17,
        }
    };

    let bg_data_base_addr = registers.bg_tile_base_address[bg];
    let tile_size_words = bpp.tile_size_words();
    let tile_addr = (bg_data_base_addr.wrapping_add(tile_number * tile_size_words)
        & VRAM_ADDRESS_MASK) as usize;
    &vram[tile_addr..tile_addr + tile_size_words as usize]
}

fn get_bg_map_entry(vram: &Vram, registers: &Registers, bg: usize, x: u16, y: u16) -> u16 {
    let bg_mode = registers.bg_mode;
    let bg_tile_size = registers.bg_tile_size[bg];
    let (bg_tile_width_pixels, bg_tile_height_pixels) = get_bg_tile_size(bg_mode, bg_tile_size);

    let bg_screen_size = registers.bg_screen_size[bg];
    let screen_width_pixels = bg_screen_size.width_tiles() * bg_tile_width_pixels;
    let screen_height_pixels = bg_screen_size.height_tiles() * bg_tile_height_pixels;

    let mut bg_map_base_addr = registers.bg_base_address[bg];
    let mut x = x & (screen_width_pixels - 1);
    let mut y = y & (screen_height_pixels - 1);

    // The larger BG screen is made up of 1-4 smaller 32x32 tile screens
    let single_screen_width_pixels = 32 * bg_tile_width_pixels;
    let single_screen_height_pixels = 32 * bg_tile_height_pixels;

    if x >= single_screen_width_pixels {
        bg_map_base_addr += 32 * 32;
        x &= single_screen_width_pixels - 1;
    }

    if y >= single_screen_height_pixels {
        bg_map_base_addr += match bg_screen_size {
            BgScreenSize::HorizontalMirror => 32 * 32,
            BgScreenSize::FourScreen => 2 * 32 * 32,
            _ => panic!(
                "y should always be <= 256/512 in OneScreen and VerticalMirror sizes; was {y}"
            ),
        };
        y &= single_screen_height_pixels - 1;
    }

    let tile_row = y / bg_tile_height_pixels;
    let tile_col = x / bg_tile_width_pixels;
    let tile_map_addr = 32 * tile_row + tile_col;

    vram[(bg_map_base_addr.wrapping_add(tile_map_addr) & VRAM_ADDRESS_MASK) as usize]
}

fn get_bg_tile_size(bg_mode: BgMode, tile_size: TileSize) -> (u16, u16) {
    match (bg_mode, tile_size) {
        (BgMode::Six, _) | (BgMode::Five, TileSize::Small) => (16, 8),
        (_, TileSize::Small) => (8, 8),
        (_, TileSize::Large) => (16, 16),
    }
}

fn line_overlaps_sprite(sprite_y: u8, sprite_height: u16, scanline: u16) -> bool {
    let scanline = scanline as u8;
    let sprite_bottom = sprite_y.wrapping_add(sprite_height as u8);
    if sprite_bottom > sprite_y {
        (sprite_y..sprite_bottom).contains(&scanline)
    } else {
        scanline >= sprite_y || scanline < sprite_bottom
    }
}

fn resolve_pixel_color(
    cgram: &Cgram,
    layer: Layer,
    bg_mode: BgMode,
    direct_color_mode: bool,
    palette: u8,
    color: u8,
) -> u16 {
    let bpp = match (layer, bg_mode) {
        (Layer::Bg1 | Layer::Bg2, BgMode::Seven) => BitsPerPixel::Eight,
        (Layer::Bg1, _) => bg_mode.bg1_bpp(),
        (Layer::Bg2, _) => bg_mode.bg2_bpp(),
        (Layer::Bg3, _) => BitsPerPixel::BG3,
        (Layer::Bg4, _) => BitsPerPixel::BG4,
        (Layer::Obj, _) => BitsPerPixel::OBJ,
        (Layer::Backdrop, _) => {
            panic!("invalid input to resolve_pixel_color: mode={bg_mode:?}, layer={layer:?}")
        }
    };

    let two_bpp_offset = if bg_mode == BgMode::Zero {
        // Mode 0 gives each BG layer its own set of 8 palettes
        match layer {
            Layer::Bg1 | Layer::Obj => 0x00,
            Layer::Bg2 => 0x20,
            Layer::Bg3 => 0x40,
            Layer::Bg4 => 0x60,
            Layer::Backdrop => unreachable!("above match checks layer is not backdrop"),
        }
    } else {
        0
    };

    // Sprites only use palettes in the second half of CGRAM
    let four_bpp_offset = if layer == Layer::Obj { 0x80 } else { 0 };

    match bpp {
        BitsPerPixel::Two => cgram[(two_bpp_offset | (palette << 2) | color) as usize],
        BitsPerPixel::Four => cgram[(four_bpp_offset | (palette << 4) | color) as usize],
        BitsPerPixel::Eight => {
            if direct_color_mode {
                resolve_direct_color(palette, color)
            } else {
                cgram[color as usize]
            }
        }
    }
}

fn resolve_direct_color(palette: u8, color: u8) -> u16 {
    let color: u16 = color.into();
    let palette: u16 = palette.into();

    // Color (8-bit) interpreted as BBGGGRRR
    // Palette (3-bit) interpreted as bgr
    // Result (16-bit): 0 BBb00 GGGg0 RRRr0
    let r_component = ((color & 0b00_000_111) << 2) | ((palette & 0b001) << 1);
    let g_component = ((color & 0b00_111_000) << 4) | ((palette & 0b010) << 5);
    let b_component = ((color & 0b11_000_000) << 7) | ((palette & 0b100) << 10);
    r_component | g_component | b_component
}

fn convert_snes_color(snes_color: u16, brightness: u8) -> Color {
    let color_table = &colortable::TABLE[brightness as usize];

    let r = color_table[(snes_color & 0x1F) as usize];
    let g = color_table[((snes_color >> 5) & 0x1F) as usize];
    let b = color_table[((snes_color >> 10) & 0x1F) as usize];
    Color::rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_color() {
        assert_eq!(0b00000_00000_11100, resolve_direct_color(0b000, 0b00_000_111));
        assert_eq!(0b00000_00000_11110, resolve_direct_color(0b001, 0b00_000_111));

        assert_eq!(0b00000_11100_00000, resolve_direct_color(0b000, 0b00_111_000));
        assert_eq!(0b00000_11110_00000, resolve_direct_color(0b010, 0b00_111_000));

        assert_eq!(0b11000_00000_00000, resolve_direct_color(0b000, 0b11_000_000));
        assert_eq!(0b11100_00000_00000, resolve_direct_color(0b100, 0b11_000_000));

        assert_eq!(0b11100_11110_11110, resolve_direct_color(0b111, 0b11_111_111));
    }
}
