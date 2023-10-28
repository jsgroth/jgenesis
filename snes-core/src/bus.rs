use crate::memory::{CpuInternalRegisters, Memory, Memory2Speed};
use crate::ppu::Ppu;
use wdc65816_emu::traits::BusInterface;

const FAST_MASTER_CYCLES: u64 = 6;
const SLOW_MASTER_CYCLES: u64 = 8;
const XSLOW_MASTER_CYCLES: u64 = 12;

impl Memory2Speed {
    fn master_cycles(self) -> u64 {
        match self {
            Self::Fast => FAST_MASTER_CYCLES,
            Self::Slow => SLOW_MASTER_CYCLES,
        }
    }
}

pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub cpu_registers: &'a mut CpuInternalRegisters,
    pub ppu: &'a mut Ppu,
    pub access_master_cycles: u64,
}

impl<'a> Bus<'a> {
    fn read_system_area(&mut self, address: u32) -> u8 {
        match address & 0x7FFF {
            0x0000..=0x1FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // First 8KB of WRAM
                self.memory.read_wram(address)
            }
            0x2100..=0x213F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // PPU ports
                self.ppu.read_port(address)
            }
            0x2180 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMDATA: WRAM port in address bus B
                self.memory.read_wram_port()
            }
            0x4000..=0x40FF => {
                self.access_master_cycles = XSLOW_MASTER_CYCLES;

                // CPU I/O ports (manual joypad ports)
                self.cpu_registers.read_register(address)
            }
            0x4100..=0x4FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // CPU I/O ports (everything except manual joypad ports)
                self.cpu_registers.read_register(address)
            }
            _ => todo!("read system area {address:06X}"),
        }
    }

    fn write_system_area(&mut self, address: u32, value: u8) {
        match address & 0x7FFF {
            0x0000..=0x1FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // First 8KB of WRAM
                self.memory.write_wram(address, value);
            }
            0x2100..=0x213F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // PPU ports
                self.ppu.write_port(address, value);
            }
            0x2180 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMDATA: WRAM port in address bus B
                self.memory.write_wram_port(value);
            }
            0x2181 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMADDL: WRAM port address, low byte
                self.memory.write_wram_port_address_low(value);
            }
            0x2182 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMADDM: WRAM port address, middle byte
                self.memory.write_wram_port_address_mid(value);
            }
            0x2183 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMADDH: WRAM port address, high byte
                self.memory.write_wram_port_address_high(value);
            }
            0x4000..=0x40FF => {
                self.access_master_cycles = XSLOW_MASTER_CYCLES;

                // CPU I/O ports (manual joypad ports)
                self.cpu_registers.write_register(address, value);
            }
            0x4100..=0x4FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // CPU I/O ports (everything except manual joypad ports)
                self.cpu_registers.write_register(address, value);
            }
            _ => todo!("write system area {address:06X} {value:02X}"),
        }
    }
}

impl<'a> BusInterface for Bus<'a> {
    #[inline]
    fn read(&mut self, address: u32) -> u8 {
        let bank = address >> 16;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x7FFF) => {
                // System area
                self.read_system_area(address)
            }
            (0x00..=0x3F, 0x8000..=0xFFFF) | (0x40..=0x7D, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // Cartridge (Memory-1)
                self.memory.read_cartridge(address)
            }
            (0x80..=0xBF, 0x8000..=0xFFFF) | (0xC0..=0xFF, _) => {
                self.access_master_cycles = self.cpu_registers.memory_2_speed().master_cycles();

                // Cartridge (Memory-2)
                self.memory.read_cartridge(address)
            }
            (0x7E..=0x7F, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // WRAM
                self.memory.read_wram(address)
            }
            _ => todo!("read address {address:06X}"),
        }
    }

    #[inline]
    fn write(&mut self, address: u32, value: u8) {
        let bank = address >> 16;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x7FFF) => {
                // System area
                self.write_system_area(address, value);
            }
            (0x00..=0x3F, 0x8000..=0xFFFF) | (0x40..=0x7D, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // Cartridge (Memory-1)
                self.memory.write_cartridge(address, value);
            }
            (0x80..=0xBF, 0x8000..=0xFFFF) | (0xC0..=0xFF, _) => {
                self.access_master_cycles = self.cpu_registers.memory_2_speed().master_cycles();

                // Cartridge (Memory-2)
                self.memory.write_cartridge(address, value);
            }
            (0x7E..=0x7F, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // WRAM
                self.memory.write_wram(address, value);
            }
            _ => todo!("write address {address:06X} {value:02X}"),
        }
    }

    #[inline]
    fn idle(&mut self) {
        self.access_master_cycles = FAST_MASTER_CYCLES;
    }

    #[inline]
    fn nmi(&self) -> bool {
        // TODO VBlank NMIs
        false
    }

    #[inline]
    fn irq(&self) -> bool {
        // TODO H/V IRQs
        false
    }
}
