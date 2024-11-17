use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BgMode {
    // Tile map, 4 BGs in text mode
    #[default]
    Zero = 0,
    // Tile map, 2 BGs in text mode and 1 BG in rotation/scaling mode
    One = 1,
    // Tile map, 2 BGs in rotation/scaling mode
    Two = 2,
    // Bitmap, 1 frame buffer with 15bpp pixels
    Three = 3,
    // Bitmap, 2 frame buffers with 8bpp pixels
    Four = 4,
    // Bitmap, 2 reduced-size frame buffers with 15bpp pixels
    Five = 5,
}

impl BgMode {
    fn from_bits(bits: u16) -> Self {
        match bits & 7 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 | 7 => {
                log::error!("Invalid BG mode (0-5): {}", bits & 7);
                Self::Zero
            }
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ObjTileLayout {
    // 32x32 4bpp tiles / 16x32 8bpp tiles
    #[default]
    TwoD = 0,
    // 1024x1 4bpp tiles / 512x1 8bpp tiles
    OneD = 1,
}

impl ObjTileLayout {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::OneD } else { Self::TwoD }
    }
}

impl Display for ObjTileLayout {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TwoD => write!(f, "2D"),
            Self::OneD => write!(f, "1D"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    // DISPCNT: PPU control
    pub bg_mode: BgMode,
    pub bitmap_frame_buffer_1: bool,
    pub oam_free_during_hblank: bool,
    pub obj_tile_layout: ObjTileLayout,
    pub forced_blanking: bool,
    pub bg_enabled: [bool; 4],
    pub obj_enabled: bool,
    pub window_0_enabled: bool,
    pub window_1_enabled: bool,
    pub obj_window_enabled: bool,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            bg_mode: BgMode::default(),
            bitmap_frame_buffer_1: false,
            oam_free_during_hblank: false,
            obj_tile_layout: ObjTileLayout::default(),
            forced_blanking: true,
            bg_enabled: [false; 4],
            obj_enabled: false,
            window_0_enabled: false,
            window_1_enabled: false,
            obj_window_enabled: false,
        }
    }

    // $04000000: DISPCNT (Display control)
    pub fn read_dispcnt(&self) -> u16 {
        (self.bg_mode as u16)
            | (u16::from(self.bitmap_frame_buffer_1) << 4)
            | (u16::from(self.oam_free_during_hblank) << 5)
            | ((self.obj_tile_layout as u16) << 6)
            | (u16::from(self.forced_blanking) << 7)
            | (u16::from(self.bg_enabled[0]) << 8)
            | (u16::from(self.bg_enabled[1]) << 9)
            | (u16::from(self.bg_enabled[2]) << 10)
            | (u16::from(self.bg_enabled[3]) << 11)
            | (u16::from(self.obj_enabled) << 12)
            | (u16::from(self.window_0_enabled) << 13)
            | (u16::from(self.window_1_enabled) << 14)
            | (u16::from(self.obj_window_enabled) << 15)
    }

    // $04000000: DISPCNT (Display control)
    pub fn write_dispcnt(&mut self, value: u16) {
        self.bg_mode = BgMode::from_bits(value);
        self.bitmap_frame_buffer_1 = value.bit(4);
        self.oam_free_during_hblank = value.bit(5);
        self.obj_tile_layout = ObjTileLayout::from_bit(value.bit(6));
        self.forced_blanking = value.bit(7);
        self.bg_enabled = array::from_fn(|i| value.bit((8 + i) as u8));
        self.obj_enabled = value.bit(12);
        self.window_0_enabled = value.bit(13);
        self.window_1_enabled = value.bit(14);
        self.obj_window_enabled = value.bit(15);

        log::trace!("DISPCNT write: {value:04X}");
        log::trace!("  BG mode: {:?}", self.bg_mode);
        log::trace!(
            "  Mode 4/5 displayed frame buffer: {}",
            if self.bitmap_frame_buffer_1 { "1" } else { "0" }
        );
        log::trace!("  OAM accessible during HBlank: {}", self.oam_free_during_hblank);
        log::trace!("  OBJ tile data area layout: {}", self.obj_tile_layout);
        log::trace!("  Forced blanking: {}", self.forced_blanking);
        log::trace!("  BG layers enabled: {:?}", self.bg_enabled);
        log::trace!("  OBJ layer enabled: {}", self.obj_enabled);
        log::trace!("  Window 0 enabled: {}", self.window_0_enabled);
        log::trace!("  Window 1 enabled: {}", self.window_1_enabled);
        log::trace!("  OBJ window enabled: {}", self.obj_window_enabled);
    }
}
