use crate::apu::Apu;
use crate::memory::{CpuInternalRegisters, Memory, Memory2Speed};
use crate::ppu::Ppu;
use wdc65816_emu::traits::BusInterface;

// Accesses to address bus B (PPU/APU/WRAM ports) and internal CPU registers are "fast" (no waitstates)
// Accesses to the cartridge in the higher banks can also be fast depending on register $420D
const FAST_MASTER_CYCLES: u64 = 6;

// Accesses to WRAM and the cartridge are "slow" (+2 master cycles)
const SLOW_MASTER_CYCLES: u64 = 8;

// Accesses to the manual joypad read ports are "extra slow" (+6 master cycles)
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
    pub apu: &'a mut Apu,
    pub access_master_cycles: u64,
}

impl<'a> Bus<'a> {
    fn read_system_area(&mut self, full_address: u32) -> u8 {
        let address = full_address & 0x7FFF;
        match address {
            0x0000..=0x1FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // First 8KB of WRAM
                self.memory.read_wram(address)
            }
            0x2100..=0x213F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // PPU ports
                self.ppu.read_port(address).unwrap_or(self.memory.cpu_open_bus())
            }
            0x2140..=0x217F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // APU ports
                self.apu.read_port(address)
            }
            0x2180 => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // WMDATA: WRAM port in address bus B
                self.memory.read_wram_port()
            }
            0x4000..=0x41FF => {
                self.access_master_cycles = XSLOW_MASTER_CYCLES;

                // $4016 and $4017 are CPU I/O ports (manual joypad ports)
                // The rest of this range is CPU open bus with XSlow memory speed
                let cpu_open_bus = self.memory.cpu_open_bus();
                self.cpu_registers.read_register(address, cpu_open_bus).unwrap_or_else(|| {
                    self.memory.read_cartridge(full_address).unwrap_or(cpu_open_bus)
                })
            }
            0x4200..=0x5FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // CPU I/O ports (everything except manual joypad ports)
                // $4220-$42FF and $4380-$5FFF are CPU open bus with Fast memory speed
                let cpu_open_bus = self.memory.cpu_open_bus();
                self.cpu_registers.read_register(address, cpu_open_bus).unwrap_or_else(|| {
                    self.memory.read_cartridge(full_address).unwrap_or(cpu_open_bus)
                })
            }
            0x2000..=0x20FF | 0x2181..=0x3FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // Open bus with Fast memory speed
                // Send to the cartridge first because some cartridges respond to these addresses
                self.memory.read_cartridge(full_address).unwrap_or(self.memory.cpu_open_bus())
            }
            0x6000..=0x7FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // Open bus with Slow memory speed
                // Send to the cartridge first because some cartridges respond to these addresses
                self.memory.read_cartridge(full_address).unwrap_or(self.memory.cpu_open_bus())
            }
            _ => panic!("invalid system area address: {full_address:06X}"),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_system_area(&mut self, full_address: u32, value: u8) {
        let address = full_address & 0x7FFF;
        match address {
            0x0000..=0x1FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // First 8KB of WRAM
                self.memory.write_wram(address, value);
            }
            0x2000..=0x20FF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // Open bus; do nothing (no coprocessors use this range)
            }
            0x2100..=0x213F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // PPU ports
                self.ppu.write_port(address, value);
            }
            0x2140..=0x217F => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // APU ports
                self.apu.write_port(address, value);
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
            0x2184..=0x21FF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // Open bus in address bus B; do nothing
            }
            0x2200..=0x3FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // Normally open bus; some coprocessors map I/O ports or RAM to this range
                self.memory.write_cartridge(full_address, value);
            }
            0x4000..=0x41FF => {
                self.access_master_cycles = XSLOW_MASTER_CYCLES;

                // CPU I/O ports (manual joypad ports)
                self.cpu_registers.write_register(address, value);
            }
            0x4200..=0x43FF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // CPU I/O ports (everything except manual joypad ports)
                self.cpu_registers.write_register(address, value);
            }
            0x4400..=0x5FFF => {
                self.access_master_cycles = FAST_MASTER_CYCLES;

                // Normally open bus; some coprocessors map I/O ports to this range
                self.memory.write_cartridge(full_address, value);
            }
            0x6000..=0x7FFF => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // Cartridge expansion
                self.memory.write_cartridge(full_address, value);
            }
            _ => unreachable!("value & 0x7FFF is always <= 0x7FFF"),
        }
    }
}

impl<'a> BusInterface for Bus<'a> {
    #[inline]
    fn read(&mut self, address: u32) -> u8 {
        log::trace!("Bus read {address:06X}");

        let bank = (address >> 16) as u8;
        let offset = address as u16;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x7FFF) => {
                // System area
                self.read_system_area(address)
            }
            (0x00..=0x3F, 0x8000..=0xFFFF) | (0x40..=0x7D, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // Cartridge (Memory-1)
                self.memory.read_cartridge(address).unwrap_or(self.memory.cpu_open_bus())
            }
            (0x80..=0xBF, 0x8000..=0xFFFF) | (0xC0..=0xFF, _) => {
                self.access_master_cycles = self.cpu_registers.memory_2_speed().master_cycles();

                // Cartridge (Memory-2)
                self.memory.read_cartridge(address).unwrap_or(self.memory.cpu_open_bus())
            }
            (0x7E..=0x7F, _) => {
                self.access_master_cycles = SLOW_MASTER_CYCLES;

                // WRAM
                self.memory.read_wram(address)
            }
        }
    }

    #[inline]
    fn write(&mut self, address: u32, value: u8) {
        log::trace!("Bus write {address:06X} {value:02X}");

        let bank = (address >> 16) as u8;
        let offset = address as u16;
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
        }
    }

    #[inline]
    fn idle(&mut self) {
        self.access_master_cycles = FAST_MASTER_CYCLES;
    }

    #[inline]
    fn nmi(&self) -> bool {
        self.cpu_registers.nmi_pending()
    }

    #[inline]
    fn acknowledge_nmi(&mut self) {
        self.cpu_registers.acknowledge_nmi();
    }

    #[inline]
    fn irq(&self) -> bool {
        self.cpu_registers.irq_pending() || self.memory.cartridge_irq()
    }

    #[inline]
    fn halt(&self) -> bool {
        false
    }

    #[inline]
    fn reset(&self) -> bool {
        false
    }
}
