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
    #[default]
    Zero, // 4 tile map BGs
    One,   // 2 tile map BGs + 1 affine BG
    Two,   // 2 affine BGs
    Three, // 15bpp bitmap, one frame buffer
    Four,  // 8bpp bitmap, two frame buffers
    Five,  // 15bpp bitmap, two frame buffers (reduced resolution)
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
define_bit_enum!(BitsPerPixel, [Four, Eight]);
define_bit_enum!(AffineOverflowBehavior, [Transparent, Wrap]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ScreenSize {
    #[default]
    Zero = 0, // 256x256 tile map / 128x128 affine
    One = 1,   // 512x256 tile map / 256x256 affine
    Two = 2,   // 256x512 tile map / 512x512 affine
    Three = 3, // 512x512 tile map / 1024x1024 affine
}

impl ScreenSize {
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct BgControl {
    pub priority: u8,
    pub tile_data_addr: u16,
    pub mosaic: bool,
    pub bpp: BitsPerPixel,
    pub tile_map_addr: u16,
    pub affine_overflow: AffineOverflowBehavior,
    pub size: ScreenSize,
}

impl BgControl {
    fn read(&self) -> u16 {
        u16::from(self.priority)
            | ((self.tile_data_addr >> 14) << 2)
            | (u16::from(self.mosaic) << 6)
            | ((self.bpp as u16) << 7)
            | ((self.tile_map_addr >> 11) << 8)
            | ((self.affine_overflow as u16) << 13)
            | ((self.size as u16) << 14)
    }

    fn write(&mut self, value: u16) {
        self.priority = (value & 3) as u8;

        let tile_data_addr_16kb = (value >> 2) & 3;
        self.tile_data_addr = tile_data_addr_16kb << 14;

        self.mosaic = value.bit(6);
        self.bpp = BitsPerPixel::from_bit(value.bit(7));

        let tile_map_addr_2kb = (value >> 8) & 0x1F;
        self.tile_map_addr = tile_map_addr_2kb << 11;

        self.affine_overflow = AffineOverflowBehavior::from_bit(value.bit(13));
        self.size = ScreenSize::from_bits(value >> 14);
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct BgAffineParameters {
    // BG2X / BG3X
    pub reference_x: u32,
    // BG2Y / BG3Y
    pub reference_y: u32,
    // BG2PA / BG3PA
    pub a: u16,
    // BG2PB / BG3PB
    pub b: u16,
    // BG2PC / BG3PC
    pub c: u16,
    // BG2PD / BG3PD
    pub d: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BlendMode {
    #[default]
    None = 0,
    AlphaBlending = 1,
    BrightnessIncrease = 2,
    BrightnessDecrease = 3,
}

impl BlendMode {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::None,
            1 => Self::AlphaBlending,
            2 => Self::BrightnessIncrease,
            3 => Self::BrightnessDecrease,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

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
    // BGxCNT (BG0-3 control)
    pub bg_control: [BgControl; 4],
    // BGxHOFS (BG0-3 horizontal offset)
    pub bg_h_scroll: [u16; 4],
    // BGxVOFS (BG0-3 vertical offset)
    pub bg_v_scroll: [u16; 4],
    // BG2/3 affine registers
    pub bg_affine_parameters: [BgAffineParameters; 2],
    // WINxH (Window horizontal coordinates)
    pub window_x1: [u8; 2],
    pub window_x2: [u8; 2],
    // WINxV (Window vertical coordinates)
    pub window_y1: [u8; 2],
    pub window_y2: [u8; 2],
    // WININ (Window inside control)
    pub window_in_bg_enabled: [[bool; 4]; 2],
    pub window_in_obj_enabled: [bool; 2],
    pub window_in_blend_enabled: [bool; 2],
    // WINOUT (Window outside control)
    pub window_out_bg_enabled: [bool; 4],
    pub window_out_obj_enabled: bool,
    pub window_out_blend_enabled: bool,
    pub obj_window_bg_enabled: [bool; 4],
    pub obj_window_obj_enabled: bool,
    pub obj_window_blend_enabled: bool,
    // MOSAIC (Mosaic size)
    pub bg_mosaic_h_size: u8,
    pub bg_mosaic_v_size: u8,
    pub obj_mosaic_h_size: u8,
    pub obj_mosaic_v_size: u8,
    // BLDCNT (Blending control)
    pub bg_blend_1st_target: [bool; 4],
    pub obj_blend_1st_target: bool,
    pub backdrop_blend_1st_target: bool,
    pub blend_mode: BlendMode,
    pub bg_blend_2nd_target: [bool; 4],
    pub obj_blend_2nd_target: bool,
    pub backdrop_blend_2nd_target: bool,
    // BLDALPHA (Alpha blending coefficients)
    pub blend_alpha_a: u8,
    pub blend_alpha_b: u8,
    // BLDY (Blending brightness coefficient)
    pub blend_brightness: u8,
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
            bg_control: [BgControl::default(); 4],
            bg_h_scroll: [0; 4],
            bg_v_scroll: [0; 4],
            bg_affine_parameters: array::from_fn(|_| BgAffineParameters::default()),
            window_x1: [0; 2],
            window_x2: [0; 2],
            window_y1: [0; 2],
            window_y2: [0; 2],
            window_in_bg_enabled: [[false; 4]; 2],
            window_in_obj_enabled: [false; 2],
            window_in_blend_enabled: [false; 2],
            window_out_bg_enabled: [false; 4],
            window_out_obj_enabled: false,
            window_out_blend_enabled: false,
            obj_window_bg_enabled: [false; 4],
            obj_window_obj_enabled: false,
            obj_window_blend_enabled: false,
            bg_mosaic_h_size: 0,
            bg_mosaic_v_size: 0,
            obj_mosaic_h_size: 0,
            obj_mosaic_v_size: 0,
            bg_blend_1st_target: [false; 4],
            obj_blend_1st_target: false,
            backdrop_blend_1st_target: false,
            blend_mode: BlendMode::default(),
            bg_blend_2nd_target: [false; 4],
            obj_blend_2nd_target: false,
            backdrop_blend_2nd_target: false,
            blend_alpha_a: 0,
            blend_alpha_b: 0,
            blend_brightness: 0,
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
        let bg_enabled_bits = bool_array_to_bits(self.bg_enabled);

        u16::from(self.bg_mode.to_bits())
            | ((self.bitmap_frame_buffer as u16) << 4)
            | (u16::from(self.oam_free_during_hblank) << 5)
            | ((self.obj_vram_map_dimensions as u16) << 6)
            | (u16::from(self.forced_blanking) << 7)
            | (bg_enabled_bits << 8)
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

    // $4000008-$400000E: BG0CNT/BG1CNT/BG2CNT/BG3CNT (BG0-3 control)
    pub fn read_bgcnt(&self, index: usize) -> u16 {
        self.bg_control[index].read()
    }

    // $4000008-$400000E: BG0CNT/BG1CNT/BG2CNT/BG3CNT (BG0-3 control)
    pub fn write_bgcnt(&mut self, index: usize, value: u16) {
        self.bg_control[index].write(value);

        log::debug!("BG{index}CNT write: {value:04X}");
        log::debug!("  Priority: {}", self.bg_control[index].priority);
        log::debug!("  Tile data base address: {:04X}", self.bg_control[index].tile_data_addr);
        log::debug!("  Mosaic enabled: {}", self.bg_control[index].mosaic);
        log::debug!("  Bits per pixel: {:?}", self.bg_control[index].bpp);
        log::debug!("  Tile map base address: {:04X}", self.bg_control[index].tile_map_addr);
        log::debug!("  Affine overflow behavior: {:?}", self.bg_control[index].affine_overflow);
        log::debug!("  Screen size: {}", self.bg_control[index].size as u8);
    }

    // $4000010/$4000014/$4000018/$400001C: BG1HOFS/BG2HOFS/BG3HOFS/BG4HOFS (BG0-3 horizontal offset)
    pub fn write_bghofs(&mut self, index: usize, value: u16) {
        self.bg_h_scroll[index] = value & 0x1FF;

        log::debug!("BG{index}HOFS write: {value:04X}");
    }

    // $4000012/$4000016/$400001A/$400001E: BG1VOFS/BG2VOFS/BG3VOFS/BG4VOFS (BG0-3 vertical offset)
    pub fn write_bgvofs(&mut self, index: usize, value: u16) {
        self.bg_v_scroll[index] = value & 0x1FF;

        log::debug!("BG{index}VOFS write: {value:04X}");
    }

    // $4000020-$400003E: BG2/3 affine parameter registers
    pub fn write_bg_affine_register(&mut self, address: u32, value: u16) {
        let bg_idx = ((address >> 4) & 1) as usize;
        let affine_parameters = &mut self.bg_affine_parameters[bg_idx];

        match address & 0xE {
            0x0 => {
                // BG2PA / BG3PA
                affine_parameters.a = value;
                log::debug!("BG{}PA write: {value:04X}", bg_idx + 2);
            }
            0x2 => {
                // BG2PB // BG3PB
                affine_parameters.b = value;
                log::debug!("BG{}PB write: {value:04X}", bg_idx + 2);
            }
            0x4 => {
                // BG2PC // BG3PC
                affine_parameters.c = value;
                log::debug!("BG{}PC write: {value:04X}", bg_idx + 2);
            }
            0x6 => {
                // BG2PD // BG3PD
                affine_parameters.d = value;
                log::debug!("BG{}PD write: {value:04X}", bg_idx + 2);
            }
            0x8 => {
                // BG2X_L / BG3X_L
                affine_parameters.reference_x =
                    (affine_parameters.reference_x & !0xFFFF) | u32::from(value);
                log::debug!("BG{}X_L write: {value:04X}", bg_idx + 2);
                log::debug!("  Reference point X: {:07X}", affine_parameters.reference_x);
            }
            0xA => {
                // BG2X_H / BG3X_H
                affine_parameters.reference_x =
                    (affine_parameters.reference_x & 0xFFFF) | (u32::from(value & 0x0FFF) << 16);
                log::debug!("BG{}X_H write: {value:04X}", bg_idx + 2);
                log::debug!("  Reference point X: {:07X}", affine_parameters.reference_x);
            }
            0xC => {
                // BG2Y_L / BG3Y_L
                affine_parameters.reference_y =
                    (affine_parameters.reference_y & !0xFFFF) | u32::from(value);
                log::debug!("BG{}Y_L write: {value:04X}", bg_idx + 2);
                log::debug!("  Reference point Y: {:07X}", affine_parameters.reference_y);
            }
            0xE => {
                // BG2Y_H / BG3Y_H
                affine_parameters.reference_y =
                    (affine_parameters.reference_y & 0xFFFF) | (u32::from(value & 0x0FFF) << 16);
                log::debug!("BG{}X_H write: {value:04X}", bg_idx + 2);
                log::debug!("  Reference point X: {:07X}", affine_parameters.reference_y);
            }
            _ => unreachable!("value & 0xE is always one of the above 8 values"),
        }
    }

    // $4000040/$4000042: WIN0H/WIN1H (Window 0/1 horizontal coordinates)
    pub fn write_winh(&mut self, window: usize, value: u16) {
        [self.window_x1[window], self.window_x2[window]] = value.to_be_bytes();

        log::debug!("WIN{window}H write: {value:04X}");
        log::debug!("  X1: {}", self.window_x1[window]);
        log::debug!("  X2: {}", self.window_x2[window]);
    }

    // $4000044/$4000046: WIN0V/WIN1V (Window 0/1 vertical coordinates)
    pub fn write_winv(&mut self, window: usize, value: u16) {
        [self.window_y1[window], self.window_y2[window]] = value.to_be_bytes();

        log::debug!("WIN{window}V write: {value:04X}");
        log::debug!("  Y1: {}", self.window_y1[window]);
        log::debug!("  Y2: {}", self.window_y2[window]);
    }

    // $4000048: WININ (Window inside control)
    pub fn read_winin(&self) -> u16 {
        let win0_in_bg_enabled = bool_array_to_bits(self.window_in_bg_enabled[0]);
        let win1_in_bg_enabled = bool_array_to_bits(self.window_in_bg_enabled[1]);

        win0_in_bg_enabled
            | (u16::from(self.window_in_obj_enabled[0]) << 4)
            | (u16::from(self.window_in_blend_enabled[0]) << 5)
            | (win1_in_bg_enabled << 8)
            | (u16::from(self.window_in_obj_enabled[1]) << 12)
            | (u16::from(self.window_in_blend_enabled[1]) << 13)
    }

    // $4000048: WININ (Window inside control)
    pub fn write_winin(&mut self, value: u16) {
        self.window_in_bg_enabled[0] = array::from_fn(|i| value.bit(i as u8));
        self.window_in_obj_enabled[0] = value.bit(4);
        self.window_in_blend_enabled[0] = value.bit(5);
        self.window_in_bg_enabled[1] = array::from_fn(|i| value.bit((8 + i) as u8));
        self.window_in_obj_enabled[1] = value.bit(12);
        self.window_in_blend_enabled[1] = value.bit(13);

        log::debug!("WININ write: {value:04X}");
        log::debug!("  Window 0 BG enabled: {:?}", self.window_in_bg_enabled[0]);
        log::debug!("  Window 0 OBJ enabled: {}", self.window_in_obj_enabled[0]);
        log::debug!("  Window 0 blending enabled: {}", self.window_in_blend_enabled[0]);
        log::debug!("  Window 1 BG enabled: {:?}", self.window_in_bg_enabled[1]);
        log::debug!("  Window 1 OBJ enabled: {}", self.window_in_obj_enabled[1]);
        log::debug!("  Window 1 blending enabled: {}", self.window_in_blend_enabled[1]);
    }

    // $400004A: WINOUT (Window outside control)
    pub fn read_winout(&self) -> u16 {
        let window_out_bg_enabled = bool_array_to_bits(self.window_out_bg_enabled);
        let obj_window_bg_enabled = bool_array_to_bits(self.obj_window_bg_enabled);

        window_out_bg_enabled
            | (u16::from(self.window_out_obj_enabled) << 4)
            | (u16::from(self.window_out_blend_enabled) << 5)
            | (obj_window_bg_enabled << 8)
            | (u16::from(self.obj_window_obj_enabled) << 12)
            | (u16::from(self.obj_window_blend_enabled) << 13)
    }

    // $400004A: WINOUT (Window outside control)
    pub fn write_winout(&mut self, value: u16) {
        self.window_out_bg_enabled = array::from_fn(|i| value.bit(i as u8));
        self.window_out_obj_enabled = value.bit(4);
        self.window_out_blend_enabled = value.bit(5);
        self.obj_window_bg_enabled = array::from_fn(|i| value.bit((8 + i) as u8));
        self.obj_window_obj_enabled = value.bit(12);
        self.obj_window_blend_enabled = value.bit(13);

        log::debug!("WINOUT write: {value:04X}");
        log::debug!("  Window outside BG enabled: {:?}", self.window_in_bg_enabled[0]);
        log::debug!("  Window outside OBJ enabled: {}", self.window_in_obj_enabled[0]);
        log::debug!("  Window outside blending enabled: {}", self.window_in_blend_enabled[0]);
        log::debug!("  OBJ window BG enabled: {:?}", self.window_in_bg_enabled[1]);
        log::debug!("  OBJ window OBJ enabled: {}", self.window_in_obj_enabled[1]);
        log::debug!("  OBJ window blending enabled: {}", self.window_in_blend_enabled[1]);
    }

    // $4000004C: MOSAIC (Mosaic size)
    pub fn write_mosaic(&mut self, value: u16) {
        let [bg_mosaic, obj_mosaic] = value.to_le_bytes();
        self.bg_mosaic_h_size = bg_mosaic & 0xF;
        self.bg_mosaic_v_size = bg_mosaic >> 4;
        self.obj_mosaic_h_size = obj_mosaic & 0xF;
        self.obj_mosaic_v_size = obj_mosaic >> 4;

        log::debug!("MOSAIC write: {value:04X}");
        log::debug!("  BG H size: {}", self.bg_mosaic_h_size);
        log::debug!("  BG V size: {}", self.bg_mosaic_v_size);
        log::debug!("  OBJ H size: {}", self.obj_mosaic_h_size);
        log::debug!("  OBJ V size: {}", self.obj_mosaic_v_size);
    }

    // $4000050: BLDCNT (Blending control)
    pub fn read_bldcnt(&self) -> u16 {
        let bg_1st_target = bool_array_to_bits(self.bg_blend_1st_target);
        let bg_2nd_target = bool_array_to_bits(self.bg_blend_2nd_target);

        bg_1st_target
            | (u16::from(self.obj_blend_1st_target) << 4)
            | (u16::from(self.backdrop_blend_1st_target) << 5)
            | ((self.blend_mode as u16) << 6)
            | (bg_2nd_target << 8)
            | (u16::from(self.obj_blend_2nd_target) << 12)
            | (u16::from(self.backdrop_blend_2nd_target) << 13)
    }

    // $4000050: BLDCNT (Blending control)
    pub fn write_bldcnt(&mut self, value: u16) {
        self.bg_blend_1st_target = array::from_fn(|i| value.bit(i as u8));
        self.obj_blend_1st_target = value.bit(4);
        self.backdrop_blend_1st_target = value.bit(5);
        self.blend_mode = BlendMode::from_bits(value >> 6);
        self.bg_blend_2nd_target = array::from_fn(|i| value.bit((8 + i) as u8));
        self.obj_blend_2nd_target = value.bit(12);
        self.backdrop_blend_2nd_target = value.bit(13);

        log::debug!("BLDCNT write: {value:04X}");
        log::debug!("  BG 1st target: {:?}", self.bg_blend_1st_target);
        log::debug!("  OBJ 1st target: {:?}", self.obj_blend_1st_target);
        log::debug!("  Backdrop 1st target: {:?}", self.backdrop_blend_1st_target);
        log::debug!("  BG 2nd target: {:?}", self.bg_blend_2nd_target);
        log::debug!("  OBJ 2nd target: {}", self.obj_blend_2nd_target);
        log::debug!("  Backdrop 2nd target: {}", self.backdrop_blend_2nd_target);
    }

    // $4000052: BLDALPHA (Alpha blending coefficients)
    pub fn read_bldalpha(&self) -> u16 {
        u16::from_le_bytes([self.blend_alpha_a, self.blend_alpha_b])
    }

    // $4000052: BLDALPHA (Alpha blending coefficients)
    pub fn write_bldalpha(&mut self, value: u16) {
        self.blend_alpha_a = (value & 0x1F) as u8;
        self.blend_alpha_b = ((value >> 8) & 0x1F) as u8;

        log::debug!("BLDALPHA write: {value:04X}");
        log::debug!("  A: {}", self.blend_alpha_a);
        log::debug!("  B: {}", self.blend_alpha_b);
    }

    // $4000054: BLDY (Blending brightness coefficient)
    pub fn write_bldy(&mut self, value: u16) {
        self.blend_brightness = (value & 0x1F) as u8;

        log::debug!("BLDY write: {value:04X} (coefficient = {})", self.blend_brightness);
    }
}

fn bool_array_to_bits(arr: [bool; 4]) -> u16 {
    (0..4).map(|i| u16::from(arr[i]) << i).reduce(|a, b| a | b).unwrap()
}
