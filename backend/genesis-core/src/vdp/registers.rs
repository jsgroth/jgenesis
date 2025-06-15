use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::{GetBit, U16Ext};
use std::fmt::{Display, Formatter};

// Values from https://gendev.spritesmind.net/forum/viewtopic.php?p=37011#p37011
pub const H32_LEFT_BORDER: u16 = 14;
pub const H40_LEFT_BORDER: u16 = 13;
pub const RIGHT_BORDER: u16 = 14;

pub const NTSC_TOP_BORDER: u16 = 11;
pub const PAL_V28_TOP_BORDER: u16 = 38;
pub const PAL_V30_TOP_BORDER: u16 = 30;

pub const NTSC_BOTTOM_BORDER: u16 = 8;
pub const PAL_V28_BOTTOM_BORDER: u16 = 32;
pub const PAL_V30_BOTTOM_BORDER: u16 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VerticalScrollMode {
    #[default]
    FullScreen,
    TwoCell,
}

impl Display for VerticalScrollMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullScreen => write!(f, "Full screen"),
            Self::TwoCell => write!(f, "Per 2 cell"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum HorizontalScrollMode {
    #[default]
    FullScreen,
    Cell,
    Line,
    // Repeatedly uses the scroll values for the first 8 lines
    Invalid,
}

impl Display for HorizontalScrollMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullScreen => write!(f, "Full screen"),
            Self::Cell => write!(f, "Per cell"),
            Self::Line => write!(f, "Per line"),
            Self::Invalid => write!(f, "Prohibited"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum HorizontalDisplaySize {
    #[default]
    ThirtyTwoCell,
    FortyCell,
}

impl Display for HorizontalDisplaySize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ThirtyTwoCell => write!(f, "H32 (256px)"),
            Self::FortyCell => write!(f, "H40 (320px)"),
        }
    }
}

impl HorizontalDisplaySize {
    pub const fn active_display_pixels(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 256,
            Self::FortyCell => 320,
        }
    }

    pub const fn pixels_including_hblank(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 342,
            Self::FortyCell => 420,
        }
    }

    // Length in sprites
    pub const fn sprite_table_len(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 64,
            Self::FortyCell => 80,
        }
    }

    pub const fn max_sprites_per_line(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 16,
            Self::FortyCell => 20,
        }
    }

    pub const fn max_sprite_pixels_per_line(self) -> u16 {
        self.active_display_pixels()
    }

    pub const fn window_width_cells(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 32,
            Self::FortyCell => 64,
        }
    }

    pub const fn sprite_attribute_table_mask(self) -> u16 {
        // Sprite attribute table A9 is ignored in H40 mode
        match self {
            Self::ThirtyTwoCell => !0,
            Self::FortyCell => !0x3FF,
        }
    }

    pub const fn left_border(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => H32_LEFT_BORDER,
            Self::FortyCell => H40_LEFT_BORDER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VerticalDisplaySize {
    #[default]
    TwentyEightCell,
    ThirtyCell,
}

impl Display for VerticalDisplaySize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TwentyEightCell => write!(f, "V28 (224px)"),
            Self::ThirtyCell => write!(f, "V30 (240px)"),
        }
    }
}

impl VerticalDisplaySize {
    pub const fn active_scanlines(self) -> u16 {
        match self {
            Self::TwentyEightCell => 224,
            Self::ThirtyCell => 240,
        }
    }

    pub const fn top_border(self, timing_mode: TimingMode) -> u16 {
        match (self, timing_mode) {
            (_, TimingMode::Ntsc) => NTSC_TOP_BORDER,
            (Self::TwentyEightCell, TimingMode::Pal) => PAL_V28_TOP_BORDER,
            (Self::ThirtyCell, TimingMode::Pal) => PAL_V30_TOP_BORDER,
        }
    }

    pub const fn bottom_border(self, timing_mode: TimingMode) -> u16 {
        match (self, timing_mode) {
            (_, TimingMode::Ntsc) => NTSC_BOTTOM_BORDER,
            (Self::TwentyEightCell, TimingMode::Pal) => PAL_V28_BOTTOM_BORDER,
            (Self::ThirtyCell, TimingMode::Pal) => PAL_V30_BOTTOM_BORDER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum InterlacingMode {
    #[default]
    Progressive,
    Interlaced,
    InterlacedDouble,
}

impl Display for InterlacingMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Progressive => write!(f, "Progressive"),
            Self::Interlaced => write!(f, "Single-screen interlaced"),
            Self::InterlacedDouble => write!(f, "Double-screen interlaced"),
        }
    }
}

impl InterlacingMode {
    pub const fn v_scroll_mask(self) -> u16 {
        // V scroll values are 10 bits normally, 11 bits in interlaced 2x mode
        match self {
            Self::Progressive | Self::Interlaced => 0x03FF,
            Self::InterlacedDouble => 0x07FF,
        }
    }

    pub const fn sprite_display_top(self) -> u16 {
        match self {
            // The sprite display area starts at $080 normally, $100 in interlaced 2x mode
            Self::Progressive | Self::Interlaced => 0x080,
            Self::InterlacedDouble => 0x100,
        }
    }

    pub const fn sprite_display_mask(self) -> u16 {
        match self {
            Self::Progressive | Self::Interlaced => 0x1FF,
            Self::InterlacedDouble => 0x3FF,
        }
    }

    pub const fn cell_height(self) -> u16 {
        1 << self.cell_height_shift()
    }

    pub const fn cell_height_shift(self) -> u16 {
        match self {
            // Cells are 8x8 normally, 8x16 in interlaced 2x mode
            Self::Progressive | Self::Interlaced => 3,
            Self::InterlacedDouble => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ScrollSize {
    #[default]
    ThirtyTwo,
    SixtyFour,
    OneTwentyEight,
    Invalid,
}

impl Display for ScrollSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ThirtyTwo => write!(f, "32 tiles"),
            Self::SixtyFour => write!(f, "64 tiles"),
            Self::OneTwentyEight => write!(f, "128 tiles"),
            Self::Invalid => write!(f, "Prohibited"),
        }
    }
}

impl ScrollSize {
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 => Self::ThirtyTwo,
            0x01 => Self::SixtyFour,
            0x02 => Self::Invalid,
            0x03 => Self::OneTwentyEight,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }

    pub fn to_pixels(self) -> u16 {
        match self {
            Self::ThirtyTwo | Self::Invalid => 32 * 8,
            Self::SixtyFour => 64 * 8,
            Self::OneTwentyEight => 128 * 8,
        }
    }
}

impl From<ScrollSize> for u16 {
    fn from(value: ScrollSize) -> Self {
        match value {
            ScrollSize::ThirtyTwo => 32,
            ScrollSize::SixtyFour => 64,
            ScrollSize::OneTwentyEight => 128,
            ScrollSize::Invalid => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WindowHorizontalMode {
    #[default]
    LeftToCenter,
    CenterToRight,
}

impl Display for WindowHorizontalMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LeftToCenter => write!(f, "Left to center"),
            Self::CenterToRight => write!(f, "Center to right"),
        }
    }
}

impl WindowHorizontalMode {
    pub fn h_range(self, window_x: u16, active_display_pixels: u16) -> (u16, u16) {
        match self {
            Self::LeftToCenter => (0, 8 * window_x),
            Self::CenterToRight => (8 * window_x, active_display_pixels),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WindowVerticalMode {
    #[default]
    TopToCenter,
    CenterToBottom,
}

impl Display for WindowVerticalMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TopToCenter => write!(f, "Top to center"),
            Self::CenterToBottom => write!(f, "Center to bottom"),
        }
    }
}

impl WindowVerticalMode {
    pub fn in_window(self, scanline: u16, window_y: u16) -> bool {
        let cell = scanline / 8;
        match self {
            Self::TopToCenter => cell < window_y,
            Self::CenterToBottom => cell >= window_y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaMode {
    #[default]
    MemoryToVram,
    VramFill,
    VramCopy,
}

impl Display for DmaMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemoryToVram => write!(f, "ROM/RAM to VRAM"),
            Self::VramFill => write!(f, "VRAM fill"),
            Self::VramCopy => write!(f, "VRAM-to-VRAM copy"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VramSizeKb {
    #[default]
    SixtyFour,
    OneTwentyEight,
}

impl Display for VramSizeKb {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SixtyFour => write!(f, "64KB"),
            Self::OneTwentyEight => write!(f, "128KB"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    // Register #0
    pub h_interrupt_enabled: bool,
    pub hv_counter_stopped: bool,
    // Register #1
    pub display_enabled: bool,
    pub v_interrupt_enabled: bool,
    pub dma_enabled: bool,
    pub vertical_display_size: VerticalDisplaySize,
    pub mode_4: bool,
    pub vram_size: VramSizeKb,
    // Register #2
    pub scroll_a_base_nt_addr: u16,
    // Register #3
    pub window_base_nt_addr: u16,
    // Register #4
    pub scroll_b_base_nt_addr: u16,
    // Register #5
    pub sprite_attribute_table_base_addr: u16,
    // Register #7
    pub background_palette: u8,
    pub background_color_id: u8,
    // Register #10
    pub h_interrupt_interval: u16,
    // Register #11
    // TODO external interrupts enabled
    pub vertical_scroll_mode: VerticalScrollMode,
    pub horizontal_scroll_mode: HorizontalScrollMode,
    // Register #12
    pub horizontal_display_size: HorizontalDisplaySize,
    pub shadow_highlight_flag: bool,
    pub interlacing_mode: InterlacingMode,
    // Register #13
    pub h_scroll_table_base_addr: u16,
    // Register #15
    pub data_port_auto_increment: u16,
    // Register #16
    pub vertical_scroll_size: ScrollSize,
    pub horizontal_scroll_size: ScrollSize,
    // Register #17
    pub window_horizontal_mode: WindowHorizontalMode,
    pub window_x_position: u16,
    // Register #18
    pub window_vertical_mode: WindowVerticalMode,
    pub window_y_position: u16,
    // Registers #19 & #20
    pub dma_length: u16,
    // Registers #21, #22, & #23
    pub dma_source_address: u32,
    pub dma_mode: DmaMode,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            h_interrupt_enabled: false,
            hv_counter_stopped: false,
            display_enabled: false,
            v_interrupt_enabled: false,
            dma_enabled: false,
            vertical_display_size: VerticalDisplaySize::default(),
            mode_4: false,
            vram_size: VramSizeKb::default(),
            scroll_a_base_nt_addr: 0,
            window_base_nt_addr: 0,
            scroll_b_base_nt_addr: 0,
            sprite_attribute_table_base_addr: 0,
            background_palette: 0,
            background_color_id: 0,
            h_interrupt_interval: 0,
            vertical_scroll_mode: VerticalScrollMode::default(),
            horizontal_scroll_mode: HorizontalScrollMode::default(),
            horizontal_display_size: HorizontalDisplaySize::default(),
            shadow_highlight_flag: false,
            interlacing_mode: InterlacingMode::default(),
            h_scroll_table_base_addr: 0,
            data_port_auto_increment: 0,
            vertical_scroll_size: ScrollSize::default(),
            horizontal_scroll_size: ScrollSize::default(),
            window_horizontal_mode: WindowHorizontalMode::default(),
            window_x_position: 0,
            window_vertical_mode: WindowVerticalMode::default(),
            window_y_position: 0,
            dma_length: 0,
            dma_source_address: 0,
            dma_mode: DmaMode::default(),
        }
    }

    pub fn write_internal_register(&mut self, register: u8, value: u8) {
        log::trace!("Wrote register #{register} with value {value:02X}");

        if self.mode_4 && register > 10 {
            // Writing to register numbers >10 while in SMS mode has no effect
            // Bass Masters Classic: Pro Edition depends on this - during boot it attempts to change
            // the H display size while in SMS mode, and it expects that write to do nothing
            return;
        }

        match register {
            0 => {
                // Register #0: Mode set register 1
                self.h_interrupt_enabled = value.bit(4);
                self.hv_counter_stopped = value.bit(1);

                log::trace!("  H interrupt enabled: {}", self.h_interrupt_enabled);
                log::trace!("  HV counter stopped: {}", self.hv_counter_stopped);
            }
            1 => {
                // Register #1: Mode set register 2
                self.display_enabled = value.bit(6);
                self.v_interrupt_enabled = value.bit(5);
                self.dma_enabled = value.bit(4);
                self.vertical_display_size = if value.bit(3) {
                    VerticalDisplaySize::ThirtyCell
                } else {
                    VerticalDisplaySize::TwentyEightCell
                };

                // Undocumented: Register #1 bit 2 toggles between mode 5 (Genesis) and mode 4 (SMS)
                // Mode 4 / SMS mode is not actually emulated, but some games depend on writes to
                // VDP registers >10 doing nothing while in mode 4
                self.mode_4 = !value.bit(2);

                // Undocumented: Register #1 bit 7 enables "128KB" VRAM mode, which effectively enables byte-size access
                // to VRAM
                self.vram_size =
                    if value.bit(7) { VramSizeKb::OneTwentyEight } else { VramSizeKb::SixtyFour };

                log::trace!("  Display enabled: {}", self.display_enabled);
                log::trace!("  V interrupt enabled: {}", self.v_interrupt_enabled);
                log::trace!("  DMA enabled: {}", self.dma_enabled);
                log::trace!("  Vertical display size: {:?}", self.vertical_display_size);
                log::trace!("  Mode 4 enabled: {}", self.mode_4);
                log::trace!("  VRAM size: {}", self.vram_size);
            }
            2 => {
                // Register #2: Scroll A name table base address (bits 15-13)
                self.scroll_a_base_nt_addr = u16::from(value & 0x38) << 10;

                log::trace!(
                    "  Scroll A base nametable address: {:04X}",
                    self.scroll_a_base_nt_addr
                );
            }
            3 => {
                // Register #3: Window name table base address (bits 15-11)
                self.window_base_nt_addr = u16::from(value & 0x3E) << 10;

                log::trace!("  Window base nametable address: {:04X}", self.window_base_nt_addr);
            }
            4 => {
                // Register #4: Scroll B name table base address (bits 15-13)
                self.scroll_b_base_nt_addr = u16::from(value & 0x07) << 13;

                log::trace!(
                    "  Scroll B base nametable address: {:04X}",
                    self.scroll_b_base_nt_addr
                );
            }
            5 => {
                // Register #5: Sprite attribute table base address (bits 15-9)
                self.sprite_attribute_table_base_addr = u16::from(value & 0x7F) << 9;

                log::trace!(
                    "  Sprite attribute table base address: {:04X}",
                    self.sprite_attribute_table_base_addr
                );
            }
            7 => {
                // Register #7: Background color
                self.background_palette = (value >> 4) & 0x03;
                self.background_color_id = value & 0x0F;

                log::trace!("  BG palette: {}", self.background_palette);
                log::trace!("  BG color id: {}", self.background_color_id);
            }
            10 => {
                // Register #10: H interrupt interval
                self.h_interrupt_interval = value.into();

                log::trace!("  H interrupt interval: {}", self.h_interrupt_interval);
            }
            11 => {
                // Register #11: Mode set register 3
                // TODO external interrupt enable
                self.vertical_scroll_mode = if value.bit(2) {
                    VerticalScrollMode::TwoCell
                } else {
                    VerticalScrollMode::FullScreen
                };
                self.horizontal_scroll_mode = match value & 0x03 {
                    0x00 => HorizontalScrollMode::FullScreen,
                    0x01 => HorizontalScrollMode::Invalid,
                    0x02 => HorizontalScrollMode::Cell,
                    0x03 => HorizontalScrollMode::Line,
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                };

                log::trace!("  Vertical scroll mode: {:?}", self.vertical_scroll_mode);
                log::trace!("  Horizontal scroll mode: {:?}", self.horizontal_scroll_mode);
            }
            12 => {
                // Register #12: Mode set register 4
                self.horizontal_display_size = if value.bit(7) || value.bit(0) {
                    HorizontalDisplaySize::FortyCell
                } else {
                    HorizontalDisplaySize::ThirtyTwoCell
                };
                self.shadow_highlight_flag = value.bit(3);
                self.interlacing_mode = match (value >> 1) & 0x03 {
                    // TODO how should the "prohibited" mode behave?
                    0x00 | 0x02 => InterlacingMode::Progressive,
                    0x01 => InterlacingMode::Interlaced,
                    0x03 => InterlacingMode::InterlacedDouble,
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                };

                log::trace!("  Horizontal display size: {:?}", self.horizontal_display_size);
                log::trace!("  Shadow/highlight flag: {}", self.shadow_highlight_flag);
                log::trace!("  Interlacing mode: {:?}", self.interlacing_mode);
            }
            13 => {
                // Register #13: Horizontal scroll table base address (bits 15-10)
                self.h_scroll_table_base_addr = u16::from(value & 0x3F) << 10;

                log::trace!("  H scroll table base address: {:04X}", self.h_scroll_table_base_addr);
            }
            15 => {
                // Register #15: VRAM address auto increment
                self.data_port_auto_increment = value.into();

                log::trace!("  Data port auto increment: {value:02X}");
            }
            16 => {
                // Register #16: Scroll size
                self.vertical_scroll_size = ScrollSize::from_bits(value >> 4);
                self.horizontal_scroll_size = ScrollSize::from_bits(value);

                log::trace!("  Vertical scroll size: {:?}", self.vertical_scroll_size);
                log::trace!("  Horizontal scroll size: {:?}", self.horizontal_scroll_size);
            }
            17 => {
                // Register #17: Window horizontal position
                self.window_horizontal_mode = if value.bit(7) {
                    WindowHorizontalMode::CenterToRight
                } else {
                    WindowHorizontalMode::LeftToCenter
                };
                self.window_x_position = u16::from(value & 0x1F) << 1;

                log::trace!("  Window horizontal mode: {:?}", self.window_horizontal_mode);
                log::trace!("  Window X position: {}", self.window_x_position);
            }
            18 => {
                // Register #18: Window vertical position
                self.window_vertical_mode = if value.bit(7) {
                    WindowVerticalMode::CenterToBottom
                } else {
                    WindowVerticalMode::TopToCenter
                };
                self.window_y_position = (value & 0x1F).into();

                log::trace!("  Window vertical mode: {:?}", self.window_vertical_mode);
                log::trace!("  Window Y position: {}", self.window_y_position);
            }
            19 => {
                // Register #19: DMA length counter (bits 7-0)
                self.dma_length.set_lsb(value);

                log::trace!("  DMA length: {}", self.dma_length);
            }
            20 => {
                // Register #20: DMA length counter (bits 15-8)
                self.dma_length.set_msb(value);

                log::trace!("  DMA length: {}", self.dma_length);
            }
            21 => {
                // Register 21: DMA source address (bits 9-1)
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFF_FE00) | (u32::from(value) << 1);

                log::trace!("  DMA source address: {:06X}", self.dma_source_address);
            }
            22 => {
                // Register 22: DMA source address (bits 16-9)
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFE_01FF) | (u32::from(value) << 9);

                log::trace!("  DMA source address: {:06X}", self.dma_source_address);
            }
            23 => {
                // Register 23: DMA source address (bits 22-17) and mode
                self.dma_source_address =
                    (self.dma_source_address & 0x0001_FFFF) | (u32::from(value & 0x3F) << 17);
                self.dma_mode = match value & 0xC0 {
                    0x00 => DmaMode::MemoryToVram,
                    0x40 => {
                        // If DMD1=0, DMD0 is used as bit 23 in source address
                        self.dma_source_address |= 1 << 23;

                        DmaMode::MemoryToVram
                    }
                    0x80 => DmaMode::VramFill,
                    0xC0 => DmaMode::VramCopy,
                    _ => unreachable!("value & 0x0C is always 0x00/0x40/0x80/0xC0"),
                };

                log::trace!("  DMA source address: {:06X}", self.dma_source_address);
                log::trace!("  DMA mode: {:?}", self.dma_mode);
            }
            _ => {}
        }
    }

    pub fn is_line_in_v_window(&self, scanline: u16) -> bool {
        self.window_vertical_mode.in_window(scanline, self.window_y_position)
    }

    pub fn window_h_range(&self, active_display_pixels: u16) -> (u16, u16) {
        self.window_horizontal_mode.h_range(self.window_x_position, active_display_pixels)
    }

    pub fn masked_window_nametable_addr(&self) -> u16 {
        // A11 is ignored in H40 mode; Cheese Cat-Astrophe depends on this
        let mask = match self.horizontal_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => 0xF800,
            HorizontalDisplaySize::FortyCell => 0xF000,
        };
        self.window_base_nt_addr & mask
    }

    pub fn masked_sprite_attribute_table_addr(&self) -> u16 {
        self.sprite_attribute_table_base_addr
            & self.horizontal_display_size.sprite_attribute_table_mask()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum Plane {
    #[default]
    Background,
    Sprite,
    ScrollA,
    ScrollB,
}

impl Display for Plane {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Background => write!(f, "Backdrop"),
            Self::Sprite => write!(f, "Sprites"),
            Self::ScrollA => write!(f, "Plane A"),
            Self::ScrollB => write!(f, "Plane B"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct DebugRegister {
    pub display_disabled: bool,
    pub forced_plane: Plane,
}

impl DebugRegister {
    pub fn new() -> Self {
        Self { display_disabled: false, forced_plane: Plane::default() }
    }

    pub fn write(&mut self, value: u16) {
        self.display_disabled = value.bit(6);
        self.forced_plane = match (value >> 7) & 0x3 {
            0x0 => Plane::Background,
            0x1 => Plane::Sprite,
            0x2 => Plane::ScrollA,
            0x3 => Plane::ScrollB,
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        };
    }
}
