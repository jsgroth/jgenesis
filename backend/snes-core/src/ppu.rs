//! SNES PPU (picture processing unit)

mod colortable;
mod debug;
mod registers;

use crate::ppu::registers::{
    AccessFlipflop, BgMode, BgScreenSize, BitsPerPixel, Mode7OobBehavior, ObjPriorityMode,
    Registers, TileSize, VramIncrementMode,
};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut};

const MAX_BRIGHTNESS: u8 = 15;

const NORMAL_SCREEN_WIDTH: usize = 256;
const HIRES_SCREEN_WIDTH: usize = 512;
const MAX_SCREEN_HEIGHT: usize = 478;
const FRAME_BUFFER_LEN: usize = HIRES_SCREEN_WIDTH * MAX_SCREEN_HEIGHT;

const VRAM_LEN_WORDS: usize = 64 * 1024 / 2;
const OAM_LEN: usize = 512 + 32;
const CGRAM_LEN_WORDS: usize = 256;

const VRAM_ADDRESS_MASK: u16 = (1 << 15) - 1;
const OAM_ADDRESS_MASK: u16 = (1 << 10) - 1;

const MCLKS_PER_NORMAL_SCANLINE: u64 = 1364;
const MCLKS_PER_SHORT_SCANLINE: u64 = 1360;
const MCLKS_PER_LONG_SCANLINE: u64 = 1368;

type Vram = [u16; VRAM_LEN_WORDS];
type Oam = [u8; OAM_LEN];
type Cgram = [u16; CGRAM_LEN_WORDS];

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u16,
    scanline_master_cycles: u64,
    odd_frame: bool,
    pending_sprite_pixel_overflow: bool,
    ppu1_open_bus: u8,
    ppu2_open_bus: u8,
    last_rendered_scanline: Option<u16>,
    // Tracks if Mode 5/6 or pseudo-hi-res was enabled at any point during active display
    hi_res_frame: bool,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            scanline_master_cycles: 0,
            odd_frame: false,
            pending_sprite_pixel_overflow: false,
            ppu1_open_bus: 0,
            ppu2_open_bus: 0,
            last_rendered_scanline: None,
            hi_res_frame: false,
        }
    }

    fn frame_screen_width(&self) -> u32 {
        if self.hi_res_frame { HIRES_SCREEN_WIDTH as u32 } else { NORMAL_SCREEN_WIDTH as u32 }
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

#[derive(Debug, Clone, Encode, Decode)]
struct Buffers {
    bg_pixels: [[Pixel; HIRES_SCREEN_WIDTH]; 4],
    obj_pixels: [Pixel; NORMAL_SCREEN_WIDTH],
    offset_per_tile_h_scroll: [[u16; HIRES_SCREEN_WIDTH]; 2],
    offset_per_tile_v_scroll: [[u16; HIRES_SCREEN_WIDTH]; 2],
}

impl Buffers {
    fn new() -> Self {
        Self {
            bg_pixels: [[Pixel::TRANSPARENT; HIRES_SCREEN_WIDTH]; 4],
            obj_pixels: [Pixel::TRANSPARENT; NORMAL_SCREEN_WIDTH],
            offset_per_tile_h_scroll: [[0; HIRES_SCREEN_WIDTH]; 2],
            offset_per_tile_v_scroll: [[0; HIRES_SCREEN_WIDTH]; 2],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Bg1,
    Bg2,
    Bg3,
    Bg4,
    Obj,
    Backdrop,
}

#[derive(Debug, Clone)]
struct PriorityResolver {
    mode: BgMode,
    layers: [Option<Pixel>; 12],
}

impl PriorityResolver {
    const MODE_01_LAYERS: [Layer; 12] = [
        Layer::Obj,
        Layer::Bg1,
        Layer::Bg2,
        Layer::Obj,
        Layer::Bg1,
        Layer::Bg2,
        Layer::Obj,
        Layer::Bg3,
        Layer::Bg4,
        Layer::Obj,
        Layer::Bg3,
        Layer::Bg4,
    ];
    const OTHER_MODE_LAYERS: [Layer; 8] = [
        Layer::Obj,
        Layer::Bg1,
        Layer::Obj,
        Layer::Bg2,
        Layer::Obj,
        Layer::Bg1,
        Layer::Obj,
        Layer::Bg2,
    ];

    fn new(mode: BgMode) -> Self {
        Self { mode, layers: [None; 12] }
    }

    fn add(&mut self, layer: Layer, pixel: Pixel) {
        let idx = match self.mode {
            BgMode::Zero | BgMode::One => match (layer, pixel.priority) {
                // Modes 0-1:
                // OBJ.3 > BG1.1 > BG2.1 > OBJ.2 > BG1.0 > BG2.0 > OBJ.1 > BG3.1 > BG4.1 > OBJ.0 > BG3.0 > BG4.0
                (Layer::Obj, 3) => 0,
                (Layer::Bg1, 1) => 1,
                (Layer::Bg2, 1) => 2,
                (Layer::Obj, 2) => 3,
                (Layer::Bg1, 0) => 4,
                (Layer::Bg2, 0) => 5,
                (Layer::Obj, 1) => 6,
                (Layer::Bg3, 1) => 7,
                (Layer::Bg4, 1) => 8,
                (Layer::Obj, 0) => 9,
                (Layer::Bg3, 0) => 10,
                (Layer::Bg4, 0) => 11,
                _ => panic!(
                    "invalid mode/layer/priority combination: {:?} / {layer:?} / {}",
                    self.mode, pixel.priority
                ),
            },
            _ => match (layer, pixel.priority) {
                // Modes 2-7:
                // OBJ.3 > BG1.1 > OBJ.2 > BG2.1 > OBJ.1 > BG1.0 > OBJ.0 > BG2.0
                (Layer::Obj, 3) => 0,
                (Layer::Bg1, 1) => 1,
                (Layer::Obj, 2) => 2,
                (Layer::Bg2, 1) => 3,
                (Layer::Obj, 1) => 4,
                (Layer::Bg1, 0) => 5,
                (Layer::Obj, 0) => 6,
                (Layer::Bg2, 0) => 7,
                _ => panic!(
                    "invalid mode/layer/priority combination: {:?} / {layer:?} / {}",
                    self.mode, pixel.priority
                ),
            },
        };
        self.layers[idx] = Some(pixel);
    }

    fn get(&self, bg3_high_priority: bool) -> Option<(Pixel, Layer)> {
        if bg3_high_priority {
            // BG3.1 is at idx 7 in Mode 1
            if let Some(pixel) = self.layers[7] {
                return Some((pixel, Layer::Bg3));
            }
        }

        self.layers.iter().copied().enumerate().find_map(|(i, color)| {
            color.map(|pixel| match self.mode {
                BgMode::Zero | BgMode::One => (pixel, Self::MODE_01_LAYERS[i]),
                _ => (pixel, Self::OTHER_MODE_LAYERS[i]),
            })
        })
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpriteData {
    oam_idx: u8,
    x: u16,
    y: u8,
    tile_number: u16,
    palette: u8,
    priority: u8,
    x_flip: bool,
    y_flip: bool,
    size: TileSize,
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
struct SpriteBitSet([u64; 4]);

impl SpriteBitSet {
    fn new() -> Self {
        Self([0; 4])
    }

    fn get(&self, i: u16) -> bool {
        let idx = i >> 6;
        self.0[idx as usize] & (1 << (i & 0x3F)) != 0
    }

    fn set(&mut self, i: u16) {
        let idx = i >> 6;
        self.0[idx as usize] |= 1 << (i & 0x3F);
    }

    fn clear(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    timing_mode: TimingMode,
    registers: Registers,
    state: State,
    buffers: Box<Buffers>,
    vram: Box<Vram>,
    oam: Box<Oam>,
    cgram: Box<Cgram>,
    frame_buffer: FrameBuffer,
    sprite_buffer: Vec<SpriteData>,
    sprite_bit_set: SpriteBitSet,
}

// PPU starts rendering pixels at H=22
// Some games depend on this 88-cycle delay to finish HDMA before rendering starts, e.g. Final Fantasy 6
const RENDER_LINE_MCLK: u64 = 88;

impl Ppu {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            timing_mode,
            registers: Registers::new(),
            state: State::new(),
            buffers: Box::new(Buffers::new()),
            vram: vec![0; VRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            oam: vec![0; OAM_LEN].into_boxed_slice().try_into().unwrap(),
            cgram: vec![0; CGRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            frame_buffer: FrameBuffer::new(),
            sprite_buffer: Vec::with_capacity(32),
            sprite_bit_set: SpriteBitSet::new(),
        }
    }

    #[must_use]
    pub fn tick(&mut self, master_cycles: u64) -> PpuTickEffect {
        let prev_scanline_mclks = self.state.scanline_master_cycles;
        let new_scanline_mclks = self.state.scanline_master_cycles + master_cycles;
        self.state.scanline_master_cycles = new_scanline_mclks;

        let mclks_per_scanline = self.mclks_per_current_scanline();
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
                self.state.scanline = 0;
                // TODO wait until H=1?
                self.state.odd_frame = !self.state.odd_frame;
                self.state.last_rendered_scanline = None;
                self.state.hi_res_frame = self.registers.in_hi_res_mode();

                if !self.registers.forced_blanking {
                    self.registers.sprite_overflow = false;
                    self.registers.sprite_pixel_overflow = false;
                }
            }

            let v_display_size = self.registers.v_display_size.to_lines();
            if self.state.scanline >= 1
                && self.state.scanline <= v_display_size
                && self.state.scanline_master_cycles >= RENDER_LINE_MCLK
            {
                self.render_current_line();
            }

            if self.state.scanline == v_display_size + 1 {
                // Reload OAM data port address at start of VBlank if not in forced blanking
                if !self.registers.forced_blanking {
                    self.registers.oam_address = self.registers.oam_address_reload_value << 1;
                }

                return PpuTickEffect::FrameComplete;
            }
        } else if prev_scanline_mclks < RENDER_LINE_MCLK
            && new_scanline_mclks >= RENDER_LINE_MCLK
            && self.state.scanline >= 1
            && self.state.scanline <= self.registers.v_display_size.to_lines()
        {
            self.render_current_line();
        }

        PpuTickEffect::None
    }

    fn render_current_line(&mut self) {
        let scanline = self.state.scanline;
        self.state.last_rendered_scanline = Some(scanline);

        if self.registers.forced_blanking {
            // Forced blanking always draws black
            let screen_width = self.state.frame_screen_width();
            for pixel in 0..screen_width as u16 {
                self.set_in_frame_buffer(scanline, pixel, Color::rgb(0, 0, 0));
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

        self.populate_sprite_buffer(scanline);
        self.render_obj_layer_to_buffer(scanline);

        if hi_res_mode == HiResMode::True && self.registers.interlaced {
            self.render_bg_layers_to_buffer(2 * scanline, hi_res_mode);
            self.render_scanline(2 * scanline, hi_res_mode);

            self.render_bg_layers_to_buffer(2 * scanline + 1, hi_res_mode);
            self.render_scanline(2 * scanline + 1, hi_res_mode);
        } else {
            self.render_bg_layers_to_buffer(scanline, hi_res_mode);
            self.render_scanline(scanline, hi_res_mode);
        }
    }

    fn render_bg_layers_to_buffer(&mut self, scanline: u16, hi_res_mode: HiResMode) {
        let mode = self.registers.bg_mode;

        let bg1_enabled = self.registers.main_bg_enabled[0] || self.registers.sub_bg_enabled[0];
        let bg2_enabled = mode.bg2_enabled()
            && (self.registers.main_bg_enabled[1] || self.registers.sub_bg_enabled[1]);
        let bg3_enabled = mode.bg3_enabled()
            && (self.registers.main_bg_enabled[2] || self.registers.sub_bg_enabled[2]);
        let bg4_enabled = mode.bg4_enabled()
            && (self.registers.main_bg_enabled[3] || self.registers.sub_bg_enabled[3]);

        if mode.is_offset_per_tile() && (bg1_enabled || bg2_enabled) {
            // Populate offset-per-tile buffers before rendering BG1 or BG2
            self.populate_offset_per_tile_buffers(hi_res_mode);
        }

        if bg1_enabled {
            match mode {
                BgMode::Seven => self.render_mode_7_to_buffer(scanline),
                _ => self.render_bg_to_buffer(0, scanline, hi_res_mode),
            }
        }

        if bg2_enabled {
            self.render_bg_to_buffer(1, scanline, hi_res_mode);
        }

        if bg3_enabled {
            self.render_bg_to_buffer(2, scanline, hi_res_mode);
        }

        if bg4_enabled {
            self.render_bg_to_buffer(3, scanline, hi_res_mode);
        }
    }

    fn render_bg_to_buffer(&mut self, bg: usize, scanline: u16, hi_res_mode: HiResMode) {
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

        for pixel_idx in 0..screen_width as u16 {
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
                (
                    self.buffers.offset_per_tile_h_scroll[bg][pixel_idx as usize],
                    self.buffers.offset_per_tile_v_scroll[bg][pixel_idx as usize],
                )
            } else {
                (bg_h_scroll, bg_v_scroll)
            };

            if hi_res_mode == HiResMode::True {
                // H scroll values are effectively doubled in mode 5/6
                h_scroll *= 2;
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

    fn render_mode_7_to_buffer(&mut self, scanline: u16) {
        // Mode 7 tile map is always 128x128
        const TILE_MAP_SIZE_PIXELS: i32 = 128 * 8;

        // Affine transformation parameters (fixed point, 1/256 pixel units)
        let m7a: i32 = (self.registers.mode_7_parameter_a as i16).into();
        let m7b: i32 = (self.registers.mode_7_parameter_b as i16).into();
        let m7c: i32 = (self.registers.mode_7_parameter_c as i16).into();
        let m7d: i32 = (self.registers.mode_7_parameter_d as i16).into();

        // Center of rotation
        let m7x = sign_extend_13_bit(self.registers.mode_7_center_x);
        let m7y = sign_extend_13_bit(self.registers.mode_7_center_y);

        let h_scroll = sign_extend_13_bit(self.registers.mode_7_h_scroll);
        let v_scroll = sign_extend_13_bit(self.registers.mode_7_v_scroll);

        let h_flip = self.registers.mode_7_h_flip;
        let v_flip = self.registers.mode_7_v_flip;

        let oob_behavior = self.registers.mode_7_oob_behavior;

        for pixel in 0..NORMAL_SCREEN_WIDTH as u16 {
            let (scanline, pixel) = self.apply_mosaic(0, scanline, pixel, HiResMode::None);

            let screen_x: i32 = (if h_flip { 255 - pixel } else { pixel }).into();
            let screen_y: i32 = (if v_flip { 255 - scanline } else { scanline }).into();

            // Perform the following matrix transformation:
            //   [ vram_x ] = [ m7a  m7b ] * [ screen_x + m7hofs - m7x ] + [ m7x ]
            //   [ vram_y ]   [ m7c  m7d ]   [ screen_y + m7vofs - m7y ]   [ m7y ]
            // m7a/m7b/m7c/m7d are in 1/256 pixel units, so the multiplication result is also in
            // 1/256 pixel units, and m7x/m7y need to be converted for the addition
            let scrolled_x = screen_x + h_scroll - m7x;
            let scrolled_y = screen_y + v_scroll - m7y;

            let mut tile_map_x = m7a * scrolled_x + m7b * scrolled_y + (m7x << 8);
            let mut tile_map_y = m7c * scrolled_x + m7d * scrolled_y + (m7y << 8);

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

    fn render_obj_layer_to_buffer(&mut self, scanline: u16) {
        if !(self.registers.main_obj_enabled || self.registers.sub_obj_enabled) {
            return;
        }

        for pixel_idx in 0..NORMAL_SCREEN_WIDTH as u16 {
            let pixel = self.resolve_sprite_color(scanline, pixel_idx);
            self.buffers.obj_pixels[pixel_idx as usize] = pixel;
        }
    }

    fn populate_offset_per_tile_buffers(&mut self, hi_res_mode: HiResMode) {
        let screen_width =
            if hi_res_mode == HiResMode::True { HIRES_SCREEN_WIDTH } else { NORMAL_SCREEN_WIDTH };

        let mode = self.registers.bg_mode;
        let bg3_h_scroll = self.registers.bg_h_scroll[2];
        let bg3_v_scroll = self.registers.bg_v_scroll[2];

        for pixel in 0..screen_width as u16 {
            // Lowest 3 bits of BG3 H scroll do not apply in offset-per-tile
            let bg3_x = (pixel.wrapping_sub(8) & !0x7).wrapping_add(bg3_h_scroll & !0x7);

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
                    let v_offset_entry =
                        get_bg_map_entry(&self.vram, &self.registers, 2, bg3_x, bg3_v_scroll + 8);
                    (h_offset_entry, v_offset_entry)
                }
            };

            // Offset-per-tile can only apply to BG1 and BG2
            for bg in 0..2 {
                let bg_h_scroll = self.registers.bg_h_scroll[bg];
                let bg_v_scroll = self.registers.bg_v_scroll[bg];
                if pixel + (bg_h_scroll & 0x07) < 8 {
                    // Offset-per-tile only applies to the 2nd visible tile and onwards
                    self.buffers.offset_per_tile_h_scroll[bg][pixel as usize] = bg_h_scroll;
                    self.buffers.offset_per_tile_v_scroll[bg][pixel as usize] = bg_v_scroll;
                    continue;
                }

                // BG1 uses bit 13 to determine whether to apply offset-per-tile, while BG2 uses bit 14
                let bg_offset_bit = if bg == 0 { 13 } else { 14 };
                self.buffers.offset_per_tile_h_scroll[bg][pixel as usize] =
                    if h_offset_entry.bit(bg_offset_bit) {
                        h_offset_entry & 0x03FF
                    } else {
                        bg_h_scroll
                    };
                self.buffers.offset_per_tile_v_scroll[bg][pixel as usize] =
                    if v_offset_entry.bit(bg_offset_bit) {
                        v_offset_entry & 0x03FF
                    } else {
                        bg_v_scroll
                    };
            }
        }
    }

    fn render_scanline(&mut self, scanline: u16, hi_res_mode: HiResMode) {
        let screen_width =
            if hi_res_mode.is_hi_res() { HIRES_SCREEN_WIDTH } else { NORMAL_SCREEN_WIDTH };

        let brightness = self.registers.brightness;
        let mode = self.registers.bg_mode;
        let direct_color_mode = self.registers.direct_color_mode_enabled;

        let main_backdrop_color = self.cgram[0];
        let sub_backdrop_color = self.registers.sub_backdrop_color;

        for pixel in 0..screen_width as u16 {
            let main_screen = if hi_res_mode.is_hi_res() && !pixel.bit(0) {
                // Even pixels draw the sub screen in hi-res mode
                Screen::Sub
            } else {
                Screen::Main
            };

            // In hi-res modes, window coordinates are effectively doubled and then shifted to
            // the right by 1 pixel
            let (screen_x, window_x) = match hi_res_mode {
                HiResMode::None => (pixel, pixel),
                HiResMode::Pseudo => (pixel / 2, (pixel / 2).wrapping_sub((pixel & 0x01) ^ 0x01)),
                HiResMode::True => (pixel, (pixel / 2).wrapping_sub((pixel & 0x01) ^ 0x01)),
            };

            // Find the frontmost non-transparent pixel in the main screen
            let (mut main_screen_color, main_screen_palette, main_screen_layer) = self
                .resolve_screen_color(screen_x, window_x, main_screen, hi_res_mode)
                .map_or((main_backdrop_color, 0, Layer::Backdrop), |(pixel, layer)| {
                    let color = resolve_pixel_color(
                        &self.cgram,
                        layer,
                        mode,
                        direct_color_mode,
                        pixel.palette,
                        pixel.color,
                    );
                    (color, pixel.palette, layer)
                });

            // Check if inside the color window (used for clipping and color math)
            let in_color_window = self.registers.math_window_mask_logic.apply(
                self.registers
                    .math_window_1_area
                    .to_optional_bool(self.registers.is_inside_window_1(window_x)),
                self.registers
                    .math_window_2_area
                    .to_optional_bool(self.registers.is_inside_window_2(window_x)),
            );

            let force_main_screen_black =
                self.registers.force_main_screen_black.enabled(in_color_window);
            if force_main_screen_black {
                // Pixel is clipped; force color to 0 (black)
                main_screen_color = 0;
            }

            // Check if color math is enabled globally and for this layer
            let color_math_enabled_global =
                self.registers.color_math_enabled.enabled(in_color_window);

            let color_math_enabled_layer = match main_screen_layer {
                Layer::Bg1 => self.registers.bg_color_math_enabled[0],
                Layer::Bg2 => self.registers.bg_color_math_enabled[1],
                Layer::Bg3 => self.registers.bg_color_math_enabled[2],
                Layer::Bg4 => self.registers.bg_color_math_enabled[3],
                Layer::Obj => self.registers.obj_color_math_enabled && main_screen_palette >= 4,
                Layer::Backdrop => self.registers.backdrop_color_math_enabled,
            };

            let snes_color = if color_math_enabled_global && color_math_enabled_layer {
                // Find the frontmost sub screen pixel
                let (sub_screen_color, sub_transparent) =
                    if self.registers.sub_bg_obj_enabled {
                        self.resolve_screen_color(screen_x, window_x, Screen::Sub, hi_res_mode)
                            .map_or((sub_backdrop_color, true), |(pixel, layer)| {
                                let color = resolve_pixel_color(
                                    &self.cgram,
                                    layer,
                                    mode,
                                    direct_color_mode,
                                    pixel.palette,
                                    pixel.color,
                                );
                                (color, false)
                            })
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
                    main_screen_color,
                    sub_screen_color,
                    divide,
                )
            } else {
                main_screen_color
            };

            let final_color = convert_snes_color(snes_color, brightness);

            if self.state.hi_res_frame && !hi_res_mode.is_hi_res() {
                // Hi-res mode is not currently enabled, but it was enabled earlier in the frame;
                // draw in 512px
                self.set_in_frame_buffer(scanline, 2 * pixel, final_color);
                self.set_in_frame_buffer(scanline, 2 * pixel + 1, final_color);
            } else {
                self.set_in_frame_buffer(scanline, pixel, final_color);
            }
        }
    }

    fn resolve_screen_color(
        &self,
        x: u16,
        window_x: u16,
        screen: Screen,
        hi_res_mode: HiResMode,
    ) -> Option<(Pixel, Layer)> {
        let in_window_1 = self.registers.is_inside_window_1(window_x);
        let in_window_2 = self.registers.is_inside_window_2(window_x);

        let (bg_enabled, bg_disabled_in_window) = match screen {
            Screen::Main => {
                (self.registers.main_bg_enabled, self.registers.main_bg_disabled_in_window)
            }
            Screen::Sub => {
                (self.registers.sub_bg_enabled, self.registers.sub_bg_disabled_in_window)
            }
        };

        let mode = self.registers.bg_mode;

        let mut priority_resolver = PriorityResolver::new(mode);

        if bg_enabled[0]
            && !(bg_disabled_in_window[0]
                && self.registers.bg_in_window(0, in_window_1, in_window_2))
        {
            let bg1_pixel = self.buffers.bg_pixels[0][x as usize];
            if !bg1_pixel.is_transparent() {
                priority_resolver.add(Layer::Bg1, bg1_pixel);

                if mode == BgMode::Seven && self.registers.extbg_enabled {
                    // When EXTBG is enabled in Mode 7, BG1 pixels are duplicated into BG2
                    // but use the highest color bit as priority
                    let bg2_pixel_color = bg1_pixel.color & 0x7F;
                    if bg2_pixel_color != 0 {
                        let bg2_priority = bg1_pixel.color >> 7;
                        priority_resolver.add(
                            Layer::Bg2,
                            Pixel {
                                priority: bg2_priority,
                                color: bg2_pixel_color,
                                palette: bg1_pixel.palette,
                            },
                        );
                    }
                }
            }
        }

        if mode.bg2_enabled()
            && bg_enabled[1]
            && !(bg_disabled_in_window[1]
                && self.registers.bg_in_window(1, in_window_1, in_window_2))
        {
            let bg2_pixel = self.buffers.bg_pixels[1][x as usize];
            if !bg2_pixel.is_transparent() {
                priority_resolver.add(Layer::Bg2, bg2_pixel);
            }
        }

        if mode.bg3_enabled()
            && bg_enabled[2]
            && !(bg_disabled_in_window[2]
                && self.registers.bg_in_window(2, in_window_1, in_window_2))
        {
            let bg3_pixel = self.buffers.bg_pixels[2][x as usize];
            if !bg3_pixel.is_transparent() {
                priority_resolver.add(Layer::Bg3, bg3_pixel);
            }
        }

        if mode.bg4_enabled()
            && bg_enabled[3]
            && !(bg_disabled_in_window[3]
                && self.registers.bg_in_window(3, in_window_1, in_window_2))
        {
            let bg4_pixel = self.buffers.bg_pixels[3][x as usize];
            if !bg4_pixel.is_transparent() {
                priority_resolver.add(Layer::Bg4, bg4_pixel);
            }
        }

        let (obj_enabled, obj_disabled_in_window) = match screen {
            Screen::Main => {
                (self.registers.main_obj_enabled, self.registers.main_obj_disabled_in_window)
            }
            Screen::Sub => {
                (self.registers.sub_obj_enabled, self.registers.sub_obj_disabled_in_window)
            }
        };
        let obj_in_window = self.registers.obj_window_mask_logic.apply(
            self.registers.obj_window_1_area.to_optional_bool(in_window_1),
            self.registers.obj_window_2_area.to_optional_bool(in_window_2),
        );
        if obj_enabled && !(obj_in_window && obj_disabled_in_window) {
            let obj_x = if hi_res_mode == HiResMode::True { x / 2 } else { x };
            let obj_pixel = self.buffers.obj_pixels[obj_x as usize];
            if !obj_pixel.is_transparent() {
                priority_resolver.add(Layer::Obj, obj_pixel);
            }
        }

        // In mode 1, non-transparent high-priority BG3 pixels display over all other layers
        let bg3_high_priority = mode == BgMode::One && self.registers.mode_1_bg3_priority;
        priority_resolver.get(bg3_high_priority)
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

        // Mosaic size of N fills each (N+1)x(N+1) square with the pixel in the top-left corner
        // Mosaic sizes are doubled in true hi-res mode
        let mosaic_size: u16 = (mosaic_size + 1).into();
        let mosaic_width = match hi_res_mode {
            HiResMode::True => 2 * mosaic_size,
            _ => mosaic_size,
        };
        let mosaic_height = match hi_res_mode {
            HiResMode::True if self.registers.interlaced => 2 * mosaic_size,
            _ => mosaic_size,
        };

        (scanline / mosaic_height * mosaic_height, pixel / mosaic_width * mosaic_width)
    }

    fn populate_sprite_buffer(&mut self, scanline: u16) {
        const OAM_LEN: usize = 128;
        const MAX_SPRITES_PER_LINE: usize = 32;

        self.sprite_buffer.clear();
        self.sprite_bit_set.clear();

        let (small_width, small_height) = self.registers.obj_tile_size.small_size();
        let (large_width, large_height) = self.registers.obj_tile_size.large_size();

        // If priority rotate mode is set, start iteration at the current OAM address instead of
        // index 0
        let oam_offset = match self.registers.obj_priority_mode {
            ObjPriorityMode::Normal => 0,
            ObjPriorityMode::Rotate => (((self.registers.oam_address) >> 1) & 0x7F) as usize,
        };
        let mut total_pixels = 0;
        for i in 0..OAM_LEN {
            let oam_idx = (i + oam_offset) & 0x7F;

            let oam_addr = oam_idx << 2;
            let x_lsb = self.oam[oam_addr];
            // Sprites at y=0 should display on scanline=1, and so on; add 1 to correct for this
            let y = self.oam[oam_addr + 1].wrapping_add(1);
            let tile_number_lsb = self.oam[oam_addr + 2];
            let attributes = self.oam[oam_addr + 3];

            let additional_bits_addr = 512 + (oam_idx >> 2);
            let additional_bits_shift = 2 * (oam_idx & 0x03);
            let additional_bits = self.oam[additional_bits_addr] >> additional_bits_shift;
            let x_msb = additional_bits.bit(0);
            let size = if additional_bits.bit(1) { TileSize::Large } else { TileSize::Small };

            let (sprite_width, sprite_height) = match size {
                TileSize::Small => (small_width, small_height),
                TileSize::Large => (large_width, large_height),
            };

            if !line_overlaps_sprite(y, sprite_height, scanline) {
                continue;
            }

            // Only sprites with pixels in the range [0, 256) are scanned into the sprite buffer
            let x = u16::from_le_bytes([x_lsb, u8::from(x_msb)]);
            if x >= 256 && x + sprite_width <= 512 {
                continue;
            }

            if self.sprite_buffer.len() == MAX_SPRITES_PER_LINE {
                // TODO more accurate timing - this flag should get set partway through the previous line
                self.registers.sprite_overflow = true;
                break;
            }

            let tile_number = u16::from_le_bytes([tile_number_lsb, u8::from(attributes.bit(0))]);
            let palette = (attributes >> 1) & 0x07;
            let priority = (attributes >> 4) & 0x03;
            let x_flip = attributes.bit(6);
            let y_flip = attributes.bit(7);

            self.sprite_buffer.push(SpriteData {
                oam_idx: oam_idx as u8,
                x,
                y,
                tile_number,
                palette,
                priority,
                x_flip,
                y_flip,
                size,
            });
            total_pixels += sprite_width;

            for i in 0..sprite_width {
                let sprite_pixel_x = x.wrapping_add(i) & 0x1FF;
                if sprite_pixel_x < 256 {
                    self.sprite_bit_set.set(sprite_pixel_x);
                }
            }
        }

        // Sprite pixel overflow occurs when there are more than 34 tiles' worth of sprite pixels
        // on a single line
        if total_pixels > 34 * 8 {
            self.registers.sprite_pixel_overflow = true;

            // TODO properly truncate overflow pixels if the last sprite is larger than 8px wide
            // Sprites in range are processed from last-to-first
            self.sprite_buffer.reverse();

            let mut pixel_count = 0;
            for i in 0..self.sprite_buffer.len() {
                let sprite_width = match self.sprite_buffer[i].size {
                    TileSize::Small => small_width,
                    TileSize::Large => large_width,
                };
                pixel_count += sprite_width;

                // Truncate after passing *or reaching* 272 pixels
                if pixel_count >= 34 * 8 {
                    self.sprite_buffer.truncate(i + 1);
                    break;
                }
            }

            self.sprite_buffer.reverse();
        }
    }

    fn resolve_sprite_color(&self, scanline: u16, pixel: u16) -> Pixel {
        if !self.sprite_bit_set.get(pixel) {
            return Pixel::TRANSPARENT;
        }

        let (small_width, small_height) = self.registers.obj_tile_size.small_size();
        let (large_width, large_height) = self.registers.obj_tile_size.large_size();

        self.sprite_buffer
            .iter()
            .find_map(|sprite| {
                let (sprite_width, sprite_height) = match sprite.size {
                    TileSize::Small => (small_width, small_height),
                    TileSize::Large => (large_width, large_height),
                };

                if !pixel_overlaps_sprite(sprite.x, sprite_width, pixel) {
                    return None;
                }

                let sprite_line = if sprite.y_flip {
                    sprite_height as u8
                        - 1
                        - ((scanline as u8).wrapping_sub(sprite.y) & ((sprite_height - 1) as u8))
                } else {
                    (scanline as u8).wrapping_sub(sprite.y) & ((sprite_height - 1) as u8)
                };
                let sprite_pixel = if sprite.x_flip {
                    sprite_width - 1 - (pixel.wrapping_sub(sprite.x) & (sprite_width - 1))
                } else {
                    pixel.wrapping_sub(sprite.x) & (sprite_width - 1)
                };

                let tile_x_offset = sprite_pixel / 8;
                let tile_y_offset: u16 = (sprite_line / 8).into();

                // Unlike BG tiles in 16x16 mode, overflows in large OBJ tiles do not carry to the next nibble
                let mut tile_number = sprite.tile_number;
                tile_number =
                    (tile_number & !0xF) | (tile_number.wrapping_add(tile_x_offset) & 0xF);
                tile_number =
                    (tile_number & !0xF0) | (tile_number.wrapping_add(tile_y_offset << 4) & 0xF0);

                let tile_size_words = BitsPerPixel::OBJ.tile_size_words();
                let tile_base_addr = self.registers.obj_tile_base_address
                    + u16::from(tile_number.bit(8))
                        * (256 * tile_size_words + self.registers.obj_tile_gap_size);
                let tile_addr = ((tile_base_addr + (tile_number & 0x00FF) * tile_size_words)
                    & VRAM_ADDRESS_MASK) as usize;

                let tile_data = &self.vram[tile_addr..tile_addr + tile_size_words as usize];

                let tile_row: u16 = (sprite_line % 8).into();
                let tile_col = sprite_pixel % 8;
                let bit_index = (7 - tile_col) as u8;

                let mut color = 0_u8;
                for i in 0..2 {
                    let tile_word = tile_data[(tile_row + 8 * i) as usize];
                    color |= u8::from(tile_word.bit(bit_index)) << (2 * i);
                    color |= u8::from(tile_word.bit(bit_index + 8)) << (2 * i + 1);
                }

                (color != 0).then_some(Pixel {
                    palette: sprite.palette,
                    color,
                    priority: sprite.priority,
                })
            })
            .unwrap_or(Pixel::TRANSPARENT)
    }

    fn enter_hi_res_mode(&mut self) {
        if !self.vblank_flag() && !self.state.hi_res_frame {
            // Hi-res mode enabled mid-frame; redraw previously rendered scanlines to 512x224 in-place
            if let Some(last_rendered_scanline) = self.state.last_rendered_scanline {
                for scanline in (1..=last_rendered_scanline).rev() {
                    let src_line_addr = 256 * u32::from(scanline - 1);
                    let dest_line_addr = 512 * u32::from(scanline - 1);
                    for pixel in (0..256).rev() {
                        let color = self.frame_buffer[(src_line_addr + pixel) as usize];
                        self.frame_buffer[(dest_line_addr + 2 * pixel) as usize] = color;
                        self.frame_buffer[(dest_line_addr + 2 * pixel + 1) as usize] = color;
                    }
                }
            }
        }

        self.state.hi_res_frame = true;
    }

    fn set_in_frame_buffer(&mut self, scanline: u16, pixel: u16, color: Color) {
        let screen_width = self.state.frame_screen_width();
        let index = u32::from(scanline - 1) * screen_width + u32::from(pixel);
        self.frame_buffer[index as usize] = color;
    }

    fn scanlines_per_frame(&self) -> u16 {
        match self.timing_mode {
            TimingMode::Ntsc => 262,
            TimingMode::Pal => 312,
        }
    }

    fn mclks_per_current_scanline(&self) -> u64 {
        if self.is_short_scanline() {
            MCLKS_PER_SHORT_SCANLINE
        } else if self.is_long_scanline() {
            MCLKS_PER_LONG_SCANLINE
        } else {
            MCLKS_PER_NORMAL_SCANLINE
        }
    }

    fn is_short_scanline(&self) -> bool {
        self.state.scanline == 240
            && self.timing_mode == TimingMode::Ntsc
            && !self.registers.interlaced
            && self.state.odd_frame
    }

    fn is_long_scanline(&self) -> bool {
        self.state.scanline == 311
            && self.timing_mode == TimingMode::Pal
            && self.registers.interlaced
            && self.state.odd_frame
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

    pub fn is_first_vblank_scanline(&self) -> bool {
        self.state.scanline == self.registers.v_display_size.to_lines() + 1
    }

    pub fn scanline_master_cycles(&self) -> u64 {
        self.state.scanline_master_cycles
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.as_ref()
    }

    pub fn frame_size(&self) -> FrameSize {
        let screen_width = self.state.frame_screen_width();

        let mut screen_height = self.registers.v_display_size.to_lines();
        if self.is_v_hi_res() {
            screen_height *= 2;
        }

        FrameSize { width: screen_width, height: screen_height.into() }
    }

    fn is_v_hi_res(&self) -> bool {
        self.registers.bg_mode.is_hi_res() && self.registers.interlaced
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
                    "PPU register write: 21{address:02X} {value:02X} (scanline {})",
                    self.state.scanline
                );
            }
        }

        match address & 0xFF {
            0x00 => self.registers.write_inidisp(value, self.is_first_vblank_scanline()),
            0x01 => self.registers.write_obsel(value),
            0x02 => self.registers.write_oamaddl(value),
            0x03 => self.registers.write_oamaddh(value),
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
        let oam_addr = self.registers.oam_address;
        if oam_addr >= 0x200 {
            // Writes to $200 or higher immediately go through
            // $220-$3FF are mirrors of $200-$21F
            self.oam[(0x200 | (oam_addr & 0x01F)) as usize] = value;
        } else if !oam_addr.bit(0) {
            // Even address < $200: latch LSB
            self.registers.oam_write_buffer = value;
        } else {
            // Odd address < $200: Write word to OAM
            self.oam[(oam_addr & !0x001) as usize] = self.registers.oam_write_buffer;
            self.oam[oam_addr as usize] = value;
        }

        self.registers.oam_address = (oam_addr + 1) & OAM_ADDRESS_MASK;
    }

    fn read_oam_data_port(&mut self) -> u8 {
        let oam_addr = self.registers.oam_address;
        let oam_byte = if oam_addr >= 0x200 {
            // $220-$3FF are mirrors of $200-$21F
            self.oam[(0x200 | (oam_addr & 0x01F)) as usize]
        } else {
            self.oam[oam_addr as usize]
        };

        self.registers.oam_address = (oam_addr + 1) & OAM_ADDRESS_MASK;

        oam_byte
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

    pub fn reset(&mut self) {
        // Enable forced blanking
        self.registers.write_inidisp(0x80, self.is_first_vblank_scanline());

        // Return to default rendering mode (224-line, non-interlaced, no pseudo-hi-res or smaller OBJs)
        self.registers.write_setini(0x00);
    }
}

fn sign_extend_13_bit(value: u16) -> i32 {
    (((value as i16) << 3) >> 3).into()
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

fn pixel_overlaps_sprite(sprite_x: u16, sprite_width: u16, pixel: u16) -> bool {
    let sprite_right = (sprite_x + sprite_width) & 0x01FF;
    if sprite_right > sprite_x {
        (sprite_x..sprite_right).contains(&pixel)
    } else {
        pixel >= sprite_x || pixel < sprite_right
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
