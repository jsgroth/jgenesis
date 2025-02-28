//! SNES internal memory and on-chip CPU internal registers/ports

pub(crate) mod cartridge;
pub(crate) mod dma;
mod inputs;

use crate::api::{CoprocessorRoms, SnesLoadResult};
use crate::input::SnesInputs;
use crate::memory::cartridge::Cartridge;
use crate::memory::inputs::InputState;
use crate::ppu::Ppu;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{SaveWriter, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext, U24Ext};
use jgenesis_proc_macros::PartialClone;
use std::num::NonZeroU64;
use std::{array, iter};

const MAIN_RAM_LEN: usize = 128 * 1024;

// H=32.5
const AUTO_JOYPAD_START_MCLK: u64 = 130;

// Scanline MCLK at which to generate V IRQ
const V_IRQ_H_MCLK: u64 = 10;

type MainRam = [u8; MAIN_RAM_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum Memory2Speed {
    Fast,
    #[default]
    Slow,
}

impl Memory2Speed {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(0) { Self::Fast } else { Self::Slow }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Memory {
    #[partial_clone(partial)]
    cartridge: Cartridge,
    main_ram: Box<MainRam>,
    wram_port_address: u32,
    cpu_open_bus: u8,
}

impl Memory {
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        coprocessor_roms: &CoprocessorRoms,
        forced_timing_mode: Option<TimingMode>,
        gsu_overclock_factor: NonZeroU64,
        save_writer: &mut S,
    ) -> SnesLoadResult<Self> {
        let cartridge = Cartridge::create(
            rom,
            initial_sram,
            coprocessor_roms,
            forced_timing_mode,
            gsu_overclock_factor,
            save_writer,
        )?;

        log::info!("Cartridge has battery-backed SRAM: {}", cartridge.has_battery());

        let main_ram = Vec::from_iter(iter::repeat_with(rand::random).take(MAIN_RAM_LEN));

        Ok(Self {
            cartridge,
            main_ram: main_ram.into_boxed_slice().try_into().unwrap(),
            wram_port_address: 0,
            cpu_open_bus: 0,
        })
    }

    pub fn read_cartridge(&mut self, address: u32) -> Option<u8> {
        match self.cartridge.read(address) {
            Some(value) => {
                self.cpu_open_bus = value;
                Some(value)
            }
            None => None,
        }
    }

    pub fn write_cartridge(&mut self, address: u32, value: u8) {
        self.cartridge.write(address, value);
    }

    pub fn cartridge_irq(&self) -> bool {
        self.cartridge.irq()
    }

    pub fn cartridge_title(&mut self) -> String {
        // Cartridge title is always at $00FFC0-$00FFD4 (inclusive)
        let mut title_bytes = [0; 0xFFD4 - 0xFFC0 + 1];
        for (i, byte) in title_bytes.iter_mut().enumerate() {
            *byte = self.read_cartridge(0xFFC0 + i as u32).unwrap_or(0);
        }

        title_bytes
            .into_iter()
            .filter_map(|byte| {
                (byte.is_ascii_whitespace()
                    || byte.is_ascii_alphanumeric()
                    || byte.is_ascii_punctuation())
                .then_some(byte as char)
            })
            .collect()
    }

    pub fn cartridge_timing_mode(&mut self) -> TimingMode {
        // Region byte is always at $00FFD9
        let region_byte = self.read_cartridge(0xFFD9).unwrap_or(0);
        cartridge::region_to_timing_mode(region_byte)
    }

    pub fn read_wram(&self, address: u32) -> u8 {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)]
    }

    pub fn write_wram(&mut self, address: u32, value: u8) {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)] = value;
    }

    pub fn read_wram_port(&mut self) -> u8 {
        let value = self.main_ram[self.wram_port_address as usize];
        self.increment_wram_port_address();
        value
    }

    pub fn write_wram_port(&mut self, value: u8) {
        self.main_ram[self.wram_port_address as usize] = value;
        self.increment_wram_port_address();
    }

    fn increment_wram_port_address(&mut self) {
        self.wram_port_address = (self.wram_port_address + 1) & ((MAIN_RAM_LEN - 1) as u32);
    }

    pub fn write_wram_port_address_low(&mut self, value: u8) {
        self.wram_port_address.set_low_byte(value);
        log::trace!("WRAM port address: {:06X}", self.wram_port_address);
    }

    pub fn write_wram_port_address_mid(&mut self, value: u8) {
        self.wram_port_address.set_mid_byte(value);
        log::trace!("WRAM port address: {:06X}", self.wram_port_address);
    }

    pub fn write_wram_port_address_high(&mut self, value: u8) {
        // Only 1 bit used from high byte
        self.wram_port_address.set_high_byte(value & 0x01);
        log::trace!("WRAM port address: {:06X}", self.wram_port_address);
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        self.cartridge.take_rom()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.take_rom_from(&mut other.cartridge);
    }

    pub fn sram(&self) -> Option<&[u8]> {
        self.cartridge.sram()
    }

    pub fn write_auxiliary_save_files<S: SaveWriter>(
        &self,
        save_writer: &mut S,
    ) -> Result<(), S::Err> {
        self.cartridge.write_auxiliary_save_files(save_writer)
    }

    pub fn has_battery_backed_sram(&self) -> bool {
        self.cartridge.has_battery()
    }

    pub fn cpu_open_bus(&self) -> u8 {
        self.cpu_open_bus
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        self.cartridge.tick(master_cycles_elapsed);
    }

    pub fn reset(&mut self) {
        self.wram_port_address = 0;
        self.cartridge.reset();
    }

    // Called when GPDMA begins, or when it starts on a new channel
    pub fn notify_dma_start(&mut self, channel: u8, source_address: u32) {
        self.cartridge.notify_dma_start(channel, source_address);
    }

    // Called when GPDMA completes (all channels done)
    pub fn notify_dma_end(&mut self) {
        self.cartridge.notify_dma_end();
    }

    pub fn update_gsu_overclock_factor(&mut self, overclock_factor: NonZeroU64) {
        self.cartridge.update_gsu_overclock_factor(overclock_factor);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum IrqMode {
    // No IRQs
    #[default]
    Off,
    // IRQ at H=HTIME, every line
    H,
    // IRQ at V=VTIME + H=0
    V,
    // IRQ at V=VTIME + H=HTIME
    HV,
}

impl IrqMode {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x30 {
            0x00 => Self::Off,
            0x10 => Self::H,
            0x20 => Self::V,
            0x30 => Self::HV,
            _ => unreachable!("value & 0x30 will always be one of the above values"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum DmaDirection {
    AtoB,
    #[default]
    BtoA,
}

impl DmaDirection {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(7) { Self::BtoA } else { Self::AtoB }
    }

    fn to_byte(self) -> u8 {
        u8::from(self == Self::BtoA) << 7
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum HdmaAddressingMode {
    Direct,
    #[default]
    Indirect,
}

impl HdmaAddressingMode {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(6) { Self::Indirect } else { Self::Direct }
    }

    fn to_byte(self) -> u8 {
        u8::from(self == Self::Indirect) << 6
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum DmaIncrementMode {
    #[default]
    Fixed0,
    Fixed1,
    Increment,
    Decrement,
}

impl DmaIncrementMode {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x18 {
            0x00 => Self::Increment,
            0x10 => Self::Decrement,
            0x08 => Self::Fixed0,
            0x18 => Self::Fixed1,
            _ => unreachable!("value & 0x18 is always one of the above values"),
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::Increment => 0x00,
            Self::Decrement => 0x10,
            Self::Fixed0 => 0x08,
            Self::Fixed1 => 0x18,
        }
    }
}

// Registers/ports that are on the 5A22 chip but are not part of the 65816
#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuInternalRegisters {
    nmi_enabled: bool,
    nmi_pending: bool,
    irq_mode: IrqMode,
    irq_pending: bool,
    auto_joypad_read_enabled: bool,
    irq_htime: u16,
    irq_vtime: u16,
    multiply_operand_l: u8,
    multiply_operand_r: u8,
    multiply_product: u16,
    division_dividend: u16,
    division_divisor: u8,
    division_quotient: u16,
    memory_2_speed: Memory2Speed,
    active_gpdma_channels: [bool; 8],
    active_hdma_channels: [bool; 8],
    dma_direction: [DmaDirection; 8],
    hdma_addressing_mode: [HdmaAddressingMode; 8],
    dma_increment_mode: [DmaIncrementMode; 8],
    dma_transfer_unit: [u8; 8],
    dmap_unused_bit: [bool; 8],
    dma_bus_b_address: [u8; 8],
    // GPDMA current address is also used as HDMA table start address
    gpdma_current_address: [u16; 8],
    dma_bank: [u8; 8],
    // GPDMA byte counter is also used as HDMA indirect address
    gpdma_byte_counter: [u16; 8],
    hdma_indirect_bank: [u8; 8],
    hdma_table_current_address: [u16; 8],
    hdma_line_counter: [u8; 8],
    unused_dma_register: [u8; 8],
    vblank_flag: bool,
    vblank_nmi_flag: bool,
    hblank_flag: bool,
    programmable_joypad_port: u8,
    input_state: InputState,
}

impl CpuInternalRegisters {
    pub fn new() -> Self {
        Self {
            nmi_enabled: false,
            nmi_pending: false,
            irq_mode: IrqMode::default(),
            irq_pending: false,
            auto_joypad_read_enabled: false,
            irq_htime: 0,
            irq_vtime: 0,
            multiply_operand_l: 0xFF,
            multiply_operand_r: 0xFF,
            multiply_product: 0,
            division_dividend: 0xFFFF,
            division_divisor: 0xFF,
            division_quotient: 0,
            memory_2_speed: Memory2Speed::default(),
            active_gpdma_channels: [false; 8],
            active_hdma_channels: [false; 8],
            dma_direction: [DmaDirection::default(); 8],
            hdma_addressing_mode: [HdmaAddressingMode::default(); 8],
            dma_increment_mode: [DmaIncrementMode::default(); 8],
            dma_transfer_unit: [0x07; 8],
            dmap_unused_bit: [true; 8],
            dma_bus_b_address: [0xFF; 8],
            gpdma_current_address: [0xFFFF; 8],
            dma_bank: [0xFF; 8],
            gpdma_byte_counter: [0xFFFF; 8],
            hdma_indirect_bank: [0xFF; 8],
            hdma_table_current_address: [0xFFFF; 8],
            hdma_line_counter: [0xFF; 8],
            unused_dma_register: [0xFF; 8],
            vblank_flag: false,
            vblank_nmi_flag: false,
            hblank_flag: false,
            programmable_joypad_port: 0xFF,
            input_state: InputState::new(),
        }
    }

    pub fn read_register(&mut self, address: u32, cpu_open_bus: u8) -> Option<u8> {
        log::trace!("Read CPU register: {address:06X}");

        let value = match address {
            0x4016 => {
                // JOYA: Manual joypad register A
                // Bits 7-2 are open bus
                u8::from(self.input_state.next_manual_p1_bit()) | (cpu_open_bus & 0xFC)
            }
            0x4017 => {
                // JOYB: Manual joypad register B
                // Bits 2-4 always set
                // Bits 7-5 are open bus
                0x1C | u8::from(self.input_state.next_manual_p2_bit()) | (cpu_open_bus & 0xE0)
            }
            0x4210 => {
                // RDNMI: VBlank NMI flag and CPU version number

                // Reading this register clears the VBlank NMI flag
                let vblank_nmi_flag = self.vblank_nmi_flag;
                self.vblank_nmi_flag = false;

                // Hardcode version number to 2
                // Bits 6-4 are open bus
                (u8::from(vblank_nmi_flag) << 7) | 0x02 | (cpu_open_bus & 0x70)
            }
            0x4211 => {
                // TIMEUP: H/V IRQ flag

                // Reading this register clears the IRQ flag
                let irq_pending = self.irq_pending;
                self.irq_pending = false;

                // Bits 6-0 are open bus
                (u8::from(irq_pending) << 7) | (cpu_open_bus & 0x7F)
            }
            0x4212 => {
                // HVBJOY: H/V blank flags and auto joypad in-progress flag
                // Bits 5-1 are open bus
                (u8::from(self.vblank_flag) << 7)
                    | (u8::from(self.hblank_flag) << 6)
                    | (cpu_open_bus & 0x3E)
                    | u8::from(self.input_state.auto_joypad_read_in_progress())
            }
            0x4213 => {
                // RDIO: Programmable joypad I/O port (read)
                self.programmable_joypad_port
            }
            0x4214 => {
                // RDDIVL: Division quotient, low byte
                self.division_quotient.lsb()
            }
            0x4215 => {
                // RDDIVH: Division quotient, high byte
                self.division_quotient.msb()
            }
            0x4216 => {
                // RDMPYL: Multiply product / division remainder, low byte
                self.multiply_product.lsb()
            }
            0x4217 => {
                // RDMPYH: Multiply product / division remainder, high byte
                self.multiply_product.msb()
            }
            0x4218 => {
                // JOY1L: Joypad 1, low byte (auto read)
                self.input_state.auto_joypad_p1_inputs().lsb()
            }
            0x4219 => {
                // JOY1H: Joypad 1, high byte (auto read)
                self.input_state.auto_joypad_p1_inputs().msb()
            }
            0x421A => {
                // JOY2L: Joypad 2, low byte (auto read)
                self.input_state.auto_joypad_p2_inputs().lsb()
            }
            0x421B => {
                // JOY2H: Joypad 2, high byte (auto read)
                self.input_state.auto_joypad_p2_inputs().msb()
            }
            0x421C..=0x421F => {
                // JOY3L/JOY3H/JOY4L/JOY4H: Joypad 3/4 (not implemented)
                0x00
            }
            0x4300..=0x437F => {
                // DMA registers
                return self.read_dma_register(address);
            }
            _ => {
                // Open bus
                return None;
            }
        };

        Some(value)
    }

    pub fn write_register(&mut self, address: u32, value: u8) {
        log::trace!("CPU internal register write: {address:06X} {value:02X}");

        match address & 0xFFFF {
            0x4016 => {
                // JOYWR: Joypad output
                self.input_state.set_strobe(value.bit(0));
            }
            0x4200 => {
                // NMITIMEN: Interrupt enable and joypad request
                self.auto_joypad_read_enabled = value.bit(0);
                self.irq_mode = IrqMode::from_byte(value);
                let nmi_enabled = value.bit(7);
                if !self.nmi_enabled && nmi_enabled && self.vblank_nmi_flag {
                    // Enabling NMIs while the VBlank NMI flag is set immediately triggers an NMI
                    self.nmi_pending = true;
                }
                self.nmi_enabled = nmi_enabled;

                // Disabling IRQs acknowledges any pending IRQ
                if self.irq_mode == IrqMode::Off {
                    self.irq_pending = false;
                }

                log::trace!("  Auto joypad read enabled: {}", self.auto_joypad_read_enabled);
                log::trace!("  IRQ mode: {:?}", self.irq_mode);
                log::trace!("  NMI enabled: {nmi_enabled}");
            }
            0x4201 => {
                // WRIO: Joypad programmable I/O port (write)
                self.programmable_joypad_port = value;

                log::trace!("  Programmable joypad I/O port write: {value:02X}");
            }
            0x4202 => {
                // WRMPYA: Multiplication 8-bit operand A
                self.multiply_operand_l = value;

                log::trace!("  Unsigned multiply operand A: {value:02X}");
            }
            0x4203 => {
                // WRMPYB: Multiplication 8-bit operand B + start multiplication
                self.multiply_operand_r = value;

                // TODO delay setting the result? takes 8 CPU cycles on real hardware
                self.multiply_product = u16::from(self.multiply_operand_l) * u16::from(value);

                // Multiplication always writes operand B to the division quotient register
                self.division_quotient = value.into();

                log::trace!("  Unsigned multiply operand B: {value:02X}");
                log::trace!("  Unsigned multiply product: {:04X}", self.multiply_product);
            }
            0x4204 => {
                // WRDIVL: Division 16-bit dividend, low byte
                self.division_dividend.set_lsb(value);

                log::trace!("  Unsigned divide dividend: {:04X}", self.division_dividend);
            }
            0x4205 => {
                // WRDIVH: Division 16-bit dividend, high byte
                self.division_dividend.set_msb(value);

                log::trace!("  Unsigned divide dividend: {:04X}", self.division_dividend);
            }
            0x4206 => {
                // WRDIVB: Division 8-bit divisor + start division
                self.division_divisor = value;

                // TODO delay setting the result? takes 16 CPU cycles on real hardware
                if value != 0 {
                    self.division_quotient = self.division_dividend / u16::from(value);

                    // Division writes remainder to the multiply product register
                    self.multiply_product = self.division_dividend % u16::from(value);
                } else {
                    // Divide by 0 always sets quotient to $FFFF and remainder to dividend
                    self.division_quotient = 0xFFFF;
                    self.multiply_product = self.division_dividend;
                }

                log::trace!("  Unsigned divide divisor: {value:02X}");
                log::trace!("  Unsigned divide quotient: {:04X}", self.division_quotient);
                log::trace!("  Unsigned divide remainder: {:04X}", self.multiply_product);
            }
            0x4207 => {
                // HTIMEL: H-count timer setting, low byte
                self.irq_htime.set_lsb(value);

                log::trace!("  HTIME: {:04X}", self.irq_htime);
            }
            0x4208 => {
                // HTIMEH: H-count timer setting, high byte (really just highest bit)
                self.irq_htime.set_msb(value & 0x01);

                log::trace!("  HTIME: {:04X}", self.irq_htime);
            }
            0x4209 => {
                // VTIMEL: V-count timer setting, low byte
                self.irq_vtime.set_lsb(value);

                log::trace!("  VTIME: {:04X}", self.irq_vtime);
            }
            0x420A => {
                // VTIMEH: V-count timer setting, high byte (really just highest bit)
                self.irq_vtime.set_msb(value & 0x01);

                log::trace!("  VTIME: {:04X}", self.irq_vtime);
            }
            0x420B => {
                // MDMAEN: Select general purpose DMA channels + start transfer (if non-zero)
                self.active_gpdma_channels = array::from_fn(|i| value.bit(i as u8));

                log::trace!("  GPDMA active channels: {value:02X}");
            }
            0x420C => {
                // HDMAEN: Select HBlank DMA channels
                self.active_hdma_channels = array::from_fn(|i| value.bit(i as u8));

                log::trace!("  HDMA active channels: {value:02X}");
            }
            0x420D => {
                // MEMSEL: Memory-2 waitstate control
                self.memory_2_speed = Memory2Speed::from_byte(value);

                log::trace!("  Memory-2 speed: {:?}", self.memory_2_speed);
            }
            address @ 0x4300..=0x437F => {
                // DMA registers
                self.write_dma_register(address, value);
            }
            _ => {
                // Open bus; do nothing
            }
        }
    }

    fn read_dma_register(&self, address: u32) -> Option<u8> {
        // Second-least significant nibble is channel
        let channel = ((address >> 4) & 0x7) as usize;

        let value = match address & 0xFF0F {
            0x4300 => {
                // DMAPx: DMA parameters 0-7
                self.dma_transfer_unit[channel]
                    | self.dma_increment_mode[channel].to_byte()
                    | (u8::from(self.dmap_unused_bit[channel]) << 5)
                    | self.hdma_addressing_mode[channel].to_byte()
                    | self.dma_direction[channel].to_byte()
            }
            0x4301 => {
                // BBADx: DMA bus B address
                self.dma_bus_b_address[channel]
            }
            0x4302 => {
                // A1TxL: GPDMA current address / HDMA table start address, low byte
                self.gpdma_current_address[channel].lsb()
            }
            0x4303 => {
                // A1TxH: GPDMA current address / HDMA table start address, high byte
                self.gpdma_current_address[channel].msb()
            }
            0x4304 => {
                // A1Bx: GPDMA current address / HDMA table start address, bank
                self.dma_bank[channel]
            }
            0x4305 => {
                // DASxL: GPDMA byte counter / HDMA indirect address, low byte
                self.gpdma_byte_counter[channel].lsb()
            }
            0x4306 => {
                // DASxH: GPDMA byte counter / HDMA indirect address, high byte
                self.gpdma_byte_counter[channel].msb()
            }
            0x4307 => {
                // DASBx: HDMA indirect address, bank
                self.hdma_indirect_bank[channel]
            }
            0x4308 => {
                // A2AxL: HDMA current table address, low byte
                self.hdma_table_current_address[channel].lsb()
            }
            0x4309 => {
                // A2AxH: HDMA current table address, high byte
                self.hdma_table_current_address[channel].msb()
            }
            0x430A => {
                // NTRLx: HDMA line counter
                self.hdma_line_counter[channel]
            }
            0x430B | 0x430F => {
                // Unused DMA registers; R/W byte
                self.unused_dma_register[channel]
            }
            _ => {
                // Open bus
                return None;
            }
        };

        Some(value)
    }

    fn write_dma_register(&mut self, address: u32, value: u8) {
        // Second-least significant nibble is channel
        let channel = ((address >> 4) & 0x7) as usize;

        log::trace!("  DMA channel: {channel}");

        match address & 0xFF0F {
            0x4300 => {
                // DMAPx: DMA parameters 0-7
                self.dma_transfer_unit[channel] = value & 0x07;
                self.dma_increment_mode[channel] = DmaIncrementMode::from_byte(value);
                self.dmap_unused_bit[channel] = value.bit(5);
                self.hdma_addressing_mode[channel] = HdmaAddressingMode::from_byte(value);
                self.dma_direction[channel] = DmaDirection::from_byte(value);

                log::trace!("  DMA transfer unit: {}", self.dma_transfer_unit[channel]);
                log::trace!("  DMA increment mode: {:?}", self.dma_increment_mode[channel]);
                log::trace!("  HDMA addressing mode: {:?}", self.hdma_addressing_mode[channel]);
                log::trace!("  DMA direction: {:?}", self.dma_direction[channel]);
            }
            0x4301 => {
                // BBADx: DMA bus B address
                self.dma_bus_b_address[channel] = value;

                log::trace!("  DMA bus B address: {value:02X}");
            }
            0x4302 => {
                // A1TxL: GPDMA current address / HDMA table start address, low byte
                self.gpdma_current_address[channel].set_lsb(value);

                log::trace!(
                    "  GPDMA current address / HDMA table start address: {:04X}",
                    self.gpdma_current_address[channel]
                );
            }
            0x4303 => {
                // A1TxH: GPDMA current address / HDMA table start address, high byte
                self.gpdma_current_address[channel].set_msb(value);

                log::trace!(
                    "  GPDMA current address / HDMA table start address: {:04X}",
                    self.gpdma_current_address[channel]
                );
            }
            0x4304 => {
                // A1Bx: GPDMA current address / HDMA table start address, bank
                self.dma_bank[channel] = value;

                log::trace!(
                    "  GPDMA current address bank / HDMA table start address bank: {value:02X}"
                );
            }
            0x4305 => {
                // DASxL: GPDMA byte counter / HDMA indirect address, low byte
                self.gpdma_byte_counter[channel].set_lsb(value);

                log::trace!(
                    "  GPDMA byte counter / HDMA indirect address: {:04X}",
                    self.gpdma_byte_counter[channel]
                );
            }
            0x4306 => {
                // DASxH: GPDMA byte counter / HDMA indirect address, high byte
                self.gpdma_byte_counter[channel].set_msb(value);

                log::trace!(
                    "  GPDMA byte counter / HDMA indirect address: {:04X}",
                    self.gpdma_byte_counter[channel]
                );
            }
            0x4307 => {
                // DASBx: HDMA indirect address, bank
                self.hdma_indirect_bank[channel] = value;

                log::trace!("  HDMA indirect address bank: {value:02X}");
            }
            0x4308 => {
                // A2AxL: HDMA table current address, low byte
                self.hdma_table_current_address[channel].set_lsb(value);

                log::trace!(
                    "  HDMA table current address: {:04X}",
                    self.hdma_table_current_address[channel]
                );
            }
            0x4309 => {
                // A2AxH: HDMA table current address, high byte
                self.hdma_table_current_address[channel].set_msb(value);

                log::trace!(
                    "  HDMA table current address: {:04X}",
                    self.hdma_table_current_address[channel]
                );
            }
            0x430A => {
                // NTRLx: HDMA line counter
                self.hdma_line_counter[channel] = value;

                log::trace!("  HDMA line counter: {value:02X}");
            }
            0x430B | 0x430F => {
                // Unused DMA registers; R/W byte
                self.unused_dma_register[channel] = value;

                log::trace!("  Unused DMA register: {value:02X}");
            }
            _ => {
                // Open bus; do nothing
            }
        }
    }

    pub fn memory_2_speed(&self) -> Memory2Speed {
        self.memory_2_speed
    }

    pub fn wrio_register(&self) -> u8 {
        self.programmable_joypad_port
    }

    pub fn tick(
        &mut self,
        master_cycles_elapsed: u64,
        ppu: &Ppu,
        prev_scanline_mclk: u64,
        inputs: &SnesInputs,
    ) {
        // Progress auto joypad read if it's running
        self.input_state.tick(master_cycles_elapsed, *inputs);

        // Update VBlank, HBlank, and NMI flags
        self.update_hv_blank_flags(ppu);

        // Check H/V IRQs
        self.check_irq(master_cycles_elapsed, prev_scanline_mclk, ppu);

        // Check if auto joypad read should start
        if self.auto_joypad_read_enabled
            && ppu.is_first_vblank_scanline()
            && ppu.scanline_master_cycles() >= AUTO_JOYPAD_START_MCLK
            && (ppu.scanline_master_cycles() - master_cycles_elapsed) < AUTO_JOYPAD_START_MCLK
        {
            self.input_state.start_auto_joypad_read();
        }
    }

    fn update_hv_blank_flags(&mut self, ppu: &Ppu) {
        let vblank_flag = ppu.vblank_flag();
        if !self.vblank_flag && vblank_flag {
            // Start of VBlank
            if self.nmi_enabled && !self.vblank_nmi_flag {
                self.nmi_pending = true;
            }
            self.vblank_nmi_flag = true;
        } else if self.vblank_flag && !vblank_flag {
            // End of VBlank
            self.vblank_nmi_flag = false;
        }
        self.vblank_flag = vblank_flag;

        self.hblank_flag = ppu.hblank_flag();
    }

    fn check_irq(&mut self, master_cycles_elapsed: u64, prev_scanline_mclk: u64, ppu: &Ppu) {
        match self.irq_mode {
            IrqMode::Off => {}
            IrqMode::H => {
                // Generate H IRQ at H=HTIME+3.5, every line (mclks: 4*HTIME + 14)
                if check_htime_passed(
                    prev_scanline_mclk,
                    ppu.scanline_master_cycles(),
                    self.irq_htime,
                ) {
                    self.irq_pending = true;
                }
            }
            IrqMode::V => {
                // Generate V IRQ at V=VTIME and H=2.5 (10 mclks into scanline)
                if ppu.scanline() == self.irq_vtime
                    && check_v_irq(ppu.scanline_master_cycles(), master_cycles_elapsed)
                {
                    self.irq_pending = true;
                }
            }
            IrqMode::HV => {
                // Generate HV IRQ at V=VTIME and H=HTIME+3.5 (mclks: 4*HTIME + 14)
                // Unless HTIME=0, then generate at V=VTIME and H=2.5 (same as V IRQ)
                if ppu.scanline() == self.irq_vtime {
                    let htime_passed = if self.irq_htime == 0 {
                        check_v_irq(ppu.scanline_master_cycles(), master_cycles_elapsed)
                    } else {
                        check_htime_passed(
                            prev_scanline_mclk,
                            ppu.scanline_master_cycles(),
                            self.irq_htime,
                        )
                    };

                    if htime_passed {
                        self.irq_pending = true;
                    }
                }
            }
        }
    }

    pub fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    pub fn acknowledge_nmi(&mut self) {
        self.nmi_pending = false;
    }

    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    pub fn reset(&mut self) {
        // Reset NMITIMEN and clear any pending NMI
        self.write_register(0x4200, 0x00);
        self.vblank_nmi_flag = false;
        self.nmi_pending = false;

        // Reset WRIO
        self.write_register(0x4201, 0xFF);

        // Reset MDMAEN
        self.write_register(0x420B, 0x00);

        // Reset HDMAEN
        self.write_register(0x420C, 0x00);

        // Reset MEMSEL
        self.write_register(0x420D, 0x00);
    }

    pub fn controller_hv_latch(&self) -> Option<(u16, u16)> {
        // Controllers can only latch H/V when WRIO bit 7 is set
        self.programmable_joypad_port.bit(7).then_some(self.input_state.hv_latch()).flatten()
    }
}

fn check_v_irq(scanline_mclk: u64, master_cycles_elapsed: u64) -> bool {
    scanline_mclk >= V_IRQ_H_MCLK
        && scanline_mclk.saturating_sub(master_cycles_elapsed) < V_IRQ_H_MCLK
}

fn check_htime_passed(prev_scanline_mclk: u64, scanline_mclk: u64, htime: u16) -> bool {
    // H IRQs and HV IRQs should trigger at H=HTIME+3.5, or mclks=4*(HTIME+3.5)
    // Allow the +3.5 to go past the end of the scanline, but also take care not to miss low HTIMEs
    let htime_mclk: u64 = (4 * htime + 14).into();
    scanline_mclk >= htime_mclk
        && (prev_scanline_mclk < htime_mclk || scanline_mclk < prev_scanline_mclk)
}
