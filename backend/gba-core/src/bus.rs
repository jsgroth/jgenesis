use crate::control::ControlRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use arm7tdmi_emu::bus::BusInterface;

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
    pub control: &'a mut ControlRegisters,
}

impl Bus<'_> {
    fn read_io_register(&mut self, address: u32) -> u16 {
        match address {
            0x04000000..=0x04000056 => self.ppu.read_register(address),
            // TODO joypad registers
            0x04000130 => !0,
            _ => todo!("I/O register read {address:08X}"),
        }
    }

    fn write_io_register(&mut self, address: u32, value: u16) {
        match address {
            0x04000000..=0x04000056 => self.ppu.write_register(address, value),
            0x04000208 => self.control.write_ime(value.into()),
            _ => todo!("I/O register write {address:08X} {value:04X}"),
        }
    }

    // TODO maybe collapse this to not be as separate from the u16 version
    fn write_io_register_u32(&mut self, address: u32, value: u32) {
        match address {
            0x04000000..=0x004000056 => {
                self.ppu.write_register(address, value as u16);
                self.ppu.write_register(address + 2, (value >> 16) as u16);
            }
            0x04000208 => self.control.write_ime(value),
            _ => todo!("I/O register write {address:08X} {value:08X}"),
        }
    }
}

impl BusInterface for Bus<'_> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                self.memory.cartridge.read_rom_byte(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_byte(address),
            _ => todo!("read byte {address:08X}"),
        }
    }

    #[inline]
    fn read_halfword(&mut self, address: u32) -> u16 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                self.memory.cartridge.read_rom_halfword(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_halfword(address),
            MMIO_START..=MMIO_END => self.read_io_register(address),
            _ => todo!("read halfword {address:08X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u32 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                self.memory.cartridge.read_rom_word(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_word(address),
            _ => todo!("read word {address:08X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_byte(address, value),
            _ => todo!("write byte {address:08X} {value:02X}"),
        }
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_halfword(address, value),
            MMIO_START..=MMIO_END => self.write_io_register(address, value),
            VRAM_START..=VRAM_END => self.ppu.write_vram_halfword(address, value),
            PALETTES_START..=PALETTES_END => self.ppu.write_palette_halfword(address, value),
            _ => todo!("write halfword {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_word(address, value),
            EWRAM_START..=EWRAM_END => self.memory.write_ewram_word(address, value),
            MMIO_START..=MMIO_END => self.write_io_register_u32(address, value),
            VRAM_START..=VRAM_END => {
                self.ppu.write_vram_halfword(address & !3, value as u16);
                self.ppu.write_vram_halfword(address | 2, (value >> 16) as u16);
            }
            _ => todo!("write word {address:08X} {value:08X}"),
        }
    }
}
