use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::frontend::{Color, FrameSize, TimingMode};
use jgenesis_traits::num::GetBit;
use std::ops::{Deref, DerefMut};

// TODO 512px for hi-res mode?
const SCREEN_WIDTH: usize = 256;
const MAX_SCREEN_HEIGHT: usize = 239;
const FRAME_BUFFER_LEN: usize = SCREEN_WIDTH * MAX_SCREEN_HEIGHT;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum VerticalDisplaySize {
    #[default]
    TwoTwentyFour,
    TwoThirtyNine,
}

impl VerticalDisplaySize {
    fn to_lines(self) -> u16 {
        match self {
            Self::TwoTwentyFour => 224,
            Self::TwoThirtyNine => 239,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum BgMode {
    // Mode 0: 4x 2bpp background layers
    Zero,
    // Mode 1: 2x 4bpp and 1x 2bpp background layers
    One,
    // Mode 2: 2x 4bpp background layers with offset-per-tile
    Two,
    // Mode 3: 1x 8bpp and 1x 4bpp background layers
    Three,
    // Mode 4: 1x 8bpp and 1x 2bpp background layers with offset-per-tile
    Four,
    // Mode 5: 1x 4bpp and 1x 2bpp background layers in 512px hi-res
    Five,
    // Mode 6: 1x 4bpp background layer in 512px hi-res with offset-per-tile
    Six,
    // Mode 7: 1x 8bpp background layer with rotation/scaling, with support for an optional external background layer
    #[default]
    Seven,
}

impl BgMode {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x07 {
            0x00 => Self::Zero,
            0x01 => Self::One,
            0x02 => Self::Two,
            0x03 => Self::Three,
            0x04 => Self::Four,
            0x05 => Self::Five,
            0x06 => Self::Six,
            0x07 => Self::Seven,
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum BgTileSize {
    // 8x8
    #[default]
    Small,
    // 16x16
    Large,
}

impl BgTileSize {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Large } else { Self::Small }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ObjTileSize {
    // 0: 8x8 / 16x16
    #[default]
    Zero,
    // 1: 8x8 / 32x32
    One,
    // 2: 8x8 / 64x64
    Two,
    // 3: 16x16 / 32x32
    Three,
    // 4: 16x16 / 64x64
    Four,
    // 5: 32x32 / 64x64
    Five,
    // 6: 16x32 / 32x64
    Six,
    // 7: 16x32 / 32x32
    Seven,
}

impl ObjTileSize {
    fn from_byte(byte: u8) -> Self {
        match byte & 0xE0 {
            0x00 => Self::Zero,
            0x20 => Self::One,
            0x40 => Self::Two,
            0x60 => Self::Three,
            0x80 => Self::Four,
            0xA0 => Self::Five,
            0xC0 => Self::Six,
            0xE0 => Self::Seven,
            _ => unreachable!("value & 0xE0 will always be one of the above values"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum BgScreenSize {
    #[default]
    OneScreen,
    VerticalMirror,
    HorizontalMirror,
    FourScreen,
}

impl BgScreenSize {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::OneScreen,
            0x01 => Self::VerticalMirror,
            0x02 => Self::HorizontalMirror,
            0x03 => Self::FourScreen,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum WindowAreaMode {
    #[default]
    Disabled,
    Inside,
    Outside,
}

impl WindowAreaMode {
    fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 | 0x01 => Self::Disabled,
            0x02 => Self::Inside,
            0x03 => Self::Outside,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum WindowMaskLogic {
    #[default]
    Or,
    And,
    Xor,
    Xnor,
}

impl WindowMaskLogic {
    fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 => Self::Or,
            0x01 => Self::And,
            0x02 => Self::Xor,
            0x03 => Self::Xnor,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ColorMathEnableMode {
    Never,
    NotMathWindow,
    MathWindow,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ColorMathOperation {
    #[default]
    Add,
    Subtract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum VramAddressTranslation {
    #[default]
    None,
    EightBit,
    NineBit,
    TenBit,
}

impl VramAddressTranslation {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x0C {
            0x00 => Self::None,
            0x04 => Self::EightBit,
            0x08 => Self::NineBit,
            0x0C => Self::TenBit,
            _ => unreachable!("value & 0x0C is always one of the above values"),
        }
    }

    fn apply(self, vram_addr: u16) -> u16 {
        match self {
            Self::None => vram_addr,
            Self::EightBit => {
                (vram_addr & 0xFF00) | ((vram_addr >> 5) & 0x0007) | ((vram_addr & 0x001F) << 3)
            }
            Self::NineBit => {
                (vram_addr & 0xFE00) | ((vram_addr >> 6) & 0x0007) | ((vram_addr & 0x003F) << 3)
            }
            Self::TenBit => {
                (vram_addr & 0xFC00) | ((vram_addr >> 7) & 0x0007) | ((vram_addr & 0x007F) << 3)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum VramIncrementMode {
    #[default]
    Low,
    High,
}

impl VramIncrementMode {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(7) { Self::High } else { Self::Low }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ObjPriorityMode {
    #[default]
    Normal,
    Reverse,
}

impl ObjPriorityMode {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(7) { Self::Reverse } else { Self::Normal }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Mode7OobBehavior {
    #[default]
    Wrap,
    Transparent,
    Tile0,
}

impl Mode7OobBehavior {
    fn from_byte(byte: u8) -> Self {
        match byte & 0xC0 {
            0x00 | 0x40 => Self::Wrap,
            0x80 => Self::Transparent,
            0xC0 => Self::Tile0,
            _ => unreachable!("value & 0xC0 is always one of the above values"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum AccessFlipflop {
    #[default]
    First,
    Second,
}

impl AccessFlipflop {
    #[must_use]
    fn toggle(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    // INIDISP
    forced_blanking: bool,
    brightness: u8,
    // SETINI
    extbg_enabled: bool,
    pseudo_h_hi_res: bool,
    pseudo_obj_hi_res: bool,
    v_display_size: VerticalDisplaySize,
    interlaced: bool,
    // TM
    main_bg_enabled: [bool; 4],
    main_obj_enabled: bool,
    // TS
    sub_bg_enabled: [bool; 4],
    sub_obj_enabled: bool,
    // BGMODE
    bg_mode: BgMode,
    mode_1_bg3_priority: bool,
    bg_tile_size: [BgTileSize; 4],
    // MOSAIC
    mosaic_size: u8,
    bg_mosaic_enabled: [bool; 4],
    // BG1SC/BG2SC/BG3SC/BG4SC
    bg_screen_size: [BgScreenSize; 4],
    bg_base_address: [u16; 4],
    // BG12NBA/BG34NBA
    bg_tile_base_address: [u16; 4],
    // BG1HOFS/BG2HOFS/BG3HOFS/BG4HOFS
    // BG1VOFS/BG2VOFS/BG3VOFS/BG4VOFS
    bg_h_scroll: [u16; 4],
    bg_v_scroll: [u16; 4],
    bg_scroll_write_buffer: u8,
    // OBSEL
    obj_tile_base_address: u16,
    obj_tile_gap_size: u16,
    obj_tile_size: ObjTileSize,
    // TMW
    main_bg_window_enabled: [bool; 4],
    main_obj_window_enabled: bool,
    // TSW
    sub_bg_window_enabled: [bool; 4],
    sub_obj_window_enabled: bool,
    // WH0/WH1/WH2/WH3
    window_1_left: u16,
    window_1_right: u16,
    window_2_left: u16,
    window_2_right: u16,
    // W12SEL/W34SEL/WOBJSEL
    bg_window_1_area: [WindowAreaMode; 4],
    bg_window_2_area: [WindowAreaMode; 4],
    obj_window_1_area: WindowAreaMode,
    obj_window_2_area: WindowAreaMode,
    math_window_1_area: WindowAreaMode,
    math_window_2_area: WindowAreaMode,
    // WBGLOG/WOBJLOG
    bg_window_mask_logic: [WindowMaskLogic; 4],
    obj_window_mask_logic: WindowMaskLogic,
    math_window_mask_logic: WindowMaskLogic,
    // CGWSEL
    force_main_screen_black: ColorMathEnableMode,
    color_math_enabled: ColorMathEnableMode,
    sub_bg_obj_enabled: bool,
    direct_color_mode_enabled: bool,
    // CGADSUB
    color_math_operation: ColorMathOperation,
    color_math_divide_enabled: bool,
    bg_color_math_enabled: [bool; 4],
    obj_color_math_enabled: bool,
    backdrop_color_math_enabled: bool,
    // COLDATA
    sub_backdrop_color: u16,
    // VMAIN
    vram_address_increment_step: u16,
    vram_address_translation: VramAddressTranslation,
    vram_address_increment_mode: VramIncrementMode,
    // VMADDL/VMADDH
    vram_address: u16,
    // RDVRAML/RDVRAMH
    vram_prefetch_buffer: u16,
    // OAMADDL/OAMADDH
    oam_address: u16,
    oam_address_reload_value: u16,
    obj_priority_mode: ObjPriorityMode,
    oam_write_buffer: u8,
    // CGADD
    cgram_address: u8,
    // CGDATA/RDCGRAM
    cgram_write_buffer: u8,
    cgram_flipflop: AccessFlipflop,
    // M7SEL
    mode_7_h_flip: bool,
    mode_7_v_flip: bool,
    mode_7_oob_behavior: Mode7OobBehavior,
    // M7A/M7B/M7C/M7D
    mode_7_parameter_a: u16,
    mode_7_parameter_b: u16,
    mode_7_parameter_c: u16,
    mode_7_parameter_d: u16,
    mode_7_write_buffer: u8,
    // M7HOFS/M7VOFS
    mode_7_h_scroll: u16,
    mode_7_v_scroll: u16,
    // M7X/M7Y
    mode_7_center_x: u16,
    mode_7_center_y: u16,
    // PPU multiply unit (M7A/M7B)
    multiply_operand_l: i16,
    multiply_operand_r: i8,
}

impl Registers {
    fn new() -> Self {
        Self {
            forced_blanking: true,
            brightness: 0,
            extbg_enabled: false,
            pseudo_h_hi_res: false,
            pseudo_obj_hi_res: false,
            v_display_size: VerticalDisplaySize::default(),
            interlaced: false,
            main_bg_enabled: [false; 4],
            main_obj_enabled: false,
            sub_bg_enabled: [false; 4],
            sub_obj_enabled: false,
            bg_mode: BgMode::default(),
            mode_1_bg3_priority: true,
            bg_tile_size: [BgTileSize::default(); 4],
            mosaic_size: 0,
            bg_mosaic_enabled: [false; 4],
            bg_screen_size: [BgScreenSize::default(); 4],
            bg_base_address: [0; 4],
            bg_tile_base_address: [0; 4],
            bg_h_scroll: [0; 4],
            bg_v_scroll: [0; 4],
            bg_scroll_write_buffer: 0,
            obj_tile_base_address: 0,
            obj_tile_gap_size: 0,
            obj_tile_size: ObjTileSize::default(),
            main_bg_window_enabled: [false; 4],
            main_obj_window_enabled: false,
            sub_bg_window_enabled: [false; 4],
            sub_obj_window_enabled: false,
            window_1_left: 0,
            window_1_right: 0,
            window_2_left: 0,
            window_2_right: 0,
            bg_window_1_area: [WindowAreaMode::default(); 4],
            bg_window_2_area: [WindowAreaMode::default(); 4],
            obj_window_1_area: WindowAreaMode::default(),
            obj_window_2_area: WindowAreaMode::default(),
            math_window_1_area: WindowAreaMode::default(),
            math_window_2_area: WindowAreaMode::default(),
            bg_window_mask_logic: [WindowMaskLogic::default(); 4],
            obj_window_mask_logic: WindowMaskLogic::default(),
            math_window_mask_logic: WindowMaskLogic::default(),
            force_main_screen_black: ColorMathEnableMode::Never,
            color_math_enabled: ColorMathEnableMode::Always,
            sub_bg_obj_enabled: false,
            direct_color_mode_enabled: false,
            color_math_operation: ColorMathOperation::default(),
            color_math_divide_enabled: false,
            bg_color_math_enabled: [false; 4],
            obj_color_math_enabled: false,
            backdrop_color_math_enabled: false,
            sub_backdrop_color: 0,
            vram_address_increment_step: 1,
            vram_address_translation: VramAddressTranslation::default(),
            vram_address_increment_mode: VramIncrementMode::default(),
            vram_address: 0,
            vram_prefetch_buffer: 0,
            oam_address: 0,
            oam_address_reload_value: 0,
            obj_priority_mode: ObjPriorityMode::default(),
            oam_write_buffer: 0,
            cgram_address: 0,
            cgram_write_buffer: 0,
            cgram_flipflop: AccessFlipflop::default(),
            mode_7_h_flip: false,
            mode_7_v_flip: false,
            mode_7_oob_behavior: Mode7OobBehavior::default(),
            mode_7_parameter_a: 0xFFFF,
            mode_7_parameter_b: 0xFFFF,
            mode_7_parameter_c: 0,
            mode_7_parameter_d: 0,
            mode_7_write_buffer: 0,
            mode_7_h_scroll: 0,
            mode_7_v_scroll: 0,
            mode_7_center_x: 0,
            mode_7_center_y: 0,
            multiply_operand_l: !0,
            multiply_operand_r: !0,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u16,
    scanline_master_cycles: u64,
    odd_frame: bool,
}

impl State {
    fn new() -> Self {
        Self { scanline: 0, scanline_master_cycles: 0, odd_frame: false }
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

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    timing_mode: TimingMode,
    registers: Registers,
    state: State,
    vram: Box<Vram>,
    oam: Box<Oam>,
    cgram: Box<Cgram>,
    frame_buffer: FrameBuffer,
}

impl Ppu {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            timing_mode,
            registers: Registers::new(),
            state: State::new(),
            vram: vec![0; VRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            oam: vec![0; OAM_LEN].into_boxed_slice().try_into().unwrap(),
            cgram: vec![0; CGRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            frame_buffer: FrameBuffer::new(),
        }
    }

    #[must_use]
    pub fn tick(&mut self, master_cycles: u64) -> PpuTickEffect {
        let new_scanline_mclks = self.state.scanline_master_cycles + master_cycles;
        self.state.scanline_master_cycles = new_scanline_mclks;

        let mclks_per_scanline = self.mclks_per_current_scanline();
        if new_scanline_mclks >= mclks_per_scanline {
            self.state.scanline += 1;
            self.state.scanline_master_cycles = new_scanline_mclks - mclks_per_scanline;

            // Interlaced mode adds an extra scanline every other frame
            let scanlines_per_frame = self.scanlines_per_frame();
            if (self.state.scanline == scanlines_per_frame
                && (!self.registers.interlaced || self.state.odd_frame))
                || self.state.scanline == scanlines_per_frame + 1
            {
                self.state.scanline = 0;
                // TODO wait until H=1?
                self.state.odd_frame = !self.state.odd_frame;
            }

            if self.state.scanline == self.registers.v_display_size.to_lines() + 1 {
                self.dumb_render();
                return PpuTickEffect::FrameComplete;
            }
        }

        PpuTickEffect::None
    }

    // Temporary rendering implementation that only renders BG1 and assumes 2bpp, 8x8 tiles, no scroll, etc.
    fn dumb_render(&mut self) {
        let bg_map_base_addr = self.registers.bg_base_address[0];
        let bg_data_base_addr = self.registers.bg_tile_base_address[0];
        let backdrop_color = convert_snes_color(self.cgram[0]);

        for scanline in 0..224 {
            for pixel in 0..256 {
                let tile_row = scanline / 8;
                let tile_col = pixel / 8;
                let tile_map_addr = 32 * tile_row + tile_col;
                let tile_map_entry =
                    self.vram[((bg_map_base_addr + tile_map_addr) & VRAM_ADDRESS_MASK) as usize];

                let tile_number = tile_map_entry & 0x3FF;
                let palette = (tile_map_entry >> 10) & 0x07;

                let tile_addr =
                    ((bg_data_base_addr + 8 * tile_number) & VRAM_ADDRESS_MASK) as usize;
                let tile_data = &self.vram[tile_addr..tile_addr + 8];

                let cell_row = scanline % 8;
                let cell_col = pixel % 8;

                let word = tile_data[cell_row as usize];
                let shift = 7 - cell_col;
                let bit0 = (word >> shift) & 0x01;
                let bit1 = (word >> (8 + shift)) & 0x01;
                let color = (bit1 << 1) | bit0;

                let fb_index = scanline * 256 + pixel;
                if color != 0 {
                    let cgram_index = (palette << 5) | color;
                    self.frame_buffer[fb_index as usize] =
                        convert_snes_color(self.cgram[cgram_index as usize]);
                } else {
                    self.frame_buffer[fb_index as usize] = backdrop_color;
                }
            }
        }
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

    pub fn scanline_master_cycles(&self) -> u64 {
        self.state.scanline_master_cycles
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.as_ref()
    }

    pub fn frame_size(&self) -> FrameSize {
        let screen_height = self.registers.v_display_size.to_lines();
        FrameSize { width: SCREEN_WIDTH as u32, height: screen_height.into() }
    }

    pub fn read_port(&mut self, address: u32) -> u8 {
        todo!("PPU read {address:06X}")
    }

    pub fn write_port(&mut self, address: u32, value: u8) {
        if log::log_enabled!(log::Level::Trace) {
            // Don't log data port writes
            let address = address & 0xFF;
            if address != 0x04 && address != 0x18 && address != 0x19 && address != 0x22 {
                log::trace!("PPU register write: 21{address:02X} {value:02X}");
            }
        }

        match address & 0xFF {
            0x00 => {
                // INIDISP: Display control 1
                self.registers.forced_blanking = value.bit(7);
                self.registers.brightness = value & 0x0F;

                log::trace!("  Forced blanking: {}", self.registers.forced_blanking);
                log::trace!("  Brightness: {}", self.registers.brightness);
            }
            0x01 => {
                // OBSEL: Object size and base
                self.registers.obj_tile_base_address = u16::from(value & 0x07) << 13;
                self.registers.obj_tile_gap_size = u16::from(value & 0x18) << 9;
                self.registers.obj_tile_size = ObjTileSize::from_byte(value);

                log::trace!(
                    "  OBJ tile base address: {:04X}",
                    self.registers.obj_tile_base_address
                );
                log::trace!("  OBJ tile gap size: {:04X}", self.registers.obj_tile_gap_size);
                log::trace!("  OBJ tile size: {:?}", self.registers.obj_tile_size);
            }
            0x02 => {
                // OAMADDL: OAM address, low byte
                let reload_value =
                    (self.registers.oam_address_reload_value & 0xFF00) | u16::from(value);
                self.registers.oam_address_reload_value = reload_value;
                self.registers.oam_address = reload_value << 1;

                log::trace!(
                    "  OAM address reload value: {:04X}",
                    self.registers.oam_address_reload_value
                );
            }
            0x03 => {
                // OAMADDH: OAM address, high byte
                let reload_value = (self.registers.oam_address_reload_value & 0x00FF)
                    | (u16::from(value & 0x01) << 8);
                self.registers.oam_address_reload_value = reload_value;
                self.registers.oam_address = reload_value << 1;

                self.registers.obj_priority_mode = ObjPriorityMode::from_byte(value);

                log::trace!(
                    "  OAM address reload value: {:04X}",
                    self.registers.oam_address_reload_value
                );
                log::trace!("  OBJ priority mode: {:?}", self.registers.obj_priority_mode);
            }
            0x04 => {
                // OAMDATA: OAM data port (write)
                self.write_oam_data_port(value);
            }
            0x05 => {
                // BGMODE: BG mode and character size
                self.registers.bg_mode = BgMode::from_byte(value);
                self.registers.mode_1_bg3_priority = value.bit(3);

                for (i, tile_size) in self.registers.bg_tile_size.iter_mut().enumerate() {
                    *tile_size = BgTileSize::from_bit(value.bit(i as u8 + 4));
                }

                log::trace!("  BG mode: {:?}", self.registers.bg_mode);
                log::trace!("  Mode 1 BG3 priority: {}", self.registers.mode_1_bg3_priority);
                log::trace!("  BG tile sizes: {:?}", self.registers.bg_tile_size);
            }
            0x06 => {
                // MOSAIC: Mosaic size and enable
                self.registers.mosaic_size = value >> 4;

                for (i, mosaic_enabled) in self.registers.bg_mosaic_enabled.iter_mut().enumerate() {
                    *mosaic_enabled = value.bit(i as u8);
                }

                log::trace!("  Mosaic size: {}", self.registers.mosaic_size);
                log::trace!("  Mosaic enabled: {:?}", self.registers.bg_mosaic_enabled);
            }
            address @ 0x07..=0x0A => {
                // BG1SC/BG2SC/BG3SC/BG4SC: BG1-4 screen base and size
                let i = ((address + 1) & 0x3) as usize;
                self.registers.bg_screen_size[i] = BgScreenSize::from_byte(value);
                self.registers.bg_base_address[i] = u16::from(value & 0xFC) << 8;

                log::trace!("  BG{} screen size: {:?}", i + 1, self.registers.bg_screen_size[i]);
                log::trace!(
                    "  BG{} base address: {:04X}",
                    i + 1,
                    self.registers.bg_base_address[i]
                );
            }
            0x0B => {
                // BG12NBA: BG 1/2 character data area designation
                self.registers.bg_tile_base_address[0] = u16::from(value & 0x0F) << 12;
                self.registers.bg_tile_base_address[1] = u16::from(value & 0xF0) << 8;

                log::trace!(
                    "  BG1 tile base address: {:04X}",
                    self.registers.bg_tile_base_address[0]
                );
                log::trace!(
                    "  BG2 tile base address: {:04X}",
                    self.registers.bg_tile_base_address[1]
                );
            }
            0x0C => {
                // BG34NBA: BG 3/4 character data area designation
                self.registers.bg_tile_base_address[2] = u16::from(value & 0x0F) << 12;
                self.registers.bg_tile_base_address[3] = u16::from(value & 0xF0) << 8;

                log::trace!(
                    "  BG3 tile base address: {:04X}",
                    self.registers.bg_tile_base_address[2]
                );
                log::trace!(
                    "  BG4 tile base address: {:04X}",
                    self.registers.bg_tile_base_address[3]
                );
            }
            0x0D => {
                // BG1HOFS: BG1 horizontal scroll / M7HOFS: Mode 7 horizontal scroll
                self.write_bg_h_scroll(0, value);

                self.registers.mode_7_h_scroll =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 H scroll: {:04X}", self.registers.mode_7_h_scroll);
            }
            0x0E => {
                // BG1VOFS: BG1 vertical scroll / M7VOFS: Mode 7 vertical scroll
                self.write_bg_v_scroll(0, value);

                self.registers.mode_7_v_scroll =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 V scroll: {:04X}", self.registers.mode_7_v_scroll);
            }
            address @ (0x0F | 0x11 | 0x13) => {
                // BG2HOFS/BG3HOFS/BG4HOFS: BG2-4 horizontal scroll
                let i = (((address - 0x0F) >> 1) + 1) as usize;
                self.write_bg_h_scroll(i, value);
            }
            address @ (0x10 | 0x12 | 0x14) => {
                // BG2VOFS/BG3VOFS/BG4VOFS: BG2-4 vertical scroll
                let i = (((address & 0x0F) >> 1) + 1) as usize;
                self.write_bg_v_scroll(i, value);
            }
            0x15 => {
                // VMAIN: VRAM address increment mode
                self.registers.vram_address_increment_step = match value & 0x03 {
                    0x00 => 1,
                    0x01 => 32,
                    0x02 | 0x03 => 128,
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                };
                self.registers.vram_address_translation = VramAddressTranslation::from_byte(value);
                self.registers.vram_address_increment_mode = VramIncrementMode::from_byte(value);

                log::trace!(
                    "  VRAM data port increment step: {}",
                    self.registers.vram_address_increment_step
                );
                log::trace!(
                    "  VRAM data port address translation: {:?}",
                    self.registers.vram_address_translation
                );
                log::trace!(
                    "  VRAM data port increment on byte: {:?}",
                    self.registers.vram_address_increment_mode
                );
            }
            0x16 => {
                // VMADDL: VRAM address, low byte
                self.registers.vram_address =
                    (self.registers.vram_address & 0xFF00) | u16::from(value);
                self.fill_vram_prefetch();

                log::trace!("  VRAM data port address: {:04X}", self.registers.vram_address);
            }
            0x17 => {
                // VMADDH: VRAM address, high byte
                self.registers.vram_address =
                    (self.registers.vram_address & 0x00FF) | (u16::from(value) << 8);
                self.fill_vram_prefetch();

                log::trace!("  VRAM data port address: {:04X}", self.registers.vram_address);
            }
            0x18 => {
                // VMDATAL: VRAM data port (write), low byte
                self.write_vram_data_port_low(value);
            }
            0x19 => {
                // VMDATAH: VRAM data port (write), high byte
                self.write_vram_data_port_high(value);
            }
            0x1A => {
                // M7SEL: Mode 7 settings
                self.registers.mode_7_h_flip = value.bit(0);
                self.registers.mode_7_v_flip = value.bit(1);
                self.registers.mode_7_oob_behavior = Mode7OobBehavior::from_byte(value);

                log::trace!("  Mode 7 H flip: {}", self.registers.mode_7_h_flip);
                log::trace!("  Mode 7 V flip: {}", self.registers.mode_7_v_flip);
                log::trace!("  Mode 7 OOB behavior: {:?}", self.registers.mode_7_oob_behavior);
            }
            0x1B => {
                // M7A: Mode 7 parameter A / multiply 16-bit operand
                self.registers.mode_7_parameter_a =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.multiply_operand_l =
                    i16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 parameter A: {:04X}", self.registers.mode_7_parameter_a);
            }
            0x1C => {
                // M7B: Mode 7 parameter B / multiply 8-bit operand
                self.registers.mode_7_parameter_b =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.multiply_operand_r = value as i8;
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 parameter B: {:04X}", self.registers.mode_7_parameter_b);
            }
            0x1D => {
                // M7C: Mode 7 parameter C
                self.registers.mode_7_parameter_c =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 parameter C: {:04X}", self.registers.mode_7_parameter_c);
            }
            0x1E => {
                // M7D: Mode 7 parameter D
                self.registers.mode_7_parameter_d =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]);
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 parameter D: {:04X}", self.registers.mode_7_parameter_d);
            }
            0x1F => {
                // M7X: Mode 7 center X coordinate
                self.registers.mode_7_center_x =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]) & 0x1FFF;
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 center X: {:04X}", self.registers.mode_7_center_x);
            }
            0x20 => {
                // M7Y: Mode 7 center Y coordinate
                self.registers.mode_7_center_y =
                    u16::from_le_bytes([self.registers.mode_7_write_buffer, value]) & 0x1FFF;
                self.registers.mode_7_write_buffer = value;

                log::trace!("  Mode 7 center Y: {:04X}", self.registers.mode_7_center_y);
            }
            0x21 => {
                // CGADD: CGRAM address
                self.registers.cgram_address = value;
                self.registers.cgram_flipflop = AccessFlipflop::First;

                log::trace!("  CGRAM data port address: {value:02X}");
            }
            0x22 => {
                // CGDATA: CGRAM data port (write)
                self.write_cgram_data_port(value);
            }
            0x23 => {
                // W12SEL: Window BG1/2 mask settings
                self.registers.bg_window_1_area[0] = WindowAreaMode::from_bits(value);
                self.registers.bg_window_2_area[0] = WindowAreaMode::from_bits(value >> 2);
                self.registers.bg_window_1_area[1] = WindowAreaMode::from_bits(value >> 4);
                self.registers.bg_window_2_area[1] = WindowAreaMode::from_bits(value >> 6);

                log::trace!("  BG1 window 1 mask: {:?}", self.registers.bg_window_1_area[0]);
                log::trace!("  BG1 window 2 mask: {:?}", self.registers.bg_window_2_area[0]);
                log::trace!("  BG2 window 1 mask: {:?}", self.registers.bg_window_1_area[1]);
                log::trace!("  BG2 window 2 mask: {:?}", self.registers.bg_window_2_area[1]);
            }
            0x24 => {
                // W23SEL: Window BG3/4 mask settings
                self.registers.bg_window_1_area[2] = WindowAreaMode::from_bits(value);
                self.registers.bg_window_2_area[2] = WindowAreaMode::from_bits(value >> 2);
                self.registers.bg_window_1_area[3] = WindowAreaMode::from_bits(value >> 4);
                self.registers.bg_window_2_area[3] = WindowAreaMode::from_bits(value >> 6);

                log::trace!("  BG3 window 1 mask: {:?}", self.registers.bg_window_1_area[2]);
                log::trace!("  BG3 window 2 mask: {:?}", self.registers.bg_window_2_area[2]);
                log::trace!("  BG4 window 1 mask: {:?}", self.registers.bg_window_1_area[3]);
                log::trace!("  BG4 window 2 mask: {:?}", self.registers.bg_window_2_area[3]);
            }
            0x25 => {
                // WOBJSEL: Window OBJ/MATH mask settings
                self.registers.obj_window_1_area = WindowAreaMode::from_bits(value);
                self.registers.obj_window_2_area = WindowAreaMode::from_bits(value >> 2);
                self.registers.math_window_1_area = WindowAreaMode::from_bits(value >> 4);
                self.registers.math_window_2_area = WindowAreaMode::from_bits(value >> 6);

                log::trace!("  OBJ window 1 mask: {:?}", self.registers.obj_window_1_area);
                log::trace!("  OBJ window 2 mask: {:?}", self.registers.obj_window_2_area);
                log::trace!("  MATH window 1 mask: {:?}", self.registers.math_window_1_area);
                log::trace!("  MATH window 2 mask: {:?}", self.registers.math_window_2_area);
            }
            0x26 => {
                // WHO: Window 1 left position
                self.registers.window_1_left = value.into();

                log::trace!("  Window 1 left: {value:02X}");
            }
            0x27 => {
                // WH1: Window 1 right position
                self.registers.window_1_right = value.into();

                log::trace!("  Window 1 right: {value:02X}");
            }
            0x28 => {
                // WH2: Window 2 left position
                self.registers.window_2_left = value.into();

                log::trace!("  Window 2 left: {value:02X}");
            }
            0x29 => {
                // WH3: Window 2 right position
                self.registers.window_2_right = value.into();

                log::trace!("  Window 2 right: {value:02X}");
            }
            0x2A => {
                // WBGLOG: Window BG mask logic
                for (i, mask_logic) in self.registers.bg_window_mask_logic.iter_mut().enumerate() {
                    *mask_logic = WindowMaskLogic::from_bits(value >> (2 * i));
                }

                log::trace!("  BG window mask logic: {:?}", self.registers.bg_window_mask_logic);
            }
            0x2B => {
                // WOBJLOG: Window OBJ/MATH mask logic
                self.registers.obj_window_mask_logic = WindowMaskLogic::from_bits(value);
                self.registers.math_window_mask_logic = WindowMaskLogic::from_bits(value >> 2);

                log::trace!("  OBJ window mask logic: {:?}", self.registers.obj_window_mask_logic);
                log::trace!(
                    "  MATH window mask logic: {:?}",
                    self.registers.math_window_mask_logic
                );
            }
            0x2C => {
                // TM: Main screen designation
                for (i, bg_enabled) in self.registers.main_bg_enabled.iter_mut().enumerate() {
                    *bg_enabled = value.bit(i as u8);
                }
                self.registers.main_obj_enabled = value.bit(4);

                log::trace!("  Main screen BG enabled: {:?}", self.registers.main_bg_enabled);
                log::trace!("  Main screen OBJ enabled: {}", self.registers.main_obj_enabled);
            }
            0x2D => {
                // TS: Sub screen designation
                for (i, bg_enabled) in self.registers.sub_bg_enabled.iter_mut().enumerate() {
                    *bg_enabled = value.bit(i as u8);
                }
                self.registers.sub_obj_enabled = value.bit(4);

                log::trace!("  Sub screen BG enabled: {:?}", self.registers.sub_bg_enabled);
                log::trace!("  Sub screen OBJ enabled: {:?}", self.registers.sub_obj_enabled);
            }
            0x2E => {
                // TMW: Window area main screen disable
                for (i, bg_enabled) in self.registers.main_bg_window_enabled.iter_mut().enumerate()
                {
                    *bg_enabled = !value.bit(i as u8);
                }
                self.registers.main_obj_window_enabled = !value.bit(4);

                log::trace!(
                    "  Main screen BG window enabled: {:?}",
                    self.registers.main_bg_window_enabled
                );
                log::trace!(
                    "  Main screen OBJ window enabled: {}",
                    self.registers.main_obj_window_enabled
                );
            }
            0x2F => {
                // TSW: Window area sub screen disable
                for (i, bg_enabled) in self.registers.sub_bg_window_enabled.iter_mut().enumerate() {
                    *bg_enabled = !value.bit(i as u8);
                }
                self.registers.sub_obj_window_enabled = !value.bit(4);

                log::trace!(
                    "  Sub screen BG window enabled: {:?}",
                    self.registers.sub_bg_window_enabled
                );
                log::trace!(
                    "  Sub screen OBJ window enabled: {}",
                    self.registers.sub_obj_window_enabled
                );
            }
            0x30 => {
                // CGWSEL: Color math control register 1
                self.registers.direct_color_mode_enabled = value.bit(0);
                self.registers.sub_bg_obj_enabled = value.bit(1);

                self.registers.color_math_enabled = match value & 0x30 {
                    0x00 => ColorMathEnableMode::Always,
                    0x10 => ColorMathEnableMode::MathWindow,
                    0x20 => ColorMathEnableMode::NotMathWindow,
                    0x30 => ColorMathEnableMode::Never,
                    _ => unreachable!("value & 0x30 is always one of the above values"),
                };
                self.registers.force_main_screen_black = match value & 0xC0 {
                    0x00 => ColorMathEnableMode::Never,
                    0x40 => ColorMathEnableMode::NotMathWindow,
                    0x80 => ColorMathEnableMode::MathWindow,
                    0xC0 => ColorMathEnableMode::Always,
                    _ => unreachable!("value & 0xC0 is always one of the above values"),
                };

                log::trace!(
                    "  Direct color mode enabled: {}",
                    self.registers.direct_color_mode_enabled
                );
                log::trace!("  Sub screen BG/OBJ enabled: {}", self.registers.sub_bg_obj_enabled);
                log::trace!("  Color math enabled: {:?}", self.registers.color_math_enabled);
                log::trace!(
                    "  Force main screen black: {:?}",
                    self.registers.force_main_screen_black
                );
            }
            0x31 => {
                // CGADSUB: Color math control register 2
                for (i, enabled) in self.registers.bg_color_math_enabled.iter_mut().enumerate() {
                    *enabled = value.bit(i as u8);
                }

                self.registers.obj_color_math_enabled = value.bit(4);
                self.registers.backdrop_color_math_enabled = value.bit(5);
                self.registers.color_math_divide_enabled = value.bit(6);
                self.registers.color_math_operation = if value.bit(7) {
                    ColorMathOperation::Subtract
                } else {
                    ColorMathOperation::Add
                };

                log::trace!("  Color math operation: {:?}", self.registers.color_math_operation);
                log::trace!("  Color math divide: {}", self.registers.color_math_divide_enabled);
                log::trace!("  BG color math enabled: {:?}", self.registers.bg_color_math_enabled);
                log::trace!("  OBJ color math enabled: {}", self.registers.obj_color_math_enabled);
                log::trace!(
                    "  Backdrop color math enabled: {}",
                    self.registers.backdrop_color_math_enabled
                );
            }
            0x32 => {
                // COLDATA: Sub screen backdrop color
                let intensity = u16::from(value & 0x1F);
                let b = (u16::from(value.bit(7)) * intensity) << 10;
                let g = (u16::from(value.bit(6)) * intensity) << 5;
                let r = u16::from(value.bit(5)) * intensity;
                self.registers.sub_backdrop_color = b | g | r;

                log::trace!("  Sub screen backdrop color: r={r}, g={g}, b={b}");
            }
            0x33 => {
                // SETINI: Display control 2
                self.registers.interlaced = value.bit(0);
                self.registers.pseudo_obj_hi_res = value.bit(1);
                self.registers.v_display_size = if value.bit(2) {
                    VerticalDisplaySize::TwoThirtyNine
                } else {
                    VerticalDisplaySize::TwoTwentyFour
                };
                self.registers.pseudo_h_hi_res = value.bit(3);
                self.registers.extbg_enabled = value.bit(6);

                log::trace!("  Interlaced: {}", self.registers.interlaced);
                log::trace!("  Pseudo H hi-res: {}", self.registers.pseudo_h_hi_res);
                log::trace!("  Smaller OBJs: {}", self.registers.pseudo_obj_hi_res);
                log::trace!("  EXTBG enabled: {}", self.registers.extbg_enabled);
                log::trace!("  V display size: {:?}", self.registers.v_display_size);
            }
            _ => {
                // No other mappings are valid; do nothing
            }
        }
    }

    fn write_vram_data_port_low(&mut self, value: u8) {
        let vram_addr = (self.registers.vram_address_translation.apply(self.registers.vram_address)
            & VRAM_ADDRESS_MASK) as usize;
        self.vram[vram_addr] = (self.vram[vram_addr] & 0xFF00) | u16::from(value);

        if self.registers.vram_address_increment_mode == VramIncrementMode::Low {
            self.increment_vram_address();
        }
    }

    fn write_vram_data_port_high(&mut self, value: u8) {
        let vram_addr = (self.registers.vram_address_translation.apply(self.registers.vram_address)
            & VRAM_ADDRESS_MASK) as usize;
        self.vram[vram_addr] = (self.vram[vram_addr] & 0x00FF) | (u16::from(value) << 8);

        if self.registers.vram_address_increment_mode == VramIncrementMode::High {
            self.increment_vram_address();
        }
    }

    fn increment_vram_address(&mut self) {
        self.registers.vram_address =
            self.registers.vram_address.wrapping_add(self.registers.vram_address_increment_step);
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

    fn write_cgram_data_port(&mut self, value: u8) {
        match self.registers.cgram_flipflop {
            AccessFlipflop::First => {
                self.registers.cgram_write_buffer = value;
                self.registers.cgram_flipflop = AccessFlipflop::Second;
            }
            AccessFlipflop::Second => {
                self.cgram[self.registers.cgram_address as usize] =
                    u16::from_le_bytes([self.registers.cgram_write_buffer, value]);
                self.registers.cgram_flipflop = AccessFlipflop::First;

                self.registers.cgram_address = self.registers.cgram_address.wrapping_add(1);
            }
        }
    }

    fn write_bg_h_scroll(&mut self, i: usize, value: u8) {
        let current = self.registers.bg_h_scroll[i];
        let prev = self.registers.bg_scroll_write_buffer;

        self.registers.bg_h_scroll[i] =
            ((u16::from(value) << 8) | u16::from(prev & !0x07) | ((current >> 8) & 0x07)) & 0x03FF;
        self.registers.bg_scroll_write_buffer = value;

        log::trace!("  BG{} H scroll: {:04X}", i + 1, self.registers.bg_h_scroll[i]);
    }

    fn write_bg_v_scroll(&mut self, i: usize, value: u8) {
        let prev = self.registers.bg_scroll_write_buffer;

        self.registers.bg_v_scroll[i] = u16::from_le_bytes([prev, value]) & 0x03FF;
        self.registers.bg_scroll_write_buffer = value;

        log::trace!("  BG{} V scroll: {:04X}", i + 1, self.registers.bg_v_scroll[i]);
    }

    fn fill_vram_prefetch(&mut self) {
        self.registers.vram_prefetch_buffer =
            self.vram[(self.registers.vram_address & VRAM_ADDRESS_MASK) as usize];
    }
}

// [round(i * 255 / 31) for i in range(32)]
const COLOR_TABLE: [u8; 32] = [
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

fn convert_snes_color(snes_color: u16) -> Color {
    let r = snes_color & 0x1F;
    let g = (snes_color >> 5) & 0x1F;
    let b = (snes_color >> 10) & 0x1F;
    Color::rgb(COLOR_TABLE[r as usize], COLOR_TABLE[g as usize], COLOR_TABLE[b as usize])
}
