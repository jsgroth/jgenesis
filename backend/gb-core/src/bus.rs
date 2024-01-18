//! Game Boy bus / address mapping

use crate::cartridge::Cartridge;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::sm83::bus::BusInterface;
use crate::sm83::InterruptType;

pub struct Bus<'a> {
    pub ppu: &'a mut Ppu,
    pub memory: &'a mut Memory,
    pub cartridge: &'a mut Cartridge,
    pub interrupt_registers: &'a mut InterruptRegisters,
}

impl<'a> Bus<'a> {
    fn read_io_register(&self, address: u16) -> u8 {
        match address & 0x7F {
            0x0F => self.interrupt_registers.read_if(),
            0x40..=0x4B => self.ppu.read_register(address),
            _ => todo!("I/O register at {address:04X}"),
        }
    }

    fn write_io_register(&mut self, address: u16, value: u8) {
        match address & 0x7F {
            0x0F => self.interrupt_registers.write_if(value),
            0x40..=0x4B => self.ppu.write_register(address, value),
            _ => todo!("I/O register at {address:04X} value {value:02X}"),
        }
    }

    fn tick_components(&mut self) {
        self.ppu.tick(self.interrupt_registers);
    }
}

impl<'a> BusInterface for Bus<'a> {
    fn read(&mut self, address: u16) -> u8 {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => self.cartridge.read(address),
            0x8000..=0x9FFF => self.ppu.read_vram(address),
            0xA000..=0xBFFF => todo!("cartridge RAM"),
            0xC000..=0xFDFF => self.memory.read_main_ram(address),
            0xFE00..=0xFE9F => self.ppu.read_oam(address),
            // Unusable memory
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00..=0xFF7F => self.read_io_register(address),
            0xFF80..=0xFFFE => self.memory.read_hram(address),
            0xFFFF => self.interrupt_registers.read_ie(),
        }
    }

    fn write(&mut self, address: u16, value: u8) {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => todo!("cartridge mapper registers"),
            0x8000..=0x9FFF => self.ppu.write_vram(address, value),
            0xA000..=0xBFFF => todo!("cartridge RAM"),
            0xC000..=0xFDFF => self.memory.write_main_ram(address, value),
            0xFE00..=0xFE9F => self.ppu.write_oam(address, value),
            // Unusable memory
            0xFEA0..=0xFEFF => {}
            0xFF00..=0xFF7F => self.write_io_register(address, value),
            0xFF80..=0xFFFE => self.memory.write_hram(address, value),
            0xFFFF => self.interrupt_registers.write_ie(value),
        }
    }

    fn idle(&mut self) {
        self.tick_components();
    }

    fn highest_priority_interrupt(&self) -> Option<InterruptType> {
        self.interrupt_registers.highest_priority_interrupt()
    }

    fn acknowledge_interrupt(&mut self, interrupt_type: InterruptType) {
        self.interrupt_registers.clear_flag(interrupt_type);
    }
}
