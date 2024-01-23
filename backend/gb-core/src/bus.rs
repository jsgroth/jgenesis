//! Game Boy bus / address mapping

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::DmaUnit;
use crate::inputs::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::sm83::bus::BusInterface;
use crate::sm83::InterruptType;
use crate::timer::GbTimer;

pub struct Bus<'a> {
    pub ppu: &'a mut Ppu,
    pub apu: &'a mut Apu,
    pub memory: &'a mut Memory,
    pub cartridge: &'a mut Cartridge,
    pub interrupt_registers: &'a mut InterruptRegisters,
    pub timer: &'a mut GbTimer,
    pub dma_unit: &'a mut DmaUnit,
    pub input_state: &'a mut InputState,
}

impl<'a> Bus<'a> {
    fn read_io_register(&self, address: u16) -> u8 {
        match address & 0x7F {
            0x00 => self.input_state.read_joyp(),
            0x04 => self.timer.read_div(),
            0x05 => self.timer.read_tima(),
            0x06 => self.timer.read_tma(),
            0x07 => self.timer.read_tac(),
            0x0F => self.interrupt_registers.read_if(),
            0x10..=0x3F => self.apu.read_register(address),
            0x40..=0x45 | 0x47..=0x4B => self.ppu.read_register(address),
            0x46 => self.dma_unit.read_dma_register(),
            _ => {
                log::warn!("read I/O register at {address:04X}");
                0xFF
            }
        }
    }

    fn write_io_register(&mut self, address: u16, value: u8) {
        match address & 0x7F {
            0x00 => self.input_state.write_joyp(value),
            0x04 => self.timer.write_div(),
            0x05 => self.timer.write_tima(value),
            0x06 => self.timer.write_tma(value),
            0x07 => self.timer.write_tac(value),
            0x0F => self.interrupt_registers.write_if(value),
            0x10..=0x3F => self.apu.write_register(address, value),
            0x40..=0x45 | 0x47..=0x4B => self.ppu.write_register(address, value),
            0x46 => self.dma_unit.write_dma_register(value),
            _ => log::warn!("write I/O register at {address:04X} value {value:02X}"),
        }
    }

    fn tick_components(&mut self) {
        self.timer.tick_m_cycle(self.interrupt_registers);
        self.dma_unit.tick_m_cycle(self.cartridge, self.memory, self.ppu);

        // TODO only 2 ticks in GBC double speed mode, and other components should tick every other cycle
        for _ in 0..4 {
            self.ppu.tick_dot(self.interrupt_registers);
        }

        self.apu.tick_m_cycle(self.timer);
    }
}

impl<'a> BusInterface for Bus<'a> {
    fn read(&mut self, address: u16) -> u8 {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => self.cartridge.read_rom(address),
            0x8000..=0x9FFF => self.ppu.read_vram(address),
            0xA000..=0xBFFF => self.cartridge.read_ram(address),
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
            0x0000..=0x7FFF => self.cartridge.write_rom(address, value),
            0x8000..=0x9FFF => self.ppu.write_vram(address, value),
            0xA000..=0xBFFF => self.cartridge.write_ram(address, value),
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
