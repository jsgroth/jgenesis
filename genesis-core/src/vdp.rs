use crate::memory::Memory;
use m68000_emu::M68000;
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
    fn to_pixels(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 256,
            Self::FortyCell => 320,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowVerticalMode {
    TopToCenter,
    CenterToBottom,
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
    sprite_overflow: bool,
    sprite_collision: bool,
    scanline: u16,
    dot: u16,
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
            sprite_overflow: false,
            sprite_collision: false,
            scanline: 0,
            dot: 0,
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
    master_clock_cycles: u64,
}

const DEFAULT_SCREEN_WIDTH: usize = 256;
const MAX_SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 224;
const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * SCREEN_HEIGHT;

const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
const SCANLINES_PER_FRAME: u16 = 262;
const ACTIVE_SCANLINES: u16 = 224;
const MCLK_CYCLES_PER_FRAME: u64 = SCANLINES_PER_FRAME as u64 * MCLK_CYCLES_PER_SCANLINE;

impl Vdp {
    pub fn new() -> Self {
        Self {
            frame_buffer: vec![0; FRAME_BUFFER_LEN],
            vram: vec![0; VRAM_LEN],
            cram: [0; CRAM_LEN],
            vsram: [0; VSRAM_LEN],
            registers: Registers::new(),
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
    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        memory: &mut Memory,
        m68k: &mut M68000,
    ) -> VdpTickEffect {
        // The longest 68k instruction (DIVS) takes at most around 150 68k cycles
        assert!(master_clock_cycles < 1100);

        if let Some(active_dma) = self.registers.active_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, m68k, active_dma);
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

        if prev_mclk_cycles / MCLK_CYCLES_PER_SCANLINE
            != self.master_clock_cycles / MCLK_CYCLES_PER_SCANLINE
        {
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
    fn run_dma(&mut self, memory: &mut Memory, m68k: &mut M68000, active_dma: ActiveDma) {
        match active_dma {
            ActiveDma::MemoryToVram => {
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
                            self.write_vram_word(self.registers.data_address & !0x01, word);
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

    fn write_byte(&mut self, address: u16, value: u8) {
        match self.registers.data_port_location {
            DataPortLocation::Vram => {
                self.vram[address as usize] = value;
            }
            DataPortLocation::Cram => {
                self.cram[(address & 0x7F) as usize] = value;
            }
            DataPortLocation::Vsram => {
                self.vsram[(address as usize) % VSRAM_LEN] = value;
            }
        }
    }

    fn write_vram_word(&mut self, address: u16, value: u16) {
        // A0 is ignored in VRAM writes
        let address = address & !0x01;
        self.vram[address as usize] = (value >> 8) as u8;
        self.vram[address.wrapping_add(1) as usize] = value as u8;
    }

    fn in_vblank(&self) -> bool {
        self.registers.scanline < ACTIVE_SCANLINES
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

        let scanline = self.registers.scanline;
        let screen_width = self.registers.horizontal_display_size.to_pixels();

        let bg_color = resolve_color(
            &self.cram,
            self.registers.background_palette,
            self.registers.background_color_id,
        );

        let nt_row = scanline / 8;
        for col in 0..screen_width {
            let nt_col = col / 8;
            let scroll_a_nt_word = read_name_table_word(
                &self.vram,
                self.registers.scroll_a_base_nt_addr,
                self.registers.horizontal_scroll_size.into(),
                nt_row,
                nt_col,
            );

            let cell_row = scanline % 8;
            let cell_col = col % 8;
            let color_id = read_pattern_generator(
                &self.vram,
                scroll_a_nt_word.pattern_generator,
                cell_row,
                cell_col,
            );

            let scroll_a_color = resolve_color(&self.cram, scroll_a_nt_word.palette, color_id);

            let color = if color_id != 0 {
                scroll_a_color
            } else {
                bg_color
            };
            self.set_in_frame_buffer(scanline.into(), col.into(), color);
        }
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
                    let color_id = read_pattern_generator(&self.vram, i, row, col);
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

#[derive(Debug, Clone)]
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

fn read_pattern_generator(vram: &[u8], pattern_generator: u16, cell_row: u16, cell_col: u16) -> u8 {
    // TODO patterns are 64 bytes in interlaced 2x mode
    let addr = (32 * pattern_generator + 4 * cell_row + (cell_col >> 1)) as usize;
    if cell_col.bit(0) {
        vram[addr] & 0x0F
    } else {
        vram[addr] >> 4
    }
}

fn gen_color_to_rgb(gen_color: u16) -> u32 {
    [0, 36, 73, 109, 146, 182, 219, 255][gen_color as usize]
}
