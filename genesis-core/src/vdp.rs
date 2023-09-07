mod debug;

use crate::api::GenesisTimingMode;
use crate::memory::Memory;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::frontend::Color;
use jgenesis_traits::num::GetBit;
use m68000_emu::M68000;
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
    sprite_overflow: bool,
    dot_overflow_on_prev_line: bool,
    sprite_collision: bool,
    scanline: u16,
    active_dma: Option<ActiveDma>,
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
            sprite_overflow: false,
            dot_overflow_on_prev_line: false,
            sprite_collision: false,
            scanline: 0,
            active_dma: None,
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
    // TODO handle HV latching and interrupts
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
                self.dma_length = (self.dma_length & 0xFF00) | u16::from(value);

                log::trace!("  DMA length: {}", self.dma_length);
            }
            20 => {
                // Register #20: DMA length counter (bits 15-8)
                self.dma_length = (self.dma_length & 0x00FF) | (u16::from(value) << 8);

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
    bytes_remaining: f64,
}

impl DmaTracker {
    fn new() -> Self {
        Self { in_progress: false, bytes_remaining: 0.0 }
    }

    fn init(&mut self, dma_length: u32) {
        self.bytes_remaining = f64::from(2 * dma_length);
        self.in_progress = true;
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

        let bytes_per_line: u32 = match (h_display_size, line_type) {
            (HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 16,
            (HorizontalDisplaySize::FortyCell, LineType::Active) => 18,
            (HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 167,
            (HorizontalDisplaySize::FortyCell, LineType::Blanked) => 205,
        };
        let bytes_per_line: f64 = bytes_per_line.into();
        self.bytes_remaining -=
            bytes_per_line * master_clock_cycles as f64 / MCLK_CYCLES_PER_SCANLINE as f64;
        if self.bytes_remaining <= 0.0 {
            self.in_progress = false;
        }
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
struct FrameBuffer(Vec<Color>);

impl FrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN])
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FrameBuffer {
    type Target = Vec<Color>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    vram: Vec<u8>,
    cram: [u8; CRAM_LEN],
    vsram: [u8; VSRAM_LEN],
    timing_mode: GenesisTimingMode,
    state: InternalState,
    registers: Registers,
    cached_sprite_attributes: Vec<CachedSpriteData>,
    sprite_buffer: Vec<SpriteData>,
    sprite_bit_set: SpriteBitSet,
    // Cache of CRAM in u16 form
    color_buffer: [u16; CRAM_LEN / 2],
    master_clock_cycles: u64,
    dma_tracker: DmaTracker,
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

impl GenesisTimingMode {
    fn scanlines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME,
            Self::Pal => PAL_SCANLINES_PER_FRAME,
        }
    }
}

impl Vdp {
    pub fn new(timing_mode: GenesisTimingMode) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: vec![0; VRAM_LEN],
            cram: [0; CRAM_LEN],
            vsram: [0; VSRAM_LEN],
            timing_mode,
            state: InternalState::new(),
            registers: Registers::new(),
            cached_sprite_attributes: vec![CachedSpriteData::default(); MAX_SPRITES_PER_FRAME],
            sprite_buffer: Vec::with_capacity(MAX_SPRITES_PER_FRAME),
            sprite_bit_set: SpriteBitSet::new(),
            color_buffer: [0; CRAM_LEN / 2],
            master_clock_cycles: 0,
            dma_tracker: DmaTracker::new(),
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write on scanline {}: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.state.scanline,
            self.state.control_write_flag,
            self.registers.dma_enabled
        );

        if self.state.active_dma.is_some() {
            self.state.pending_writes.push(PendingWrite::Control(value));
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

                    // Re-render the next scanline if display enabled status or background color changed
                    if self.in_hblank()
                        && (prev_display_enabled != self.registers.display_enabled
                            || prev_bg_palette != self.registers.background_palette
                            || prev_bg_color_id != self.registers.background_color_id)
                    {
                        self.render_next_scanline();
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
                    self.state.active_dma = match self.registers.dma_mode {
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

        if self.state.data_port_mode != DataPortMode::Read {
            return 0xFFFF;
        }

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        let data = match self.state.data_port_location {
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

        data
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!("VDP data write on scanline {}: {value:04X}", self.state.scanline);

        if self.state.data_port_mode != DataPortMode::Write {
            return;
        }

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        if self.state.active_dma.is_some() {
            self.state.pending_writes.push(PendingWrite::Data(value));
            return;
        }

        if self.state.code.bit(5)
            && self.registers.dma_enabled
            && self.registers.dma_mode == DmaMode::VramFill
        {
            log::trace!("Initiated VRAM fill DMA with fill data = {value:04X}");
            self.state.active_dma = Some(ActiveDma::VramFill(value));
            return;
        }

        match self.state.data_port_location {
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
    }

    pub fn read_status(&mut self) -> u16 {
        // Queue empty (bit 9) hardcoded to true
        // Queue full (bit 8) hardcoded to false
        // DMA busy (bit 1) hardcoded to false
        let interlaced_odd =
            self.registers.interlacing_mode.is_interlaced() && self.state.frame_count % 2 == 1;
        let status = 0x0200
            | (u16::from(self.state.v_interrupt_pending) << 7)
            | (u16::from(self.state.sprite_overflow) << 6)
            | (u16::from(self.state.sprite_collision) << 5)
            | (u16::from(interlaced_odd) << 4)
            | (u16::from(self.in_vblank()) << 3)
            | (u16::from(self.in_hblank()) << 2)
            | u16::from(self.timing_mode == GenesisTimingMode::Pal);

        self.state.sprite_overflow = false;
        self.state.sprite_collision = false;

        // Reset control write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        status
    }

    pub fn hv_counter(&self) -> u16 {
        log::trace!("HV counter read");

        let v_counter = match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => self.state.scanline as u8,
            InterlacingMode::InterlacedDouble => {
                let scanline = self.state.scanline << 1;
                ((scanline & !0x01) as u8) | u8::from(scanline.bit(8))
            }
        };

        let scanline_mclk = self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE;
        let h_counter = if scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE {
            let divider = match self.registers.horizontal_display_size {
                HorizontalDisplaySize::ThirtyTwoCell => 10,
                HorizontalDisplaySize::FortyCell => 8,
            };
            (scanline_mclk / divider) as u8
        } else {
            0
        };

        u16::from_be_bytes([v_counter, h_counter])
    }

    #[must_use]
    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        memory: &Memory,
        m68k: &mut M68000,
    ) -> VdpTickEffect {
        // The longest 68k instruction (DIVS) takes at most around 150 68k cycles
        assert!(master_clock_cycles < 1100);

        if let Some(active_dma) = self.state.active_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, active_dma);
        }

        let line_type = LineType::from_vdp(self);
        self.dma_tracker.tick(
            master_clock_cycles,
            self.registers.horizontal_display_size,
            line_type,
        );
        m68k.set_halted(self.dma_tracker.in_progress);

        if !self.dma_tracker.in_progress && !self.state.pending_writes.is_empty() {
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

        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();

        let prev_mclk_cycles = self.master_clock_cycles;
        self.master_clock_cycles += master_clock_cycles;

        // Check if the VDP just entered the HBlank period
        let prev_scanline_mclk = prev_mclk_cycles % MCLK_CYCLES_PER_SCANLINE;
        if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE
            && master_clock_cycles >= ACTIVE_MCLK_CYCLES_PER_SCANLINE - prev_scanline_mclk
        {
            // Render scanlines at the start of HBlank so that mid-HBlank writes will not affect
            // the next scanline
            self.render_next_scanline();

            // Check if an H/V interrupt has occurred
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

        // Check if the VDP has advanced to a new scanline
        if prev_scanline_mclk + master_clock_cycles >= MCLK_CYCLES_PER_SCANLINE {
            self.state.scanline += 1;
            if self.state.scanline == scanlines_per_frame {
                self.state.scanline = 0;
                self.state.frame_count += 1;
                self.state.frame_completed = false;
            }

            if self.state.scanline == active_scanlines && !self.state.frame_completed {
                // Trigger V interrupt if this is the first scanline of VBlank
                log::trace!("Generating V interrupt (scanline {})", self.state.scanline);
                self.state.v_interrupt_pending = true;

                self.state.frame_completed = true;
                return VdpTickEffect::FrameComplete;
            }
        }

        VdpTickEffect::None
    }

    // TODO maybe do this piecemeal instead of all at once
    fn run_dma(&mut self, memory: &Memory, active_dma: ActiveDma) {
        match active_dma {
            ActiveDma::MemoryToVram => {
                let dma_length = self.registers.dma_length();
                self.dma_tracker.init(dma_length);

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
                            self.cram[addr & 0x7F] = (word >> 8) as u8;
                            self.cram[(addr + 1) & 0x7F] = word as u8;
                        }
                        DataPortLocation::Vsram => {
                            let addr = self.state.data_address as usize;
                            self.vsram[addr % VSRAM_LEN] = (word >> 8) as u8;
                            self.vsram[(addr + 1) % VSRAM_LEN] = word as u8;
                        }
                    }

                    source_addr = source_addr.wrapping_add(2);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = source_addr;
            }
            ActiveDma::VramFill(fill_data) => {
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

        self.state.active_dma = None;
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
        let sprite_table_addr = self.registers.sprite_attribute_table_base_addr;
        let h_size = self.registers.horizontal_display_size;
        if !address.bit(2)
            && (sprite_table_addr..sprite_table_addr + 8 * h_size.sprite_table_len())
                .contains(&address)
        {
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
        log::trace!("M68K interrupt acknowledged");
        let interrupt_level = self.m68k_interrupt_level();
        if interrupt_level == 6 {
            self.state.v_interrupt_pending = false;
        } else if interrupt_level == 4 {
            self.state.h_interrupt_pending = false;
        }
    }

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
            (GenesisTimingMode::Ntsc, _, 261) | (GenesisTimingMode::Pal, _, 311) => {
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
                match self.registers.interlacing_mode {
                    InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                        self.clear_scanline(scanline);
                    }
                    InterlacingMode::InterlacedDouble => {
                        self.clear_scanline(2 * scanline);
                        self.clear_scanline(2 * scanline + 1);
                    }
                }
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
        let sprite_table_addr = self.registers.sprite_attribute_table_base_addr;

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
        let sprite_scanline = self.registers.interlacing_mode.sprite_display_top() + scanline;
        let cell_height = self.registers.interlacing_mode.cell_height();
        self.sprite_buffer.retain(|sprite| {
            let sprite_bottom = sprite.v_position + cell_height * u16::from(sprite.v_size_cells);
            (sprite.v_position..sprite_bottom).contains(&sprite_scanline)
        });

        // Apply max sprite per scanline limit
        let max_sprites_per_line = h_size.max_sprites_per_line() as usize;
        if self.sprite_buffer.len() > max_sprites_per_line {
            self.sprite_buffer.truncate(max_sprites_per_line);
            self.state.sprite_overflow = true;
        }

        // Apply max sprite pixel per scanline limit
        let mut line_pixels = 0;
        let mut dot_overflow = false;
        for i in 0..self.sprite_buffer.len() {
            let sprite_pixels = 8 * u16::from(self.sprite_buffer[i].h_size_cells);
            line_pixels += sprite_pixels;
            if line_pixels > h_size.max_sprite_pixels_per_line() {
                let overflow_pixels = line_pixels - h_size.max_sprite_pixels_per_line();
                self.sprite_buffer[i].partial_width = Some(sprite_pixels - overflow_pixels);

                self.sprite_buffer.truncate(i + 1);
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

            if found_non_zero && self.sprite_buffer[i].h_position == 0 {
                self.sprite_buffer.truncate(i);
                break;
            }
        }
        self.state.dot_overflow_on_prev_line = dot_overflow;

        // Fill in bit set
        self.sprite_bit_set.clear();
        for sprite in &self.sprite_buffer {
            for x in sprite.h_position..sprite.h_position + 8 * u16::from(sprite.h_size_cells) {
                let pixel = x.wrapping_sub(0x080);
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

        let sprite_display_top = self.registers.interlacing_mode.sprite_display_top();
        let cell_height = self.registers.interlacing_mode.cell_height();

        // Sprite horizontal display area starts at $080
        let sprite_pixel = 0x080 + pixel;

        let mut found_sprite: Option<(&SpriteData, u8)> = None;
        for sprite in &self.sprite_buffer {
            let sprite_width = sprite.partial_width.unwrap_or(8 * u16::from(sprite.h_size_cells));
            let sprite_right = sprite.h_position + sprite_width;
            if !(sprite.h_position..sprite_right).contains(&sprite_pixel) {
                continue;
            }

            let v_size_cells: u16 = sprite.v_size_cells.into();
            let h_size_cells: u16 = sprite.h_size_cells.into();

            let sprite_row = sprite_display_top + scanline - sprite.v_position;
            let sprite_row = if sprite.vertical_flip {
                cell_height * v_size_cells - 1 - sprite_row
            } else {
                sprite_row
            };

            let sprite_col = 0x080 + pixel - sprite.h_position;
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

    pub fn frame_buffer(&self) -> &[Color] {
        &self.frame_buffer
    }

    pub fn screen_width(&self) -> u32 {
        self.registers.horizontal_display_size.to_pixels().into()
    }

    pub fn screen_height(&self) -> u32 {
        let screen_height: u32 = self.registers.vertical_display_size.active_scanlines().into();
        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => screen_height,
            InterlacingMode::InterlacedDouble => 2 * screen_height,
        }
    }

    fn set_in_frame_buffer(&mut self, row: u32, col: u32, value: u16, modifier: ColorModifier) {
        let r = ((value >> 1) & 0x07) as u8;
        let g = ((value >> 5) & 0x07) as u8;
        let b = ((value >> 9) & 0x07) as u8;
        let color = gen_color_to_rgb(r, g, b, modifier);

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
    let mut modifier =
        if shadow_highlight_flag && !sprite_priority && !scroll_a_priority && !scroll_b_priority {
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
                modifier += ColorModifier::Shadow;
                continue;
            }
        }

        let color = color_buffer[((palette << 4) | color_id) as usize];
        // Sprite color id 14 is never shadowed/highlighted
        let modifier = if is_sprite && color_id == 14 { ColorModifier::None } else { modifier };
        return (color, modifier);
    }

    (bg_color, modifier)
}

fn resolve_color(cram: &[u8], palette: u8, color_id: u8) -> u16 {
    let addr = (32 * palette + 2 * color_id) as usize;
    u16::from_be_bytes([cram[addr], cram[addr + 1]])
}

fn read_v_scroll(
    vsram: &[u8],
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
    vram: &[u8],
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
    vram: &[u8],
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
    vram: &[u8],
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
const NORMAL_RGB_COLORS: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];

// i * 255 / 7 / 2
const SHADOWED_RGB_COLORS: [u8; 8] = [0, 18, 36, 55, 73, 91, 109, 128];

// 255 / 2 + i * 255 / 7 / 2
const HIGHLIGHTED_RGB_COLORS: [u8; 8] = [128, 146, 164, 182, 200, 219, 237, 255];

#[inline]
fn gen_color_to_rgb(r: u8, g: u8, b: u8, modifier: ColorModifier) -> Color {
    let colors = match modifier {
        ColorModifier::None => NORMAL_RGB_COLORS,
        ColorModifier::Shadow => SHADOWED_RGB_COLORS,
        ColorModifier::Highlight => HIGHLIGHTED_RGB_COLORS,
    };
    Color::rgb(colors[r as usize], colors[g as usize], colors[b as usize])
}
