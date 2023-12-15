//! Genesis VDP (video display processor)

mod debug;

use crate::memory::{Memory, PhysicalMedium};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::collections::VecDeque;
use std::ops::{Add, AddAssign, Deref, DerefMut};
use z80_emu::traits::InterruptLine;

const VRAM_LEN: usize = 64 * 1024;
const CRAM_LEN: usize = 128;
const VSRAM_LEN: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ControlWriteFlag {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortMode {
    Read,
    Write,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortLocation {
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VerticalScrollMode {
    FullScreen,
    TwoCell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HorizontalScrollMode {
    FullScreen,
    Cell,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HorizontalDisplaySize {
    ThirtyTwoCell,
    FortyCell,
}

impl HorizontalDisplaySize {
    const fn to_pixels(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 256,
            Self::FortyCell => 320,
        }
    }

    // Length in sprites
    const fn sprite_table_len(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 64,
            Self::FortyCell => 80,
        }
    }

    const fn max_sprites_per_line(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 16,
            Self::FortyCell => 20,
        }
    }

    const fn max_sprite_pixels_per_line(self) -> u16 {
        self.to_pixels()
    }

    const fn window_width_cells(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 32,
            Self::FortyCell => 64,
        }
    }

    const fn sprite_attribute_table_mask(self) -> u16 {
        // Sprite attribute table A9 is ignored in H40 mode
        match self {
            Self::ThirtyTwoCell => !0,
            Self::FortyCell => !0x3FF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VerticalDisplaySize {
    TwentyEightCell,
    ThirtyCell,
}

impl VerticalDisplaySize {
    fn active_scanlines(self) -> u16 {
        match self {
            Self::TwentyEightCell => 224,
            Self::ThirtyCell => 240,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum InterlacingMode {
    Progressive,
    Interlaced,
    InterlacedDouble,
}

impl InterlacingMode {
    const fn v_scroll_mask(self) -> u16 {
        // V scroll values are 10 bits normally, 11 bits in interlaced 2x mode
        match self {
            Self::Progressive | Self::Interlaced => 0x03FF,
            Self::InterlacedDouble => 0x07FF,
        }
    }

    const fn sprite_display_top(self) -> u16 {
        match self {
            // The sprite display area starts at $080 normally, $100 in interlaced 2x mode
            Self::Progressive | Self::Interlaced => 0x080,
            Self::InterlacedDouble => 0x100,
        }
    }

    const fn cell_height(self) -> u16 {
        match self {
            // Cells are 8x8 normally, 8x16 in interlaced 2x mode
            Self::Progressive | Self::Interlaced => 8,
            Self::InterlacedDouble => 16,
        }
    }

    const fn is_interlaced(self) -> bool {
        matches!(self, Self::Interlaced | Self::InterlacedDouble)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ScrollSize {
    ThirtyTwo,
    SixtyFour,
    OneTwentyEight,
}

impl ScrollSize {
    fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 => Self::ThirtyTwo,
            0x01 => Self::SixtyFour,
            0x03 => Self::OneTwentyEight,
            0x02 => {
                log::warn!("Prohibited scroll size set; defaulting to 32");
                Self::ThirtyTwo
            }
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }

    // Used to mask line and pixel values; return value is equal to (size << 3) - 1
    const fn pixel_bit_mask(self) -> u16 {
        match self {
            Self::ThirtyTwo => 0x00FF,
            Self::SixtyFour => 0x01FF,
            Self::OneTwentyEight => 0x03FF,
        }
    }
}

impl From<ScrollSize> for u16 {
    fn from(value: ScrollSize) -> Self {
        match value {
            ScrollSize::ThirtyTwo => 32,
            ScrollSize::SixtyFour => 64,
            ScrollSize::OneTwentyEight => 128,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WindowHorizontalMode {
    LeftToCenter,
    CenterToRight,
}

impl WindowHorizontalMode {
    fn in_window(self, pixel: u16, window_x: u16) -> bool {
        let cell = pixel / 8;
        match self {
            Self::LeftToCenter => cell < window_x,
            Self::CenterToRight => cell >= window_x,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WindowVerticalMode {
    TopToCenter,
    CenterToBottom,
}

impl WindowVerticalMode {
    fn in_window(self, scanline: u16, window_y: u16) -> bool {
        let cell = scanline / 8;
        match self {
            Self::TopToCenter => cell < window_y,
            Self::CenterToBottom => cell >= window_y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DmaMode {
    MemoryToVram,
    VramFill,
    VramCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ActiveDma {
    MemoryToVram,
    VramFill(u16),
    VramCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PendingWrite {
    Control(u16),
    Data(u16),
}

impl Default for PendingWrite {
    fn default() -> Self {
        Self::Control(0)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct InternalState {
    control_write_flag: ControlWriteFlag,
    code: u8,
    data_port_mode: DataPortMode,
    data_port_location: DataPortLocation,
    data_address: u16,
    latched_high_address_bits: u16,
    v_interrupt_pending: bool,
    h_interrupt_pending: bool,
    h_interrupt_counter: u16,
    latched_hv_counter: Option<u16>,
    sprite_overflow: bool,
    dot_overflow_on_prev_line: bool,
    sprite_collision: bool,
    scanline: u16,
    pending_dma: Option<ActiveDma>,
    pending_writes: Vec<PendingWrite>,
    frame_count: u64,
    // Marks whether a frame has been completed so that frames don't get double rendered if
    // a game switches from V28 to V30 mode on scanlines 224-239
    frame_completed: bool,
}

impl InternalState {
    fn new() -> Self {
        Self {
            control_write_flag: ControlWriteFlag::First,
            code: 0,
            data_port_mode: DataPortMode::Write,
            data_port_location: DataPortLocation::Vram,
            data_address: 0,
            latched_high_address_bits: 0,
            v_interrupt_pending: false,
            h_interrupt_pending: false,
            h_interrupt_counter: 0,
            latched_hv_counter: None,
            sprite_overflow: false,
            dot_overflow_on_prev_line: false,
            sprite_collision: false,
            scanline: 0,
            pending_dma: None,
            pending_writes: Vec::with_capacity(10),
            frame_count: 0,
            frame_completed: false,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    // Register #0
    h_interrupt_enabled: bool,
    hv_counter_stopped: bool,
    // Register #1
    display_enabled: bool,
    v_interrupt_enabled: bool,
    dma_enabled: bool,
    vertical_display_size: VerticalDisplaySize,
    // Register #2
    scroll_a_base_nt_addr: u16,
    // Register #3
    window_base_nt_addr: u16,
    // Register #4
    scroll_b_base_nt_addr: u16,
    // Register #5
    sprite_attribute_table_base_addr: u16,
    // Register #7
    background_palette: u8,
    background_color_id: u8,
    // Register #10
    h_interrupt_interval: u16,
    // Register #11
    // TODO external interrupts enabled
    vertical_scroll_mode: VerticalScrollMode,
    horizontal_scroll_mode: HorizontalScrollMode,
    // Register #12
    horizontal_display_size: HorizontalDisplaySize,
    shadow_highlight_flag: bool,
    interlacing_mode: InterlacingMode,
    // Register #13
    h_scroll_table_base_addr: u16,
    // Register #15
    data_port_auto_increment: u16,
    // Register #16
    vertical_scroll_size: ScrollSize,
    horizontal_scroll_size: ScrollSize,
    // Register #17
    window_horizontal_mode: WindowHorizontalMode,
    window_x_position: u16,
    // Register #18
    window_vertical_mode: WindowVerticalMode,
    window_y_position: u16,
    // Registers #19 & #20
    dma_length: u16,
    // Registers #21, #22, & #23
    dma_source_address: u32,
    dma_mode: DmaMode,
}

impl Registers {
    fn new() -> Self {
        Self {
            h_interrupt_enabled: false,
            hv_counter_stopped: false,
            display_enabled: false,
            v_interrupt_enabled: false,
            dma_enabled: false,
            vertical_display_size: VerticalDisplaySize::TwentyEightCell,
            scroll_a_base_nt_addr: 0,
            window_base_nt_addr: 0,
            scroll_b_base_nt_addr: 0,
            sprite_attribute_table_base_addr: 0,
            background_palette: 0,
            background_color_id: 0,
            h_interrupt_interval: 0,
            vertical_scroll_mode: VerticalScrollMode::FullScreen,
            horizontal_scroll_mode: HorizontalScrollMode::FullScreen,
            horizontal_display_size: HorizontalDisplaySize::ThirtyTwoCell,
            shadow_highlight_flag: false,
            interlacing_mode: InterlacingMode::Progressive,
            h_scroll_table_base_addr: 0,
            data_port_auto_increment: 0,
            vertical_scroll_size: ScrollSize::ThirtyTwo,
            horizontal_scroll_size: ScrollSize::ThirtyTwo,
            window_horizontal_mode: WindowHorizontalMode::LeftToCenter,
            window_x_position: 0,
            window_vertical_mode: WindowVerticalMode::TopToCenter,
            window_y_position: 0,
            dma_length: 0,
            dma_source_address: 0,
            dma_mode: DmaMode::MemoryToVram,
        }
    }

    fn write_internal_register(&mut self, register: u8, value: u8) {
        log::trace!("Wrote register #{register} with value {value:02X}");

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

                log::trace!("  Display enabled: {}", self.display_enabled);
                log::trace!("  V interrupt enabled: {}", self.v_interrupt_enabled);
                log::trace!("  DMA enabled: {}", self.dma_enabled);
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
                    0x02 => HorizontalScrollMode::Cell,
                    0x03 => HorizontalScrollMode::Line,
                    0x01 => {
                        log::warn!(
                            "Prohibited horizontal scroll mode set; defaulting to full scroll"
                        );
                        HorizontalScrollMode::FullScreen
                    }
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
                self.interlacing_mode = match value & 0x03 {
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

                log::trace!("  Data port auto increment: {:02X}", value);
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

    fn is_in_window(&self, scanline: u16, pixel: u16) -> bool {
        self.window_horizontal_mode.in_window(pixel, self.window_x_position)
            || self.window_vertical_mode.in_window(scanline, self.window_y_position)
    }

    fn dma_length(&self) -> u32 {
        if self.dma_length > 0 {
            self.dma_length.into()
        } else {
            // DMA length of 0 is treated as 65536
            65536
        }
    }

    fn masked_sprite_attribute_table_addr(&self) -> u16 {
        self.sprite_attribute_table_base_addr
            & self.horizontal_display_size.sprite_attribute_table_mask()
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct CachedSpriteData {
    v_position: u16,
    h_size_cells: u8,
    v_size_cells: u8,
    link_data: u8,
}

impl CachedSpriteData {
    fn update_first_word(&mut self, msb: u8, lsb: u8) {
        self.v_position = u16::from_be_bytes([msb & 0x03, lsb]);
    }

    fn update_second_word(&mut self, msb: u8, lsb: u8) {
        self.h_size_cells = ((msb >> 2) & 0x03) + 1;
        self.v_size_cells = (msb & 0x03) + 1;
        self.link_data = lsb & 0x7F;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteData {
    pattern_generator: u16,
    v_position: u16,
    h_position: u16,
    h_size_cells: u8,
    v_size_cells: u8,
    palette: u8,
    vertical_flip: bool,
    horizontal_flip: bool,
    priority: bool,
    link_data: u8,
    // Set if this sprite gets cut off because of the pixels-per-scanline limit
    partial_width: Option<u16>,
}

impl SpriteData {
    fn create(cached_data: CachedSpriteData, uncached_bytes: &[u8]) -> Self {
        // 3rd word
        let priority = uncached_bytes[0].bit(7);
        let palette = (uncached_bytes[0] >> 5) & 0x03;
        let vertical_flip = uncached_bytes[0].bit(4);
        let horizontal_flip = uncached_bytes[0].bit(3);
        let pattern_generator = u16::from_be_bytes([uncached_bytes[0] & 0x07, uncached_bytes[1]]);

        // 4th word
        let h_position = u16::from_be_bytes([uncached_bytes[2] & 0x01, uncached_bytes[3]]);

        Self {
            pattern_generator,
            v_position: cached_data.v_position,
            h_position,
            h_size_cells: cached_data.h_size_cells,
            v_size_cells: cached_data.v_size_cells,
            palette,
            vertical_flip,
            horizontal_flip,
            priority,
            link_data: cached_data.link_data,
            // Will maybe get set later
            partial_width: None,
        }
    }

    fn v_position(&self, interlacing_mode: InterlacingMode) -> u16 {
        // V position is 9 bits in progressive mode and interlaced mode 1, and 10 bits in
        // interlaced mode 2
        match interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => self.v_position & 0x1FF,
            InterlacingMode::InterlacedDouble => self.v_position & 0x3FF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorModifier {
    None,
    Shadow,
    Highlight,
}

impl Add for ColorModifier {
    type Output = Self;

    #[allow(clippy::unnested_or_patterns)]
    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::None, Self::None)
            | (Self::Shadow, Self::Highlight)
            | (Self::Highlight, Self::Shadow) => Self::None,
            (Self::None, Self::Shadow)
            | (Self::Shadow, Self::None)
            | (Self::Shadow, Self::Shadow) => Self::Shadow,
            (Self::None, Self::Highlight)
            | (Self::Highlight, Self::None)
            | (Self::Highlight, Self::Highlight) => Self::Highlight,
        }
    }
}

impl AddAssign for ColorModifier {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineType {
    Active,
    Blanked,
}

impl LineType {
    fn from_vdp(vdp: &Vdp) -> Self {
        if !vdp.registers.display_enabled || vdp.in_vblank() { Self::Blanked } else { Self::Active }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct DmaTracker {
    // TODO avoid floating point arithmetic?
    in_progress: bool,
    mode: DmaMode,
    bytes_remaining: f64,
    data_port_read: bool,
}

impl DmaTracker {
    fn new() -> Self {
        Self {
            in_progress: false,
            mode: DmaMode::MemoryToVram,
            bytes_remaining: 0.0,
            data_port_read: false,
        }
    }

    fn init(&mut self, mode: DmaMode, dma_length: u32, data_port_location: DataPortLocation) {
        self.mode = mode;
        self.bytes_remaining = f64::from(match data_port_location {
            DataPortLocation::Vram => 2 * dma_length,
            DataPortLocation::Cram | DataPortLocation::Vsram => dma_length,
        });
        self.in_progress = true;
        self.data_port_read = false;
    }

    #[inline]
    fn tick(
        &mut self,
        master_clock_cycles: u64,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        if !self.in_progress {
            return;
        }

        let bytes_per_line: u32 = match (self.mode, h_display_size, line_type) {
            (DmaMode::MemoryToVram, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 16,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::FortyCell, LineType::Active) => 18,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 167,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 205,
            (DmaMode::VramFill, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 15,
            (DmaMode::VramFill, HorizontalDisplaySize::FortyCell, LineType::Active) => 17,
            (DmaMode::VramFill, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 166,
            (DmaMode::VramFill, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 204,
            (DmaMode::VramCopy, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 8,
            (DmaMode::VramCopy, HorizontalDisplaySize::FortyCell, LineType::Active) => 9,
            (DmaMode::VramCopy, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 83,
            (DmaMode::VramCopy, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 102,
        };
        let bytes_per_line: f64 = bytes_per_line.into();
        self.bytes_remaining -=
            bytes_per_line * master_clock_cycles as f64 / MCLK_CYCLES_PER_SCANLINE as f64;
        if self.bytes_remaining <= 0.0 {
            log::trace!("VDP DMA in mode {:?} complete", self.mode);
            self.in_progress = false;
        }
    }

    fn should_halt_cpu(&self, pending_writes: &[PendingWrite]) -> bool {
        // Memory-to-VRAM DMA always halts the CPU; VRAM fill & VRAM copy only halt the CPU if it
        // accesses the VDP data port during the DMA
        self.in_progress
            && (self.mode == DmaMode::MemoryToVram
                || self.data_port_read
                || pending_writes.iter().any(|write| matches!(write, PendingWrite::Data(..))))
    }
}

const FIFO_CAPACITY: usize = 4;

#[derive(Debug, Clone, Encode, Decode)]
struct FifoTracker {
    fifo: VecDeque<DataPortLocation>,
    mclk_elapsed: f64,
}

impl FifoTracker {
    fn new() -> Self {
        Self { fifo: VecDeque::with_capacity(FIFO_CAPACITY + 1), mclk_elapsed: 0.0 }
    }

    fn record_access(&mut self, line_type: LineType, data_port_location: DataPortLocation) {
        // VRAM/CRAM/VSRAM accesses can only delay the CPU during active display
        if line_type == LineType::Blanked {
            return;
        }

        self.fifo.push_back(data_port_location);
    }

    fn tick(
        &mut self,
        master_clock_cycles: u64,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        if self.fifo.is_empty() {
            self.mclk_elapsed = 0.0;
            return;
        }

        if line_type == LineType::Blanked {
            // CPU never gets delayed during VBlank or when the display is off
            self.fifo.clear();
            self.mclk_elapsed = 0.0;
            return;
        }

        // TODO track individual slot cycles instead of doing floating-point arithmetic?

        let mclks_per_slot = match h_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => {
                // 3420 mclks/line / 16 slots/line
                213.75
            }
            HorizontalDisplaySize::FortyCell => {
                // 3420 mclks/line / 18 slots/line
                190.0
            }
        };

        self.mclk_elapsed += master_clock_cycles as f64;
        while self.mclk_elapsed >= mclks_per_slot {
            let Some(&data_port_location) = self.fifo.front() else { break };

            let slots_required = match data_port_location {
                DataPortLocation::Vram => 2.0,
                DataPortLocation::Cram | DataPortLocation::Vsram => 1.0,
            };

            if self.mclk_elapsed < slots_required * mclks_per_slot {
                break;
            }

            self.mclk_elapsed -= slots_required * mclks_per_slot;
            self.fifo.pop_front();
        }
    }

    fn is_empty(&self) -> bool {
        self.fifo.is_empty()
    }

    fn is_full(&self) -> bool {
        self.fifo.len() >= FIFO_CAPACITY
    }

    fn should_halt_cpu(&self) -> bool {
        self.fifo.len() > FIFO_CAPACITY
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteBitSet([u64; 5]);

impl SpriteBitSet {
    const LEN: u16 = 64 * 5;

    fn new() -> Self {
        Self([0; 5])
    }

    fn clear(&mut self) {
        self.0 = [0; 5];
    }

    fn set(&mut self, bit: u16) {
        self.0[(bit / 64) as usize] |= 1 << (bit % 64);
    }

    fn get(&self, bit: u16) -> bool {
        self.0[(bit / 64) as usize] & (1 << (bit % 64)) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdpTickEffect {
    None,
    FrameComplete,
}

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl FrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
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

const MAX_SCREEN_WIDTH: usize = 320;
const MAX_SCREEN_HEIGHT: usize = 240;

// Double screen height to account for interlaced 2x mode
const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * MAX_SCREEN_HEIGHT * 2;

const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
const NTSC_SCANLINES_PER_FRAME: u16 = 262;
const PAL_SCANLINES_PER_FRAME: u16 = 313;

const MAX_SPRITES_PER_FRAME: usize = 80;

// Sprites with X = $080 display at the left edge of the screen
const SPRITE_H_DISPLAY_START: u16 = 0x080;

// Master clock cycle on which to trigger VINT on scanline 224/240.
const V_INTERRUPT_DELAY: u64 = 48;

trait TimingModeExt: Copy {
    fn scanlines_per_frame(self) -> u16;
}

impl TimingModeExt for TimingMode {
    fn scanlines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME,
            Self::Pal => PAL_SCANLINES_PER_FRAME,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdpConfig {
    pub enforce_sprite_limits: bool,
    pub emulate_non_linear_dac: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    vram: Box<[u8; VRAM_LEN]>,
    cram: [u8; CRAM_LEN],
    vsram: [u8; VSRAM_LEN],
    timing_mode: TimingMode,
    state: InternalState,
    registers: Registers,
    cached_sprite_attributes: Box<[CachedSpriteData; MAX_SPRITES_PER_FRAME]>,
    sprite_buffer: Vec<SpriteData>,
    sprite_bit_set: SpriteBitSet,
    enforce_sprite_limits: bool,
    emulate_non_linear_dac: bool,
    // Cache of CRAM in u16 form
    color_buffer: [u16; CRAM_LEN / 2],
    master_clock_cycles: u64,
    dma_tracker: DmaTracker,
    fifo_tracker: FifoTracker,
}

impl Vdp {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: VdpConfig) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: vec![0; VRAM_LEN].into_boxed_slice().try_into().unwrap(),
            cram: [0; CRAM_LEN],
            vsram: [0; VSRAM_LEN],
            timing_mode,
            state: InternalState::new(),
            registers: Registers::new(),
            cached_sprite_attributes: vec![CachedSpriteData::default(); MAX_SPRITES_PER_FRAME]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            sprite_buffer: Vec::with_capacity(MAX_SPRITES_PER_FRAME),
            sprite_bit_set: SpriteBitSet::new(),
            enforce_sprite_limits: config.enforce_sprite_limits,
            emulate_non_linear_dac: config.emulate_non_linear_dac,
            color_buffer: [0; CRAM_LEN / 2],
            master_clock_cycles: 0,
            dma_tracker: DmaTracker::new(),
            fifo_tracker: FifoTracker::new(),
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write on scanline {}: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.state.scanline,
            self.state.control_write_flag,
            self.registers.dma_enabled
        );

        if self.maybe_push_pending_write(PendingWrite::Control(value)) {
            return;
        }

        match self.state.control_write_flag {
            ControlWriteFlag::First => {
                // Always latch lowest 2 code bits, even if this is a register write
                self.state.code = (self.state.code & 0xFC) | ((value >> 14) & 0x03) as u8;
                self.update_data_port_location();

                if value & 0xE000 == 0x8000 {
                    // Register set

                    let prev_display_enabled = self.registers.display_enabled;
                    let prev_bg_palette = self.registers.background_palette;
                    let prev_bg_color_id = self.registers.background_color_id;

                    let register_number = ((value >> 8) & 0x1F) as u8;
                    self.registers.write_internal_register(register_number, value as u8);

                    if self.registers.hv_counter_stopped && self.state.latched_hv_counter.is_none()
                    {
                        self.state.latched_hv_counter = Some(self.hv_counter());
                    } else if !self.registers.hv_counter_stopped
                        && self.state.latched_hv_counter.is_some()
                    {
                        self.state.latched_hv_counter = None;
                    }

                    // Re-render the next scanline if display enabled status or background color changed
                    if self.in_hblank()
                        && (prev_display_enabled != self.registers.display_enabled
                            || prev_bg_palette != self.registers.background_palette
                            || prev_bg_color_id != self.registers.background_color_id)
                    {
                        self.render_next_scanline();
                    } else if !self.in_vblank()
                        && prev_display_enabled
                        && !self.registers.display_enabled
                    {
                        // Blank out the current scanline if display is disabled near the start of a
                        // scanline during active display.
                        // 150 chosen fairly arbitrarily (15 pixels in H32 mode or 18-19 pixels in H40 mode)
                        if self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE < 150 {
                            self.clear_scanline(self.state.scanline);
                        }
                    }
                } else {
                    // First word of command write
                    self.state.data_address =
                        (self.state.latched_high_address_bits) | (value & 0x3FFF);

                    self.state.control_write_flag = ControlWriteFlag::Second;
                }
            }
            ControlWriteFlag::Second => {
                let high_address_bits = value << 14;
                self.state.latched_high_address_bits = high_address_bits;
                self.state.data_address = (self.state.data_address & 0x3FFF) | high_address_bits;
                self.state.control_write_flag = ControlWriteFlag::First;

                self.state.code = (((value >> 2) & 0x3C) as u8) | (self.state.code & 0x03);
                self.update_data_port_location();

                if self.state.code.bit(5)
                    && self.registers.dma_enabled
                    && self.registers.dma_mode != DmaMode::VramFill
                {
                    // This is a DMA initiation, not a normal control write
                    log::trace!("DMA transfer initiated, mode={:?}", self.registers.dma_mode);
                    self.state.pending_dma = match self.registers.dma_mode {
                        DmaMode::MemoryToVram => Some(ActiveDma::MemoryToVram),
                        DmaMode::VramCopy => Some(ActiveDma::VramCopy),
                        DmaMode::VramFill => unreachable!("dma_mode != VramFill"),
                    }
                }
            }
        }
    }

    fn update_data_port_location(&mut self) {
        let (data_port_location, data_port_mode) = match self.state.code & 0x0F {
            0x01 => (DataPortLocation::Vram, DataPortMode::Write),
            0x03 => (DataPortLocation::Cram, DataPortMode::Write),
            0x05 => (DataPortLocation::Vsram, DataPortMode::Write),
            0x00 => (DataPortLocation::Vram, DataPortMode::Read),
            0x08 => (DataPortLocation::Cram, DataPortMode::Read),
            0x04 => (DataPortLocation::Vsram, DataPortMode::Read),
            _ => {
                // Invalid code
                (DataPortLocation::Vram, DataPortMode::Invalid)
            }
        };

        self.state.data_port_location = data_port_location;
        self.state.data_port_mode = data_port_mode;

        log::trace!(
            "Set data port location to {data_port_location:?} and mode to {data_port_mode:?}"
        );
    }

    pub fn read_data(&mut self) -> u16 {
        log::trace!("VDP data read");

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        if self.state.data_port_mode != DataPortMode::Read {
            return 0xFFFF;
        }

        self.dma_tracker.data_port_read = true;

        let data_port_location = self.state.data_port_location;
        let data = match data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                let address = (self.state.data_address & !0x01) as usize;
                u16::from_be_bytes([self.vram[address], self.vram[(address + 1) & 0xFFFF]])
            }
            DataPortLocation::Cram => {
                let address = (self.state.data_address & 0x7F) as usize;
                u16::from_be_bytes([self.cram[address], self.cram[(address + 1) & 0x7F]])
            }
            DataPortLocation::Vsram => {
                let address = (self.state.data_address as usize) % VSRAM_LEN;
                u16::from_be_bytes([self.vsram[address], self.vsram[(address + 1) % VSRAM_LEN]])
            }
        };

        self.increment_data_address();

        let line_type = LineType::from_vdp(self);
        self.fifo_tracker.record_access(line_type, data_port_location);

        data
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!("VDP data write on scanline {}: {value:04X}", self.state.scanline);

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        if self.state.data_port_mode != DataPortMode::Write {
            return;
        }

        if self.maybe_push_pending_write(PendingWrite::Data(value)) {
            return;
        }

        if self.state.code.bit(5)
            && self.registers.dma_enabled
            && self.registers.dma_mode == DmaMode::VramFill
        {
            log::trace!("Initiated VRAM fill DMA with fill data = {value:04X}");
            self.state.pending_dma = Some(ActiveDma::VramFill(value));
            return;
        }

        let data_port_location = self.state.data_port_location;
        match data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                log::trace!("Writing to {:04X} in VRAM", self.state.data_address);
                self.write_vram_word(self.state.data_address, value);
            }
            DataPortLocation::Cram => {
                let address = (self.state.data_address & 0x7F) as usize;
                log::trace!("Writing to {address:02X} in CRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.cram[address] = msb;
                self.cram[(address + 1) & 0x7F] = lsb;
            }
            DataPortLocation::Vsram => {
                let address = (self.state.data_address as usize) % VSRAM_LEN;
                log::trace!("Writing to {address:02X} in VSRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.vsram[address] = msb;
                self.vsram[(address + 1) % VSRAM_LEN] = lsb;
            }
        }

        self.increment_data_address();

        let line_type = LineType::from_vdp(self);
        self.fifo_tracker.record_access(line_type, data_port_location);
    }

    fn maybe_push_pending_write(&mut self, write: PendingWrite) -> bool {
        if self.state.pending_dma.is_some()
            || (self.dma_tracker.in_progress && matches!(write, PendingWrite::Data(..)))
        {
            self.state.pending_writes.push(write);
            true
        } else {
            false
        }
    }

    pub fn read_status(&mut self) -> u16 {
        log::trace!("VDP status register read");

        let interlaced_odd =
            self.registers.interlacing_mode.is_interlaced() && self.state.frame_count % 2 == 1;

        let scanline_mclk = self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE;
        let v_counter: u16 = self.v_counter(scanline_mclk).into();
        let vblank_flag = match self.timing_mode {
            TimingMode::Ntsc => {
                v_counter >= VerticalDisplaySize::TwentyEightCell.active_scanlines()
                    && v_counter != 0xFF
            }
            TimingMode::Pal => {
                let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                // This OR is mecessary because the PAL V counter briefly wraps around to $00-$0A
                // during VBlank.
                // >300 comparison is because the V counter hits 0xFF twice, once at scanline 255
                // and again at scanline 312.
                (v_counter >= active_scanlines || self.state.scanline > active_scanlines)
                    && !((v_counter == 0x00 || v_counter == 0xFF) && self.state.scanline > 300)
            }
        };

        // HBlank flag is based on the H counter crossing specific values, not on mclk being >= 2560
        let h_counter = self.h_counter(scanline_mclk);
        let hblank_flag = match self.registers.horizontal_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => h_counter <= 0x04 || h_counter >= 0x93,
            HorizontalDisplaySize::FortyCell => h_counter <= 0x05 || h_counter >= 0xB3,
        };

        let status = (u16::from(self.fifo_tracker.is_empty()) << 9)
            | (u16::from(self.fifo_tracker.is_full()) << 8)
            | (u16::from(self.state.v_interrupt_pending) << 7)
            | (u16::from(self.state.sprite_overflow) << 6)
            | (u16::from(self.state.sprite_collision) << 5)
            | (u16::from(interlaced_odd) << 4)
            | (u16::from(vblank_flag) << 3)
            | (u16::from(hblank_flag) << 2)
            | (u16::from(self.dma_tracker.in_progress) << 1)
            | u16::from(self.timing_mode == TimingMode::Pal);

        self.state.sprite_overflow = false;
        self.state.sprite_collision = false;

        // Reset control write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        status
    }

    #[must_use]
    pub fn hv_counter(&self) -> u16 {
        if let Some(latched_hv_counter) = self.state.latched_hv_counter {
            return latched_hv_counter;
        }

        let scanline_mclk = self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE;

        let h_counter = self.h_counter(scanline_mclk);
        let v_counter = self.v_counter(scanline_mclk);

        log::trace!(
            "HV counter read on scanline {}; H={h_counter:02X}, V={v_counter:02X}",
            self.state.scanline
        );

        u16::from_be_bytes([v_counter, h_counter])
    }

    #[inline]
    fn h_counter(&self, scanline_mclk: u64) -> u8 {
        // Values from https://gendev.spritesmind.net/forum/viewtopic.php?t=768
        match self.registers.horizontal_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => {
                let h = (scanline_mclk / 20) as u8;
                if h <= 0x93 { h } else { h + (0xE9 - 0x94) }
            }
            HorizontalDisplaySize::FortyCell => {
                // Special cases due to pixel clock varying during HSYNC in H40 mode
                // https://gendev.spritesmind.net/forum/viewtopic.php?t=3221
                // TODO turn this into a lookup table
                match scanline_mclk {
                    // 320 pixels of active display + 25 pixels of border + 1 pixel of HSYNC, all at mclk/8
                    0..=2767 => (scanline_mclk / 16) as u8,
                    // 8 pixels of HSYNC at mclk/10
                    2768..=2847 => 173 + ((scanline_mclk - 2768) / 20) as u8,
                    // 2 pixels of HSYNC at mclk/9
                    2848..=2865 => 177 + ((scanline_mclk - 2848) / 18) as u8,
                    // 8 pixels of HSYNC at mclk/10
                    2866..=2945 => 178 + ((scanline_mclk - 2866) / 20) as u8,
                    // 1 pixel of HSYNC at mclk/8 followed by 1 pixel of HSYNC at mclk/10
                    2946..=2963 => 182,
                    // 7 pixels of HSYNC at mclk/10, wrapping around to $E4
                    2964..=3033 => ((2 * 0xE4 + 1 + (scanline_mclk - 2964) / 10) / 2) as u8,
                    // 2 pixels of HSYNC at mclk/9
                    3034..=3051 => ((2 * 0xE8 + 1 + (scanline_mclk - 3034) / 9) / 2) as u8,
                    // 8 pixels of HSYNC at mclk/10
                    3052..=3131 => ((2 * 0xE9 + 1 + (scanline_mclk - 3052) / 10) / 2) as u8,
                    // Remaining border pixels at mclk/8
                    3132..=3419 => ((2 * 0xED + 1 + (scanline_mclk - 3132) / 8) / 2) as u8,
                    _ => panic!("scanline mclk must be < 3420"),
                }
            }
        }
    }

    #[inline]
    fn v_counter(&self, scanline_mclk: u64) -> u8 {
        // Values from https://gendev.spritesmind.net/forum/viewtopic.php?t=768

        // V counter increments for the next line shortly after the start of HBlank
        let in_hblank = scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE;
        let scanline = if in_hblank {
            (self.state.scanline + 1) % self.timing_mode.scanlines_per_frame()
        } else {
            self.state.scanline
        };

        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                match (self.timing_mode, self.registers.vertical_display_size) {
                    (TimingMode::Ntsc, _) => {
                        if scanline <= 0xEA {
                            scanline as u8
                        } else {
                            (scanline - 6) as u8
                        }
                    }
                    (TimingMode::Pal, VerticalDisplaySize::TwentyEightCell) => {
                        if scanline <= 0x102 {
                            scanline as u8
                        } else {
                            (scanline - (0x103 - 0xCA)) as u8
                        }
                    }
                    (TimingMode::Pal, VerticalDisplaySize::ThirtyCell) => {
                        if scanline <= 0x10A {
                            scanline as u8
                        } else {
                            (scanline - (0x10B - 0xD2)) as u8
                        }
                    }
                }
            }
            InterlacingMode::InterlacedDouble => {
                // TODO this is not accurate
                let scanline = scanline << 1;
                (scanline as u8) | u8::from(scanline.bit(8))
            }
        }
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn tick<Medium: PhysicalMedium>(
        &mut self,
        master_clock_cycles: u64,
        memory: &mut Memory<Medium>,
    ) -> VdpTickEffect {
        // The longest 68k instruction (DIVS (xxx).l, Dn) takes 172 68k cycles / 1204 mclk cycles
        assert!(master_clock_cycles < 1250);

        // Count down DMA time before checking if a DMA was initiated in the last CPU instruction
        let line_type = LineType::from_vdp(self);
        let h_display_size = self.registers.horizontal_display_size;
        self.dma_tracker.tick(master_clock_cycles, h_display_size, line_type);
        self.fifo_tracker.tick(master_clock_cycles, h_display_size, line_type);

        if let Some(active_dma) = self.state.pending_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, active_dma);
        }

        if !self.dma_tracker.in_progress && !self.state.pending_writes.is_empty() {
            self.apply_pending_writes();
        }

        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();

        let prev_mclk_cycles = self.master_clock_cycles;
        self.master_clock_cycles += master_clock_cycles;

        // H interrupts occur a set number of mclk cycles after the end of the active display,
        // not right at the start of HBlank
        let h_interrupt_delay = match self.registers.horizontal_display_size {
            // 12 pixels after active display
            HorizontalDisplaySize::ThirtyTwoCell => 120,
            // 16 pixels after active display
            HorizontalDisplaySize::FortyCell => 128,
        };
        let prev_scanline_mclk = prev_mclk_cycles % MCLK_CYCLES_PER_SCANLINE;
        if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay
            && master_clock_cycles
                >= ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay - prev_scanline_mclk
        {
            // Render scanlines when HINT is triggered so that mid-HBlank writes will not affect
            // the next scanline
            self.render_next_scanline();

            // Check if an H interrupt has occurred
            if self.state.scanline < active_scanlines
                || self.state.scanline == scanlines_per_frame - 1
            {
                if self.state.h_interrupt_counter == 0 {
                    self.state.h_interrupt_counter = self.registers.h_interrupt_interval;

                    log::trace!("Generating H interrupt (scanline {})", self.state.scanline);
                    self.state.h_interrupt_pending = true;
                } else {
                    self.state.h_interrupt_counter -= 1;
                }
            } else {
                // H interrupt counter is constantly refreshed during VBlank
                self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
            }
        }

        // Check if a V interrupt has triggered
        if self.state.scanline == active_scanlines
            && prev_scanline_mclk < V_INTERRUPT_DELAY
            && prev_scanline_mclk + master_clock_cycles >= V_INTERRUPT_DELAY
        {
            log::trace!("Generating V interrupt");
            self.state.v_interrupt_pending = true;
        }

        // Check if the VDP has advanced to a new scanline
        if prev_scanline_mclk + master_clock_cycles >= MCLK_CYCLES_PER_SCANLINE {
            self.state.scanline += 1;
            if self.state.scanline == scanlines_per_frame {
                self.state.scanline = 0;
                self.state.frame_count += 1;
                self.state.frame_completed = false;
            }

            // Check if we already passed the VINT threshold
            if self.state.scanline == active_scanlines
                && prev_scanline_mclk + master_clock_cycles - MCLK_CYCLES_PER_SCANLINE
                    >= V_INTERRUPT_DELAY
            {
                log::trace!("Generating V interrupt");
                self.state.v_interrupt_pending = true;
            }

            if self.state.scanline == active_scanlines && !self.state.frame_completed {
                self.state.frame_completed = true;
                return VdpTickEffect::FrameComplete;
            }
        }

        VdpTickEffect::None
    }

    fn apply_pending_writes(&mut self) {
        let mut pending_writes = [PendingWrite::default(); 10];
        let pending_writes_len = self.state.pending_writes.len();
        pending_writes[..pending_writes_len].copy_from_slice(&self.state.pending_writes);
        self.state.pending_writes.clear();

        for &pending_write in &pending_writes[..pending_writes_len] {
            match pending_write {
                PendingWrite::Control(value) => {
                    self.write_control(value);
                }
                PendingWrite::Data(value) => {
                    self.write_data(value);
                }
            }
        }
    }

    // TODO maybe do this piecemeal instead of all at once
    fn run_dma<Medium: PhysicalMedium>(
        &mut self,
        memory: &mut Memory<Medium>,
        active_dma: ActiveDma,
    ) {
        match active_dma {
            ActiveDma::MemoryToVram => {
                let dma_length = self.registers.dma_length();
                self.dma_tracker.init(
                    DmaMode::MemoryToVram,
                    dma_length,
                    self.state.data_port_location,
                );

                let mut source_addr = self.registers.dma_source_address;

                log::trace!(
                    "Copying {} words from {source_addr:06X} to {:04X}, write location={:?}; data_addr_increment={:04X}",
                    dma_length,
                    self.state.data_address,
                    self.state.data_port_location,
                    self.registers.data_port_auto_increment
                );

                for _ in 0..dma_length {
                    let word = memory.read_word_for_dma(source_addr);
                    match self.state.data_port_location {
                        DataPortLocation::Vram => {
                            self.write_vram_word(self.state.data_address, word);
                        }
                        DataPortLocation::Cram => {
                            let addr = self.state.data_address as usize;
                            self.cram[addr & 0x7F] = word.msb();
                            self.cram[(addr + 1) & 0x7F] = word.lsb();
                        }
                        DataPortLocation::Vsram => {
                            let addr = self.state.data_address as usize;
                            // TODO fix VSRAM wrapping
                            self.vsram[addr % VSRAM_LEN] = word.msb();
                            self.vsram[(addr + 1) % VSRAM_LEN] = word.lsb();
                        }
                    }

                    source_addr = source_addr.wrapping_add(2);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = source_addr;
            }
            ActiveDma::VramFill(fill_data) => {
                self.dma_tracker.init(
                    DmaMode::VramFill,
                    self.registers.dma_length(),
                    DataPortLocation::Vram,
                );

                log::trace!(
                    "Running VRAM fill with addr {:04X} and length {}",
                    self.state.data_address,
                    self.registers.dma_length()
                );

                // VRAM fill is weird; it first performs a normal VRAM write with the given fill
                // data, then it repeatedly writes the MSB only to (address ^ 1)

                self.write_vram_word(self.state.data_address, fill_data);
                self.increment_data_address();

                let [msb, _] = fill_data.to_be_bytes();
                for _ in 0..self.registers.dma_length() {
                    self.vram[(self.state.data_address ^ 0x01) as usize] = msb;
                    self.maybe_update_sprite_cache(self.state.data_address);

                    self.increment_data_address();
                }
            }
            ActiveDma::VramCopy => {
                self.dma_tracker.init(
                    DmaMode::VramFill,
                    self.registers.dma_length(),
                    DataPortLocation::Vram,
                );

                log::trace!(
                    "Running VRAM copy with source addr {:04X}, dest addr {:04X}, and length {}",
                    self.registers.dma_source_address,
                    self.state.data_address,
                    self.registers.dma_length()
                );

                // VRAM copy DMA treats the source address as A15-A0 instead of A23-A1
                let mut source_addr = (self.registers.dma_source_address >> 1) as u16;
                for _ in 0..self.registers.dma_length() {
                    let dest_addr = self.state.data_address;
                    self.vram[dest_addr as usize] = self.vram[source_addr as usize];
                    self.maybe_update_sprite_cache(dest_addr);

                    source_addr = source_addr.wrapping_add(1);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = u32::from(source_addr) << 1;
            }
        }

        self.state.pending_dma = None;
        self.registers.dma_length = 0;
    }

    fn increment_data_address(&mut self) {
        self.state.data_address =
            self.state.data_address.wrapping_add(self.registers.data_port_auto_increment);
    }

    fn write_vram_word(&mut self, address: u16, value: u16) {
        let [msb, lsb] = value.to_be_bytes();
        self.vram[address as usize] = msb;
        self.vram[(address ^ 0x01) as usize] = lsb;

        self.maybe_update_sprite_cache(address);
    }

    #[inline]
    fn maybe_update_sprite_cache(&mut self, address: u16) {
        let sprite_table_addr = self.registers.masked_sprite_attribute_table_addr();
        let h_size = self.registers.horizontal_display_size;

        let (sprite_table_end, overflowed) =
            sprite_table_addr.overflowing_add(8 * h_size.sprite_table_len());
        let is_in_sprite_table = if overflowed {
            // Address overflowed; this can happen if a game puts the SAT at the very end of VRAM (e.g. Snatcher)
            // Address overflow is only possible in H32 mode when the table is located at $FE00-$FFFF, so simply check
            // if address is past start address
            address >= sprite_table_addr
        } else {
            (sprite_table_addr..sprite_table_end).contains(&address)
        };

        if !address.bit(2) && is_in_sprite_table {
            let idx = ((address - sprite_table_addr) / 8) as usize;
            let msb = self.vram[(address & !0x01) as usize];
            let lsb = self.vram[(address | 0x01) as usize];
            if !address.bit(1) {
                self.cached_sprite_attributes[idx].update_first_word(msb, lsb);
            } else {
                self.cached_sprite_attributes[idx].update_second_word(msb, lsb);
            }
        }
    }

    fn in_vblank(&self) -> bool {
        self.state.scanline >= self.registers.vertical_display_size.active_scanlines()
            && self.state.scanline < self.timing_mode.scanlines_per_frame() - 1
    }

    fn in_hblank(&self) -> bool {
        self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
    }

    #[must_use]
    pub fn m68k_interrupt_level(&self) -> u8 {
        // TODO external interrupts at level 2
        if self.state.v_interrupt_pending && self.registers.v_interrupt_enabled {
            6
        } else if self.state.h_interrupt_pending && self.registers.h_interrupt_enabled {
            4
        } else {
            0
        }
    }

    pub fn acknowledge_m68k_interrupt(&mut self) {
        let interrupt_level = self.m68k_interrupt_level();
        log::trace!("M68K interrupt acknowledged; level {interrupt_level}");
        if interrupt_level == 6 {
            self.state.v_interrupt_pending = false;
        } else if interrupt_level == 4 {
            self.state.h_interrupt_pending = false;
        }
    }

    #[must_use]
    pub fn should_halt_cpu(&self) -> bool {
        self.dma_tracker.should_halt_cpu(&self.state.pending_writes)
            || self.fifo_tracker.should_halt_cpu()
    }

    #[must_use]
    pub fn z80_interrupt_line(&self) -> InterruptLine {
        // Z80 INT line is low only during the first scanline of VBlank
        if self.state.scanline == self.registers.vertical_display_size.active_scanlines() {
            InterruptLine::Low
        } else {
            InterruptLine::High
        }
    }

    #[inline]
    fn render_next_scanline(&mut self) {
        match (self.timing_mode, self.registers.vertical_display_size, self.state.scanline) {
            (TimingMode::Ntsc, _, 261) | (TimingMode::Pal, _, 311) => {
                self.render_scanline(0);
            }
            (_, VerticalDisplaySize::TwentyEightCell, scanline @ 0..=222)
            | (_, VerticalDisplaySize::ThirtyCell, scanline @ 0..=238) => {
                self.render_scanline(scanline + 1);
            }
            _ => {}
        }
    }

    fn render_scanline(&mut self, scanline: u16) {
        if !self.registers.display_enabled {
            if scanline < self.registers.vertical_display_size.active_scanlines() {
                self.clear_scanline(scanline);
            }

            return;
        }

        let bg_color = resolve_color(
            &self.cram,
            self.registers.background_palette,
            self.registers.background_color_id,
        );

        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                self.populate_sprite_buffer(scanline);

                self.render_pixels_in_scanline(bg_color, scanline);
            }
            InterlacingMode::InterlacedDouble => {
                // Render scanlines 2N and 2N+1 at the same time
                for scanline in [2 * scanline, 2 * scanline + 1] {
                    self.populate_sprite_buffer(scanline);

                    self.render_pixels_in_scanline(bg_color, scanline);
                }
            }
        }
    }

    fn clear_scanline(&mut self, scanline: u16) {
        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                self.clear_scanline_in_buffer(scanline);
            }
            InterlacingMode::InterlacedDouble => {
                self.clear_scanline_in_buffer(2 * scanline);
                self.clear_scanline_in_buffer(2 * scanline + 1);
            }
        }
    }

    fn clear_scanline_in_buffer(&mut self, scanline: u16) {
        let scanline = scanline.into();
        let screen_width = self.registers.horizontal_display_size.to_pixels().into();
        let bg_color = resolve_color(
            &self.cram,
            self.registers.background_palette,
            self.registers.background_color_id,
        );

        for pixel in 0..screen_width {
            self.set_in_frame_buffer(scanline, pixel, bg_color, ColorModifier::None);
        }
    }

    // TODO optimize this to do fewer passes for sorting/filtering
    fn populate_sprite_buffer(&mut self, scanline: u16) {
        self.sprite_buffer.clear();

        // Populate buffer from the sprite attribute table
        let h_size = self.registers.horizontal_display_size;
        let sprite_table_addr = self.registers.masked_sprite_attribute_table_addr();

        // Sprite 0 is always populated
        let sprite_0 = SpriteData::create(
            self.cached_sprite_attributes[0],
            &self.vram[sprite_table_addr as usize + 4..sprite_table_addr as usize + 8],
        );
        let mut sprite_idx: u16 = sprite_0.link_data.into();
        self.sprite_buffer.push(sprite_0);

        for _ in 0..h_size.sprite_table_len() {
            if sprite_idx == 0 || sprite_idx >= h_size.sprite_table_len() {
                break;
            }

            let sprite_addr = sprite_table_addr.wrapping_add(8 * sprite_idx) as usize;
            let sprite = SpriteData::create(
                self.cached_sprite_attributes[sprite_idx as usize],
                &self.vram[sprite_addr + 4..sprite_addr + 8],
            );
            sprite_idx = sprite.link_data.into();
            self.sprite_buffer.push(sprite);
        }

        // Remove sprites that don't fall on this scanline
        let interlacing_mode = self.registers.interlacing_mode;
        let sprite_scanline = interlacing_mode.sprite_display_top() + scanline;
        let cell_height = interlacing_mode.cell_height();
        self.sprite_buffer.retain(|sprite| {
            let sprite_top = sprite.v_position(interlacing_mode);
            let sprite_bottom = sprite_top + cell_height * u16::from(sprite.v_size_cells);
            (sprite_top..sprite_bottom).contains(&sprite_scanline)
        });

        // Apply max sprite per scanline limit
        let max_sprites_per_line = h_size.max_sprites_per_line() as usize;
        if self.sprite_buffer.len() > max_sprites_per_line {
            if self.enforce_sprite_limits {
                self.sprite_buffer.truncate(max_sprites_per_line);
            }
            self.state.sprite_overflow = true;
        }

        // Apply max sprite pixel per scanline limit
        let mut line_pixels = 0;
        let mut dot_overflow = false;
        for i in 0..self.sprite_buffer.len() {
            let sprite_pixels = 8 * u16::from(self.sprite_buffer[i].h_size_cells);
            line_pixels += sprite_pixels;
            if line_pixels > h_size.max_sprite_pixels_per_line() {
                if self.enforce_sprite_limits {
                    let overflow_pixels = line_pixels - h_size.max_sprite_pixels_per_line();
                    self.sprite_buffer[i].partial_width = Some(sprite_pixels - overflow_pixels);

                    self.sprite_buffer.truncate(i + 1);
                }

                self.state.sprite_overflow = true;
                dot_overflow = true;
                break;
            }
        }

        // Sprites with H position 0 mask all lower priority sprites on the same scanline...with
        // some quirks. There must be at least one sprite with H != 0 before the H=0 sprite, unless
        // there was a sprite pixel overflow on the previous scanline.
        let mut found_non_zero = self.state.dot_overflow_on_prev_line;
        for i in 0..self.sprite_buffer.len() {
            if self.sprite_buffer[i].h_position != 0 {
                found_non_zero = true;
                continue;
            }

            // HACK: Actual hardware doesn't work this way, but this fixes some visual glitches in
            // Mickey Mania's 3D stages and is much easier to implement than actual HW behavior.
            //
            // Mickey Mania disables display for a short time during HBlank which reduces the number
            // of sprites and sprite pixels that will be displayed on the next line. Instead of
            // emulating this behavior, take advantage of the fact that on the lines where Mickey
            // Mania does this, the first 5 sprites in the sprite list are all H=0 sprites. Thus,
            // if we see 5 H=0 sprites in a row, apply a sprite mask.
            if self.sprite_buffer[i].h_position == 0 && (found_non_zero || i == 4) {
                self.sprite_buffer.truncate(i);
                break;
            }
        }
        self.state.dot_overflow_on_prev_line = dot_overflow;

        // Fill in bit set
        self.sprite_bit_set.clear();
        for sprite in &self.sprite_buffer {
            for x in sprite.h_position..sprite.h_position + 8 * u16::from(sprite.h_size_cells) {
                let pixel = x.wrapping_sub(SPRITE_H_DISPLAY_START);
                if pixel < SpriteBitSet::LEN {
                    self.sprite_bit_set.set(pixel);
                }
            }
        }
    }

    fn render_pixels_in_scanline(&mut self, bg_color: u16, scanline: u16) {
        // Populate color buffer
        for (i, chunk) in self.cram.chunks_exact(2).enumerate() {
            let &[msb, lsb] = chunk else { unreachable!("chunks_exact(2)") };
            self.color_buffer[i] = u16::from_be_bytes([msb, lsb]);
        }

        let cell_height = self.registers.interlacing_mode.cell_height();
        let v_scroll_size = self.registers.vertical_scroll_size;
        let h_scroll_size = self.registers.horizontal_scroll_size;

        let scroll_line_bit_mask = match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                v_scroll_size.pixel_bit_mask()
            }
            InterlacingMode::InterlacedDouble => (v_scroll_size.pixel_bit_mask() << 1) | 0x01,
        };

        let h_scroll_scanline = match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => scanline,
            InterlacingMode::InterlacedDouble => scanline / 2,
        };
        let (h_scroll_a, h_scroll_b) = read_h_scroll(
            &self.vram,
            self.registers.h_scroll_table_base_addr,
            self.registers.horizontal_scroll_mode,
            h_scroll_scanline,
        );

        let mut scroll_a_nt_row = u16::MAX;
        let mut scroll_a_nt_col = u16::MAX;
        let mut scroll_a_nt_word = NameTableWord::default();

        let mut scroll_b_nt_row = u16::MAX;
        let mut scroll_b_nt_col = u16::MAX;
        let mut scroll_b_nt_word = NameTableWord::default();

        for pixel in 0..self.registers.horizontal_display_size.to_pixels() {
            let h_cell = pixel / 8;
            let (v_scroll_a, v_scroll_b) = read_v_scroll(
                &self.vsram,
                self.registers.vertical_scroll_mode,
                self.registers.interlacing_mode,
                h_cell,
            );

            let scrolled_scanline_a = scanline.wrapping_add(v_scroll_a) & scroll_line_bit_mask;
            let scroll_a_v_cell = scrolled_scanline_a / cell_height;

            let scrolled_scanline_b = scanline.wrapping_add(v_scroll_b) & scroll_line_bit_mask;
            let scroll_b_v_cell = scrolled_scanline_b / cell_height;

            let scrolled_pixel_a = pixel.wrapping_sub(h_scroll_a) & h_scroll_size.pixel_bit_mask();
            let scroll_a_h_cell = scrolled_pixel_a / 8;

            let scrolled_pixel_b = pixel.wrapping_sub(h_scroll_b) & h_scroll_size.pixel_bit_mask();
            let scroll_b_h_cell = scrolled_pixel_b / 8;

            if scroll_a_v_cell != scroll_a_nt_row || scroll_a_h_cell != scroll_a_nt_col {
                scroll_a_nt_word = read_name_table_word(
                    &self.vram,
                    self.registers.scroll_a_base_nt_addr,
                    h_scroll_size.into(),
                    scroll_a_v_cell,
                    scroll_a_h_cell,
                );
                scroll_a_nt_row = scroll_a_v_cell;
                scroll_a_nt_col = scroll_a_h_cell;
            }

            if scroll_b_v_cell != scroll_b_nt_row || scroll_b_h_cell != scroll_b_nt_col {
                scroll_b_nt_word = read_name_table_word(
                    &self.vram,
                    self.registers.scroll_b_base_nt_addr,
                    h_scroll_size.into(),
                    scroll_b_v_cell,
                    scroll_b_h_cell,
                );
                scroll_b_nt_row = scroll_b_v_cell;
                scroll_b_nt_col = scroll_b_h_cell;
            }

            let scroll_a_color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: scroll_a_nt_word.vertical_flip,
                    horizontal_flip: scroll_a_nt_word.horizontal_flip,
                    pattern_generator: scroll_a_nt_word.pattern_generator,
                    row: scrolled_scanline_a,
                    col: scrolled_pixel_a,
                    cell_height,
                },
            );
            let scroll_b_color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: scroll_b_nt_word.vertical_flip,
                    horizontal_flip: scroll_b_nt_word.horizontal_flip,
                    pattern_generator: scroll_b_nt_word.pattern_generator,
                    row: scrolled_scanline_b,
                    col: scrolled_pixel_b,
                    cell_height,
                },
            );

            let in_window = self.registers.is_in_window(scanline, pixel);
            let (window_priority, window_palette, window_color_id) = if in_window {
                let v_cell = scanline / cell_height;
                let window_nt_word = read_name_table_word(
                    &self.vram,
                    self.registers.window_base_nt_addr,
                    self.registers.horizontal_display_size.window_width_cells(),
                    v_cell,
                    h_cell,
                );
                let window_color_id = read_pattern_generator(
                    &self.vram,
                    PatternGeneratorArgs {
                        vertical_flip: window_nt_word.vertical_flip,
                        horizontal_flip: window_nt_word.horizontal_flip,
                        pattern_generator: window_nt_word.pattern_generator,
                        row: scanline,
                        col: pixel,
                        cell_height,
                    },
                );
                (window_nt_word.priority, window_nt_word.palette, window_color_id)
            } else {
                (false, 0, 0)
            };

            let (sprite_priority, sprite_palette, sprite_color_id) = self
                .find_first_overlapping_sprite(scanline, pixel)
                .map_or((false, 0, 0), |(sprite, color_id)| {
                    (sprite.priority, sprite.palette, color_id)
                });

            let (scroll_a_priority, scroll_a_palette, scroll_a_color_id) = if in_window {
                // Window replaces scroll A if this pixel is inside the window
                (window_priority, window_palette, window_color_id)
            } else {
                (scroll_a_nt_word.priority, scroll_a_nt_word.palette, scroll_a_color_id)
            };

            let (pixel_color, color_modifier) = determine_pixel_color(
                &self.color_buffer,
                PixelColorArgs {
                    sprite_priority,
                    sprite_palette,
                    sprite_color_id,
                    scroll_a_priority,
                    scroll_a_palette,
                    scroll_a_color_id,
                    scroll_b_priority: scroll_b_nt_word.priority,
                    scroll_b_palette: scroll_b_nt_word.palette,
                    scroll_b_color_id,
                    bg_color,
                    shadow_highlight_flag: self.registers.shadow_highlight_flag,
                },
            );

            self.set_in_frame_buffer(scanline.into(), pixel.into(), pixel_color, color_modifier);
        }
    }

    fn find_first_overlapping_sprite(
        &mut self,
        scanline: u16,
        pixel: u16,
    ) -> Option<(&SpriteData, u8)> {
        if !self.sprite_bit_set.get(pixel) {
            return None;
        }

        let interlacing_mode = self.registers.interlacing_mode;
        let sprite_display_top = interlacing_mode.sprite_display_top();
        let cell_height = interlacing_mode.cell_height();

        let sprite_pixel = SPRITE_H_DISPLAY_START + pixel;

        let mut found_sprite: Option<(&SpriteData, u8)> = None;
        for sprite in &self.sprite_buffer {
            let sprite_width = sprite.partial_width.unwrap_or(8 * u16::from(sprite.h_size_cells));
            let sprite_right = sprite.h_position + sprite_width;
            if !(sprite.h_position..sprite_right).contains(&sprite_pixel) {
                continue;
            }

            let v_size_cells: u16 = sprite.v_size_cells.into();
            let h_size_cells: u16 = sprite.h_size_cells.into();

            let sprite_row = sprite_display_top + scanline - sprite.v_position(interlacing_mode);
            let sprite_row = if sprite.vertical_flip {
                cell_height * v_size_cells - 1 - sprite_row
            } else {
                sprite_row
            };

            let sprite_col = sprite_pixel - sprite.h_position;
            let sprite_col =
                if sprite.horizontal_flip { 8 * h_size_cells - 1 - sprite_col } else { sprite_col };

            let pattern_offset = (sprite_col / 8) * v_size_cells + sprite_row / cell_height;
            let color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: false,
                    horizontal_flip: false,
                    pattern_generator: sprite.pattern_generator.wrapping_add(pattern_offset),
                    row: sprite_row % cell_height,
                    col: sprite_col % 8,
                    cell_height,
                },
            );
            if color_id == 0 {
                // Sprite pixel is transparent
                continue;
            }

            match found_sprite {
                Some(_) => {
                    self.state.sprite_collision = true;
                    break;
                }
                None => {
                    found_sprite = Some((sprite, color_id));
                    if self.state.sprite_collision {
                        // No point in continuing to check sprites if the collision flag is
                        // already set
                        break;
                    }
                }
            }
        }

        found_sprite
    }

    #[must_use]
    pub fn frame_buffer(&self) -> &[Color; FRAME_BUFFER_LEN] {
        &self.frame_buffer
    }

    #[must_use]
    pub fn screen_width(&self) -> u32 {
        self.registers.horizontal_display_size.to_pixels().into()
    }

    #[must_use]
    pub fn screen_height(&self) -> u32 {
        let screen_height: u32 = self.registers.vertical_display_size.active_scanlines().into();
        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => screen_height,
            InterlacingMode::InterlacedDouble => 2 * screen_height,
        }
    }

    #[must_use]
    pub fn config(&self) -> VdpConfig {
        VdpConfig {
            enforce_sprite_limits: self.enforce_sprite_limits,
            emulate_non_linear_dac: self.emulate_non_linear_dac,
        }
    }

    pub fn reload_config(&mut self, config: VdpConfig) {
        self.enforce_sprite_limits = config.enforce_sprite_limits;
        self.emulate_non_linear_dac = config.emulate_non_linear_dac;
    }

    fn set_in_frame_buffer(&mut self, row: u32, col: u32, value: u16, modifier: ColorModifier) {
        let r = ((value >> 1) & 0x07) as u8;
        let g = ((value >> 5) & 0x07) as u8;
        let b = ((value >> 9) & 0x07) as u8;
        let color = gen_color_to_rgb(r, g, b, modifier, self.emulate_non_linear_dac);

        let screen_width = self.screen_width();
        self.frame_buffer[(row * screen_width + col) as usize] = color;
    }
}

#[derive(Debug, Clone, Copy)]
struct UnresolvedColor {
    palette: u8,
    color_id: u8,
    is_sprite: bool,
}

struct PixelColorArgs {
    sprite_priority: bool,
    sprite_palette: u8,
    sprite_color_id: u8,
    scroll_a_priority: bool,
    scroll_a_palette: u8,
    scroll_a_color_id: u8,
    scroll_b_priority: bool,
    scroll_b_palette: u8,
    scroll_b_color_id: u8,
    bg_color: u16,
    shadow_highlight_flag: bool,
}

#[inline]
#[allow(clippy::unnested_or_patterns)]
fn determine_pixel_color(
    color_buffer: &[u16],
    PixelColorArgs {
        sprite_priority,
        sprite_palette,
        sprite_color_id,
        scroll_a_priority,
        scroll_a_palette,
        scroll_a_color_id,
        scroll_b_priority,
        scroll_b_palette,
        scroll_b_color_id,
        bg_color,
        shadow_highlight_flag,
    }: PixelColorArgs,
) -> (u16, ColorModifier) {
    let mut modifier = if shadow_highlight_flag && !scroll_a_priority && !scroll_b_priority {
        // If shadow/highlight bit is set and all priority flags are 0, default modifier to shadow
        ColorModifier::Shadow
    } else {
        ColorModifier::None
    };

    let sprite =
        UnresolvedColor { palette: sprite_palette, color_id: sprite_color_id, is_sprite: true };
    let scroll_a = UnresolvedColor {
        palette: scroll_a_palette,
        color_id: scroll_a_color_id,
        is_sprite: false,
    };
    let scroll_b = UnresolvedColor {
        palette: scroll_b_palette,
        color_id: scroll_b_color_id,
        is_sprite: false,
    };
    let colors = match (sprite_priority, scroll_a_priority, scroll_b_priority) {
        (false, false, false) | (true, false, false) | (true, true, false) | (true, true, true) => {
            [sprite, scroll_a, scroll_b]
        }
        (false, true, false) => [scroll_a, sprite, scroll_b],
        (false, false, true) => [scroll_b, sprite, scroll_a],
        (true, false, true) => [sprite, scroll_b, scroll_a],
        (false, true, true) => [scroll_a, scroll_b, sprite],
    };

    for UnresolvedColor { palette, color_id, is_sprite } in colors {
        if color_id == 0 {
            // Pixel is transparent
            continue;
        }

        if shadow_highlight_flag && is_sprite && palette == 3 {
            if color_id == 14 {
                // Palette 3 + color 14 = highlight; sprite is transparent, underlying pixel is highlighted
                modifier += ColorModifier::Highlight;
                continue;
            } else if color_id == 15 {
                // Palette 3 + color 15 = shadow; sprite is transparent, underlying pixel is shadowed
                modifier = ColorModifier::Shadow;
                continue;
            }
        }

        let color = color_buffer[((palette << 4) | color_id) as usize];
        // Sprite color id 14 is never shadowed/highlighted, and neither is a sprite with the priority
        // bit set
        let modifier = if is_sprite && (color_id == 14 || sprite_priority) {
            ColorModifier::None
        } else {
            modifier
        };
        return (color, modifier);
    }

    (bg_color, modifier)
}

fn resolve_color(cram: &[u8; CRAM_LEN], palette: u8, color_id: u8) -> u16 {
    let addr = (32 * palette + 2 * color_id) as usize;
    u16::from_be_bytes([cram[addr], cram[addr + 1]])
}

fn read_v_scroll(
    vsram: &[u8; VSRAM_LEN],
    v_scroll_mode: VerticalScrollMode,
    interlacing_mode: InterlacingMode,
    h_cell: u16,
) -> (u16, u16) {
    let (v_scroll_a, v_scroll_b) = match v_scroll_mode {
        VerticalScrollMode::FullScreen => {
            let v_scroll_a = u16::from_be_bytes([vsram[0], vsram[1]]);
            let v_scroll_b = u16::from_be_bytes([vsram[2], vsram[3]]);
            (v_scroll_a, v_scroll_b)
        }
        VerticalScrollMode::TwoCell => {
            let addr = 4 * (h_cell as usize / 2);
            let v_scroll_a = u16::from_be_bytes([vsram[addr], vsram[addr + 1]]);
            let v_scroll_b = u16::from_be_bytes([vsram[addr + 2], vsram[addr + 3]]);
            (v_scroll_a, v_scroll_b)
        }
    };

    let v_scroll_mask = interlacing_mode.v_scroll_mask();
    (v_scroll_a & v_scroll_mask, v_scroll_b & v_scroll_mask)
}

fn read_h_scroll(
    vram: &[u8; VRAM_LEN],
    h_scroll_table_addr: u16,
    h_scroll_mode: HorizontalScrollMode,
    scanline: u16,
) -> (u16, u16) {
    let (h_scroll_a, h_scroll_b) = match h_scroll_mode {
        HorizontalScrollMode::FullScreen => {
            let h_scroll_a = u16::from_be_bytes([
                vram[h_scroll_table_addr as usize],
                vram[h_scroll_table_addr.wrapping_add(1) as usize],
            ]);
            let h_scroll_b = u16::from_be_bytes([
                vram[h_scroll_table_addr.wrapping_add(2) as usize],
                vram[h_scroll_table_addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
        HorizontalScrollMode::Cell => {
            let v_cell = scanline / 8;
            let addr = h_scroll_table_addr.wrapping_add(32 * v_cell);
            let h_scroll_a =
                u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);
            let h_scroll_b = u16::from_be_bytes([
                vram[addr.wrapping_add(2) as usize],
                vram[addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
        HorizontalScrollMode::Line => {
            let addr = h_scroll_table_addr.wrapping_add(4 * scanline);
            let h_scroll_a =
                u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);
            let h_scroll_b = u16::from_be_bytes([
                vram[addr.wrapping_add(2) as usize],
                vram[addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
    };

    (h_scroll_a & 0x03FF, h_scroll_b & 0x03FF)
}

#[derive(Debug, Clone, Copy, Default)]
struct NameTableWord {
    priority: bool,
    palette: u8,
    vertical_flip: bool,
    horizontal_flip: bool,
    pattern_generator: u16,
}

fn read_name_table_word(
    vram: &[u8; VRAM_LEN],
    base_addr: u16,
    name_table_width: u16,
    row: u16,
    col: u16,
) -> NameTableWord {
    let row_addr = base_addr.wrapping_add(2 * row * name_table_width);
    let addr = row_addr.wrapping_add(2 * col);
    let word = u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);

    NameTableWord {
        priority: word.bit(15),
        palette: ((word >> 13) & 0x03) as u8,
        vertical_flip: word.bit(12),
        horizontal_flip: word.bit(11),
        pattern_generator: word & 0x07FF,
    }
}

#[derive(Debug, Clone)]
struct PatternGeneratorArgs {
    vertical_flip: bool,
    horizontal_flip: bool,
    pattern_generator: u16,
    row: u16,
    col: u16,
    cell_height: u16,
}

#[inline]
fn read_pattern_generator(
    vram: &[u8; VRAM_LEN],
    PatternGeneratorArgs {
        vertical_flip,
        horizontal_flip,
        pattern_generator,
        row,
        col,
        cell_height,
    }: PatternGeneratorArgs,
) -> u8 {
    let cell_row =
        if vertical_flip { cell_height - 1 - (row % cell_height) } else { row % cell_height };
    let cell_col = if horizontal_flip { 7 - (col % 8) } else { col % 8 };

    let row_addr = (4 * cell_height).wrapping_mul(pattern_generator);
    let addr = (row_addr + 4 * cell_row + (cell_col >> 1)) as usize;
    (vram[addr] >> (4 - ((cell_col & 0x01) << 2))) & 0x0F
}

// i * 255 / 7
const NORMAL_RGB_COLORS_LINEAR: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];

// i * 255 / 7 / 2
const SHADOWED_RGB_COLORS_LINEAR: [u8; 8] = [0, 18, 36, 55, 73, 91, 109, 128];

// 255 / 2 + i * 255 / 7 / 2
const HIGHLIGHTED_RGB_COLORS_LINEAR: [u8; 8] = [128, 146, 164, 182, 200, 219, 237, 255];

// Values from http://gendev.spritesmind.net/forum/viewtopic.php?f=22&t=2188
const NORMAL_RGB_COLORS_NON_LINEAR: [u8; 8] = [0, 52, 87, 116, 144, 172, 206, 255];
const SHADOWED_RGB_COLORS_NON_LINEAR: [u8; 8] = [0, 29, 52, 70, 87, 101, 116, 130];
const HIGHLIGHTED_RGB_COLORS_NON_LINEAR: [u8; 8] = [130, 144, 158, 172, 187, 206, 228, 255];

#[inline]
fn gen_color_to_rgb(
    r: u8,
    g: u8,
    b: u8,
    modifier: ColorModifier,
    emulate_non_linear_dac: bool,
) -> Color {
    let colors = match (modifier, emulate_non_linear_dac) {
        (ColorModifier::None, false) => NORMAL_RGB_COLORS_LINEAR,
        (ColorModifier::Shadow, false) => SHADOWED_RGB_COLORS_LINEAR,
        (ColorModifier::Highlight, false) => HIGHLIGHTED_RGB_COLORS_LINEAR,
        (ColorModifier::None, true) => NORMAL_RGB_COLORS_NON_LINEAR,
        (ColorModifier::Shadow, true) => SHADOWED_RGB_COLORS_NON_LINEAR,
        (ColorModifier::Highlight, true) => HIGHLIGHTED_RGB_COLORS_NON_LINEAR,
    };
    Color::rgb(colors[r as usize], colors[g as usize], colors[b as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_vdp() -> Vdp {
        Vdp::new(
            TimingMode::Ntsc,
            VdpConfig { enforce_sprite_limits: true, emulate_non_linear_dac: false },
        )
    }

    #[test]
    fn h_counter_basic_functionality() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::ThirtyTwoCell;
        assert_eq!(vdp.h_counter(0), 0);
        assert_eq!(vdp.h_counter(80), 4);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE - 1), 0x7F);

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::FortyCell;
        assert_eq!(vdp.h_counter(0), 0);
        assert_eq!(vdp.h_counter(80), 5);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE - 1), 0x9F);
    }

    #[test]
    fn h_counter_hblank_h32() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::ThirtyTwoCell;
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE), 0x80);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 80), 0x84);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 380), 0x93);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 400), 0xE9);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 41), 0xFD);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 21), 0xFE);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 1), 0xFF);
    }

    #[test]
    fn h_counter_hblank_h40() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::FortyCell;
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE), 0xA0);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 200), 0xAC);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 208), 0xAD);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 288), 0xB1);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 386), 0xB6);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 404), 0xE4);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 16), 0xFE);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 1), 0xFF);
    }
}
