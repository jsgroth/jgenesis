use crate::ppu;
use crate::ppu::Vram;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::cmp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VerticalDisplaySize {
    #[default]
    TwoTwentyFour,
    TwoThirtyNine,
}

impl VerticalDisplaySize {
    pub fn to_lines(self) -> u16 {
        match self {
            Self::TwoTwentyFour => 224,
            Self::TwoThirtyNine => 239,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitsPerPixel {
    // 4-color
    Two,
    // 16-color
    Four,
    // 256-color
    Eight,
}

impl BitsPerPixel {
    // BG3 and BG4 are always 2bpp
    pub const BG3: Self = Self::Two;
    pub const BG4: Self = Self::Two;

    // OBJ is always 4bpp
    pub const OBJ: Self = Self::Four;

    pub const fn bitplanes(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    pub const fn tile_size_words(self) -> u16 {
        match self {
            Self::Two => 8,
            Self::Four => 16,
            Self::Eight => 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BgMode {
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

    pub fn bg1_bpp(self) -> BitsPerPixel {
        use BitsPerPixel as BPP;

        match self {
            Self::Zero => BPP::Two,
            Self::One | Self::Two | Self::Five | Self::Six => BPP::Four,
            Self::Three | Self::Four | Self::Seven => BPP::Eight,
        }
    }

    pub fn bg2_enabled(self) -> bool {
        // BG2 is enabled in all modes except 6 and 7
        !matches!(self, Self::Six | Self::Seven)
    }

    pub fn bg2_bpp(self) -> BitsPerPixel {
        use BitsPerPixel as BPP;

        match self {
            Self::Zero | Self::Four | Self::Five => BPP::Two,
            Self::One | Self::Two | Self::Three => BPP::Four,
            // BG2 is not rendered in mode 6 or 7; return value doesn't matter
            Self::Six | Self::Seven => BPP::Eight,
        }
    }

    pub fn bg3_enabled(self) -> bool {
        // BG3 is only _really_ enabled in modes 0 and 1; modes 2/4/6 use it for offset-per-tile
        matches!(self, Self::Zero | Self::One)
    }

    pub fn bg4_enabled(self) -> bool {
        // BG4 is only enabled in mode 0
        self == Self::Zero
    }

    pub fn is_offset_per_tile(self) -> bool {
        // Modes 2/4/6 use BG3 map entries as offsets for BG1/BG2 tiles
        matches!(self, Self::Two | Self::Four | Self::Six)
    }

    pub fn is_hi_res(self) -> bool {
        matches!(self, Self::Five | Self::Six)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum TileSize {
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
pub enum ObjTileSize {
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

    pub fn small_size(self) -> (u16, u16) {
        match self {
            Self::Zero | Self::One | Self::Two => (8, 8),
            Self::Three | Self::Four => (16, 16),
            Self::Five => (32, 32),
            Self::Six | Self::Seven => (16, 32),
        }
    }

    pub fn large_size(self) -> (u16, u16) {
        match self {
            Self::Zero => (16, 16),
            Self::One | Self::Three | Self::Seven => (32, 32),
            Self::Two | Self::Four | Self::Five => (64, 64),
            Self::Six => (32, 64),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BgScreenSize {
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

    pub fn width_tiles(self) -> u16 {
        match self {
            Self::OneScreen | Self::HorizontalMirror => 32,
            Self::VerticalMirror | Self::FourScreen => 64,
        }
    }

    pub fn height_tiles(self) -> u16 {
        match self {
            Self::OneScreen | Self::VerticalMirror => 32,
            Self::HorizontalMirror | Self::FourScreen => 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WindowAreaMode {
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

    pub fn to_optional_bool(self, inside: bool) -> Option<bool> {
        match self {
            Self::Disabled => None,
            Self::Inside => Some(inside),
            Self::Outside => Some(!inside),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WindowMaskLogic {
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

    pub fn apply(self, window_1: Option<bool>, window_2: Option<bool>) -> bool {
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
pub enum ColorMathEnableMode {
    Never,
    OutsideColorWindow,
    InsideColorWindow,
    Always,
}

impl ColorMathEnableMode {
    pub fn enabled(self, in_color_window: bool) -> bool {
        match self {
            Self::Always => true,
            Self::InsideColorWindow => in_color_window,
            Self::OutsideColorWindow => !in_color_window,
            Self::Never => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ColorMathOperation {
    #[default]
    Add,
    Subtract,
}

impl ColorMathOperation {
    pub fn apply(self, a: u16, b: u16, divide: bool) -> u16 {
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
pub enum VramAddressTranslation {
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

    pub fn apply(self, vram_addr: u16) -> u16 {
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
pub enum VramIncrementMode {
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
pub enum ObjPriorityMode {
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
pub enum Mode7OobBehavior {
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
pub enum AccessFlipflop {
    #[default]
    First,
    Second,
}

impl AccessFlipflop {
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    // INIDISP
    pub forced_blanking: bool,
    pub brightness: u8,
    // SETINI
    pub extbg_enabled: bool,
    pub pseudo_h_hi_res: bool,
    pub pseudo_obj_hi_res: bool,
    pub v_display_size: VerticalDisplaySize,
    pub interlaced: bool,
    // TM
    pub main_bg_enabled: [bool; 4],
    pub main_obj_enabled: bool,
    // TS
    pub sub_bg_enabled: [bool; 4],
    pub sub_obj_enabled: bool,
    // BGMODE
    pub bg_mode: BgMode,
    pub mode_1_bg3_priority: bool,
    pub bg_tile_size: [TileSize; 4],
    // MOSAIC
    pub mosaic_size: u8,
    pub bg_mosaic_enabled: [bool; 4],
    // BG1SC/BG2SC/BG3SC/BG4SC
    pub bg_screen_size: [BgScreenSize; 4],
    pub bg_base_address: [u16; 4],
    // BG12NBA/BG34NBA
    pub bg_tile_base_address: [u16; 4],
    // BG1VOFS/BG2VOFS/BG3VOFS/BG4VOFS
    pub bg_h_scroll: [u16; 4],
    pub bg_v_scroll: [u16; 4],
    pub bg_scroll_write_buffer: u8,
    // OBSEL
    pub obj_tile_base_address: u16,
    pub obj_tile_gap_size: u16,
    pub obj_tile_size: ObjTileSize,
    // TMW
    pub main_bg_disabled_in_window: [bool; 4],
    pub main_obj_disabled_in_window: bool,
    // TSW
    pub sub_bg_disabled_in_window: [bool; 4],
    pub sub_obj_disabled_in_window: bool,
    // WH0/WH1/WH2/WH3
    pub window_1_left: u16,
    pub window_1_right: u16,
    pub window_2_left: u16,
    pub window_2_right: u16,
    // W12SEL/W34SEL/WOBJSEL
    pub bg_window_1_area: [WindowAreaMode; 4],
    pub bg_window_2_area: [WindowAreaMode; 4],
    pub obj_window_1_area: WindowAreaMode,
    pub obj_window_2_area: WindowAreaMode,
    pub math_window_1_area: WindowAreaMode,
    pub math_window_2_area: WindowAreaMode,
    // WBGLOG/WOBJLOG
    pub bg_window_mask_logic: [WindowMaskLogic; 4],
    pub obj_window_mask_logic: WindowMaskLogic,
    pub math_window_mask_logic: WindowMaskLogic,
    // CGWSEL
    pub force_main_screen_black: ColorMathEnableMode,
    pub color_math_enabled: ColorMathEnableMode,
    pub sub_bg_obj_enabled: bool,
    pub direct_color_mode_enabled: bool,
    // CGADSUB
    pub color_math_operation: ColorMathOperation,
    pub color_math_divide_enabled: bool,
    pub bg_color_math_enabled: [bool; 4],
    pub obj_color_math_enabled: bool,
    pub backdrop_color_math_enabled: bool,
    // COLDATA
    pub sub_backdrop_color: u16,
    // VMAIN
    pub vram_address_increment_step: u16,
    pub vram_address_translation: VramAddressTranslation,
    pub vram_address_increment_mode: VramIncrementMode,
    // VMADDL/VMADDH
    pub vram_address: u16,
    // RDVRAML/RDVRAMH
    pub vram_prefetch_buffer: u16,
    // OAMADDL/OAMADDH
    pub oam_address: u16,
    pub oam_address_reload_value: u16,
    pub obj_priority_mode: ObjPriorityMode,
    pub oam_write_buffer: u8,
    // CGADD
    pub cgram_address: u8,
    // CGDATA/RDCGRAM
    pub cgram_write_buffer: u8,
    pub cgram_flipflop: AccessFlipflop,
    // M7SEL
    pub mode_7_h_flip: bool,
    pub mode_7_v_flip: bool,
    pub mode_7_oob_behavior: Mode7OobBehavior,
    // M7A/M7B/M7C/M7D
    pub mode_7_parameter_a: u16,
    pub mode_7_parameter_b: u16,
    pub mode_7_parameter_c: u16,
    pub mode_7_parameter_d: u16,
    pub mode_7_write_buffer: u8,
    // M7HOFS/M7VOFS
    pub mode_7_h_scroll: u16,
    pub mode_7_v_scroll: u16,
    // M7X/M7Y
    pub mode_7_center_x: u16,
    pub mode_7_center_y: u16,
    // PPU multiply unit (M7A/M7B)
    pub multiply_operand_l: i16,
    pub multiply_operand_r: i8,
    // OPHCT/OPVCT
    pub latched_h_counter: u16,
    pub latched_v_counter: u16,
    pub new_hv_latched: bool,
    pub h_counter_flipflop: AccessFlipflop,
    pub v_counter_flipflop: AccessFlipflop,
    // Sprite overflow flags (readable in STAT77)
    pub sprite_overflow: bool,
    pub sprite_pixel_overflow: bool,
    // Copied from WRIO CPU register (needed for H/V counter latching)
    pub programmable_joypad_port: u8,
}

impl Registers {
    pub fn new() -> Self {
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

    pub fn write_inidisp(&mut self, value: u8) {
        // INIDISP: Display control 1
        let prev_forced_blanking = self.forced_blanking;
        self.forced_blanking = value.bit(7);
        self.brightness = value & 0x0F;

        // Disabling forced blanking immediately reloads OAM address
        if prev_forced_blanking && !self.forced_blanking {
            self.oam_address = self.oam_address_reload_value << 1;
        }

        log::trace!("  Forced blanking: {}", self.forced_blanking);
        log::trace!("  Brightness: {}", self.brightness);
    }

    pub fn write_setini(&mut self, value: u8) {
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

    pub fn write_tm(&mut self, value: u8) {
        // TM: Main screen designation
        for (i, bg_enabled) in self.main_bg_enabled.iter_mut().enumerate() {
            *bg_enabled = value.bit(i as u8);
        }
        self.main_obj_enabled = value.bit(4);

        log::trace!("  Main screen BG enabled: {:?}", self.main_bg_enabled);
        log::trace!("  Main screen OBJ enabled: {}", self.main_obj_enabled);
    }

    pub fn write_ts(&mut self, value: u8) {
        // TS: Sub screen designation
        for (i, bg_enabled) in self.sub_bg_enabled.iter_mut().enumerate() {
            *bg_enabled = value.bit(i as u8);
        }
        self.sub_obj_enabled = value.bit(4);

        log::trace!("  Sub screen BG enabled: {:?}", self.sub_bg_enabled);
        log::trace!("  Sub screen OBJ enabled: {:?}", self.sub_obj_enabled);
    }

    pub fn write_bgmode(&mut self, value: u8) {
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

    pub fn write_mosaic(&mut self, value: u8) {
        // MOSAIC: Mosaic size and enable
        self.mosaic_size = value >> 4;

        for (i, mosaic_enabled) in self.bg_mosaic_enabled.iter_mut().enumerate() {
            *mosaic_enabled = value.bit(i as u8);
        }

        log::trace!("  Mosaic size: {}", self.mosaic_size);
        log::trace!("  Mosaic enabled: {:?}", self.bg_mosaic_enabled);
    }

    pub fn write_bg1234sc(&mut self, bg: usize, value: u8) {
        // BG1SC/BG2SC/BG3SC/BG4SC: BG1-4 screen base and size
        self.bg_screen_size[bg] = BgScreenSize::from_byte(value);
        self.bg_base_address[bg] = u16::from(value & 0xFC) << 8;

        log::trace!("  BG{} screen size: {:?}", bg + 1, self.bg_screen_size[bg]);
        log::trace!("  BG{} base address: {:04X}", bg + 1, self.bg_base_address[bg]);
    }

    pub fn write_bg1234nba(&mut self, base_bg: usize, value: u8) {
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

    pub fn write_bg1hofs(&mut self, value: u8) {
        // BG1HOFS: BG1 horizontal scroll / M7HOFS: Mode 7 horizontal scroll
        self.write_bg_h_scroll(0, value);

        self.mode_7_h_scroll = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 H scroll: {:04X}", self.mode_7_h_scroll);
    }

    pub fn write_bg1vofs(&mut self, value: u8) {
        // BG1VOFS: BG1 vertical scroll / M7VOFS: Mode 7 vertical scroll
        self.write_bg_v_scroll(0, value);

        self.mode_7_v_scroll = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 V scroll: {:04X}", self.mode_7_v_scroll);
    }

    pub fn write_bg_h_scroll(&mut self, i: usize, value: u8) {
        let current = self.bg_h_scroll[i];
        let prev = self.bg_scroll_write_buffer;

        // H scroll formula from https://wiki.superfamicom.org/backgrounds
        self.bg_h_scroll[i] =
            (u16::from(value) << 8) | u16::from(prev & !0x07) | ((current >> 8) & 0x07);
        self.bg_scroll_write_buffer = value;

        log::trace!("  BG{} H scroll: {:04X}", i + 1, self.bg_h_scroll[i]);
    }

    pub fn write_bg_v_scroll(&mut self, i: usize, value: u8) {
        let prev = self.bg_scroll_write_buffer;

        self.bg_v_scroll[i] = u16::from_le_bytes([prev, value]);
        self.bg_scroll_write_buffer = value;

        log::trace!("  BG{} V scroll: {:04X}", i + 1, self.bg_v_scroll[i]);
    }

    pub fn write_m7sel(&mut self, value: u8) {
        // M7SEL: Mode 7 settings
        self.mode_7_h_flip = value.bit(0);
        self.mode_7_v_flip = value.bit(1);
        self.mode_7_oob_behavior = Mode7OobBehavior::from_byte(value);

        log::trace!("  Mode 7 H flip: {}", self.mode_7_h_flip);
        log::trace!("  Mode 7 V flip: {}", self.mode_7_v_flip);
        log::trace!("  Mode 7 OOB behavior: {:?}", self.mode_7_oob_behavior);
    }

    pub fn write_m7a(&mut self, value: u8) {
        // M7A: Mode 7 parameter A / multiply 16-bit operand
        self.mode_7_parameter_a = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.multiply_operand_l = i16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter A: {:04X}", self.mode_7_parameter_a);
    }

    pub fn write_m7b(&mut self, value: u8) {
        // M7B: Mode 7 parameter B / multiply 8-bit operand
        self.mode_7_parameter_b = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.multiply_operand_r = value as i8;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter B: {:04X}", self.mode_7_parameter_b);
    }

    pub fn write_m7c(&mut self, value: u8) {
        // M7C: Mode 7 parameter C
        self.mode_7_parameter_c = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter C: {:04X}", self.mode_7_parameter_c);
    }

    pub fn write_m7d(&mut self, value: u8) {
        // M7D: Mode 7 parameter D
        self.mode_7_parameter_d = u16::from_le_bytes([self.mode_7_write_buffer, value]);
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 parameter D: {:04X}", self.mode_7_parameter_d);
    }

    pub fn write_m7x(&mut self, value: u8) {
        // M7X: Mode 7 center X coordinate
        self.mode_7_center_x = u16::from_le_bytes([self.mode_7_write_buffer, value]) & 0x1FFF;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 center X: {:04X}", self.mode_7_center_x);
    }

    pub fn write_m7y(&mut self, value: u8) {
        // M7Y: Mode 7 center Y coordinate
        self.mode_7_center_y = u16::from_le_bytes([self.mode_7_write_buffer, value]) & 0x1FFF;
        self.mode_7_write_buffer = value;

        log::trace!("  Mode 7 center Y: {:04X}", self.mode_7_center_y);
    }

    pub fn write_obsel(&mut self, value: u8) {
        // OBSEL: Object size and base
        self.obj_tile_base_address = u16::from(value & 0x07) << 13;
        self.obj_tile_gap_size = u16::from(value & 0x18) << 9;
        self.obj_tile_size = ObjTileSize::from_byte(value);

        log::trace!("  OBJ tile base address: {:04X}", self.obj_tile_base_address);
        log::trace!("  OBJ tile gap size: {:04X}", self.obj_tile_gap_size);
        log::trace!("  OBJ tile size: {:?}", self.obj_tile_size);
    }

    pub fn write_w1234sel(&mut self, base_bg: usize, value: u8) {
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

    pub fn write_wobjsel(&mut self, value: u8) {
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

    pub fn write_wh0(&mut self, value: u8) {
        // WH0: Window 1 left position
        self.window_1_left = value.into();
        log::trace!("  Window 1 left: {value:02X}");
    }

    pub fn write_wh1(&mut self, value: u8) {
        // WH1: Window 1 right position
        self.window_1_right = value.into();
        log::trace!("  Window 1 right: {value:02X}");
    }

    pub fn write_wh2(&mut self, value: u8) {
        // WH2: Window 2 left position
        self.window_2_left = value.into();
        log::trace!("  Window 2 left: {value:02X}");
    }

    pub fn write_wh3(&mut self, value: u8) {
        // WH3: Window 2 right position
        self.window_2_right = value.into();
        log::trace!("  Window 2 right: {value:02X}");
    }

    pub fn write_wbglog(&mut self, value: u8) {
        // WBGLOG: Window BG mask logic
        for (i, mask_logic) in self.bg_window_mask_logic.iter_mut().enumerate() {
            *mask_logic = WindowMaskLogic::from_bits(value >> (2 * i));
        }

        log::trace!("  BG window mask logic: {:?}", self.bg_window_mask_logic);
    }

    pub fn write_wobjlog(&mut self, value: u8) {
        // WOBJLOG: Window OBJ/MATH mask logic
        self.obj_window_mask_logic = WindowMaskLogic::from_bits(value);
        self.math_window_mask_logic = WindowMaskLogic::from_bits(value >> 2);

        log::trace!("  OBJ window mask logic: {:?}", self.obj_window_mask_logic);
        log::trace!("  MATH window mask logic: {:?}", self.math_window_mask_logic);
    }

    pub fn write_tmw(&mut self, value: u8) {
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

    pub fn write_tsw(&mut self, value: u8) {
        // TSW: Window area sub screen disable
        for (i, bg_disabled) in self.sub_bg_disabled_in_window.iter_mut().enumerate() {
            *bg_disabled = value.bit(i as u8);
        }
        self.sub_obj_disabled_in_window = value.bit(4);

        log::trace!("  Sub screen BG disabled inside window: {:?}", self.sub_bg_disabled_in_window);
        log::trace!("  Sub screen OBJ disabled inside window: {}", self.sub_obj_disabled_in_window);
    }

    pub fn write_cgwsel(&mut self, value: u8) {
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

    pub fn write_cgadsub(&mut self, value: u8) {
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

    pub fn write_coldata(&mut self, value: u8) {
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

    pub fn write_oamaddl(&mut self, value: u8) {
        // OAMADDL: OAM address, low byte
        let reload_value = (self.oam_address_reload_value & 0xFF00) | u16::from(value);
        self.oam_address_reload_value = reload_value;
        self.oam_address = reload_value << 1;

        log::trace!("  OAM address reload value: {:04X}", self.oam_address_reload_value);
    }

    pub fn write_oamaddh(&mut self, value: u8) {
        // OAMADDH: OAM address, high byte
        let reload_value =
            (self.oam_address_reload_value & 0x00FF) | (u16::from(value & 0x01) << 8);
        self.oam_address_reload_value = reload_value;
        self.oam_address = reload_value << 1;

        self.obj_priority_mode = ObjPriorityMode::from_byte(value);

        log::trace!("  OAM address reload value: {:04X}", self.oam_address_reload_value);
        log::trace!("  OBJ priority mode: {:?}", self.obj_priority_mode);
    }

    pub fn write_vmain(&mut self, value: u8) {
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

    pub fn write_vmaddl(&mut self, value: u8, vram: &Vram) {
        // VMADDL: VRAM address, low byte
        self.vram_address = (self.vram_address & 0xFF00) | u16::from(value);
        self.vram_prefetch_buffer = vram[(self.vram_address & ppu::VRAM_ADDRESS_MASK) as usize];

        log::trace!("  VRAM data port address: {:04X}", self.vram_address);
    }

    pub fn write_vmaddh(&mut self, value: u8, vram: &Vram) {
        // VMADDH: VRAM address, high byte
        self.vram_address = (self.vram_address & 0x00FF) | (u16::from(value) << 8);
        self.vram_prefetch_buffer = vram[(self.vram_address & ppu::VRAM_ADDRESS_MASK) as usize];

        log::trace!("  VRAM data port address: {:04X}", self.vram_address);
    }

    pub fn write_cgadd(&mut self, value: u8) {
        // CGADD: CGRAM address
        self.cgram_address = value;
        self.cgram_flipflop = AccessFlipflop::First;

        log::trace!("  CGRAM data port address: {value:02X}");
    }

    pub fn read_mpyl(&self) -> u8 {
        // MPYL: PPU multiply result, low byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        mpy_result as u8
    }

    pub fn read_mpym(&self) -> u8 {
        // MPYM: PPU multiply result, middle byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        (mpy_result >> 8) as u8
    }

    pub fn read_mpyh(&self) -> u8 {
        // MPYH: PPU multiply result, high byte
        let mpy_result = i32::from(self.multiply_operand_l) * i32::from(self.multiply_operand_r);
        (mpy_result >> 16) as u8
    }

    pub fn read_slhv(&mut self, h_counter: u16, v_counter: u16) {
        if self.programmable_joypad_port.bit(7) {
            self.latched_h_counter = h_counter;
            self.latched_v_counter = v_counter;
        }
    }

    pub fn read_ophct(&mut self, ppu2_open_bus: u8) -> u8 {
        // Bits 1-7 of high byte are PPU2 open bus
        let value = match self.h_counter_flipflop {
            AccessFlipflop::First => self.latched_h_counter as u8,
            AccessFlipflop::Second => (ppu2_open_bus & 0xFE) | (self.latched_h_counter >> 8) as u8,
        };
        self.h_counter_flipflop = self.h_counter_flipflop.toggle();
        value
    }

    pub fn read_opvct(&mut self, ppu2_open_bus: u8) -> u8 {
        // Bits 1-7 of high byte are PPU2 open bus
        let value = match self.v_counter_flipflop {
            AccessFlipflop::First => self.latched_v_counter as u8,
            AccessFlipflop::Second => (ppu2_open_bus & 0xFE) | (self.latched_v_counter >> 8) as u8,
        };
        self.v_counter_flipflop = self.v_counter_flipflop.toggle();
        value
    }

    pub fn reset_hv_counter_flipflops(&mut self) {
        self.h_counter_flipflop = AccessFlipflop::First;
        self.v_counter_flipflop = AccessFlipflop::First;
    }

    pub fn update_wrio(&mut self, wrio: u8, h_counter: u16, v_counter: u16) {
        if self.programmable_joypad_port.bit(7) & !wrio.bit(7) {
            self.latched_h_counter = h_counter;
            self.latched_v_counter = v_counter;
        }
        self.programmable_joypad_port = wrio;
    }

    #[allow(clippy::range_plus_one)]
    pub fn is_inside_window_1(&self, pixel: u16) -> bool {
        (self.window_1_left..self.window_1_right + 1).contains(&pixel)
    }

    #[allow(clippy::range_plus_one)]
    pub fn is_inside_window_2(&self, pixel: u16) -> bool {
        (self.window_2_left..self.window_2_right + 1).contains(&pixel)
    }

    pub fn bg_in_window(&self, bg: usize, in_window_1: bool, in_window_2: bool) -> bool {
        self.bg_window_mask_logic[bg].apply(
            self.bg_window_1_area[bg].to_optional_bool(in_window_1),
            self.bg_window_2_area[bg].to_optional_bool(in_window_2),
        )
    }

    pub fn in_hi_res_mode(&self) -> bool {
        self.bg_mode.is_hi_res() || self.pseudo_h_hi_res
    }
}
