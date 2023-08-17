use crate::memory::Memory;
use m68000_emu::M68000;
use smsgg_core::num::GetBit;

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

#[derive(Debug, Clone)]
struct Registers {
    // Internal state
    control_write_flag: ControlWriteFlag,
    first_word_code_bits: u8,
    data_port_mode: DataPortMode,
    data_port_location: DataPortLocation,
    data_address: u16,
    v_interrupt_pending: bool,
    h_interrupt_pending: bool,
    sprite_overflow: bool,
    sprite_collision: bool,
    scanline: u16,
    dot: u16,
    pending_command_writes: Vec<u16>,
    // Register #0
    h_interrupt_enabled: bool,
    hv_counter_stopped: bool,
    // Register #1
    display_enabled: bool,
    v_interupt_enabled: bool,
    dma_enabled: bool,
    // TODO PAL V30-cell mode
    // Register #2
    scroll_a_base_name_table_address: u16,
    // Register #3
    window_base_name_table_address: u16,
    // Register #4
    scroll_b_base_name_table_address: u16,
    // Register #5
    sprite_attribute_table_base_address: u16,
    // Register #7
    background_palette: u8,
    background_color: u8,
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
    h_scroll_table_base_address: u16,
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
            data_port_mode: DataPortMode::Write,
            data_port_location: DataPortLocation::Vram,
            data_address: 0,
            v_interrupt_pending: false,
            h_interrupt_pending: false,
            sprite_overflow: false,
            sprite_collision: false,
            scanline: 0,
            dot: 0,
            pending_command_writes: Vec::with_capacity(10),
            h_interrupt_enabled: false,
            hv_counter_stopped: false,
            display_enabled: false,
            v_interupt_enabled: false,
            dma_enabled: false,
            scroll_a_base_name_table_address: 0,
            window_base_name_table_address: 0,
            scroll_b_base_name_table_address: 0,
            sprite_attribute_table_base_address: 0,
            background_palette: 0,
            background_color: 0,
            h_interrupt_interval: 0,
            vertical_scroll_mode: VerticalScrollMode::FullScreen,
            horizontal_scroll_mode: HorizontalScrollMode::FullScreen,
            horizontal_display_size: HorizontalDisplaySize::ThirtyTwoCell,
            interlacing_mode: InterlacingMode::Progressive,
            h_scroll_table_base_address: 0,
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
                self.v_interupt_enabled = value.bit(5);
                self.dma_enabled = value.bit(4);
                // TODO PAL V30-cell mode
            }
            2 => {
                // Register #2: Scroll A name table base address (bits 15-13)
                self.scroll_a_base_name_table_address = u16::from(value & 0x38) << 10;
            }
            3 => {
                // Register #3: Window name table base address (bits 15-11)
                self.window_base_name_table_address = u16::from(value & 0x3E) << 10;
            }
            4 => {
                // Register #4: Scroll B name table base address (bits 15-13)
                self.scroll_b_base_name_table_address = u16::from(value & 0x07) << 13;
            }
            5 => {
                // Register #5: Sprite attribute table base address (bits 15-9)
                self.sprite_attribute_table_base_address = u16::from(value & 0x7F) << 9;
            }
            7 => {
                // Register #7: Background color
                self.background_palette = (value >> 4) & 0x03;
                self.background_color = value & 0x0F;
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
                self.interlacing_mode = match value & 0x06 {
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
                self.h_scroll_table_base_address = u16::from(value & 0x3F) << 10;
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

#[derive(Debug, Clone)]
pub struct Vdp {
    vram: Vec<u8>,
    cram: [u8; CRAM_LEN],
    vsram: [u8; VSRAM_LEN],
    registers: Registers,
}

impl Vdp {
    pub fn new() -> Self {
        Self {
            vram: vec![0; VRAM_LEN],
            cram: [0; CRAM_LEN],
            vsram: [0; VSRAM_LEN],
            registers: Registers::new(),
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.registers.control_write_flag,
            self.registers.dma_enabled
        );

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
                self.registers.data_address |= value << 14;
                self.registers.control_write_flag = ControlWriteFlag::First;

                if self.registers.dma_enabled && self.registers.dma_length > 0 {
                    // This is a DMA initiation, not a normal control write
                    let dma_code =
                        (((value >> 4) & 0x01) as u8) | self.registers.first_word_code_bits;
                    let dma_dest = match dma_code {
                        0x01 => DataPortLocation::Vram,
                        0x03 => DataPortLocation::Cram,
                        0x05 => DataPortLocation::Vsram,
                        _ => {
                            log::warn!("Invalid DMA code: {dma_code:02X}");
                            DataPortLocation::Vram
                        }
                    };
                    todo!("DMA")
                }

                let code = (((value >> 2) & 0x3C) as u8) | self.registers.first_word_code_bits;
                let (data_port_location, data_port_mode) = match code {
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

                self.registers.data_port_location = data_port_location;
                self.registers.data_port_mode = data_port_mode;

                log::trace!("Set data port location to {data_port_location:?} and mode to {data_port_mode:?}");
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
                let address = (self.registers.data_address & 0xFFFE) as usize;
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

        match self.registers.data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                let address = (self.registers.data_address & 0xFFFE) as usize;
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
        // TODO interlacing odd/even flag, VBlank flag, HBlank flag, DMA busy flag
        0x0200
            | (u16::from(self.registers.v_interrupt_pending) << 7)
            | (u16::from(self.registers.sprite_overflow) << 6)
            | (u16::from(self.registers.sprite_collision) << 5)
    }

    pub fn tick(&mut self, memory: &mut Memory, m68k: &mut M68000) {
        todo!("tick VDP")
    }
}
