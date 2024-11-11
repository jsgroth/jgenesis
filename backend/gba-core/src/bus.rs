use crate::memory::Memory;
use crate::ppu::Ppu;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};

// $00000000-$00003FFF: BIOS ROM (16KB)
pub const BIOS_START: u32 = 0x00000000;
pub const BIOS_END: u32 = 0x00003FFF;

// $02000000-$0203FFFF: External working RAM (256KB)
pub const EWRAM_START: u32 = 0x02000000;
pub const EWRAM_END: u32 = 0x0203FFFF;

// $03000000-$03007FFF: Internal working RAM (32KB)
pub const IWRAM_START: u32 = 0x03000000;
pub const IWRAM_END: u32 = 0x03007FFF;

// $04000000-$040003FF: Memory-mapped I/O registers
pub const MMIO_START: u32 = 0x04000000;
pub const MMIO_END: u32 = 0x040003FF;

// $05000000-$050003FF: Palette RAM (1KB)
pub const PALETTES_START: u32 = 0x05000000;
pub const PALETTES_END: u32 = 0x050003FF;

// $06000000-$06017FFF: VRAM (96KB)
pub const VRAM_START: u32 = 0x06000000;
pub const VRAM_END: u32 = 0x06017FFF;

// $07000000-$070003FF: OAM (1KB)
pub const OAM_START: u32 = 0x07000000;
pub const OAM_END: u32 = 0x070003FF;

// $08000000-$09FFFFFF: Cartridge ROM (up to 32MB), waitstate config 0
pub const CARTRIDGE_ROM_0_START: u32 = 0x08000000;
pub const CARTRIDGE_ROM_0_END: u32 = 0x09FFFFFF;

// $0A000000-$0BFFFFFF: Cartridge ROM (up to 32MB), waitstate config 1
pub const CARTRIDGE_ROM_1_START: u32 = 0x0A000000;
pub const CARTRIDGE_ROM_1_END: u32 = 0x0BFFFFFF;

// $0C000000-$0DFFFFFF: Cartridge ROM (up to 32MB), waitstate config 2
pub const CARTRIDGE_ROM_2_START: u32 = 0x0C000000;
pub const CARTRIDGE_ROM_2_END: u32 = 0x0DFFFFFF;

// $0E000000-$0E00FFFF: Cartridge SRAM (up to 64KB)
pub const CARTRIDGE_RAM_START: u32 = 0x0E000000;
pub const CARTRIDGE_RAM_END: u32 = 0x0E00FFFF;

pub struct Bus<'a> {
    pub ppu: &'a mut Ppu,
    pub memory: &'a mut Memory,
}

impl Bus<'_> {
    fn write_io_register(&mut self, address: u32, value: u16) {
        match address {
            0x04000000..=0x04000056 => self.ppu.write_register(address, value),
            _ => todo!("I/O register write {address:08X} {value:04X}"),
        }
    }
}

impl BusInterface for Bus<'_> {
    #[inline]
    fn read_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8 {
        todo!("read byte {address:08X}")
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                self.memory.cartridge.read_rom_halfword(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_halfword(address),
            _ => todo!("read halfword {address:08X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                self.memory.cartridge.read_rom_word(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_word(address),
            _ => todo!("read word {address:08X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle) {
        todo!("write byte {address:08X} {value:02X}")
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_halfword(address, value),
            MMIO_START..=MMIO_END => self.write_io_register(address, value),
            VRAM_START..=VRAM_END => self.ppu.write_vram_halfword(address, value),
            _ => todo!("write halfword {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, cycle: MemoryCycle) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_word(address, value),
            _ => todo!("write word {address:08X} {value:08X}"),
        }
    }

    #[inline]
    fn access_cycles(&self) -> u32 {
        // TODO
        0
    }
}
