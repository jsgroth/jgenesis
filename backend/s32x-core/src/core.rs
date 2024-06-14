mod bus;
mod registers;

use crate::core::bus::{Sh2Bus, WhichCpu};
use crate::core::registers::Sega32XRegisters;
use bincode::{Decode, Encode};
use genesis_core::memory::PhysicalMedium;
use genesis_core::GenesisRegion;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use sh2_emu::Sh2;
use std::ops::Deref;

const M68K_VECTORS: &[u8; 256] = include_bytes!("m68k_vectors.bin");
const SH2_MASTER_BOOT_ROM: &[u8; 2048] = include_bytes!("sh2_master_boot_rom.bin");
const SH2_SLAVE_BOOT_ROM: &[u8; 1024] = include_bytes!("sh2_slave_boot_rom.bin");

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Rom {
    fn get_u16(&self, address: u32) -> u16 {
        let address = (address & !1) as usize;
        if address < self.0.len() {
            u16::from_be_bytes(self.0[address..address + 2].try_into().unwrap())
        } else {
            0xFFFF
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    #[partial_clone(default)]
    rom: Rom,
    sh2_master: Sh2,
    sh2_slave: Sh2,
    registers: Sega32XRegisters,
    sh2_cycles: u64,
}

impl Sega32X {
    pub fn new(rom: Box<[u8]>) -> Self {
        Self {
            rom: Rom(rom),
            sh2_master: Sh2::new(),
            sh2_slave: Sh2::new(),
            registers: Sega32XRegisters::new(),
            sh2_cycles: 0,
        }
    }

    pub fn tick(&mut self, m68k_cycles: u64) {
        if !self.registers.system.adapter_enabled {
            return;
        }

        self.sh2_cycles += 3 * m68k_cycles;

        // TODO actual timing
        let sh2_ticks = self.sh2_cycles / 2;
        self.sh2_cycles %= 2;

        let mut bus = Sh2Bus {
            boot_rom: SH2_MASTER_BOOT_ROM,
            registers: &mut self.registers,
            which: WhichCpu::Master,
        };
        for _ in 0..sh2_ticks {
            self.sh2_master.tick(&mut bus);
        }

        bus.boot_rom = SH2_SLAVE_BOOT_ROM;
        bus.which = WhichCpu::Slave;
        for _ in 0..sh2_ticks {
            self.sh2_slave.tick(&mut bus);
        }
    }
}

impl PhysicalMedium for Sega32X {
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x0000FF => {
                if self.registers.system.adapter_enabled {
                    M68K_VECTORS[address as usize]
                } else {
                    self.rom.get(address as usize).copied().unwrap_or(0xFF)
                }
            }
            0x000010..=0x3FFFFF => {
                // TODO access only when RV=1 or adapter disabled
                self.rom.get(address as usize).copied().unwrap_or(0xFF)
            }
            0xA15100..=0xA1517F => self.registers.m68k_read_byte(address),
            _ => todo!("read byte {address:06X}"),
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x0000FF => {
                if self.registers.system.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(M68K_VECTORS[address..address + 2].try_into().unwrap())
                } else {
                    self.rom.get_u16(address)
                }
            }
            0x000100..=0x3FFFFF => {
                // TODO access only when RV=1 or adapter disabled
                self.rom.get_u16(address)
            }
            0x880000..=0x8FFFFF => {
                // TODO access only when RV=0
                if self.registers.system.adapter_enabled {
                    self.rom.get_u16(address & 0x7FFFF)
                } else {
                    0xFF
                }
            }
            // 32X ID - "MARS"
            0xA130EC => u16::from_be_bytes([b'M', b'A']),
            0xA130EE => u16::from_be_bytes([b'R', b'S']),
            _ => todo!("read word {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        todo!("read word for DMA {address:06X}")
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            0xA15100..=0xA1517F => self.registers.m68k_write_byte(address, value),
            _ => todo!("write byte {address:06X} {value:02X}"),
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            0xA15100..=0xA1517F => self.registers.m68k_write_word(address, value),
            _ => todo!("write word {address:06X} {value:04X}"),
        }
    }

    fn region(&self) -> GenesisRegion {
        // TODO
        GenesisRegion::Americas
    }
}
