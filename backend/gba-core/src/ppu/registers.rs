use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::array;

macro_rules! define_bit_enum {
    ($name:ident, [$zero:ident, $one:ident]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
        pub enum $name {
            #[default]
            $zero = 0,
            $one = 1,
        }

        impl $name {
            fn from_bit(bit: bool) -> Self {
                if bit { Self::$one } else { Self::$zero }
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BgMode {
    // Mode 0: 4 tile map BGs
    #[default]
    Zero,
    // Mode 1: 2 tile map BGs, 1 affine BG
    One,
    // Mode 2: 2 affine BGs
    Two,
    // Mode 3: 32768-color bitmap, single frame buffer
    Three,
    // Mode 4: 256-color bitmap, page flipped frame buffers
    Four,
    // Mode 5: 32768-color bitmap, page flipped frame buffers (reduced size)
    Five,
    Invalid(u8),
}

impl BgMode {
    fn to_bits(self) -> u8 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Five => 5,
            Self::Invalid(bits) => bits,
        }
    }

    fn from_bits(bits: u16) -> Self {
        match bits & 7 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            b @ (6 | 7) => Self::Invalid(b as u8),
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }
}

define_bit_enum!(BitmapFrameBuffer, [Zero, One]);
define_bit_enum!(ObjVramMapDimensions, [Two, One]);

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    // DISPCNT (Display control)
    pub bg_mode: BgMode,
    pub bitmap_frame_buffer: BitmapFrameBuffer,
    pub oam_free_during_hblank: bool,
    pub obj_vram_map_dimensions: ObjVramMapDimensions,
    pub forced_blanking: bool,
    pub bg_enabled: [bool; 4],
    pub obj_enabled: bool,
    pub window_enabled: [bool; 2],
    pub obj_window_enabled: bool,
    // DISPSTAT (Display status)
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub v_counter_irq_enabled: bool,
    pub v_counter_match: u8,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            bg_mode: BgMode::default(),
            bitmap_frame_buffer: BitmapFrameBuffer::default(),
            oam_free_during_hblank: false,
            obj_vram_map_dimensions: ObjVramMapDimensions::default(),
            forced_blanking: true,
            bg_enabled: [false; 4],
            obj_enabled: false,
            window_enabled: [false; 2],
            obj_window_enabled: false,
            vblank_irq_enabled: false,
            hblank_irq_enabled: false,
            v_counter_irq_enabled: false,
            v_counter_match: 255,
        }
    }

    // $4000000: DISPCNT (Display control)
    pub fn write_dispcnt(&mut self, value: u16) {
        self.bg_mode = BgMode::from_bits(value);
        self.bitmap_frame_buffer = BitmapFrameBuffer::from_bit(value.bit(4));
        self.oam_free_during_hblank = value.bit(5);
        self.obj_vram_map_dimensions = ObjVramMapDimensions::from_bit(value.bit(6));
        self.forced_blanking = value.bit(7);
        self.bg_enabled = array::from_fn(|i| value.bit((8 + i) as u8));
        self.obj_enabled = value.bit(12);
        self.window_enabled = [value.bit(13), value.bit(14)];
        self.obj_window_enabled = value.bit(15);

        log::debug!("DISPCNT write: {value:04X}");
        log::debug!("  BG mode: {:?}", self.bg_mode);
        log::debug!("  Bitmap frame buffer: {:?}", self.bitmap_frame_buffer);
        log::debug!("  OAM accessible during HBlank: {}", self.oam_free_during_hblank);
        log::debug!("  OBJ VRAM map dimensions: {:?}", self.obj_vram_map_dimensions);
        log::debug!("  Forced blanking enabled: {}", self.forced_blanking);
        log::debug!("  BGs enabled: {:?}", self.bg_enabled);
        log::debug!("  OBJ enabled: {}", self.obj_enabled);
        log::debug!("  Window 0 enabled: {}", self.window_enabled[0]);
        log::debug!("  Window 1 enabled: {}", self.window_enabled[1]);
        log::debug!("  OBJ window enabled: {}", self.obj_window_enabled);
    }

    // $4000000: DISPCNT (Display control)
    pub fn read_dispcnt(&self) -> u16 {
        let bg_enabled_bits =
            (0..4).map(|i| u16::from(self.bg_enabled[i]) << (8 + i)).reduce(|a, b| a | b).unwrap();

        u16::from(self.bg_mode.to_bits())
            | ((self.bitmap_frame_buffer as u16) << 4)
            | (u16::from(self.oam_free_during_hblank) << 5)
            | ((self.obj_vram_map_dimensions as u16) << 6)
            | (u16::from(self.forced_blanking) << 7)
            | bg_enabled_bits
            | (u16::from(self.obj_enabled) << 12)
            | (u16::from(self.window_enabled[0]) << 13)
            | (u16::from(self.window_enabled[1]) << 14)
            | (u16::from(self.obj_window_enabled) << 15)
    }

    // $4000004: DISPSTAT (Display status)
    pub fn write_dispstat(&mut self, value: u16) {
        self.vblank_irq_enabled = value.bit(3);
        self.hblank_irq_enabled = value.bit(4);
        self.v_counter_irq_enabled = value.bit(5);
        self.v_counter_match = value.msb();

        log::debug!("DISPSTAT write: {value:04X}");
        log::debug!("  VBlank IRQs enabled: {}", self.vblank_irq_enabled);
        log::debug!("  HBlank IRQs enabled: {}", self.hblank_irq_enabled);
        log::debug!("  V counter IRQs enabled: {}", self.v_counter_irq_enabled);
        log::debug!("  V counter match target: {}", self.v_counter_match);
    }
}
