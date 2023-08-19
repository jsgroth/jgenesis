use crate::memory::Memory;
use smsgg_core::num::GetBit;
use std::cmp::Ordering;

const VRAM_LEN: usize = 64 * 1024;
const CRAM_LEN: usize = 128;
const VSRAM_LEN: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlWriteFlag {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataPortMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataPortLocation {
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerticalScrollMode {
    FullScreen,
    TwoCell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HorizontalScrollMode {
    FullScreen,
    Cell,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    const fn window_cell_width(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 32,
            Self::FortyCell => 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterlacingMode {
    Progressive,
    Interlaced,
    InterlacedDouble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaMode {
    MemoryToVram,
    VramFill,
    VramCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveDma {
    MemoryToVram,
    VramFill(u16),
    VramCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingWrite {
    Control(u16),
    Data(u16),
}

impl Default for PendingWrite {
    fn default() -> Self {
        Self::Control(0)
    }
}

#[derive(Debug, Clone)]
struct Registers {
    // Internal state
    control_write_flag: ControlWriteFlag,
    first_word_code_bits: u8,
    code: u8,
    data_port_mode: DataPortMode,
    data_port_location: DataPortLocation,
    data_address: u16,
    v_interrupt_pending: bool,
    h_interrupt_pending: bool,
    h_interrupt_counter: u16,
    sprite_overflow: bool,
    sprite_collision: bool,
    scanline: u16,
    active_dma: Option<ActiveDma>,
    pending_writes: Vec<PendingWrite>,
    // Register #0
    h_interrupt_enabled: bool,
    hv_counter_stopped: bool,
    // Register #1
    display_enabled: bool,
    v_interrupt_enabled: bool,
    dma_enabled: bool,
    // TODO PAL V30-cell mode
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
    interlacing_mode: InterlacingMode,
    // TODO shadows/highlights
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
            control_write_flag: ControlWriteFlag::First,
            first_word_code_bits: 0,
            code: 0,
            data_port_mode: DataPortMode::Write,
            data_port_location: DataPortLocation::Vram,
            data_address: 0,
            v_interrupt_pending: false,
            h_interrupt_pending: false,
            h_interrupt_counter: 0,
            sprite_overflow: false,
            sprite_collision: false,
            scanline: 0,
            active_dma: None,
            pending_writes: Vec::with_capacity(10),
            h_interrupt_enabled: false,
            hv_counter_stopped: false,
            display_enabled: false,
            v_interrupt_enabled: false,
            dma_enabled: false,
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

    fn increment_data_address(&mut self) {
        self.data_address = self
            .data_address
            .wrapping_add(self.data_port_auto_increment);
    }

    fn write_internal_register(&mut self, register: u8, value: u8) {
        log::trace!("Wrote register #{register} with value {value:02X}");

        match register {
            0 => {
                // Register #0: Mode set register 1
                self.h_interrupt_enabled = value.bit(4);
                self.hv_counter_stopped = value.bit(1);
            }
            1 => {
                // Register #1: Mode set register 2
                self.display_enabled = value.bit(6);
                self.v_interrupt_enabled = value.bit(5);
                self.dma_enabled = value.bit(4);
                // TODO PAL V30-cell mode
            }
            2 => {
                // Register #2: Scroll A name table base address (bits 15-13)
                self.scroll_a_base_nt_addr = u16::from(value & 0x38) << 10;
            }
            3 => {
                // Register #3: Window name table base address (bits 15-11)
                self.window_base_nt_addr = u16::from(value & 0x3E) << 10;
            }
            4 => {
                // Register #4: Scroll B name table base address (bits 15-13)
                self.scroll_b_base_nt_addr = u16::from(value & 0x07) << 13;
            }
            5 => {
                // Register #5: Sprite attribute table base address (bits 15-9)
                self.sprite_attribute_table_base_addr = u16::from(value & 0x7F) << 9;
            }
            7 => {
                // Register #7: Background color
                self.background_palette = (value >> 4) & 0x03;
                self.background_color_id = value & 0x0F;
            }
            10 => {
                // Register #10: H interrupt interval
                self.h_interrupt_interval = value.into();
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
                }
            }
            12 => {
                // Register #12: Mode set register 4
                self.horizontal_display_size = if value.bit(7) || value.bit(0) {
                    HorizontalDisplaySize::FortyCell
                } else {
                    HorizontalDisplaySize::ThirtyTwoCell
                };
                // TODO shadows/highlights
                self.interlacing_mode = match value & 0x03 {
                    0x00 => InterlacingMode::Progressive,
                    0x01 => InterlacingMode::Interlaced,
                    0x03 => InterlacingMode::InterlacedDouble,
                    0x02 => {
                        log::warn!("Prohibited interlacing mode set; defaulting to progressive");
                        InterlacingMode::Progressive
                    }
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                };
            }
            13 => {
                // Register #13: Horizontal scroll table base address (bits 15-10)
                self.h_scroll_table_base_addr = u16::from(value & 0x3F) << 10;
            }
            15 => {
                // Register #15: VRAM address auto increment
                self.data_port_auto_increment = value.into();
            }
            16 => {
                // Register #16: Scroll size
                self.vertical_scroll_size = ScrollSize::from_bits(value >> 4);
                self.horizontal_scroll_size = ScrollSize::from_bits(value);
            }
            17 => {
                // Register #17: Window horizontal position
                self.window_horizontal_mode = if value.bit(7) {
                    WindowHorizontalMode::CenterToRight
                } else {
                    WindowHorizontalMode::LeftToCenter
                };
                self.window_x_position = u16::from(value & 0x1F) << 1;
            }
            18 => {
                // Register #18: Window vertical position
                self.window_vertical_mode = if value.bit(7) {
                    WindowVerticalMode::CenterToBottom
                } else {
                    WindowVerticalMode::TopToCenter
                };
                self.window_y_position = (value & 0x1F).into();
            }
            19 => {
                // Register #19: DMA length counter (bits 7-0)
                self.dma_length = (self.dma_length & 0xFF00) | u16::from(value);
            }
            20 => {
                // Register #20: DMA length counter (bits 15-8)
                self.dma_length = (self.dma_length & 0x00FF) | (u16::from(value) << 8);
            }
            21 => {
                // Register 21: DMA source address (bits 9-1)
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFF_FE00) | (u32::from(value) << 1);
            }
            22 => {
                // Register 22: DMA source address (bits 16-9)
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFE_01FF) | (u32::from(value) << 9);
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
                }
            }
            _ => {}
        }
    }

    fn is_in_window(&self, scanline: u16, pixel: u16) -> bool {
        self.window_horizontal_mode
            .in_window(pixel, self.window_x_position)
            || self
                .window_vertical_mode
                .in_window(scanline, self.window_y_position)
    }
}

#[derive(Debug, Clone)]
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
    sprite_priority: u8,
}

impl SpriteData {
    fn from_attribute_table(sprite_bytes: &[u8]) -> Self {
        // 1st word
        let v_position = u16::from_be_bytes([sprite_bytes[0] & 0x03, sprite_bytes[1]]);

        // 2nd word
        let h_size_cells = ((sprite_bytes[2] >> 2) & 0x03) + 1;
        let v_size_cells = (sprite_bytes[2] & 0x03) + 1;
        let link_data = sprite_bytes[3] & 0x7F;

        // 3rd word
        let priority = sprite_bytes[4].bit(7);
        let palette = (sprite_bytes[4] >> 5) & 0x03;
        let vertical_flip = sprite_bytes[4].bit(4);
        let horizontal_flip = sprite_bytes[4].bit(3);
        let pattern_generator = u16::from_be_bytes([sprite_bytes[4] & 0x07, sprite_bytes[5]]);

        // 4th word
        let h_position = u16::from_be_bytes([sprite_bytes[6] & 0x01, sprite_bytes[7]]);

        Self {
            pattern_generator,
            v_position,
            h_position,
            h_size_cells,
            v_size_cells,
            palette,
            vertical_flip,
            horizontal_flip,
            priority,
            link_data,
            // Will get filled in later
            sprite_priority: 0xFF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdpTickEffect {
    None,
    FrameComplete,
}

#[derive(Debug, Clone)]
pub struct Vdp {
    frame_buffer: Vec<u16>,
    vram: Vec<u8>,
    cram: [u8; CRAM_LEN],
    vsram: [u8; VSRAM_LEN],
    registers: Registers,
    sprite_buffer: Vec<SpriteData>,
    master_clock_cycles: u64,
}

const MAX_SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 224;
const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * SCREEN_HEIGHT;

const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
const SCANLINES_PER_FRAME: u16 = 262;
const ACTIVE_SCANLINES: u16 = 224;

const MAX_SPRITES_PER_FRAME: usize = 80;

impl Vdp {
    pub fn new() -> Self {
        Self {
            frame_buffer: vec![0; FRAME_BUFFER_LEN],
            vram: vec![0; VRAM_LEN],
            cram: [0; CRAM_LEN],
            vsram: [0; VSRAM_LEN],
            registers: Registers::new(),
            sprite_buffer: Vec::with_capacity(MAX_SPRITES_PER_FRAME),
            master_clock_cycles: 0,
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.registers.control_write_flag,
            self.registers.dma_enabled
        );

        if self.registers.active_dma.is_some() {
            self.registers
                .pending_writes
                .push(PendingWrite::Control(value));
            return;
        }

        // TODO DMA
        match self.registers.control_write_flag {
            ControlWriteFlag::First => {
                if value & 0xE000 == 0x8000 {
                    // Register set
                    let register_number = ((value >> 8) & 0x1F) as u8;
                    self.registers
                        .write_internal_register(register_number, value as u8);
                } else {
                    // First word of command write
                    self.registers.first_word_code_bits = ((value >> 14) & 0x03) as u8;
                    self.registers.data_address =
                        (self.registers.data_address & 0xC000) | (value & 0x3FFF);

                    self.registers.control_write_flag = ControlWriteFlag::Second;
                }
            }
            ControlWriteFlag::Second => {
                self.registers.data_address =
                    (self.registers.data_address & 0x3FFF) | (value << 14);
                self.registers.control_write_flag = ControlWriteFlag::First;

                let code = (((value >> 2) & 0x3C) as u8) | self.registers.first_word_code_bits;
                let (data_port_location, data_port_mode) = match code & 0x0F {
                    0x01 => (DataPortLocation::Vram, DataPortMode::Write),
                    0x03 => (DataPortLocation::Cram, DataPortMode::Write),
                    0x05 => (DataPortLocation::Vsram, DataPortMode::Write),
                    0x00 => (DataPortLocation::Vram, DataPortMode::Read),
                    0x08 => (DataPortLocation::Cram, DataPortMode::Read),
                    0x04 => (DataPortLocation::Vsram, DataPortMode::Read),
                    _ => {
                        log::warn!("Invalid VDP control code: {code:02X}");
                        (DataPortLocation::Vram, DataPortMode::Write)
                    }
                };

                self.registers.code = code;
                self.registers.data_port_location = data_port_location;
                self.registers.data_port_mode = data_port_mode;

                log::trace!("Set data port location to {data_port_location:?} and mode to {data_port_mode:?}");

                if code.bit(5)
                    && self.registers.dma_enabled
                    && self.registers.dma_mode != DmaMode::VramFill
                    && self.registers.dma_length > 0
                {
                    // This is a DMA initiation, not a normal control write
                    log::trace!("DMA transfer initiated, mode={:?}", self.registers.dma_mode);
                    self.registers.active_dma = match self.registers.dma_mode {
                        DmaMode::MemoryToVram => Some(ActiveDma::MemoryToVram),
                        DmaMode::VramCopy => Some(ActiveDma::VramCopy),
                        DmaMode::VramFill => unreachable!("dma_mode != VramFill"),
                    }
                }
            }
        }
    }

    pub fn read_data(&mut self) -> u16 {
        log::trace!("VDP data read");

        if self.registers.data_port_mode != DataPortMode::Read {
            return 0xFFFF;
        }

        let data = match self.registers.data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                let address = (self.registers.data_address & !0x01) as usize;
                u16::from_be_bytes([self.vram[address], self.vram[(address + 1) & 0xFFFF]])
            }
            DataPortLocation::Cram => {
                let address = (self.registers.data_address & 0x7F) as usize;
                u16::from_be_bytes([self.cram[address], self.cram[(address + 1) & 0x7F]])
            }
            DataPortLocation::Vsram => {
                let address = (self.registers.data_address as usize) % VSRAM_LEN;
                u16::from_be_bytes([self.vsram[address], self.vsram[(address + 1) % VSRAM_LEN]])
            }
        };

        self.registers.increment_data_address();

        data
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!("VDP data write: {value:04X}");

        if self.registers.data_port_mode != DataPortMode::Write {
            return;
        }

        if self.registers.active_dma.is_some() {
            self.registers
                .pending_writes
                .push(PendingWrite::Data(value));
            return;
        }

        if self.registers.code.bit(5)
            && self.registers.dma_enabled
            && self.registers.dma_length > 0
            && self.registers.dma_mode == DmaMode::VramFill
        {
            log::trace!("Initiated VRAM fill DMA with fill data = {value:04X}");
            self.registers.active_dma = Some(ActiveDma::VramFill(value));
            return;
        }

        match self.registers.data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                let address = (self.registers.data_address & !0x01) as usize;
                log::trace!("Writing to {address:04X} in VRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.vram[address] = msb;
                self.vram[(address + 1) & 0xFFFF] = lsb;
            }
            DataPortLocation::Cram => {
                let address = (self.registers.data_address & 0x7F) as usize;
                log::trace!("Writing to {address:02X} in CRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.cram[address] = msb;
                self.cram[(address + 1) & 0x7F] = lsb;
            }
            DataPortLocation::Vsram => {
                let address = (self.registers.data_address as usize) % VSRAM_LEN;
                log::trace!("Writing to {address:02X} in VSRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.vsram[address] = msb;
                self.vsram[(address + 1) % VSRAM_LEN] = lsb;
            }
        }

        self.registers.increment_data_address();
    }

    pub fn read_status(&self) -> u16 {
        // TODO interlacing odd/even flag
        // Queue empty (bit 9) hardcoded to true
        // Queue full (bit 8) hardcoded to false
        // DMA busy (bit 1) hardcoded to false
        // PAL (bit 0) hardcoded to false
        0x0200
            | (u16::from(self.registers.v_interrupt_pending && self.registers.v_interrupt_enabled)
                << 7)
            | (u16::from(self.registers.sprite_overflow) << 6)
            | (u16::from(self.registers.sprite_collision) << 5)
            | (u16::from(self.in_vblank()) << 3)
            | (u16::from(self.in_hblank()) << 2)
    }

    #[must_use]
    pub fn tick(&mut self, master_clock_cycles: u64, memory: &mut Memory) -> VdpTickEffect {
        // The longest 68k instruction (DIVS) takes at most around 150 68k cycles
        assert!(master_clock_cycles < 1100);

        if let Some(active_dma) = self.registers.active_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, active_dma);
        }

        if !self.registers.pending_writes.is_empty() {
            let mut pending_writes = [PendingWrite::default(); 10];
            let pending_writes_len = self.registers.pending_writes.len();
            pending_writes[..pending_writes_len].copy_from_slice(&self.registers.pending_writes);
            self.registers.pending_writes.clear();

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

        let prev_mclk_cycles = self.master_clock_cycles;
        self.master_clock_cycles += master_clock_cycles;

        let prev_scanline_mclk = prev_mclk_cycles % MCLK_CYCLES_PER_SCANLINE;

        // Check if an H interrupt has triggered
        if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE
            && master_clock_cycles >= ACTIVE_MCLK_CYCLES_PER_SCANLINE - prev_scanline_mclk
        {
            if self.registers.scanline < 224 {
                if self.registers.h_interrupt_counter == 0 {
                    self.registers.h_interrupt_counter = self.registers.h_interrupt_interval;

                    if self.registers.h_interrupt_enabled {
                        self.registers.h_interrupt_pending = true;
                    }
                } else {
                    self.registers.h_interrupt_counter -= 1;
                }
            } else {
                // H interrupt counter is constantly refreshed during VBlank
                self.registers.h_interrupt_counter = self.registers.h_interrupt_interval;
            }
        }

        // Check if the VDP has advanced to a new scanline
        if prev_scanline_mclk + master_clock_cycles >= MCLK_CYCLES_PER_SCANLINE {
            self.registers.scanline += 1;
            if self.registers.scanline == SCANLINES_PER_FRAME {
                self.registers.scanline = 0;
            }

            match self.registers.scanline.cmp(&ACTIVE_SCANLINES) {
                Ordering::Less => {
                    self.render_scanline();
                }
                Ordering::Equal => {
                    if self.registers.v_interrupt_enabled {
                        self.registers.v_interrupt_pending = true;
                    }

                    return VdpTickEffect::FrameComplete;
                }
                Ordering::Greater => {}
            }
        }

        VdpTickEffect::None
    }

    // TODO maybe do this piecemeal instead of all at once
    fn run_dma(&mut self, memory: &mut Memory, active_dma: ActiveDma) {
        match active_dma {
            ActiveDma::MemoryToVram => {
                // TODO halt 68k during memory-to-VRAM transfers

                let mut source_addr = self.registers.dma_source_address;

                log::trace!(
                    "Copying {} words from {source_addr:06X} to {:04X}, write location={:?}; data_addr_increment={:04X}",
                    self.registers.dma_length,
                    self.registers.data_address, self.registers.data_port_location, self.registers.data_port_auto_increment
                );

                for _ in 0..self.registers.dma_length {
                    let word = memory.read_word_for_dma(source_addr);
                    match self.registers.data_port_location {
                        DataPortLocation::Vram => {
                            self.write_vram_word(self.registers.data_address, word);
                        }
                        DataPortLocation::Cram => {
                            let addr = self.registers.data_address as usize;
                            self.cram[addr & 0x7F] = (word >> 8) as u8;
                            self.cram[(addr + 1) & 0x7F] = word as u8;
                        }
                        DataPortLocation::Vsram => {
                            let addr = self.registers.data_address as usize;
                            self.vsram[addr % VSRAM_LEN] = (word >> 8) as u8;
                            self.vsram[(addr + 1) % VSRAM_LEN] = word as u8;
                        }
                    }

                    source_addr = source_addr.wrapping_add(2);
                    self.registers.increment_data_address();
                }

                self.registers.dma_source_address = source_addr;
            }
            ActiveDma::VramFill(fill_data) => {
                log::trace!(
                    "Running VRAM fill with addr {:04X} and length {}",
                    self.registers.data_address,
                    self.registers.dma_length
                );

                for _ in 0..self.registers.dma_length {
                    let dest_addr = self.registers.data_address & !0x01;
                    self.write_vram_word(dest_addr, fill_data);

                    self.registers.increment_data_address();
                }
            }
            ActiveDma::VramCopy => {
                todo!("VRAM copy DMA")
            }
        }

        self.registers.active_dma = None;
        self.registers.dma_length = 0;
    }

    fn write_vram_word(&mut self, address: u16, value: u16) {
        // A0 is ignored in VRAM writes
        let address = address & !0x01;
        self.vram[address as usize] = (value >> 8) as u8;
        self.vram[address.wrapping_add(1) as usize] = value as u8;
    }

    fn in_vblank(&self) -> bool {
        self.registers.scanline >= ACTIVE_SCANLINES
    }

    fn in_hblank(&self) -> bool {
        self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
    }

    pub fn interrupt_level(&self) -> u8 {
        // TODO external interrupts at level 2
        if self.registers.v_interrupt_pending && self.registers.v_interrupt_enabled {
            6
        } else if self.registers.h_interrupt_pending && self.registers.h_interrupt_enabled {
            4
        } else {
            0
        }
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.registers.v_interrupt_pending = false;
        self.registers.h_interrupt_pending = false;
    }

    fn render_scanline(&mut self) {
        if !self.registers.display_enabled {
            return;
        }

        self.populate_sprite_buffer();

        let scanline = self.registers.scanline;
        let screen_width = self.registers.horizontal_display_size.to_pixels();

        let bg_color = resolve_color(
            &self.cram,
            self.registers.background_palette,
            self.registers.background_color_id,
        );

        for pixel in 0..screen_width {
            self.render_pixel(bg_color, scanline, pixel);
        }
    }

    // TODO optimize this to do fewer passes for sorting/filtering
    fn populate_sprite_buffer(&mut self) {
        self.sprite_buffer.clear();

        // Populate buffer from the sprite attribute table
        let h_size = self.registers.horizontal_display_size;
        let sprite_table_addr = self.registers.sprite_attribute_table_base_addr;
        for i in 0..h_size.sprite_table_len() {
            let sprite_addr = sprite_table_addr.wrapping_add(8 * i) as usize;
            let sprite_bytes = &self.vram[sprite_addr..sprite_addr + 8];
            self.sprite_buffer
                .push(SpriteData::from_attribute_table(sprite_bytes));
        }

        // Fill in sprite priorities
        self.sprite_buffer[0].sprite_priority = 0;

        let mut sprite_priority = 1;
        let mut sprite_idx = self.sprite_buffer[0].link_data as usize;
        while sprite_idx != 0 {
            // TODO this should lock up the system
            assert_eq!(
                self.sprite_buffer[sprite_idx].sprite_priority, 0xFF,
                "Link data loop detected in sprite attribute table"
            );

            self.sprite_buffer[sprite_idx].sprite_priority = sprite_priority;
            sprite_priority += 1;
            sprite_idx = self.sprite_buffer[sprite_idx].link_data as usize;
        }

        // TODO sprite overflow
        self.sprite_buffer
            .sort_by_key(|sprite| sprite.sprite_priority);
        self.sprite_buffer
            .retain(|sprite| sprite.sprite_priority != 0xFF);

        // Remove sprites that don't fall on this scanline
        // Sprite display area starts at $080 vertically
        let sprite_scanline = 0x080 + self.registers.scanline;
        self.sprite_buffer.retain(|sprite| {
            let sprite_bottom = sprite.v_position + 8 * u16::from(sprite.v_size_cells);
            (sprite.v_position..sprite_bottom).contains(&sprite_scanline)
        });

        // Sprites with H position 0 mask all lower priority sprites on the same scanline
        for i in 0..self.sprite_buffer.len() {
            if self.sprite_buffer[i].h_position == 0 {
                self.sprite_buffer.truncate(i);
                break;
            }
        }

        // Apply max sprite per scanline limit
        self.sprite_buffer
            .truncate(h_size.max_sprites_per_line() as usize);

        // Apply max sprite pixel per scanline limit
        let mut pixels = 0;
        for i in 0..self.sprite_buffer.len() {
            pixels += 8 * u16::from(self.sprite_buffer[i].h_size_cells);
            if pixels >= h_size.max_sprite_pixels_per_line() {
                self.sprite_buffer.truncate(i + 1);
                break;
            }
        }
    }

    fn render_pixel(&mut self, bg_color: u16, scanline: u16, pixel: u16) {
        let h_cell = pixel / 8;

        let (v_scroll_a, v_scroll_b) =
            read_v_scroll(&self.vsram, self.registers.vertical_scroll_mode, h_cell);
        let (h_scroll_a, h_scroll_b) = read_h_scroll(
            &self.vram,
            self.registers.h_scroll_table_base_addr,
            self.registers.horizontal_scroll_mode,
            scanline,
        );

        let v_scroll_size = self.registers.vertical_scroll_size;

        let scrolled_scanline_a =
            scanline.wrapping_add(v_scroll_a) & v_scroll_size.pixel_bit_mask();
        let scroll_a_v_cell = scrolled_scanline_a / 8;

        let scrolled_scanline_b =
            scanline.wrapping_add(v_scroll_b) & v_scroll_size.pixel_bit_mask();
        let scroll_b_v_cell = scrolled_scanline_b / 8;

        let h_scroll_size = self.registers.horizontal_scroll_size;

        let scrolled_pixel_a = pixel.wrapping_sub(h_scroll_a) & h_scroll_size.pixel_bit_mask();
        let scroll_a_h_cell = scrolled_pixel_a / 8;

        let scrolled_pixel_b = pixel.wrapping_sub(h_scroll_b) & h_scroll_size.pixel_bit_mask();
        let scroll_b_h_cell = scrolled_pixel_b / 8;

        let scroll_a_nt_word = read_name_table_word(
            &self.vram,
            self.registers.scroll_a_base_nt_addr,
            self.registers.horizontal_scroll_size.into(),
            scroll_a_v_cell,
            scroll_a_h_cell,
        );
        let scroll_b_nt_word = read_name_table_word(
            &self.vram,
            self.registers.scroll_b_base_nt_addr,
            self.registers.horizontal_scroll_size.into(),
            scroll_b_v_cell,
            scroll_b_h_cell,
        );

        let scroll_a_color_id = read_pattern_generator(
            &self.vram,
            PatternGeneratorArgs {
                vertical_flip: scroll_a_nt_word.vertical_flip,
                horizontal_flip: scroll_a_nt_word.horizontal_flip,
                pattern_generator: scroll_a_nt_word.pattern_generator,
                row: scrolled_scanline_a,
                col: scrolled_pixel_a,
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
            },
        );

        let (window_priority, window_palette, window_color_id) =
            if self.registers.is_in_window(scanline, pixel) {
                let v_cell = scanline / 8;
                let window_nt_word = read_name_table_word(
                    &self.vram,
                    self.registers.window_base_nt_addr,
                    self.registers.horizontal_display_size.window_cell_width(),
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
                    },
                );
                (
                    window_nt_word.priority,
                    window_nt_word.palette,
                    window_color_id,
                )
            } else {
                (false, 0, 0)
            };

        let (sprite_priority, sprite_palette, sprite_color_id) = self
            .find_first_overlapping_sprite(scanline, pixel)
            .map_or((false, 0, 0), |(sprite, color_id)| {
                (sprite.priority, sprite.palette, color_id)
            });

        let scroll_a_color = resolve_color(&self.cram, scroll_a_nt_word.palette, scroll_a_color_id);
        let scroll_b_color = resolve_color(&self.cram, scroll_b_nt_word.palette, scroll_b_color_id);
        let window_color = resolve_color(&self.cram, window_palette, window_color_id);
        let sprite_color = resolve_color(&self.cram, sprite_palette, sprite_color_id);

        let color = if sprite_priority && sprite_color_id != 0 {
            sprite_color
        } else if window_priority && window_color_id != 0 {
            window_color
        } else if scroll_a_nt_word.priority && scroll_a_color_id != 0 {
            scroll_a_color
        } else if scroll_b_nt_word.priority && scroll_b_color_id != 0 {
            scroll_b_color
        } else if sprite_color_id != 0 {
            sprite_color
        } else if window_color_id != 0 {
            window_color
        } else if scroll_a_color_id != 0 {
            scroll_a_color
        } else if scroll_b_color_id != 0 {
            scroll_b_color
        } else {
            bg_color
        };
        self.set_in_frame_buffer(scanline.into(), pixel.into(), color);
    }

    fn find_first_overlapping_sprite(
        &self,
        scanline: u16,
        pixel: u16,
    ) -> Option<(&SpriteData, u8)> {
        // Sprite horizontal display area starts at $080
        let sprite_pixel = 0x080 + pixel;

        // TODO sprite collision
        self.sprite_buffer.iter().find_map(|sprite| {
            let sprite_right = sprite.h_position + 8 * u16::from(sprite.h_size_cells);
            if !(sprite.h_position..sprite_right).contains(&sprite_pixel) {
                return None;
            }

            let v_size_cells: u16 = sprite.v_size_cells.into();
            let h_size_cells: u16 = sprite.h_size_cells.into();

            let sprite_row = 0x080 + scanline - sprite.v_position;
            let sprite_row = if sprite.vertical_flip {
                8 * v_size_cells - 1 - sprite_row
            } else {
                sprite_row
            };

            let sprite_col = 0x080 + pixel - sprite.h_position;
            let sprite_col = if sprite.horizontal_flip {
                8 * h_size_cells - 1 - sprite_col
            } else {
                sprite_col
            };

            let pattern_offset = (sprite_col / 8) * v_size_cells + sprite_row / 8;
            let color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: false,
                    horizontal_flip: false,
                    pattern_generator: sprite.pattern_generator.wrapping_add(pattern_offset),
                    row: sprite_row % 8,
                    col: sprite_col % 8,
                },
            );
            (color_id != 0).then_some((sprite, color_id))
        })
    }

    pub fn frame_buffer(&self) -> &[u16] {
        &self.frame_buffer
    }

    pub fn screen_width(&self) -> u32 {
        self.registers.horizontal_display_size.to_pixels().into()
    }

    fn set_in_frame_buffer(&mut self, row: u32, col: u32, value: u16) {
        let screen_width = self.screen_width();
        self.frame_buffer[(row * screen_width + col) as usize] = value;
    }

    pub fn render_pattern_debug(&self, buffer: &mut [u32], palette: u8) {
        for i in 0..2048 {
            for row in 0..8 {
                for col in 0..8 {
                    let color_id = read_pattern_generator(
                        &self.vram,
                        PatternGeneratorArgs {
                            vertical_flip: false,
                            horizontal_flip: false,
                            pattern_generator: i,
                            row,
                            col,
                        },
                    );
                    let color = if color_id != 0 {
                        resolve_color(&self.cram, palette, color_id)
                    } else {
                        0
                    };

                    let pattern_row_idx = u32::from(i / 64);
                    let pattern_col_idx = u32::from(i % 64);
                    let idx = (8 * pattern_row_idx + u32::from(row)) * 64 * 8
                        + pattern_col_idx * 8
                        + u32::from(col);

                    let r = gen_color_to_rgb((color >> 1) & 0x07);
                    let g = gen_color_to_rgb((color >> 5) & 0x07);
                    let b = gen_color_to_rgb((color >> 9) & 0x07);
                    buffer[idx as usize] = (r << 16) | (g << 8) | b;
                }
            }
        }
    }

    pub fn render_color_debug(&self, buffer: &mut [u32]) {
        for i in 0..64 {
            let color = resolve_color(&self.cram, i / 16, i % 16);
            let r = gen_color_to_rgb((color >> 1) & 0x07);
            let g = gen_color_to_rgb((color >> 5) & 0x07);
            let b = gen_color_to_rgb((color >> 9) & 0x07);
            buffer[i as usize] = (r << 16) | (g << 8) | b;
        }
    }
}

fn resolve_color(cram: &[u8], palette: u8, color_id: u8) -> u16 {
    let addr = (32 * palette + 2 * color_id) as usize;
    u16::from_be_bytes([cram[addr], cram[addr + 1]])
}

fn read_v_scroll(vsram: &[u8], v_scroll_mode: VerticalScrollMode, h_cell: u16) -> (u16, u16) {
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

    (v_scroll_a & 0x03FF, v_scroll_b & 0x03FF)
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

#[derive(Debug, Clone, Copy)]
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
}

fn read_pattern_generator(
    vram: &[u8],
    PatternGeneratorArgs {
        vertical_flip,
        horizontal_flip,
        pattern_generator,
        row,
        col,
    }: PatternGeneratorArgs,
) -> u8 {
    let cell_row = if vertical_flip {
        7 - (row % 8)
    } else {
        row % 8
    };
    let cell_col = if horizontal_flip {
        7 - (col % 8)
    } else {
        col % 8
    };

    // TODO patterns are 64 bytes in interlaced 2x mode
    let addr = (32 * pattern_generator + 4 * cell_row + (cell_col >> 1)) as usize;
    if cell_col.bit(0) {
        vram[addr] & 0x0F
    } else {
        vram[addr] >> 4
    }
}

pub fn gen_color_to_rgb(gen_color: u16) -> u32 {
    [0, 36, 73, 109, 146, 182, 219, 255][gen_color as usize]
}
