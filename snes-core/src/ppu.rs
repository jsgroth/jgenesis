mod colortable;

use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::frontend::{Color, FrameSize, TimingMode};
use jgenesis_traits::num::GetBit;
use std::cmp;
use std::ops::{Deref, DerefMut};

const SCREEN_WIDTH: usize = 512;
const MAX_SCREEN_HEIGHT: usize = 478;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitsPerPixel {
    // 4-color
    Two,
    // 16-color
    Four,
    // 256-color
    Eight,
}

impl BitsPerPixel {
    const fn bitplanes(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    const fn tile_size_words(self) -> u16 {
        match self {
            Self::Two => 8,
            Self::Four => 16,
            Self::Eight => 32,
        }
    }
}

// BG3 and BG4 are always 2bpp
const BG3_BPP: BitsPerPixel = BitsPerPixel::Two;
const BG4_BPP: BitsPerPixel = BitsPerPixel::Two;

// OBJ is always 4bpp
const OBJ_BPP: BitsPerPixel = BitsPerPixel::Four;

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
    // Mode 7: 1x 8bpp background layer with rotation/scaling
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

    fn bg1_bpp(self) -> BitsPerPixel {
        use BitsPerPixel as BPP;

        match self {
            Self::Zero => BPP::Two,
            Self::One | Self::Two | Self::Five | Self::Six => BPP::Four,
            Self::Three | Self::Four | Self::Seven => BPP::Eight,
        }
    }

    fn bg2_enabled(self) -> bool {
        // BG2 is enabled in all modes except 6 and 7
        !matches!(self, Self::Six | Self::Seven)
    }

    fn bg2_bpp(self) -> BitsPerPixel {
        use BitsPerPixel as BPP;

        match self {
            Self::Zero | Self::Four | Self::Five => BPP::Two,
            Self::One | Self::Two | Self::Three => BPP::Four,
            // BG2 is not rendered in mode 6 or 7; return value doesn't matter
            Self::Six | Self::Seven => BPP::Eight,
        }
    }

    fn bg3_enabled(self) -> bool {
        // BG3 is only _really_ enabled in modes 0 and 1; modes 2/4/6 use it for offset-per-tile
        matches!(self, Self::Zero | Self::One)
    }

    fn bg4_enabled(self) -> bool {
        // BG4 is only enabled in mode 0
        self == Self::Zero
    }

    fn is_offset_per_tile(self) -> bool {
        // Modes 2/4/6 use BG3 map entries as offsets for BG1/BG2 tiles
        matches!(self, Self::Two | Self::Four | Self::Six)
    }

    fn is_hi_res(self) -> bool {
        matches!(self, Self::Five | Self::Six)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum TileSize {
    // 8x8
    #[default]
    Small,
    // 16x16
    Large,
}

impl TileSize {
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

    fn small_size(self) -> (u16, u16) {
        match self {
            Self::Zero | Self::One | Self::Two => (8, 8),
            Self::Three | Self::Four => (16, 16),
            Self::Five => (32, 32),
            Self::Six | Self::Seven => (16, 32),
        }
    }

    fn large_size(self) -> (u16, u16) {
        match self {
            Self::Zero => (16, 16),
            Self::One | Self::Three | Self::Seven => (32, 32),
            Self::Two | Self::Four | Self::Five => (64, 64),
            Self::Six => (32, 64),
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

    fn width_tiles(self) -> u16 {
        match self {
            Self::OneScreen | Self::HorizontalMirror => 32,
            Self::VerticalMirror | Self::FourScreen => 64,
        }
    }

    fn height_tiles(self) -> u16 {
        match self {
            Self::OneScreen | Self::VerticalMirror => 32,
            Self::HorizontalMirror | Self::FourScreen => 64,
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

    fn to_optional_bool(self, inside: bool) -> Option<bool> {
        match self {
            Self::Disabled => None,
            Self::Inside => Some(inside),
            Self::Outside => Some(!inside),
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

    fn apply(self, window_1: Option<bool>, window_2: Option<bool>) -> bool {
        match (self, window_1, window_2) {
            (Self::Or, Some(window_1), Some(window_2)) => window_1 || window_2,
            (Self::And, Some(window_1), Some(window_2)) => window_1 && window_2,
            (Self::Xor, Some(window_1), Some(window_2)) => window_1 ^ window_2,
            (Self::Xnor, Some(window_1), Some(window_2)) => !(window_1 ^ window_2),
            (_, Some(window_1), None) => window_1,
            (_, None, Some(window_2)) => window_2,
            (_, None, None) => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ColorMathEnableMode {
    Never,
    OutsideColorWindow,
    InsideColorWindow,
    Always,
}

impl ColorMathEnableMode {
    fn enabled(self, in_color_window: bool) -> bool {
        match self {
            Self::Always => true,
            Self::InsideColorWindow => in_color_window,
            Self::OutsideColorWindow => !in_color_window,
            Self::Never => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ColorMathOperation {
    #[default]
    Add,
    Subtract,
}

impl ColorMathOperation {
    fn apply(self, a: u16, b: u16, divide: bool) -> u16 {
        let (r, g, b) = match self {
            Self::Add => {
                let r = (a & 0x1F) + (b & 0x1F);
                let g = ((a >> 5) & 0x1F) + ((b >> 5) & 0x1F);
                let b = ((a >> 10) & 0x1F) + ((b >> 10) & 0x1F);
                (r, g, b)
            }
            Self::Subtract => {
                let r = (a & 0x1F).saturating_sub(b & 0x1F);
                let g = ((a >> 5) & 0x1F).saturating_sub((b >> 5) & 0x1F);
                let b = ((a >> 10) & 0x1F).saturating_sub((b >> 10) & 0x1F);
                (r, g, b)
            }
        };

        if divide {
            let r = r >> 1;
            let g = g >> 1;
            let b = b >> 1;
            r | (g << 5) | (b << 10)
        } else {
            let r = cmp::min(r, 0x1F);
            let g = cmp::min(g, 0x1F);
            let b = cmp::min(b, 0x1F);
            r | (g << 5) | (b << 10)
        }
    }
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
    Rotate,
}

impl ObjPriorityMode {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(7) { Self::Rotate } else { Self::Normal }
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
    bg_tile_size: [TileSize; 4],
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
    main_bg_disabled_in_window: [bool; 4],
    main_obj_disabled_in_window: bool,
    // TSW
    sub_bg_disabled_in_window: [bool; 4],
    sub_obj_disabled_in_window: bool,
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
    // OPHCT/OPVCT
    latched_h_counter: u16,
    latched_v_counter: u16,
    new_hv_latched: bool,
    h_counter_flipflop: AccessFlipflop,
    v_counter_flipflop: AccessFlipflop,
    // Sprite overflow flags (readable in STAT77)
    sprite_overflow: bool,
    sprite_pixel_overflow: bool,
    // Copied from WRIO CPU register (needed for H/V counter latching)
    programmable_joypad_port: u8,
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
            bg_tile_size: [TileSize::default(); 4],
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
            main_bg_disabled_in_window: [false; 4],
            main_obj_disabled_in_window: false,
            sub_bg_disabled_in_window: [false; 4],
            sub_obj_disabled_in_window: false,
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
            latched_h_counter: 0,
            latched_v_counter: 0,
            new_hv_latched: false,
            h_counter_flipflop: AccessFlipflop::default(),
            v_counter_flipflop: AccessFlipflop::default(),
            sprite_overflow: false,
            sprite_pixel_overflow: false,
            programmable_joypad_port: 0xFF,
        }
    }

    fn write_inidisp(&mut self, value: u8, is_first_vblank_scanline: bool) {
        // INIDISP: Display control 1
        let prev_forced_blanking = self.forced_blanking;
        self.forced_blanking = value.bit(7);
        self.brightness = value & 0x0F;

        // Disabling forced blanking during the first VBlank scanline immediately reloads OAM address
        if prev_forced_blanking && !self.forced_blanking && is_first_vblank_scanline {
            self.oam_address = self.oam_address_reload_value << 1;
        }

        log::trace!("  Forced blanking: {}", self.forced_blanking);
        log::trace!("  Brightness: {}", self.brightness);
    }

    fn write_setini(&mut self, value: u8) {
        // SETINI: Display control 2
        self.interlaced = value.bit(0);
        self.pseudo_obj_hi_res = value.bit(1);
        self.v_display_size = if value.bit(2) {
            VerticalDisplaySize::TwoThirtyNine
        } else {
            VerticalDisplaySize::TwoTwentyFour
        };
        self.pseudo_h_hi_res = value.bit(3);
        self.extbg_enabled = value.bit(6);

        log::trace!("  Interlaced: {}", self.interlaced);
        log::trace!("  Pseudo H hi-res: {}", self.pseudo_h_hi_res);
        log::trace!("  Smaller OBJs: {}", self.pseudo_obj_hi_res);
        log::trace!("  EXTBG enabled: {}", self.extbg_enabled);
        log::trace!("  V display size: {:?}", self.v_display_size);
    }

    fn write_tm(&mut self, value: u8) {
        // TM: Main screen designation
        for (i, bg_enabled) in self.main_bg_enabled.iter_mut().enumerate() {
            *bg_enabled = value.bit(i as u8);
        }
        self.main_obj_enabled = value.bit(4);

        log::trace!("  Main screen BG enabled: {:?}", self.main_bg_enabled);
        log::trace!("  Main screen OBJ enabled: {}", self.main_obj_enabled);
    }

    fn write_ts(&mut self, value: u8) {
        // TS: Sub screen designation
        for (i, bg_enabled) in self.sub_bg_enabled.iter_mut().enumerate() {
            *bg_enabled = value.bit(i as u8);
        }
        self.sub_obj_enabled = value.bit(4);

        log::trace!("  Sub screen BG enabled: {:?}", self.sub_bg_enabled);
        log::trace!("  Sub screen OBJ enabled: {:?}", self.sub_obj_enabled);
    }

    fn write_bgmode(&mut self, value: u8) {
        // BGMODE: BG mode and character size
        self.bg_mode = BgMode::from_byte(value);
        self.mode_1_bg3_priority = value.bit(3);

        for (i, tile_size) in self.bg_tile_size.iter_mut().enumerate() {
            *tile_size = TileSize::from_bit(value.bit(i as u8 + 4));
        }

        log::trace!("  BG mode: {:?}", self.bg_mode);
        log::trace!("  Mode 1 BG3 priority: {}", self.mode_1_bg3_priority);
        log::trace!("  BG tile sizes: {:?}", self.bg_tile_size);
    }

    fn write_mosaic(&mut self, value: u8) {
        // MOSAIC: Mosaic size and enable
        self.mosaic_size = value >> 4;

        for (i, mosaic_enabled) in self.bg_mosaic_enabled.iter_mut().enumerate() {
            *mosaic_enabled = value.bit(i as u8);
        }

        log::trace!("  Mosaic size: {}", self.mosaic_size);
        log::trace!("  Mosaic enabled: {:?}", self.bg_mosaic_enabled);
    }

    fn write_bg1234sc(&mut self, bg: usize, value: u8) {
        // BG1SC/BG2SC/BG3SC/BG4SC: BG1-4 screen base and size
        self.bg_screen_size[bg] = BgScreenSize::from_byte(value);
        self.bg_base_address[bg] = u16::from(value & 0xFC) << 8;

        log::trace!("  BG{} screen size: {:?}", bg + 1, self.bg_screen_size[bg]);
        log::trace!("  BG{} base address: {:04X}", bg + 1, self.bg_base_address[bg]);
    }

    fn write_bg1234nba(&mut self, base_bg: usize, value: u8) {
        // BG12NBA/BG34NBA: BG 1/2/3/4 character data area designation
        self.bg_tile_base_address[base_bg] = u16::from(value & 0x0F) << 12;
        self.bg_tile_base_address[base_bg + 1] = u16::from(value & 0xF0) << 8;

        log::trace!(
            "  BG{} tile base address: {:04X}",
            base_bg + 1,
            self.bg_tile_base_address[base_bg]
        );
        log::trace!(
            "  BG{} tile base address: {:04X}",
            base_bg + 2,
            self.bg_tile_base_address[base_bg + 1],
        );
    }

    fn write_bg1hofs(&mut self, value: u8) {
        // BG1HOFS: BG1 horizontal scroll / M7HOFS: Mode 7 horizontal scroll
        self.write_bg_h_scroll(0, value);

        self.mode_7_h_scroll = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 H scroll: {:04X}", self.mode_7_h_scroll);
    }

    fn write_bg1vofs(&mut self, value: u8) {
        // BG1VOFS: BG1 vertical scroll / M7VOFS: Mode 7 vertical scroll
        self.write_bg_v_scroll(0, value);

        self.mode_7_v_scroll = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 V scroll: {:04X}", self.mode_7_v_scroll);
    }

    fn write_bg_h_scroll(&mut self, i: usize, value: u8) {
        let current = self.bg_h_scroll[i];
        let prev = self.bg_scroll_write_buffer;

        // H scroll formula from https://wiki.superfamicom.org/backgrounds
        self.bg_h_scroll[i] =
            (u16::from(value) << 8) | u16::from(prev & !0x07) | ((current >> 8) & 0x07);
        self.bg_scroll_write_buffer = value;

        log::trace!("  BG{} H scroll: {:04X}", i + 1, self.bg_h_scroll[i]);
    }

    fn write_bg_v_scroll(&mut self, i: usize, value: u8) {
        let prev = self.bg_scroll_write_buffer;

        self.bg_v_scroll[i] = u16::from_le_bytes([prev, value]);
        self.bg_scroll_write_buffer = value;

        log::trace!("  BG{} V scroll: {:04X}", i + 1, self.bg_v_scroll[i]);
    }

    fn write_m7sel(&mut self, value: u8) {
        // M7SEL: Mode 7 settings
        self.mode_7_h_flip = value.bit(0);
        self.mode_7_v_flip = value.bit(1);
        self.mode_7_oob_behavior = Mode7OobBehavior::from_byte(value);

        log::trace!("  Mode 7 H flip: {}", self.mode_7_h_flip);
        log::trace!("  Mode 7 V flip: {}", self.mode_7_v_flip);
        log::trace!("  Mode 7 OOB behavior: {:?}", self.mode_7_oob_behavior);
    }

    fn write_m7a(&mut self, value: u8) {
        // M7A: Mode 7 parameter A / multiply 16-bit operand
        self.mode_7_parameter_a = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.multiply_operand_l = i16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter A: {:04X}", self.mode_7_parameter_a);
    }

    fn write_m7b(&mut self, value: u8) {
        // M7B: Mode 7 parameter B / multiply 8-bit operand
        self.mode_7_parameter_b = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.multiply_operand_r = value as i8;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter B: {:04X}", self.mode_7_parameter_b);
    }

    fn write_m7c(&mut self, value: u8) {
        // M7C: Mode 7 parameter C
        self.mode_7_parameter_c = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter C: {:04X}", self.mode_7_parameter_c);
    }

    fn write_m7d(&mut self, value: u8) {
        // M7D: Mode 7 parameter D
        self.mode_7_parameter_d = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter D: {:04X}", self.mode_7_parameter_d);
    }

    fn write_m7x(&mut self, value: u8) {
        // M7X: Mode 7 center X coordinate
        self.mode_7_center_x = u16::from_le_bytes([self.mode_7_write_buffer, value]) & 0x1FFF;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 center X: {:04X}", self.mode_7_center_x);
    }

    fn write_m7y(&mut self, value: u8) {
        // M7Y: Mode 7 center Y coordinate
        self.mode_7_center_y = u16::from_le_bytes([self.mode_7_write_buffer, value]) & 0x1FFF;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 center Y: {:04X}", self.mode_7_center_y);
    }

    fn write_obsel(&mut self, value: u8) {
        // OBSEL: Object size and base
        self.obj_tile_base_address = u16::from(value & 0x07) << 13;
        self.obj_tile_gap_size = u16::from(value & 0x18) << 9;
        self.obj_tile_size = ObjTileSize::from_byte(value);

        log::trace!("  OBJ tile base address: {:04X}", self.obj_tile_base_address);
        log::trace!("  OBJ tile gap size: {:04X}", self.obj_tile_gap_size);
        log::trace!("  OBJ tile size: {:?}", self.obj_tile_size);
    }

    fn write_w1234sel(&mut self, base_bg: usize, value: u8) {
        // W12SEL/W34SEL: Window BG1/2/3/4 mask settings
        self.bg_window_1_area[base_bg] = WindowAreaMode::from_bits(value);
        self.bg_window_2_area[base_bg] = WindowAreaMode::from_bits(value >> 2);
        self.bg_window_1_area[base_bg + 1] = WindowAreaMode::from_bits(value >> 4);
        self.bg_window_2_area[base_bg + 1] = WindowAreaMode::from_bits(value >> 6);

        log::trace!("  BG{} window 1 mask: {:?}", base_bg + 1, self.bg_window_1_area[base_bg]);
        log::trace!("  BG{} window 2 mask: {:?}", base_bg + 1, self.bg_window_2_area[base_bg]);
        log::trace!("  BG{} window 1 mask: {:?}", base_bg + 2, self.bg_window_1_area[base_bg + 1]);
        log::trace!("  BG{} window 2 mask: {:?}", base_bg + 2, self.bg_window_2_area[base_bg + 1]);
    }

    fn write_wobjsel(&mut self, value: u8) {
        // WOBJSEL: Window OBJ/MATH mask settings
        self.obj_window_1_area = WindowAreaMode::from_bits(value);
        self.obj_window_2_area = WindowAreaMode::from_bits(value >> 2);
        self.math_window_1_area = WindowAreaMode::from_bits(value >> 4);
        self.math_window_2_area = WindowAreaMode::from_bits(value >> 6);

        log::trace!("  OBJ window 1 mask: {:?}", self.obj_window_1_area);
        log::trace!("  OBJ window 2 mask: {:?}", self.obj_window_2_area);
        log::trace!("  MATH window 1 mask: {:?}", self.math_window_1_area);
        log::trace!("  MATH window 2 mask: {:?}", self.math_window_2_area);
    }

    fn write_wh0(&mut self, value: u8) {
        // WH0: Window 1 left position
        self.window_1_left = value.into();
        log::trace!("  Window 1 left: {value:02X}");
    }

    fn write_wh1(&mut self, value: u8) {
        // WH1: Window 1 right position
        self.window_1_right = value.into();
        log::trace!("  Window 1 right: {value:02X}");
    }

    fn write_wh2(&mut self, value: u8) {
        // WH2: Window 2 left position
        self.window_2_left = value.into();
        log::trace!("  Window 2 left: {value:02X}");
    }

    fn write_wh3(&mut self, value: u8) {
        // WH3: Window 2 right position
        self.window_2_right = value.into();
        log::trace!("  Window 2 right: {value:02X}");
    }

    fn write_wbglog(&mut self, value: u8) {
        // WBGLOG: Window BG mask logic
        for (i, mask_logic) in self.bg_window_mask_logic.iter_mut().enumerate() {
            *mask_logic = WindowMaskLogic::from_bits(value >> (2 * i));
        }

        log::trace!("  BG window mask logic: {:?}", self.bg_window_mask_logic);
    }

    fn write_wobjlog(&mut self, value: u8) {
        // WOBJLOG: Window OBJ/MATH mask logic
        self.obj_window_mask_logic = WindowMaskLogic::from_bits(value);
        self.math_window_mask_logic = WindowMaskLogic::from_bits(value >> 2);

        log::trace!("  OBJ window mask logic: {:?}", self.obj_window_mask_logic);
        log::trace!("  MATH window mask logic: {:?}", self.math_window_mask_logic);
    }

    fn write_tmw(&mut self, value: u8) {
        // TMW: Window area main screen disable
        for (i, bg_disabled) in self.main_bg_disabled_in_window.iter_mut().enumerate() {
            *bg_disabled = value.bit(i as u8);
        }
        self.main_obj_disabled_in_window = value.bit(4);

        log::trace!(
            "  Main screen BG disabled inside window: {:?}",
            self.main_bg_disabled_in_window
        );
        log::trace!(
            "  Main screen OBJ disabled inside window: {}",
            self.main_obj_disabled_in_window
        );
    }

    fn write_tsw(&mut self, value: u8) {
        // TSW: Window area sub screen disable
        for (i, bg_disabled) in self.sub_bg_disabled_in_window.iter_mut().enumerate() {
            *bg_disabled = value.bit(i as u8);
        }
        self.sub_obj_disabled_in_window = value.bit(4);

        log::trace!("  Sub screen BG disabled inside window: {:?}", self.sub_bg_disabled_in_window);
        log::trace!("  Sub screen OBJ disabled inside window: {}", self.sub_obj_disabled_in_window);
    }

    fn write_cgwsel(&mut self, value: u8) {
        // CGWSEL: Color math control register 1
        self.direct_color_mode_enabled = value.bit(0);
        self.sub_bg_obj_enabled = value.bit(1);

        self.color_math_enabled = match value & 0x30 {
            0x00 => ColorMathEnableMode::Always,
            0x10 => ColorMathEnableMode::InsideColorWindow,
            0x20 => ColorMathEnableMode::OutsideColorWindow,
            0x30 => ColorMathEnableMode::Never,
            _ => unreachable!("value & 0x30 is always one of the above values"),
        };
        self.force_main_screen_black = match value & 0xC0 {
            0x00 => ColorMathEnableMode::Never,
            0x40 => ColorMathEnableMode::OutsideColorWindow,
            0x80 => ColorMathEnableMode::InsideColorWindow,
            0xC0 => ColorMathEnableMode::Always,
            _ => unreachable!("value & 0xC0 is always one of the above values"),
        };

        log::trace!("  Direct color mode enabled: {}", self.direct_color_mode_enabled);
        log::trace!("  Sub screen BG/OBJ enabled: {}", self.sub_bg_obj_enabled);
        log::trace!("  Color math enabled: {:?}", self.color_math_enabled);
        log::trace!("  Force main screen black: {:?}", self.force_main_screen_black);
    }

    fn write_cgadsub(&mut self, value: u8) {
        // CGADSUB: Color math control register 2
        for (i, enabled) in self.bg_color_math_enabled.iter_mut().enumerate() {
            *enabled = value.bit(i as u8);
        }

        self.obj_color_math_enabled = value.bit(4);
        self.backdrop_color_math_enabled = value.bit(5);
        self.color_math_divide_enabled = value.bit(6);
        self.color_math_operation =
            if value.bit(7) { ColorMathOperation::Subtract } else { ColorMathOperation::Add };

        log::trace!("  Color math operation: {:?}", self.color_math_operation);
        log::trace!("  Color math divide: {}", self.color_math_divide_enabled);
        log::trace!("  BG color math enabled: {:?}", self.bg_color_math_enabled);
        log::trace!("  OBJ color math enabled: {}", self.obj_color_math_enabled);
        log::trace!("  Backdrop color math enabled: {}", self.backdrop_color_math_enabled);
    }

    fn write_coldata(&mut self, value: u8) {
        // COLDATA: Sub screen backdrop color
        let intensity: u16 = (value & 0x1F).into();

        let mut sub_backdrop_color = self.sub_backdrop_color;

        if value.bit(7) {
            // Update B
            sub_backdrop_color = (sub_backdrop_color & 0x03FF) | (intensity << 10);
        }

        if value.bit(6) {
            // Update G
            sub_backdrop_color = (sub_backdrop_color & 0xFC1F) | (intensity << 5);
        }

        if value.bit(5) {
            // Update R
            sub_backdrop_color = (sub_backdrop_color & 0xFFE0) | intensity;
        }

        self.sub_backdrop_color = sub_backdrop_color;

        log::trace!("  Sub screen backdrop color: {sub_backdrop_color:04X}");
    }

    fn write_oamaddl(&mut self, value: u8) {
        // OAMADDL: OAM address, low byte
        let reload_value = (self.oam_address_reload_value & 0xFF00) | u16::from(value);
        self.oam_address_reload_value = reload_value;
        self.oam_address = reload_value << 1;

        log::trace!("  OAM address reload value: {:04X}", self.oam_address_reload_value);
    }

    fn write_oamaddh(&mut self, value: u8) {
        // OAMADDH: OAM address, high byte
        let reload_value =
            (self.oam_address_reload_value & 0x00FF) | (u16::from(value & 0x01) << 8);
        self.oam_address_reload_value = reload_value;
        self.oam_address = reload_value << 1;

        self.obj_priority_mode = ObjPriorityMode::from_byte(value);

        log::trace!("  OAM address reload value: {:04X}", self.oam_address_reload_value);
        log::trace!("  OBJ priority mode: {:?}", self.obj_priority_mode);
    }

    fn write_vmain(&mut self, value: u8) {
        // VMAIN: VRAM address increment mode
        self.vram_address_increment_step = match value & 0x03 {
            0x00 => 1,
            0x01 => 32,
            0x02 | 0x03 => 128,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        };
        self.vram_address_translation = VramAddressTranslation::from_byte(value);
        self.vram_address_increment_mode = VramIncrementMode::from_byte(value);

        log::trace!("  VRAM data port increment step: {}", self.vram_address_increment_step);
        log::trace!("  VRAM data port address translation: {:?}", self.vram_address_translation);
        log::trace!("  VRAM data port increment on byte: {:?}", self.vram_address_increment_mode);
    }

    fn write_vmaddl(&mut self, value: u8, vram: &Vram) {
        // VMADDL: VRAM address, low byte
        self.vram_address = (self.vram_address & 0xFF00) | u16::from(value);
        self.vram_prefetch_buffer = vram[(self.vram_address & VRAM_ADDRESS_MASK) as usize];

        log::trace!("  VRAM data port address: {:04X}", self.vram_address);
    }

    fn write_vmaddh(&mut self, value: u8, vram: &Vram) {
        // VMADDH: VRAM address, high byte
        self.vram_address = (self.vram_address & 0x00FF) | (u16::from(value) << 8);
        self.vram_prefetch_buffer = vram[(self.vram_address & VRAM_ADDRESS_MASK) as usize];

        log::trace!("  VRAM data port address: {:04X}", self.vram_address);
    }

    fn write_cgadd(&mut self, value: u8) {
        // CGADD: CGRAM address
        self.cgram_address = value;
        self.cgram_flipflop = AccessFlipflop::First;

        log::trace!("  CGRAM data port address: {value:02X}");
    }

    fn read_mpyl(&self) -> u8 {
        // MPYL: PPU multiply result, low byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        mpy_result as u8
    }

    fn read_mpym(&self) -> u8 {
        // MPYM: PPU multiply result, middle byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        (mpy_result >> 8) as u8
    }

    fn read_mpyh(&self) -> u8 {
        // MPYH: PPU multiply result, high byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        (mpy_result >> 16) as u8
    }

    fn read_slhv(&mut self, h_counter: u16, v_counter: u16) {
        if self.programmable_joypad_port.bit(7) {
            self.latched_h_counter = h_counter;
            self.latched_v_counter = v_counter;
        }
    }

    fn read_ophct(&mut self) -> u8 {
        let value = match self.h_counter_flipflop {
            AccessFlipflop::First => self.latched_h_counter as u8,
            AccessFlipflop::Second => (self.latched_h_counter >> 8) as u8,
        };
        self.h_counter_flipflop = self.h_counter_flipflop.toggle();
        value
    }

    fn read_opvct(&mut self) -> u8 {
        let value = match self.v_counter_flipflop {
            AccessFlipflop::First => self.latched_v_counter as u8,
            AccessFlipflop::Second => (self.latched_v_counter >> 8) as u8,
        };
        self.v_counter_flipflop = self.v_counter_flipflop.toggle();
        value
    }

    fn reset_hv_counter_flipflops(&mut self) {
        self.h_counter_flipflop = AccessFlipflop::First;
        self.v_counter_flipflop = AccessFlipflop::First;
    }

    fn update_wrio(&mut self, wrio: u8, h_counter: u16, v_counter: u16) {
        if self.programmable_joypad_port.bit(7) & !wrio.bit(7) {
            self.latched_h_counter = h_counter;
            self.latched_v_counter = v_counter;
        }
        self.programmable_joypad_port = wrio;
    }

    fn is_inside_window_1(&self, pixel: u16) -> bool {
        self.window_1_left <= self.window_1_right
            && (self.window_1_left..=self.window_1_right).contains(&pixel)
    }

    fn is_inside_window_2(&self, pixel: u16) -> bool {
        self.window_2_left <= self.window_2_right
            && (self.window_2_left..=self.window_2_right).contains(&pixel)
    }

    fn bg_in_window(&self, bg: usize, in_window_1: bool, in_window_2: bool) -> bool {
        self.bg_window_mask_logic[bg].apply(
            self.bg_window_1_area[bg].to_optional_bool(in_window_1),
            self.bg_window_2_area[bg].to_optional_bool(in_window_2),
        )
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u16,
    scanline_master_cycles: u64,
    odd_frame: bool,
    pending_sprite_pixel_overflow: bool,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            scanline_master_cycles: 0,
            odd_frame: false,
            pending_sprite_pixel_overflow: false,
        }
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

#[derive(Debug, Clone, Encode, Decode)]
struct Cache {
    bg_map_entries: [CachedBgMapEntry; 4],
}

impl Cache {
    fn new() -> Self {
        Self { bg_map_entries: [CachedBgMapEntry::default(); 4] }
    }

    fn clear(&mut self) {
        self.bg_map_entries.fill(CachedBgMapEntry::default());
    }

    fn get(&self, bg: usize, x: u16, y: u16) -> Option<CachedBgMapEntry> {
        let map_x = x / 8;
        let map_y = y / 8;
        (self.bg_map_entries[bg].map_x == map_x && self.bg_map_entries[bg].map_y == map_y)
            .then_some(self.bg_map_entries[bg])
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

#[derive(Debug, Clone, Copy)]
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
    layers: [Option<(u16, u8)>; 12],
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

    fn add(&mut self, layer: Layer, priority: u8, color: u16, palette: u8) {
        let idx = match self.mode {
            BgMode::Zero | BgMode::One => match (layer, priority) {
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
                    "invalid mode/layer/priority combination: {:?} / {layer:?} / {priority}",
                    self.mode
                ),
            },
            _ => match (layer, priority) {
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
                    "invalid mode/layer/priority combination: {:?} / {layer:?} / {priority}",
                    self.mode
                ),
            },
        };
        self.layers[idx] = Some((color, palette));
    }

    fn get(&self, bg3_high_priority: bool) -> Option<(u16, u8, Layer)> {
        if bg3_high_priority {
            // BG3.1 is at idx 7 in Mode 1
            if let Some((color, palette)) = self.layers[7] {
                return Some((color, palette, Layer::Bg3));
            }
        }

        self.layers.iter().copied().enumerate().find_map(|(i, color)| {
            color.map(|(color, palette)| match self.mode {
                BgMode::Zero | BgMode::One => (color, palette, Self::MODE_01_LAYERS[i]),
                _ => (color, palette, Self::OTHER_MODE_LAYERS[i]),
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
    cache: Cache,
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
            cache: Cache::new(),
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

        if self.registers.forced_blanking {
            // Forced blanking always draws black
            for pixel in 0..SCREEN_WIDTH as u16 {
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
        self.cache.clear();

        if hi_res_mode == HiResMode::True && self.registers.interlaced {
            self.render_scanline(2 * scanline, hi_res_mode);
            self.render_scanline(2 * scanline + 1, hi_res_mode);
        } else {
            self.render_scanline(scanline, hi_res_mode);
        }
    }

    fn render_scanline(&mut self, scanline: u16, hi_res_mode: HiResMode) {
        let brightness = self.registers.brightness;
        if hi_res_mode.is_hi_res() {
            for pixel in 0..512 {
                let snes_color = self.resolve_overall_color(scanline, pixel, hi_res_mode);
                let color = convert_snes_color(snes_color, brightness);
                self.set_in_frame_buffer(scanline, pixel, color);
            }
        } else {
            for pixel in 0..256 {
                let snes_color = self.resolve_overall_color(scanline, pixel, HiResMode::None);
                let color = convert_snes_color(snes_color, brightness);

                self.set_in_frame_buffer(scanline, 2 * pixel, color);
                self.set_in_frame_buffer(scanline, 2 * pixel + 1, color);
            }
        }
    }

    fn resolve_overall_color(&mut self, scanline: u16, pixel: u16, hi_res_mode: HiResMode) -> u16 {
        let main_screen = if hi_res_mode.is_hi_res() && !pixel.bit(0) {
            // Even pixels draw the sub screen in hi-res mode
            Screen::Sub
        } else {
            Screen::Main
        };

        let (main_x, window_x) = match hi_res_mode {
            HiResMode::None => (pixel, pixel),
            HiResMode::Pseudo => (pixel / 2, (pixel / 2).wrapping_sub((pixel & 0x01) ^ 0x01)),
            HiResMode::True => (pixel, (pixel / 2).wrapping_sub((pixel & 0x01) ^ 0x01)),
        };

        let main_backdrop_color = self.cgram[0];
        let (mut main_screen_color, main_screen_palette, main_screen_layer) = self
            .resolve_screen_color(scanline, main_x, main_screen, hi_res_mode)
            .unwrap_or((main_backdrop_color, 0, Layer::Backdrop));

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
            main_screen_color = 0;
        }

        let color_math_enabled_global = self.registers.color_math_enabled.enabled(in_color_window);

        let color_math_enabled_layer = match main_screen_layer {
            Layer::Bg1 => self.registers.bg_color_math_enabled[0],
            Layer::Bg2 => self.registers.bg_color_math_enabled[1],
            Layer::Bg3 => self.registers.bg_color_math_enabled[2],
            Layer::Bg4 => self.registers.bg_color_math_enabled[3],
            Layer::Obj => self.registers.obj_color_math_enabled && main_screen_palette >= 4,
            Layer::Backdrop => self.registers.backdrop_color_math_enabled,
        };

        let sub_backdrop_color = self.registers.sub_backdrop_color;
        if color_math_enabled_global && color_math_enabled_layer {
            let sub_x = if hi_res_mode.is_hi_res() { pixel / 2 } else { pixel };
            let (sub_screen_color, sub_transparent) = if self.registers.sub_bg_obj_enabled {
                self.resolve_screen_color(scanline, sub_x, Screen::Sub, HiResMode::None)
                    .map_or((sub_backdrop_color, true), |(color, _, _)| (color, false))
            } else {
                (sub_backdrop_color, false)
            };

            let divide = self.registers.color_math_divide_enabled
                && !force_main_screen_black
                && !sub_transparent;
            self.registers.color_math_operation.apply(main_screen_color, sub_screen_color, divide)
        } else {
            main_screen_color
        }
    }

    fn resolve_screen_color(
        &mut self,
        scanline: u16,
        pixel: u16,
        screen: Screen,
        hi_res_mode: HiResMode,
    ) -> Option<(u16, u8, Layer)> {
        let mode = self.registers.bg_mode;
        let mut priority_resolver = PriorityResolver::new(mode);

        let (bg_enabled, bg_disabled_in_window) = match screen {
            Screen::Main => {
                (self.registers.main_bg_enabled, self.registers.main_bg_disabled_in_window)
            }
            Screen::Sub => {
                (self.registers.sub_bg_enabled, self.registers.sub_bg_disabled_in_window)
            }
        };

        let in_window_1 = self.registers.is_inside_window_1(pixel);
        let in_window_2 = self.registers.is_inside_window_2(pixel);

        let direct_color_mode = self.registers.direct_color_mode_enabled;

        let bg1_in_window = self.registers.bg_in_window(0, in_window_1, in_window_2);
        let bg1_enabled = bg_enabled[0] && !(bg1_in_window && bg_disabled_in_window[0]);
        if bg1_enabled {
            if mode == BgMode::Seven {
                let pixel = self.resolve_mode_7_color(scanline, pixel);
                if !pixel.is_transparent() {
                    let color = resolve_pixel_color(
                        &self.cgram,
                        BitsPerPixel::Eight,
                        direct_color_mode,
                        0x00,
                        pixel.palette,
                        pixel.color,
                    );
                    priority_resolver.add(Layer::Bg1, pixel.priority, color, pixel.palette);

                    if self.registers.extbg_enabled {
                        // When EXTBG is enabled in Mode 7, BG1 pixels are duplicated into BG2
                        // but use the highest color bit as priority
                        let bg2_pixel_color = pixel.color & 0x7F;
                        if bg2_pixel_color != 0 {
                            let bg2_color = resolve_pixel_color(
                                &self.cgram,
                                BitsPerPixel::Eight,
                                direct_color_mode,
                                0x00,
                                pixel.palette,
                                bg2_pixel_color,
                            );
                            let bg2_priority = pixel.color >> 7;
                            priority_resolver.add(
                                Layer::Bg2,
                                bg2_priority,
                                bg2_color,
                                pixel.palette,
                            );
                        }
                    }
                }
            } else {
                let bg1_bpp = mode.bg1_bpp();
                let pixel = self.resolve_bg_color(0, bg1_bpp, scanline, pixel, hi_res_mode);
                if !pixel.is_transparent() {
                    let color = resolve_pixel_color(
                        &self.cgram,
                        bg1_bpp,
                        direct_color_mode,
                        0x00,
                        pixel.palette,
                        pixel.color,
                    );
                    priority_resolver.add(Layer::Bg1, pixel.priority, color, pixel.palette);
                }
            }
        }

        let bg2_in_window = self.registers.bg_in_window(1, in_window_1, in_window_2);
        let bg2_enabled =
            mode.bg2_enabled() && bg_enabled[1] && !(bg2_in_window && bg_disabled_in_window[1]);
        if bg2_enabled {
            let bg2_bpp = mode.bg2_bpp();
            let pixel = self.resolve_bg_color(1, bg2_bpp, scanline, pixel, hi_res_mode);
            if !pixel.is_transparent() {
                let two_bpp_offset = if mode == BgMode::Zero { 0x20 } else { 0x00 };
                let color = resolve_pixel_color(
                    &self.cgram,
                    bg2_bpp,
                    direct_color_mode,
                    two_bpp_offset,
                    pixel.palette,
                    pixel.color,
                );
                priority_resolver.add(Layer::Bg2, pixel.priority, color, pixel.palette);
            }
        }

        let bg3_in_window = self.registers.bg_in_window(2, in_window_1, in_window_2);
        let bg3_enabled =
            mode.bg3_enabled() && bg_enabled[2] && !(bg3_in_window && bg_disabled_in_window[2]);
        if bg3_enabled {
            // BG3 is always 2bpp when rendered
            let pixel = self.resolve_bg_color(2, BG3_BPP, scanline, pixel, hi_res_mode);
            if !pixel.is_transparent() {
                let two_bpp_offset = if mode == BgMode::Zero { 0x40 } else { 0x00 };
                let color = resolve_pixel_color(
                    &self.cgram,
                    BG3_BPP,
                    direct_color_mode,
                    two_bpp_offset,
                    pixel.palette,
                    pixel.color,
                );
                priority_resolver.add(Layer::Bg3, pixel.priority, color, pixel.palette);
            }
        }

        let bg4_in_window = self.registers.bg_in_window(3, in_window_1, in_window_2);
        let bg4_enabled =
            mode.bg4_enabled() && bg_enabled[3] && !(bg4_in_window && bg_disabled_in_window[3]);
        if bg4_enabled {
            // BG4 is always 2bpp
            let pixel = self.resolve_bg_color(3, BG4_BPP, scanline, pixel, hi_res_mode);
            if !pixel.is_transparent() {
                let two_bpp_offset = if mode == BgMode::Zero { 0x60 } else { 0x00 };
                let color = resolve_pixel_color(
                    &self.cgram,
                    BG4_BPP,
                    direct_color_mode,
                    two_bpp_offset,
                    pixel.palette,
                    pixel.color,
                );
                priority_resolver.add(Layer::Bg4, pixel.priority, color, pixel.palette);
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
            let obj_x = if hi_res_mode == HiResMode::True { pixel / 2 } else { pixel };
            let obj_y = if hi_res_mode == HiResMode::True && self.registers.interlaced {
                scanline / 2
            } else {
                scanline
            };

            let pixel = self.resolve_sprite_color(obj_y, obj_x);
            if !pixel.is_transparent() {
                let color = resolve_pixel_color(
                    &self.cgram,
                    OBJ_BPP,
                    direct_color_mode,
                    0x00,
                    pixel.palette | 0x08, // OBJ palettes use the second half of CGRAM
                    pixel.color,
                );
                priority_resolver.add(Layer::Obj, pixel.priority, color, pixel.palette);
            }
        }

        let bg3_high_priority = mode == BgMode::One && self.registers.mode_1_bg3_priority;
        priority_resolver.get(bg3_high_priority)
    }

    fn resolve_bg_color(
        &mut self,
        bg: usize,
        bpp: BitsPerPixel,
        scanline: u16,
        pixel: u16,
        hi_res_mode: HiResMode,
    ) -> Pixel {
        let (scanline, pixel) = self.apply_mosaic(bg, scanline, pixel, hi_res_mode);

        let (mut h_scroll, v_scroll) = if self.registers.bg_mode.is_offset_per_tile() {
            self.resolve_offset_per_tile(bg, pixel)
        } else {
            (self.registers.bg_h_scroll[bg], self.registers.bg_v_scroll[bg])
        };

        if hi_res_mode == HiResMode::True {
            // Scroll values are effectively doubled in true hi-res mode
            h_scroll *= 2;
        }

        let x = pixel.wrapping_add(h_scroll);
        let y = scanline.wrapping_add(v_scroll);

        let TileData { tile_data, palette, priority, x_flip, y_flip } =
            get_bg_tile(&self.vram, &self.registers, &mut self.cache, bg, x, y, bpp);

        let cell_row = if y_flip { 7 - (y % 8) } else { y % 8 };
        let cell_col = if x_flip { 7 - (x % 8) } else { x % 8 };
        let bit_index = (7 - cell_col) as u8;

        let mut color = 0_u8;
        for i in (0..bpp.bitplanes()).step_by(2) {
            let word_index = cell_row as usize + 4 * i;
            let word = tile_data[word_index];

            color |= u8::from(word.bit(bit_index)) << i;
            color |= u8::from(word.bit(bit_index + 8)) << (i + 1);
        }

        Pixel { palette, color, priority: priority.into() }
    }

    fn resolve_offset_per_tile(&self, bg: usize, pixel: u16) -> (u16, u16) {
        // Offset-per-tile only applies to the 2nd visible tile and onwards
        let h_scroll = self.registers.bg_h_scroll[bg];
        let v_scroll = self.registers.bg_v_scroll[bg];
        if pixel + (h_scroll & 0x07) < 8 {
            return (h_scroll, v_scroll);
        }

        let bg3_h_scroll = self.registers.bg_h_scroll[2];
        let bg3_v_scroll = self.registers.bg_v_scroll[2];

        let bg3_x = (pixel.wrapping_sub(8) & !0x7).wrapping_add(bg3_h_scroll & !0x7);

        let h_offset_entry = get_bg_map_entry(&self.vram, &self.registers, 2, bg3_x, bg3_v_scroll);

        let offset_entry_mask = if bg == 0 { 0x2000 } else { 0x4000 };

        match self.registers.bg_mode {
            BgMode::Four => {
                // In Mode 4, instead of loading the second entry, the PPU uses the highest bit
                // to determine whether to apply the offset to H or V
                if h_offset_entry & offset_entry_mask != 0 {
                    if h_offset_entry & 0x8000 != 0 {
                        (h_scroll, h_offset_entry & 0x03FF)
                    } else {
                        (h_offset_entry & 0x03FF, v_scroll)
                    }
                } else {
                    (h_scroll, v_scroll)
                }
            }
            _ => {
                let v_offset_entry =
                    get_bg_map_entry(&self.vram, &self.registers, 2, bg3_x, bg3_v_scroll + 8);

                let h_offset = if h_offset_entry & offset_entry_mask != 0 {
                    h_offset_entry & 0x03FF
                } else {
                    h_scroll
                };

                let v_offset = if v_offset_entry & offset_entry_mask != 0 {
                    v_offset_entry & 0x03FF
                } else {
                    v_scroll
                };

                (h_offset, v_offset)
            }
        }
    }

    // TODO make this more efficient
    #[allow(clippy::items_after_statements)]
    fn resolve_mode_7_color(&self, scanline: u16, pixel: u16) -> Pixel {
        // Mode 7 tile map is always 128x128
        const TILE_MAP_SIZE_PIXELS: i32 = 128 * 8;

        let (scanline, pixel) = self.apply_mosaic(0, scanline, pixel, HiResMode::None);

        let m7a: i32 = (self.registers.mode_7_parameter_a as i16).into();
        let m7b: i32 = (self.registers.mode_7_parameter_b as i16).into();
        let m7c: i32 = (self.registers.mode_7_parameter_c as i16).into();
        let m7d: i32 = (self.registers.mode_7_parameter_d as i16).into();

        let m7x = self.registers.mode_7_center_x;
        let m7y = self.registers.mode_7_center_y;

        let h_scroll = self.registers.mode_7_h_scroll;
        let v_scroll = self.registers.mode_7_v_scroll;

        let h_flip = self.registers.mode_7_h_flip;
        let v_flip = self.registers.mode_7_v_flip;

        let oob_behavior = self.registers.mode_7_oob_behavior;

        let screen_x = if h_flip { 255 - pixel } else { pixel };
        let screen_y = if v_flip { 255 - scanline } else { scanline };

        // Convert screen coordinates to 1/256 pixel units
        let screen_x = i32::from(screen_x) << 8;
        let screen_y = i32::from(screen_y) << 8;

        // Convert center coordinates and scroll values (signed 13-bit integer) to 1/256 pixel units
        fn extend_signed_13_bit(value: u16) -> i32 {
            i32::from((value << 3) as i16) << 5
        }

        let m7x = extend_signed_13_bit(m7x);
        let m7y = extend_signed_13_bit(m7y);
        let h_scroll = extend_signed_13_bit(h_scroll);
        let v_scroll = extend_signed_13_bit(v_scroll);

        let shifted_x = screen_x.wrapping_add(h_scroll).wrapping_sub(m7x);
        let shifted_y = screen_y.wrapping_add(v_scroll).wrapping_sub(m7y);

        let mut tile_map_x = m7a
            .wrapping_mul(shifted_x >> 8)
            .wrapping_add(m7b.wrapping_mul(shifted_y >> 8))
            .wrapping_add(m7x);
        let mut tile_map_y = m7c
            .wrapping_mul(shifted_x >> 8)
            .wrapping_add(m7d.wrapping_mul(shifted_y >> 8))
            .wrapping_add(m7y);

        // Convert back to pixel units
        tile_map_x >>= 8;
        tile_map_y >>= 8;

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
                    return Pixel::TRANSPARENT;
                }
                Mode7OobBehavior::Tile0 => {
                    tile_map_x &= 0x07;
                    tile_map_y &= 0x07;
                }
            }
        }

        // Mode 7 tile map is always located at $0000
        let tile_map_row = tile_map_y / 8;
        let tile_map_col = tile_map_x / 8;
        let tile_map_addr = tile_map_row * TILE_MAP_SIZE_PIXELS / 8 + tile_map_col;
        let tile_number = self.vram[tile_map_addr as usize] & 0x00FF;

        let tile_row = (tile_map_y % 8) as u16;
        let tile_col = (tile_map_x % 8) as u16;
        let pixel_addr = 64 * tile_number + 8 * tile_row + tile_col;
        let color = (self.vram[pixel_addr as usize] >> 8) as u8;

        Pixel { palette: 0, color, priority: 0 }
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

            // Sprite pixel overflow occurs when there are more than 34 tiles' worth of sprite pixels
            // on a single line
            if total_pixels > 34 * 8 {
                // TODO truncate overflow pixels
                self.registers.sprite_pixel_overflow = true;
                break;
            }
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

                let tile_size_words = OBJ_BPP.tile_size_words();
                let tile_base_addr = self.registers.obj_tile_base_address
                    + u16::from(tile_number.bit(8))
                        * (256 * tile_size_words + self.registers.obj_tile_gap_size);
                let tile_addr =
                    (tile_base_addr + (tile_number & 0x00FF) * tile_size_words) as usize;

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

    fn set_in_frame_buffer(&mut self, scanline: u16, pixel: u16, color: Color) {
        let index = u32::from(scanline - 1) * SCREEN_WIDTH as u32 + u32::from(pixel);
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
        let mut screen_height = self.registers.v_display_size.to_lines();
        if self.is_v_hi_res() {
            screen_height *= 2;
        }

        FrameSize { width: SCREEN_WIDTH as u32, height: screen_height.into() }
    }

    fn is_v_hi_res(&self) -> bool {
        self.registers.bg_mode.is_hi_res() && self.registers.interlaced
    }

    pub fn read_port(&mut self, address: u32) -> u8 {
        log::trace!("Read PPU register: {address:06X}");

        match address & 0xFF {
            0x34 => self.registers.read_mpyl(),
            0x35 => self.registers.read_mpym(),
            0x36 => self.registers.read_mpyh(),
            0x37 => {
                // SLHV: Latch H/V counter
                let h_counter = (self.state.scanline_master_cycles >> 2) as u16;
                let v_counter = self.state.scanline;
                self.registers.read_slhv(h_counter, v_counter);

                // TODO reads should return open bus; hardcoding a common open bus value for this address
                0x21
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
            0x3C => self.registers.read_ophct(),
            0x3D => self.registers.read_opvct(),
            0x3E => {
                // STAT77: PPU1 status and version number
                // Version number hardcoded to 1
                (u8::from(self.registers.sprite_pixel_overflow) << 7)
                    | (u8::from(self.registers.sprite_overflow) << 6)
                    | 0x01
            }
            0x3F => {
                // STAT78: PPU2 status and version number
                // Version number hardcoded to 1
                let value = (u8::from(self.state.odd_frame) << 7)
                    | (u8::from(self.registers.new_hv_latched) << 6)
                    | (u8::from(self.timing_mode == TimingMode::Pal) << 1)
                    | 0x01;

                self.registers.new_hv_latched = false;
                self.registers.reset_hv_counter_flipflops();

                value
            }
            _ => {
                log::warn!("Unmapped PPU read {address:06X}");
                0x00
            }
        }
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
            0x05 => self.registers.write_bgmode(value),
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
            0x33 => self.registers.write_setini(value),
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

    fn read_vram_data_port_low(&mut self) -> u8 {
        let vram_byte = self.registers.vram_prefetch_buffer as u8;

        if self.registers.vram_address_increment_mode == VramIncrementMode::Low {
            // Fill prefetch buffer *before* address increment
            self.fill_vram_prefetch_buffer();
            self.increment_vram_address();
        }

        vram_byte
    }

    fn read_vram_data_port_high(&mut self) -> u8 {
        let vram_byte = (self.registers.vram_prefetch_buffer >> 8) as u8;

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
                self.cgram[self.registers.cgram_address as usize] =
                    u16::from_le_bytes([self.registers.cgram_write_buffer, value]);
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

                word as u8
            }
            AccessFlipflop::Second => {
                // High byte
                self.registers.cgram_flipflop = AccessFlipflop::First;
                self.registers.cgram_address = self.registers.cgram_address.wrapping_add(1);

                (word >> 8) as u8
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
}

#[derive(Debug, Clone, Copy)]
struct TileData<'vram> {
    tile_data: &'vram [u16],
    palette: u8,
    priority: bool,
    x_flip: bool,
    y_flip: bool,
}

fn get_bg_tile<'vram>(
    vram: &'vram Vram,
    registers: &Registers,
    cache: &mut Cache,
    bg: usize,
    x: u16,
    y: u16,
    bpp: BitsPerPixel,
) -> TileData<'vram> {
    let CachedBgMapEntry {
        tile_number: raw_tile_number, palette, priority, x_flip, y_flip, ..
    } = cache.get(bg, x, y).unwrap_or_else(|| {
        let tile_map_entry = get_bg_map_entry(vram, registers, bg, x, y);

        let tile_number = tile_map_entry & 0x3FF;
        let palette = ((tile_map_entry >> 10) & 0x07) as u8;
        let priority = tile_map_entry.bit(13);
        let x_flip = tile_map_entry.bit(14);
        let y_flip = tile_map_entry.bit(15);

        let entry = CachedBgMapEntry {
            map_x: x / 8,
            map_y: y / 8,
            tile_number,
            palette,
            priority,
            x_flip,
            y_flip,
        };
        cache.bg_map_entries[bg] = entry;
        entry
    });

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
    let tile_data = &vram[tile_addr..tile_addr + tile_size_words as usize];

    TileData { tile_data, palette, priority, x_flip, y_flip }
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
    bpp: BitsPerPixel,
    direct_color_mode: bool,
    two_bpp_offset: u8,
    palette: u8,
    color: u8,
) -> u16 {
    match bpp {
        BitsPerPixel::Two => cgram[(two_bpp_offset | (palette << 2) | color) as usize],
        BitsPerPixel::Four => cgram[((palette << 4) | color) as usize],
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
