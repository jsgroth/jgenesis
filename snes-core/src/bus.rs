use crate::memory::{CpuInternalRegisters, Memory};
use crate::ppu::Ppu;
use wdc65816_emu::traits::BusInterface;

pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub cpu_registers: &'a mut CpuInternalRegisters,
    pub ppu: &'a mut Ppu,
}

impl<'a> Bus<'a> {
    fn read_system_area(&mut self, address: u32) -> u8 {
        match address & 0x7FFF {
            0x0000..=0x1FFF => {
                // First 8KB of WRAM
                self.memory.read_wram(address)
            }
            0x2100..=0x213F => {
                // PPU ports
                self.ppu.read_port(address)
            }
            0x2180 => {
                // WMDATA: WRAM port in address bus B
                self.memory.read_wram_port()
            }
            0x4000..=0x4FFF => {
                // CPU I/O ports
                self.cpu_registers.read_register(address)
            }
            _ => todo!("read system area {address:06X}"),
        }
    }

    fn write_system_area(&mut self, address: u32, value: u8) {
        match address & 0x7FFF {
            0x0000..=0x1FFF => {
                // First 8KB of WRAM
                self.memory.write_wram(address, value);
            }
            0x2100..=0x213F => {
                // PPU ports
                self.ppu.write_port(address, value);
            }
            0x2180 => {
                // WMDATA: WRAM port in address bus B
                self.memory.write_wram_port(value);
            }
            0x2181 => {
                // WMADDL: WRAM port address, low byte
                self.memory.write_wram_port_address_low(value);
            }
            0x2182 => {
                // WMADDM: WRAM port address, middle byte
                self.memory.write_wram_port_address_mid(value);
            }
            0x2183 => {
                // WMADDH: WRAM port address, high byte
                self.memory.write_wram_port_address_high(value);
            }
            0x4000..=0x4FFF => {
                // CPU I/O ports
                self.cpu_registers.write_register(address, value);
            }
            _ => todo!("write system area {address:06X} {value:02X}"),
        }
    }
}

impl<'a> BusInterface for Bus<'a> {
    fn read(&mut self, address: u32) -> u8 {
        let bank = address >> 16;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x7FFF) => {
                // System area
                self.read_system_area(address)
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) | (0x40..=0x7D | 0xC0..=0xFF, _) => {
                // Cartridge
                self.memory.read_cartridge(address)
            }
            (0x7E..=0x7F, _) => {
                // WRAM
                self.memory.read_wram(address)
            }
            _ => todo!("read address {address:06X}"),
        }
    }

    fn write(&mut self, address: u32, value: u8) {
        let bank = address >> 16;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x7FFF) => {
                // System area
                self.write_system_area(address, value);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) | (0x40..=0x7D | 0xC0..=0xFF, _) => {
                // Cartridge
                self.memory.write_cartridge(address, value);
            }
            (0x7E..=0x7F, _) => {
                // WRAM
                self.memory.write_wram(address, value);
            }
            _ => todo!("write address {address:06X} {value:02X}"),
        }
    }

    fn idle(&mut self) {
        // TODO record that last cycle was an internal cycle
    }

    fn nmi(&self) -> bool {
        // TODO VBlank NMIs
        false
    }

    fn irq(&self) -> bool {
        // TODO H/V IRQs
        false
    }
}
