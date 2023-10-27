use bincode::{Decode, Encode};

const VRAM_LEN: usize = 64 * 1024;
const OAM_LEN: usize = 512 + 32;
const CGRAM_LEN_WORDS: usize = 256;

type Vram = [u8; VRAM_LEN];
type Oam = [u8; OAM_LEN];
type Cgram = [u16; CGRAM_LEN_WORDS];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum VerticalDisplaySize {
    #[default]
    TwoTwentyFour,
    TwoThirtyNine,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum TileSize {
    #[default]
    Small,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum BgScreenSize {
    #[default]
    OneScreen,
    VerticalMirror,
    HorizontalMirror,
    FourScreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum WindowAreaMode {
    #[default]
    Disabled,
    Inside,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum WindowMaskLogic {
    #[default]
    Or,
    And,
    Xor,
    Xnor,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum VramIncrementMode {
    #[default]
    Low,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ObjPriorityMode {
    #[default]
    Normal,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Mode7OobBehavior {
    #[default]
    Wrap,
    Transparent,
    Tile0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum AccessFlipflop {
    #[default]
    First,
    Second,
}

impl AccessFlipflop {
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
    bg_scroll_flipflop: AccessFlipflop,
    // OBSEL
    obj_tile_base_address: u16,
    obj_tile_gap_size: u16,
    obj_tile_size: TileSize,
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
    obj_priority_mode: ObjPriorityMode,
    oam_flipflop: AccessFlipflop,
    // CGADD
    cgram_address: u16,
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
    mode_7_flipflop: AccessFlipflop,
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
            bg_tile_size: [TileSize::default(); 4],
            mosaic_size: 0,
            bg_mosaic_enabled: [false; 4],
            bg_screen_size: [BgScreenSize::default(); 4],
            bg_base_address: [0; 4],
            bg_tile_base_address: [0; 4],
            bg_h_scroll: [0; 4],
            bg_v_scroll: [0; 4],
            bg_scroll_flipflop: AccessFlipflop::default(),
            obj_tile_base_address: 0,
            obj_tile_gap_size: 0,
            obj_tile_size: TileSize::default(),
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
            vram_address_increment_step: 0,
            vram_address_translation: VramAddressTranslation::default(),
            vram_address_increment_mode: VramIncrementMode::default(),
            vram_address: 0,
            vram_prefetch_buffer: 0,
            oam_address: 0,
            obj_priority_mode: ObjPriorityMode::default(),
            oam_flipflop: AccessFlipflop::default(),
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
            mode_7_flipflop: AccessFlipflop::default(),
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
pub struct Ppu {
    registers: Registers,
    vram: Box<Vram>,
    oam: Box<Oam>,
    cgram: Box<Cgram>,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            registers: Registers::new(),
            vram: vec![0; VRAM_LEN].into_boxed_slice().try_into().unwrap(),
            oam: vec![0; OAM_LEN].into_boxed_slice().try_into().unwrap(),
            cgram: vec![0; CGRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
        }
    }

    pub fn read_port(&mut self, address: u32) -> u8 {
        todo!("PPU read {address:06X}")
    }

    pub fn write_port(&mut self, address: u32, value: u8) {
        todo!("PPU write {address:06X} {value:02X}")
    }
}
