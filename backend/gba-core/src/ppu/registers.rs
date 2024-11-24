use crate::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::fmt::{Display, Formatter};
use std::{array, cmp};

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

    pub fn bg_enabled(self, bg: usize) -> bool {
        assert!(bg < 4);
        match bg {
            // BG0 and BG1 enabled in modes 0 and 1
            0 | 1 => matches!(self, Self::Zero | Self::One),
            // BG2 always enabled
            2 => true,
            // BG3 enabled in modes 0 and 2
            3 => matches!(self, Self::Zero | Self::Two),
            _ => unreachable!("asserted bg < 4"),
        }
    }

    pub fn is_bitmap(self) -> bool {
        matches!(self, Self::Three | Self::Four | Self::Five)
    }

    pub fn is_15bpp_bitmap(self) -> bool {
        matches!(self, Self::Three | Self::Five)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ColorDepthBits {
    // 4bpp (16 palettes of 16 colors each)
    #[default]
    Four = 0,
    // 8bpp (1 palette of 256 colors)
    Eight = 1,
}

impl ColorDepthBits {
    pub fn from_bit(bit: bool) -> Self {
        if bit { Self::Eight } else { Self::Four }
    }

    pub fn tile_size_bytes(self) -> u32 {
        match self {
            Self::Four => 32,
            Self::Eight => 64,
        }
    }
}

impl Display for ColorDepthBits {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Four => write!(f, "4bpp"),
            Self::Eight => write!(f, "8bpp"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum AffineOverflowBehavior {
    #[default]
    Transparent = 0,
    Wrap = 1,
}

impl AffineOverflowBehavior {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Wrap } else { Self::Transparent }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BgScreenSize {
    // 256x256 in text mode, 128x128 in rotation/scaling mode
    #[default]
    Zero = 0,
    // 512x256 in text mode, 256x256 in rotation/scaling mode
    One = 1,
    // 256x512 in text mode, 512x512 in rotation/scaling mode
    Two = 2,
    // 512x512 in text mode, 1024x1024 in rotation/scaling mode
    Three = 3,
}

impl BgScreenSize {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    pub fn tile_map_width_pixels(self) -> u32 {
        match self {
            Self::Zero | Self::Two => 256,
            Self::One | Self::Three => 512,
        }
    }

    pub fn tile_map_height_pixels(self) -> u32 {
        match self {
            Self::Zero | Self::One => 256,
            Self::Two | Self::Three => 512,
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct BgControl {
    // BGxCNT (BG control)
    pub priority: u8,
    pub tile_data_base_addr: u32,
    pub mosaic: bool,
    pub color_depth: ColorDepthBits,
    pub tile_map_base_addr: u32,
    pub affine_overflow: AffineOverflowBehavior,
    pub screen_size: BgScreenSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BlendMode {
    #[default]
    None = 0,
    AlphaBlending = 1,
    IncreaseBrightness = 2,
    DecreaseBrightness = 3,
}

impl BlendMode {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::None,
            1 => Self::AlphaBlending,
            2 => Self::IncreaseBrightness,
            3 => Self::DecreaseBrightness,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Registers {
    // DISPCNT: Display control
    pub bg_mode: BgMode,
    pub bitmap_frame_buffer_1: bool,
    pub oam_free_during_hblank: bool,
    pub obj_tile_layout: ObjTileLayout,
    pub forced_blanking: bool,
    pub bg_enabled: [bool; 4],
    pub obj_enabled: bool,
    pub window_enabled: [bool; 2],
    pub obj_window_enabled: bool,
    // DISPSTAT: Display status and interrupt control
    pub vblank_irq_enabled: bool,
    pub hblank_irq_enabled: bool,
    pub v_counter_irq_enabled: bool,
    pub v_counter_target: u32,
    // BG0CNT/BG1CNT/BG2CNT/BG3CNT: BG0-3 control
    pub bg_control: [BgControl; 4],
    // BG0HOFS/BG1HOFS/BG2HOFS/BG3HOFS: BG0-3 horizontal offset
    pub bg_h_scroll: [u32; 4],
    // BG0VOFS/BG1VOFS/BG2VOFS/BG3VOFS: BG0-3 vertical offset
    pub bg_v_scroll: [u32; 4],
    // WIN0H/WIN1H: Window 0/1 horizontal coordinates
    pub window_x1: [u32; 2],
    pub window_x2: [u32; 2],
    // WIN0V/WIN1V: Window 0/1 vertical coordinates
    pub window_y1: [u32; 2],
    pub window_y2: [u32; 2],
    // WININ: Window inside control
    pub window_in_bg_enabled: [[bool; 4]; 2],
    pub window_in_obj_enabled: [bool; 2],
    pub window_in_color_enabled: [bool; 2],
    // WINOUT: Window outside control and OBJ window inside control
    pub window_out_bg_enabled: [bool; 4],
    pub window_out_obj_enabled: bool,
    pub window_out_color_enabled: bool,
    pub obj_window_bg_enabled: [bool; 4],
    pub obj_window_obj_enabled: bool,
    pub obj_window_color_enabled: bool,
    // BLDCNT: Blend control
    pub bg_1st_target: [bool; 4],
    pub obj_1st_target: bool,
    pub backdrop_1st_target: bool,
    pub bg_2nd_target: [bool; 4],
    pub obj_2nd_target: bool,
    pub backdrop_2nd_target: bool,
    pub blend_mode: BlendMode,
    // BLDALPHA: Alpha blending coefficients
    pub alpha_1st: u16,
    pub alpha_2nd: u16,
    // BLDY: Brightness coefficient
    pub brightness: u16,
}

impl Registers {
    pub fn new() -> Self {
        Self { forced_blanking: true, ..Self::default() }
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
            | (u16::from(self.window_enabled[0]) << 13)
            | (u16::from(self.window_enabled[1]) << 14)
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
        self.window_enabled = [value.bit(13), value.bit(14)];
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
        log::trace!("  Window 0 enabled: {}", self.window_enabled[0]);
        log::trace!("  Window 1 enabled: {}", self.window_enabled[1]);
        log::trace!("  OBJ window enabled: {}", self.obj_window_enabled);
    }

    // $04000004: DISPSTAT (Display status)
    pub fn read_dispstat(&self, in_vblank: bool, in_hblank: bool, v_counter: u32) -> u16 {
        let v_counter_match = v_counter == self.v_counter_target;

        (u16::from(in_vblank))
            | (u16::from(in_hblank) << 1)
            | (u16::from(v_counter_match) << 2)
            | (u16::from(self.vblank_irq_enabled) << 3)
            | (u16::from(self.hblank_irq_enabled) << 4)
            | (u16::from(self.v_counter_irq_enabled) << 5)
            | ((self.v_counter_target << 8) as u16)
    }

    // $04000004: DISPSTAT (Display status and interrupt control)
    pub fn write_dispstat(&mut self, value: u16) {
        self.vblank_irq_enabled = value.bit(3);
        self.hblank_irq_enabled = value.bit(4);
        self.v_counter_irq_enabled = value.bit(5);
        self.v_counter_target = (value >> 8).into();

        log::trace!("DISPSTAT write: {value:04X}");
        log::trace!("  VBlank IRQ enabled: {}", self.vblank_irq_enabled);
        log::trace!("  HBlank IRQ enabled: {}", self.hblank_irq_enabled);
        log::trace!("  V counter match IRQ enabled: {}", self.v_counter_irq_enabled);
        log::trace!("  V counter match value: {}", self.v_counter_target);
    }

    // $04000008: BG0CNT (BG0 control)
    // $0400000A: BG1CNT (BG1 control)
    // $0400000C: BG2CNT (BG2 control)
    // $0400000E: BG3CNT (BG3 control)
    pub fn read_bgcnt(&self, bg: usize) -> u16 {
        let bg_control = &self.bg_control[bg];

        u16::from(bg_control.priority)
            | (((bg_control.tile_data_base_addr >> 14) << 2) as u16)
            | (u16::from(bg_control.mosaic) << 6)
            | ((bg_control.color_depth as u16) << 7)
            | (((bg_control.tile_map_base_addr >> 11) << 8) as u16)
            | ((bg_control.affine_overflow as u16) << 13)
            | ((bg_control.screen_size as u16) << 14)
    }

    // $04000008: BG0CNT (BG0 control)
    // $0400000A: BG1CNT (BG1 control)
    // $0400000C: BG2CNT (BG2 control)
    // $0400000E: BG3CNT (BG3 control)
    pub fn write_bgcnt(&mut self, bg: usize, value: u16) {
        let bg_control = &mut self.bg_control[bg];

        bg_control.priority = (value & 3) as u8;

        // Tile data base address is in 16KB units (2^14)
        bg_control.tile_data_base_addr = u32::from((value >> 2) & 3) << 14;

        bg_control.mosaic = value.bit(6);
        bg_control.color_depth = ColorDepthBits::from_bit(value.bit(7));

        // Tile map base address is in 2KB units (2^11)
        bg_control.tile_map_base_addr = u32::from((value >> 8) & 0x1F) << 11;

        bg_control.affine_overflow = AffineOverflowBehavior::from_bit(value.bit(13));
        bg_control.screen_size = BgScreenSize::from_bits(value >> 14);

        log::trace!("BG{bg}CNT write: {value:04X}");
        log::trace!("  Priority: {}", bg_control.priority);
        log::trace!("  Tile data base address: ${:05X}", bg_control.tile_data_base_addr);
        log::trace!("  Mosaic: {}", bg_control.mosaic);
        log::trace!("  Color depth: {}", bg_control.color_depth);
        log::trace!("  Tile map base address: ${:05X}", bg_control.tile_map_base_addr);
        log::trace!("  Rotation/scaling overflow behavior: {:?}", bg_control.affine_overflow);
        log::trace!("  Screen size bits: {}", bg_control.screen_size as u8);
    }

    // $04000010: BG0HOFS (BG0 horizontal offset)
    // $04000014: BG1HOFS (BG1 horizontal offset)
    // $04000018: BG2HOFS (BG2 horizontal offset)
    // $0400001C: BG3HOFS (BG3 horizontal offset)
    pub fn write_bghofs(&mut self, bg: usize, value: u16) {
        self.bg_h_scroll[bg] = (value & 0x1FF).into();

        log::trace!("BG{bg}HOFS write: {value:04X}");
        log::trace!("  Horizontal offset: {}", self.bg_h_scroll[bg]);
    }

    // $04000012: BG0VOFS (BG0 horizontal offset)
    // $04000016: BG1VOFS (BG1 horizontal offset)
    // $0400001A: BG2VOFS (BG2 horizontal offset)
    // $0400001E: BG3VOFS (BG3 horizontal offset)
    pub fn write_bgvofs(&mut self, bg: usize, value: u16) {
        self.bg_v_scroll[bg] = (value & 0x1FF).into();

        log::trace!("BG{bg}VOFS write: {value:04X}");
        log::trace!("  Vertical offset: {}", self.bg_v_scroll[bg]);
    }

    // $04000040: WIN0H (Window 0 horizontal coordinates)
    // $04000042: WIN1H (Window 1 horizontal coordinates)
    pub fn write_winh(&mut self, window: usize, value: u16) {
        let [mut x2, x1] = value.to_le_bytes();

        // Invalid X2 coordinates force X2=240
        if x2 > SCREEN_WIDTH as u8 || x2 < x1 {
            x2 = SCREEN_WIDTH as u8;
        }

        self.window_x1[window] = x1.into();
        self.window_x2[window] = x2.into();

        log::trace!("WIN{window}H write: {value:04X}");
        log::trace!("  X1: {x1}");
        log::trace!("  X2: {x2}");
    }

    // $04000044: WIN0V (Window 0 vertical coordinates)
    // $04000046: WIN1V (Window 1 vertical coordinates)
    pub fn write_winv(&mut self, window: usize, value: u16) {
        let [mut y2, y1] = value.to_le_bytes();

        // Invalid Y2 coordinates force Y2=160
        if y2 > SCREEN_HEIGHT as u8 || y2 < y1 {
            y2 = SCREEN_HEIGHT as u8;
        }

        self.window_y1[window] = y1.into();
        self.window_y2[window] = y2.into();

        log::trace!("WIN{window}V write: {value:04X}");
        log::trace!("  Y1: {y1}");
        log::trace!("  Y2: {y2}");
    }

    // $04000048: WININ (Window inside control)
    pub fn read_winin(&self) -> u16 {
        let mut bg_bits = 0;
        for bg in 0..4 {
            bg_bits |= u16::from(self.window_in_bg_enabled[0][bg]) << bg;
            bg_bits |= u16::from(self.window_in_bg_enabled[1][bg]) << (8 + bg);
        }

        bg_bits
            | (u16::from(self.window_in_obj_enabled[0]) << 4)
            | (u16::from(self.window_in_color_enabled[0]) << 5)
            | (u16::from(self.window_in_obj_enabled[1]) << 12)
            | (u16::from(self.window_in_color_enabled[1]) << 13)
    }

    // $04000048: WININ (Window inside control)
    pub fn write_winin(&mut self, value: u16) {
        self.window_in_bg_enabled =
            array::from_fn(|window| array::from_fn(|bg| value.bit((8 * window + bg) as u8)));
        self.window_in_obj_enabled = [value.bit(4), value.bit(12)];
        self.window_in_color_enabled = [value.bit(5), value.bit(13)];

        log::trace!("WININ write: {value:04X}");
        log::trace!("  Window 0 inside BG enabled: {:?}", self.window_in_bg_enabled[0]);
        log::trace!("  Window 0 inside OBJ enabled: {}", self.window_in_obj_enabled[0]);
        log::trace!("  Window 0 inside color effects enabled: {}", self.window_in_color_enabled[0]);
        log::trace!("  Window 1 inside BG enabled: {:?}", self.window_in_bg_enabled[1]);
        log::trace!("  Window 1 inside OBJ enabled: {}", self.window_in_obj_enabled[1]);
        log::trace!("  Window 1 inside color effects enabled: {}", self.window_in_color_enabled[1]);
    }

    // $0400004A: WINOUT (Window outside control and OBJ window inside control)
    pub fn read_winout(&self) -> u16 {
        let mut bg_bits = 0;
        for bg in 0..4 {
            bg_bits |= u16::from(self.window_out_bg_enabled[bg]) << bg;
            bg_bits |= u16::from(self.obj_window_bg_enabled[bg]) << (8 + bg);
        }

        bg_bits
            | (u16::from(self.window_out_obj_enabled) << 4)
            | (u16::from(self.window_out_color_enabled) << 5)
            | (u16::from(self.obj_window_obj_enabled) << 12)
            | (u16::from(self.obj_window_color_enabled) << 13)
    }

    // $0400004A: WINOUT (Window outside control and OBJ window inside control)
    pub fn write_winout(&mut self, value: u16) {
        self.window_out_bg_enabled = array::from_fn(|bg| value.bit(bg as u8));
        self.window_out_obj_enabled = value.bit(4);
        self.window_out_color_enabled = value.bit(5);
        self.obj_window_bg_enabled = array::from_fn(|bg| value.bit((8 + bg) as u8));
        self.obj_window_obj_enabled = value.bit(12);
        self.obj_window_color_enabled = value.bit(13);

        log::trace!("WINOUT write: {value:04X}");
        log::trace!("  Window outside BG enabled: {:?}", self.window_out_bg_enabled);
        log::trace!("  Window outside OBJ enabled: {}", self.window_out_obj_enabled);
        log::trace!("  Window outside color effects enabled: {}", self.window_out_color_enabled);
        log::trace!("  OBJ window inside BG enabled: {:?}", self.obj_window_bg_enabled);
        log::trace!("  OBJ window inside OBJ enabled: {}", self.obj_window_obj_enabled);
        log::trace!("  OBJ window inside color effects enabled: {}", self.obj_window_color_enabled);
    }

    // $04000050: BLDCNT (Blend control)
    pub fn write_bldcnt(&mut self, value: u16) {
        self.bg_1st_target = array::from_fn(|i| value.bit(i as u8));
        self.obj_1st_target = value.bit(4);
        self.backdrop_1st_target = value.bit(5);
        self.blend_mode = BlendMode::from_bits(value >> 6);
        self.bg_2nd_target = array::from_fn(|i| value.bit((8 + i) as u8));
        self.obj_2nd_target = value.bit(12);
        self.backdrop_2nd_target = value.bit(13);

        log::trace!("BLDCNT write: {value:04X}");
        log::trace!("  Blend mode: {:?}", self.blend_mode);
        log::trace!("  BG 1st target enabled: {:?}", self.bg_1st_target);
        log::trace!("  OBJ 1st target enabled: {}", self.obj_1st_target);
        log::trace!("  Backdrop 1st target enabled: {}", self.backdrop_1st_target);
        log::trace!("  BG 2nd target enabled: {:?}", self.bg_2nd_target);
        log::trace!("  OBJ 2nd target enabled: {}", self.obj_2nd_target);
        log::trace!("  Backdrop 2nd target enabled: {}", self.backdrop_2nd_target);
    }

    // $04000052: BLDALPHA (Alpha blending coefficients)
    pub fn write_bldalpha(&mut self, value: u16) {
        self.alpha_1st = cmp::min(16, value & 0x1F);
        self.alpha_2nd = cmp::min(16, (value >> 8) & 0x1F);

        log::trace!("BLDALPHA write: {value:04X}");
        log::trace!("  1st target coefficient: {}/16", self.alpha_1st);
        log::trace!("  2nd target coefficient: {}/16", self.alpha_2nd);
    }

    // $04000054: BLDY (Brightness coefficient)
    pub fn write_bldy(&mut self, value: u16) {
        self.brightness = value & 0x1F;
        log::trace!("BLDY write: {value:04X}");
        log::trace!("  Brightness coefficient: {}/16", self.brightness);
    }

    pub fn any_window_enabled(&self) -> bool {
        self.window_enabled[0] || self.window_enabled[1] || self.obj_window_enabled
    }
}
