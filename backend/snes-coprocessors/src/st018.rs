//! ST018, a coprocessor with a pre-programmed 21 MHz ARMv3 CPU (probably an ARM6)
//!
//! Used by only one game, Hayazashi Nidan Morita Shougi 2
//!
//! ST018 is emulated using an ARM7TDMI implementation (ARMv4T) which is almost fully backwards
//! compatible with ARMv3. ARMv4 removed support for the legacy 26-bit addressing mode, but HNMS2
//! does not use that functionality.
//!
//! Memory access timings are not emulated because they are not known. All memory accesses are
//! assumed to take 1 clock cycle which is probably not realistic.

use arm7tdmi_emu::Arm7Tdmi;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use thiserror::Error;

const PROGRAM_ROM_LEN_WORDS: usize = 128 * 1024 / 4;
const DATA_ROM_LEN: usize = 32 * 1024;
const RAM_LEN_WORDS: usize = 16 * 1024 / 4;

const TOTAL_ROM_LEN: usize = 4 * PROGRAM_ROM_LEN_WORDS + DATA_ROM_LEN;

type ProgramRom = [u32; PROGRAM_ROM_LEN_WORDS];
type DataRom = [u8; DATA_ROM_LEN];
type Ram = [u32; RAM_LEN_WORDS];

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    snes_to_arm_data: u8,
    snes_to_arm_data_ready: bool,
    arm_to_snes_data: u8,
    arm_to_snes_data_ready: bool,
    arm_to_snes_flag: bool,
    arm_reset: bool,
}

impl Registers {
    fn new() -> Self {
        Self {
            snes_to_arm_data: 0,
            snes_to_arm_data_ready: false,
            arm_to_snes_data: 0,
            arm_to_snes_data_ready: false,
            arm_to_snes_flag: false,
            arm_reset: true,
        }
    }

    fn arm_read(&mut self, address: u32) -> Option<u8> {
        match address & 0xFF {
            0x10 => {
                // SNES-to-ARM data
                self.snes_to_arm_data_ready = false;
                Some(self.snes_to_arm_data)
            }
            0x20 => Some(self.read_status()),
            _ => {
                log::error!("Invalid ARM register read {address:08X}");
                None
            }
        }
    }

    fn arm_write(&mut self, address: u32, value: u8) {
        log::trace!("ARM register write {address:08X} {value:02X}");

        match address & 0xFF {
            0x00 => {
                // ARM-to-SNES data
                self.arm_to_snes_data = value;
                self.arm_to_snes_data_ready = true;
            }
            0x10 => {
                // ARM-to-SNES flag; writing any value sets the flag
                self.arm_to_snes_flag = true;
            }
            0x20..=0x2F => {
                // fullsnes says these are "config" registers; unclear what (if anything) they actually do
                // HNMS2 writes to these very frequently but doesn't seem to depend on them doing anything
            }
            _ => {
                log::error!("Invalid ARM register write {address:08X} {value:02X}");
            }
        }
    }

    fn snes_read(&mut self, address: u32) -> Option<u8> {
        match address & 0xFFFF {
            0x3800 => {
                // ARM-to-SNES data
                self.arm_to_snes_data_ready = false;
                Some(self.arm_to_snes_data)
            }
            0x3802 => {
                // Clear ARM-to-SNES flag; read value is undefined
                self.arm_to_snes_flag = false;
                None
            }
            0x3804 => Some(self.read_status()),
            _ => {
                log::error!("Invalid SNES register read {address:06X}");
                None
            }
        }
    }

    fn snes_write(&mut self, address: u32, value: u8) {
        log::trace!("SNES register write {address:06X} {value:02X}");

        match address & 0xFFFF {
            0x3802 => {
                // SNES-to-ARM data
                self.snes_to_arm_data = value;
                self.snes_to_arm_data_ready = true;
            }
            0x3804 => {
                // Reset ARM CPU
                self.arm_reset = value.bit(0);
            }
            _ => {
                log::error!("Invalid SNES register write {address:06X} {value:02X}");
            }
        }
    }

    fn read_status(&self) -> u8 {
        // Reset finished (bit 7) and ??? (bit 6) hardcoded to 1
        u8::from(self.arm_to_snes_data_ready)
            | (u8::from(self.arm_to_snes_flag) << 2)
            | (u8::from(self.snes_to_arm_data_ready) << 3)
            | (1 << 6)
            | (1 << 7)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct ArmBus {
    program_rom: Box<ProgramRom>,
    data_rom: Box<DataRom>,
    ram: Box<Ram>,
    registers: Registers,
    cycles: u64,
    open_bus: u32,
}

// All reads from $60000000-$7FFFFFFF return this value according to:
//   https://forums.bannister.org/ubbthreads.php?ubb=showflat&Number=77760&page=all
// Not sure if the game depends on the value read, but it does occasionally read from these addresses
const ADDRESS_60_READS: u32 = 0x40404001;

impl BusInterface for ArmBus {
    #[inline]
    fn read_byte(&mut self, address: u32, _cycle: MemoryCycle) -> u8 {
        self.cycles += 1;

        let byte = match address {
            0x00000000..=0x1FFFFFFF => {
                // Program ROM
                let rom_addr = ((address >> 2) as usize) & (PROGRAM_ROM_LEN_WORDS - 1);
                self.program_rom[rom_addr].to_le_bytes()[(address & 3) as usize]
            }
            0x40000000..=0x5FFFFFFF => self
                .registers
                .arm_read(address)
                .unwrap_or_else(|| self.open_bus.to_le_bytes()[(address & 3) as usize]),
            0x60000000..=0x7FFFFFFF => ADDRESS_60_READS.to_le_bytes()[(address & 3) as usize],
            0xA0000000..=0xBFFFFFFF => {
                // Data ROM
                let rom_addr = (address as usize) & (DATA_ROM_LEN - 1);
                self.data_rom[rom_addr]
            }
            0xE0000000..=0xFFFFFFFF => {
                // RAM
                let ram_addr = ((address >> 2) as usize) & (RAM_LEN_WORDS - 1);
                self.ram[ram_addr].to_le_bytes()[(address & 3) as usize]
            }
            _ => self.open_bus.to_le_bytes()[(address & 3) as usize],
        };

        // TODO this is probably not right for 8-bit open bus
        self.open_bus = u32::from_le_bytes([byte; 4]);

        byte
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, _cycle: MemoryCycle) -> u16 {
        self.cycles += 1;

        log::error!("ST018 CPU is ARMv3; does not support halfword memory access ({address:08X})");

        0
    }

    #[inline]
    fn read_word(&mut self, address: u32, _cycle: MemoryCycle) -> u32 {
        self.cycles += 1;

        let word = match address {
            0x00000000..=0x1FFFFFFF => {
                // Program ROM
                let rom_addr = ((address >> 2) as usize) & (PROGRAM_ROM_LEN_WORDS - 1);
                self.program_rom[rom_addr]
            }
            0x40000000..=0x5FFFFFFF => {
                self.registers.arm_read(address).map_or(self.open_bus, u32::from)
            }
            0x60000000..=0x7FFFFFFF => ADDRESS_60_READS,
            0xA0000000..=0xBFFFFFFF => {
                // Data ROM; only has 8-bit data bus
                // TODO this is probably not accurate; this code path is not exercised
                let rom_addr = (address as usize) & (DATA_ROM_LEN - 1);
                let byte = self.data_rom[rom_addr];
                u32::from_le_bytes([byte; 4])
            }
            0xE0000000..=0xFFFFFFFF => {
                // RAM
                let ram_addr = ((address >> 2) as usize) & (RAM_LEN_WORDS - 1);
                self.ram[ram_addr]
            }
            _ => self.open_bus,
        };

        self.open_bus = word;

        word
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8, _cycle: MemoryCycle) {
        self.cycles += 1;

        // TODO this is probably not right for 8-bit open bus
        self.open_bus = u32::from_le_bytes([value; 4]);

        match address {
            0x40000000..=0x5FFFFFFF => self.registers.arm_write(address, value),
            0xE0000000..=0xFFFFFFFF => {
                // RAM
                let ram_addr = ((address >> 2) as usize) & (RAM_LEN_WORDS - 1);
                let mut word_bytes = self.ram[ram_addr].to_le_bytes();
                word_bytes[(address & 3) as usize] = value;
                self.ram[ram_addr] = u32::from_le_bytes(word_bytes);
            }
            _ => {}
        }
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, _cycle: MemoryCycle) {
        self.cycles += 1;

        log::error!(
            "ST018 CPU is ARMv3; does not support halfword memory access ({address:08X} {value:04X})"
        );
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, _cycle: MemoryCycle) {
        self.cycles += 1;
        self.open_bus = value;

        match address {
            0x40000000..=0x5FFFFFFF => self.registers.arm_write(address, value as u8),
            0xE0000000..=0xFFFFFFFF => {
                // RAM
                let ram_addr = ((address >> 2) as usize) & (RAM_LEN_WORDS - 1);
                self.ram[ram_addr] = value;
            }
            _ => {}
        }
    }

    #[inline]
    fn irq(&self) -> bool {
        false
    }

    #[inline]
    fn internal_cycles(&mut self, cycles: u32) {
        self.cycles += u64::from(cycles);
    }
}

#[derive(Debug, Error)]
pub enum St018LoadError {
    #[error("Expected ROM size of {expected} bytes, was {actual} bytes")]
    IncorrectRomSize { expected: usize, actual: usize },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct St018 {
    cpu: Arm7Tdmi<ArmBus>,
    bus: ArmBus,
    snes_cycles: u64,
}

impl St018 {
    /// # Errors
    ///
    /// Returns an error if the ST018 program/data ROM is invalid.
    #[allow(clippy::missing_panics_doc)]
    pub fn new(st018_rom: &[u8]) -> Result<Self, St018LoadError> {
        let (program_rom, data_rom) = convert_st018_rom(st018_rom)?;

        let bus = ArmBus {
            program_rom,
            data_rom,
            ram: vec![0; RAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            registers: Registers::new(),
            cycles: 0,
            open_bus: 0,
        };

        Ok(Self { cpu: Arm7Tdmi::new(), bus, snes_cycles: 0 })
    }

    pub fn tick(&mut self, snes_master_cycles: u64) {
        // ST018 has its own 21 MHz oscillator, but it runs at almost the exact same frequency as
        // the SNES master oscillator, so just assume they're the same speed
        self.snes_cycles += snes_master_cycles;

        if self.bus.registers.arm_reset {
            self.bus.registers.arm_reset = false;
            self.cpu.reset(&mut self.bus);
        }

        while self.bus.cycles < self.snes_cycles {
            self.cpu.execute_instruction(&mut self.bus);
        }
    }

    pub fn snes_read(&mut self, address: u32) -> Option<u8> {
        self.bus.registers.snes_read(address)
    }

    pub fn snes_write(&mut self, address: u32, value: u8) {
        self.bus.registers.snes_write(address, value);
    }
}

fn convert_st018_rom(rom: &[u8]) -> Result<(Box<ProgramRom>, Box<DataRom>), St018LoadError> {
    if rom.len() < TOTAL_ROM_LEN {
        return Err(St018LoadError::IncorrectRomSize {
            expected: TOTAL_ROM_LEN,
            actual: rom.len(),
        });
    }

    let program_rom: Vec<_> = rom[..4 * PROGRAM_ROM_LEN_WORDS]
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    let program_rom: Box<ProgramRom> = program_rom.into_boxed_slice().try_into().unwrap();

    let data_rom =
        rom[4 * PROGRAM_ROM_LEN_WORDS..4 * PROGRAM_ROM_LEN_WORDS + DATA_ROM_LEN].to_vec();
    let data_rom: Box<DataRom> = data_rom.into_boxed_slice().try_into().unwrap();

    Ok((program_rom, data_rom))
}
