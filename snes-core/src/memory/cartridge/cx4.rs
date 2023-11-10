mod functions;

use crate::memory::cartridge;
use crate::memory::cartridge::{CartridgeAddress, Rom};
use bincode::{Decode, Encode};
use jgenesis_proc_macros::PartialClone;
use std::{cmp, mem};

const RAM_LEN: usize = 3 * 1024;

const LOROM_NMI_VECTOR: usize = 0x7FEA;
const LOROM_IRQ_VECTOR: usize = 0x7FEE;

type Cx4Ram = [u8; RAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
struct Cx4Registers {
    // CX4's general-purpose registers (16x 24-bit)
    gpr: [u32; 16],
    dma_source_address: u32,
    dma_destination_address: u16,
    dma_length: u16,
    program_rom_base: u32,
    instruction_page: u16,
    instruction_pointer: u8,
    nmi_vector: u16,
    irq_vector: u16,
    // R/W registers with unknown functionality (MMX2/MMX3 always set them to fixed values)
    unknown_register_7f50: u8,
    unknown_register_7f51: u8,
    unknown_register_7f52: u8,
}

impl Cx4Registers {
    fn new(rom: &[u8]) -> Self {
        // Pre-populate NMI and IRQ vectors with the values from ROM
        let nmi_vector = u16::from_le_bytes([rom[LOROM_NMI_VECTOR], rom[LOROM_NMI_VECTOR + 1]]);
        let irq_vector = u16::from_le_bytes([rom[LOROM_IRQ_VECTOR], rom[LOROM_IRQ_VECTOR + 1]]);
        Self {
            gpr: [0; 16],
            dma_source_address: 0,
            dma_destination_address: 0,
            dma_length: 0,
            program_rom_base: 0,
            instruction_page: 0,
            instruction_pointer: 0,
            nmi_vector,
            irq_vector,
            unknown_register_7f50: 0,
            unknown_register_7f51: 0,
            unknown_register_7f52: 0,
        }
    }

    fn read(&self, offset: u16) -> Option<u8> {
        let value = match offset {
            // Program ROM base, low byte
            0x7F49 => self.program_rom_base as u8,
            // Program ROM base, middle byte
            0x7F4A => (self.program_rom_base >> 8) as u8,
            // Program ROM base, high byte
            0x7F4B => (self.program_rom_base >> 16) as u8,
            // R/W registers with unknown functionality
            0x7F50 => self.unknown_register_7f50,
            0x7F51 => self.unknown_register_7f51,
            0x7F52 => self.unknown_register_7f52,
            // TODO should return busy bit in bit 6 after writing to $7F47/$7F48/$7F4F
            0x7F5E => 0x00,
            // NMI vector
            0x7F6A => self.nmi_vector as u8,
            0x7F6B => (self.nmi_vector >> 8) as u8,
            // IRQ vector
            0x7F6E => self.irq_vector as u8,
            0x7F6F => (self.irq_vector >> 8) as u8,
            // CX4 24-bit registers
            0x7F80..=0x7FAF => self.read_24_bit_register(offset),
            _ => {
                log::info!("CX4 register read: {offset:04X}");
                return None;
            }
        };

        Some(value)
    }

    fn write(&mut self, offset: u16, value: u8, rom: &[u8], ram: &mut Cx4Ram) {
        match offset {
            0x7F40 => {
                // DMA source address, low byte
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFF_FF00) | u32::from(value);
            }
            0x7F41 => {
                // DMA source address, middle byte
                self.dma_source_address =
                    (self.dma_source_address & 0xFFFF_00FF) | (u32::from(value) << 8);
            }
            0x7F42 => {
                // DMA source address, high byte
                self.dma_source_address =
                    (self.dma_source_address & 0x0000_FFFF) | (u32::from(value) << 16);
            }
            0x7F43 => {
                // DMA length, low byte
                self.dma_length = (self.dma_length & 0xFF00) | u16::from(value);
            }
            0x7F44 => {
                // DMA length, high byte
                self.dma_length = (self.dma_length & 0x00FF) | (u16::from(value) << 8);
            }
            0x7F45 => {
                // DMA destination, low byte
                self.dma_destination_address =
                    (self.dma_destination_address & 0xFF00) | u16::from(value);
            }
            0x7F46 => {
                // DMA destination, high byte
                self.dma_destination_address =
                    (self.dma_destination_address & 0x00FF) | (u16::from(value) << 8);
            }
            0x7F47 => {
                // Start ROM-to-CX4 DMA
                if value == 0 {
                    self.run_dma(rom, ram);
                }
            }
            0x7F48 | 0x7F4C => {
                // Write-only register with unknown functionality
                // TODO set busy bit for $7F48
            }
            0x7F49 => {
                // Program ROM base, low byte
                self.program_rom_base = (self.program_rom_base & 0xFFFF_FF00) | u32::from(value);
            }
            0x7F4A => {
                // Program ROM base, middle byte
                self.program_rom_base =
                    (self.program_rom_base & 0xFFFF_00FF) | (u32::from(value) << 8);
            }
            0x7F4B => {
                // Program ROM base, high byte
                self.program_rom_base =
                    (self.program_rom_base & 0x0000_FFFF) | (u32::from(value) << 16);
            }
            0x7F4D => {
                // Program ROM instruction page, low byte
                self.instruction_page = (self.instruction_page & 0xFF00) | u16::from(value);
            }
            0x7F4E => {
                // Program ROM instruction page, high byte
                self.instruction_page = (self.instruction_page & 0x00FF) | (u16::from(value) << 8);
            }
            0x7F4F => {
                // Program ROM instruction pointer + execute instruction
                // TODO set busy bit?
                self.instruction_pointer = value;
                functions::execute(self, rom, ram);
            }
            0x7F50 => {
                // R/W register with unknown functionality
                self.unknown_register_7f50 = value;
            }
            0x7F51 => {
                // R/W register with unknown functionality
                self.unknown_register_7f51 = value;
            }
            0x7F52 => {
                // R/W register with unknown functionality
                self.unknown_register_7f52 = value;
            }
            0x7F6A => {
                // NMI vector, low byte
                self.nmi_vector = (self.nmi_vector & 0xFF00) | u16::from(value);
            }
            0x7F6B => {
                // NMI vector, high byte
                self.nmi_vector = (self.nmi_vector & 0x00FF) | (u16::from(value) << 8);
            }
            0x7F6E => {
                // IRQ vector, low byte
                self.irq_vector = (self.irq_vector & 0xFF00) | u16::from(value);
            }
            0x7F6F => {
                // IRQ vector, high byte
                self.irq_vector = (self.irq_vector & 0x00FF) | (u16::from(value) << 8);
            }
            0x7F80..=0x7FAF => {
                // CX4 24-bit registers
                self.write_24_bit_register(offset, value);
            }
            _ => {
                log::info!("CX4 register write: {offset:04X} {value:02X}");
            }
        }
    }

    fn read_24_bit_register(&self, offset: u16) -> u8 {
        let idx = (offset & 0x3F) / 3;
        let shift = (offset % 3) * 8;
        (self.gpr[idx as usize] >> shift) as u8
    }

    fn write_24_bit_register(&mut self, offset: u16, value: u8) {
        let idx = (offset & 0x3F) / 3;
        let (mask, shift) = match offset % 3 {
            0 => (0xFFFF_FF00, 0),
            1 => (0xFFFF_00FF, 8),
            2 => (0x0000_FFFF, 16),
            _ => panic!("value % 3 is always 0/1/2"),
        };

        let existing_value = self.gpr[idx as usize];
        self.gpr[idx as usize] = (existing_value & mask) | (u32::from(value) << shift);
    }

    fn run_dma(&self, rom: &[u8], ram: &mut Cx4Ram) {
        if !(0x6000..0x6C00).contains(&self.dma_destination_address) {
            return;
        }

        log::trace!(
            "Running DMA with src={:06X}, dst={:04X}, len={:04X}",
            self.dma_source_address,
            self.dma_destination_address,
            self.dma_length
        );

        let mut src_addr = self.dma_source_address;
        let mut dest_addr: u32 = (self.dma_destination_address - 0x6000).into();
        let dma_len = cmp::min(0x0C00 - dest_addr, self.dma_length.into());

        for _ in 0..dma_len {
            let rom_addr = cartridge::lorom_map_rom_address(src_addr, rom.len() as u32);
            ram[dest_addr as usize] = rom[rom_addr as usize];

            src_addr = (src_addr + 1) & 0xFFFFFF;
            dest_addr += 1;
        }
    }

    fn risc_pc(&self) -> u32 {
        self.program_rom_base
            + (u32::from(self.instruction_page) << 9)
            + (u32::from(self.instruction_pointer) << 1)
    }

    fn increment_ip(&mut self) {
        self.instruction_pointer = self.instruction_pointer.wrapping_add(1);
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Cx4 {
    #[partial_clone(default)]
    rom: Rom,
    ram: Box<Cx4Ram>,
    registers: Cx4Registers,
}

impl Cx4 {
    pub fn new(rom: Rom) -> Self {
        let registers = Cx4Registers::new(&rom);
        Self { rom, ram: vec![0; RAM_LEN].into_boxed_slice().try_into().unwrap(), registers }
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        let bank = (address >> 16) as u8;
        let offset = address as u16;
        match (bank, offset) {
            // NMI vector
            (0x00, 0xFFEA) => Some(self.registers.nmi_vector as u8),
            (0x00, 0xFFEB) => Some((self.registers.nmi_vector >> 8) as u8),
            // IRQ vector
            (0x00, 0xFFEE) => Some(self.registers.irq_vector as u8),
            (0x00, 0xFFEF) => Some((self.registers.irq_vector >> 8) as u8),
            // CX4 RAM (3KB)
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x6BFF) => {
                Some(self.ram[(address & 0xFFF) as usize])
            }
            // CX4 registers
            (0x00..=0x3F | 0x80..=0xBF, 0x7F40..=0x7FAF) => self.registers.read(offset),
            // SRAM range, which is unmapped in all CX4 games and always reads $00
            (0x70..=0x77, _) => Some(0x00),
            // Treat other addresses as LoROM
            _ => match cartridge::lorom_map_address(address, self.rom.len() as u32, 0) {
                CartridgeAddress::Rom(rom_addr) => Some(self.rom[rom_addr as usize]),
                CartridgeAddress::None | CartridgeAddress::Sram(..) => None,
            },
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) as u8;
        let offset = address as u16;
        match (bank, offset) {
            // CX4 RAM (3KB)
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x6BFF) => {
                self.ram[(address & 0xFFF) as usize] = value;
            }
            // CX4 registers
            (0x00..=0x3F | 0x80..=0xBF, 0x7F40..=0x7FAF) => {
                self.registers.write(offset, value, &self.rom, &mut self.ram);
            }
            _ => {
                log::info!("CX4 write: {address:06X} {value:02X}");
            }
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn set_rom(&mut self, rom: Vec<u8>) {
        self.rom.0 = rom.into_boxed_slice();
    }
}
