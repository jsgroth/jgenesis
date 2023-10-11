mod ssp1601;

use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::array;

const SVP_ENTRY_POINT: u16 = 0x400;

const DRAM_LEN: usize = 128 * 1024;
const IRAM_LEN_WORDS: usize = 1024;
const INTERNAL_RAM_LEN_WORDS: usize = 256;

const STACK_LEN: usize = 6;

// External memory addresses are 21-bit
const EXTERNAL_MEMORY_MASK: u32 = (1 << 21) - 1;

type Dram = [u8; DRAM_LEN];
type Iram = [u16; IRAM_LEN_WORDS];
type InternalRam = [u16; INTERNAL_RAM_LEN_WORDS];

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct StatusRegister {
    loop_size: u8,
    st5: bool,
    st6: bool,
    zero: bool,
    negative: bool,
}

impl StatusRegister {
    fn loop_modulo(self) -> u8 {
        if self.loop_size != 0 { 1 << self.loop_size } else { 0 }
    }

    fn st_bits_set(self) -> bool {
        self.st5 || self.st6
    }

    fn write(&mut self, value: u16) {
        self.loop_size = (value & 0x07) as u8;
        self.st5 = value.bit(5);
        self.st6 = value.bit(6);
        self.zero = value.bit(13);
        self.negative = value.bit(15);
    }
}

impl From<StatusRegister> for u16 {
    fn from(value: StatusRegister) -> Self {
        (u16::from(value.negative) << 15)
            | (u16::from(value.zero) << 13)
            | (u16::from(value.st6) << 6)
            | (u16::from(value.st5) << 5)
            | u16::from(value.loop_size)
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct StackRegister {
    stack: [u16; STACK_LEN],
    pointer: u8,
}

impl StackRegister {
    fn push(&mut self, value: u16) {
        self.stack[self.pointer as usize] = value;
        self.pointer += 1;
    }

    fn pop(&mut self) -> u16 {
        self.pointer -= 1;
        self.stack[self.pointer as usize]
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct ProgrammableMemoryRegister {
    address: u32,
    auto_increment: u32,
    auto_increment_negative: bool,
    auto_increment_bits: u16,
    special_increment_mode: bool,
    overwrite_mode: bool,
}

impl ProgrammableMemoryRegister {
    fn initialize(&mut self, address: u16, mode: u16) {
        // Bits 4-0 of mode are bits 20-16 of the 21-bit address
        self.address = u32::from(address) | (u32::from(mode & 0x001F) << 16);

        self.overwrite_mode = mode.bit(10);

        // Auto increment bits of 0 indicate 0, 7 indicate 128, and other values indicate 2^(N-1)
        let auto_increment_bits = (mode >> 11) & 0x07;
        self.auto_increment_bits = auto_increment_bits;
        self.auto_increment = match auto_increment_bits {
            0 => 0,
            7 => 128,
            _ => 1 << (auto_increment_bits - 1),
        };

        self.special_increment_mode = mode.bit(14);
        self.auto_increment_negative = mode.bit(15);
    }

    fn get_and_increment_address(&mut self) -> u32 {
        let address = self.address;

        if self.special_increment_mode {
            // "Special" increment mode increments the address by 1 if it is even and 31 if it is odd
            if !address.bit(0) {
                self.address = (self.address + 1) & EXTERNAL_MEMORY_MASK;
            } else {
                self.address = (self.address + 31) & EXTERNAL_MEMORY_MASK;
            }
        } else if self.auto_increment != 0 {
            if self.auto_increment_negative {
                self.address =
                    self.address.wrapping_sub(self.auto_increment) & EXTERNAL_MEMORY_MASK;
            } else {
                self.address =
                    self.address.wrapping_add(self.auto_increment) & EXTERNAL_MEMORY_MASK;
            }
        }

        address
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum PmcWaitingFor {
    #[default]
    Address,
    Mode,
}

impl PmcWaitingFor {
    fn toggle(self) -> Self {
        match self {
            Self::Address => Self::Mode,
            Self::Mode => Self::Address,
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct ProgrammableMemoryControlRegister {
    waiting_for: PmcWaitingFor,
    address: u16,
    mode: u16,
}

impl ProgrammableMemoryControlRegister {
    fn read(&mut self) -> u16 {
        let value = match self.waiting_for {
            PmcWaitingFor::Address => self.address,
            PmcWaitingFor::Mode => {
                // If waiting for mode, return address but rotated by 4; direction doesn't matter
                // since SVP always does this with both bytes equal
                (self.address << 4) | (self.address >> 12)
            }
        };

        self.waiting_for = self.waiting_for.toggle();

        value
    }

    fn write(&mut self, value: u16) {
        match self.waiting_for {
            PmcWaitingFor::Address => {
                self.address = value;
            }
            PmcWaitingFor::Mode => {
                self.mode = value;
            }
        }

        self.waiting_for = self.waiting_for.toggle();
    }

    fn update_from(&mut self, pm_register: &ProgrammableMemoryRegister) {
        self.address = pm_register.address as u16;
        self.mode = (u16::from(pm_register.auto_increment_negative) << 15)
            | (u16::from(pm_register.special_increment_mode) << 14)
            | (pm_register.auto_increment_bits << 11)
            | (u16::from(pm_register.overwrite_mode) << 10)
            | (pm_register.address >> 16) as u16;

        log::trace!("Set PMC address to {:04X} and mode to {:04X}", self.address, self.mode);
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct ExternalStatusRegister {
    value: u16,
    m68k_written: bool,
    ssp_written: bool,
}

impl ExternalStatusRegister {
    fn m68k_write(&mut self, value: u16) {
        self.value = value;
        self.m68k_written = true;
    }

    fn ssp_write(&mut self, value: u16) {
        self.value = value;
        self.ssp_written = true;
    }

    fn status(&self) -> u16 {
        (u16::from(self.m68k_written) << 1) | u16::from(self.ssp_written)
    }

    fn m68k_read_status(&mut self) -> u16 {
        let status = self.status();
        self.ssp_written = false;
        status
    }

    fn ssp_read_status(&mut self) -> u16 {
        let status = self.status();
        self.m68k_written = false;
        status
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    // General registers (0-7)
    x: u16,
    y: u16,
    accumulator: u32,
    status: StatusRegister,
    stack: StackRegister,
    pc: u16,
    // External registers (8-15)
    pm_read: [ProgrammableMemoryRegister; 5],
    pm_write: [ProgrammableMemoryRegister; 5],
    pmc: ProgrammableMemoryControlRegister,
    xst: ExternalStatusRegister,
    // Pointer registers (0-2 and 4-6, 3 and 7 are not stored)
    ram0_pointers: [u8; 3],
    ram1_pointers: [u8; 3],
}

impl Registers {
    fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            accumulator: 0,
            status: StatusRegister::default(),
            stack: StackRegister::default(),
            pc: SVP_ENTRY_POINT,
            pm_read: array::from_fn(|_| ProgrammableMemoryRegister::default()),
            pm_write: array::from_fn(|_| ProgrammableMemoryRegister::default()),
            pmc: ProgrammableMemoryControlRegister::default(),
            xst: ExternalStatusRegister::default(),
            ram0_pointers: [0; 3],
            ram1_pointers: [0; 3],
        }
    }

    fn product(&self) -> u32 {
        // P register always contains 2 * X * Y, where X and Y are sign extended from 16 bits to 32 bits
        2_u32.wrapping_mul(self.x as i16 as u32).wrapping_mul(self.y as i16 as u32)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Svp {
    registers: Registers,
    dram: Box<Dram>,
    dram_dirty: bool,
    iram: Box<Iram>,
    ram0: Box<InternalRam>,
    ram1: Box<InternalRam>,
    halted: bool,
}

impl Svp {
    pub fn new() -> Self {
        Self {
            registers: Registers::new(),
            dram_dirty: false,
            dram: vec![0; DRAM_LEN].into_boxed_slice().try_into().unwrap(),
            iram: vec![0; IRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            ram0: vec![0; INTERNAL_RAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            ram1: vec![0; INTERNAL_RAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            halted: false,
        }
    }

    pub fn tick(&mut self, rom: &[u8], m68k_cycles: u32) {
        if self.halted {
            return;
        }

        // Somewhat arbitrarily execute 3 instructions for every 68k cycle; this is close enough to
        // the chip's actual speed of somewhere in the 20-25 MHz range, and Virtua Racing's code is
        // not timing-sensitive
        for _ in 0..3 * m68k_cycles {
            // Hacky idle loop detection: if the SSP1601 is waiting for the 68000 to give it a
            // command, don't bother executing anything until the 68000 writes to $FE06 or $FE08 in
            // DRAM
            if self.registers.pc == 0x0425 || self.registers.pc == 0x2789 {
                if !self.dram_dirty {
                    return;
                }
                self.dram_dirty = false;
            }

            // At startup, the SVP spins until the 68000 writes to the XST; don't execute until that
            // happens
            if self.registers.pc == 0x0400 && !self.registers.xst.m68k_written {
                return;
            }

            ssp1601::execute_instruction(self, rom);
        }
    }

    pub fn m68k_read(&mut self, address: u32, rom: &[u8]) -> u16 {
        match address {
            0x000000..=0x1FFFFF => {
                // ROM
                let msb = rom[address as usize];
                let lsb = rom[(address + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x300000..=0x37FFFF => {
                // DRAM
                let address = address & 0x1FFFF;
                let msb = self.dram[address as usize];
                let lsb = self.dram[(address + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0xA15000 | 0xA15002 => {
                // XST register
                self.registers.xst.value
            }
            0xA15004 => {
                // XST status
                self.registers.xst.m68k_read_status()
            }
            _ => {
                // Invalid or unused
                0xFFFF
            }
        }
    }

    pub fn m68k_write_byte(&mut self, address: u32, value: u8) {
        match address {
            0x300000..=0x37FFFF => {
                // DRAM
                self.dram[(address & 0x1FFFF) as usize] = value;

                if (0xFE06..0xFE0A).contains(&address) {
                    self.dram_dirty = true;
                }
            }
            _ => {
                // Treat other writes as word-size
                if address.bit(0) {
                    self.m68k_write_word(address & !1, value.into());
                } else {
                    self.m68k_write_word(address, u16::from(value) << 8);
                }
            }
        }
    }

    pub fn m68k_write_word(&mut self, address: u32, value: u16) {
        match address {
            0x300000..=0x37FFFF => {
                // DRAM
                let address = address & 0x1FFFF;
                let [msb, lsb] = value.to_be_bytes();
                self.dram[address as usize] = msb;
                self.dram[(address + 1) as usize] = lsb;

                if address == 0xFE06 || address == 0xFE08 {
                    self.dram_dirty = true;
                }
            }
            0xA15000 | 0xA15002 => {
                // XST register
                self.registers.xst.m68k_write(value);
            }
            0xA15006 => {
                // SVP halt register
                self.halted = value == 0x000A;
            }
            _ => {
                // Invalid or unused
            }
        }
    }

    fn read_program_memory(&self, address: u16, rom: &[u8]) -> u16 {
        match address {
            0x0000..=0x03FF => {
                // IRAM
                self.iram[address as usize]
            }
            0x0400..=0xFFFF => {
                // ROM; program memory address maps to the same address in ROM
                let byte_addr = u32::from(address) << 1;
                let msb = rom[byte_addr as usize];
                let lsb = rom[(byte_addr + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
        }
    }

    fn read_external_memory(&mut self, address: u32, rom: &[u8]) -> u16 {
        log::trace!("External memory read: {address:06X}");

        match address {
            0x000000..=0x0FFFFF => {
                // ROM
                let byte_addr = address << 1;
                let msb = rom[byte_addr as usize];
                let lsb = rom[(byte_addr + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x180000..=0x18FFFF => {
                // DRAM
                let byte_addr = (address & 0xFFFF) << 1;
                let msb = self.dram[byte_addr as usize];
                let lsb = self.dram[(byte_addr + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x1C8000..=0x1C83FF => {
                // IRAM
                self.iram[(address & 0x3FF) as usize]
            }
            _ => {
                // Invalid or unused
                0xFFFF
            }
        }
    }

    fn write_external_memory(&mut self, address: u32, value: u16) {
        log::trace!("External memory write: {address:06X} {value:04X}");

        match address {
            0x180000..=0x18FFFF => {
                // DRAM
                let byte_addr = (address & 0xFFFF) << 1;
                let [msb, lsb] = value.to_be_bytes();
                self.dram[byte_addr as usize] = msb;
                self.dram[(byte_addr + 1) as usize] = lsb;
            }
            0x1C8000..=0x1C83FF => {
                // IRAM
                self.iram[(address & 0x3FF) as usize] = value;
            }
            _ => {
                // Invalid or unused
            }
        }
    }
}
