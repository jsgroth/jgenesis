//! GBA memory map

use crate::cartridge::Cartridge;
use crate::memory::Memory;
use crate::ppu::Ppu;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};

pub struct Bus<'a> {
    pub ppu: &'a mut Ppu,
    pub memory: &'a mut Memory,
    pub cartridge: &'a mut Cartridge,
    pub cycles: u64,
}

impl BusInterface for Bus<'_> {
    #[inline]
    fn read_byte(&mut self, address: u32, _cycle: MemoryCycle) -> u8 {
        self.cycles += 1;

        match address {
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_byte(address),
            _ => todo!("read byte {address:08X}"),
        }
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, _cycle: MemoryCycle) -> u16 {
        self.cycles += 1;

        match address {
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_halfword(address),
            _ => todo!("read halfword {address:08X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32, _cycle: MemoryCycle) -> u32 {
        self.cycles += 1;

        match address {
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_word(address),
            _ => todo!("read word {address:08X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8, _cycle: MemoryCycle) {
        self.cycles += 1;

        todo!("write byte {address:08X} {value:02X}")
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, _cycle: MemoryCycle) {
        self.cycles += 1;

        match address {
            0x4000000..=0x4FFFFFF => {
                log::warn!("I/O register write {address:08X} {value:04X}");
            }
            0x6000000..=0x6017FFF => self.ppu.write_vram(address, value),
            _ => todo!("write halfword {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, _cycle: MemoryCycle) {
        self.cycles += 1;

        todo!("write word {address:08X} {value:08X}")
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
