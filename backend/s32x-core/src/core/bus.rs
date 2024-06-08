use crate::core::registers::Sega32XRegisters;
use crate::core::Sdram;
use sh2_emu::bus::BusInterface;

const SDRAM_MASK: u32 = 0x3FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichCpu {
    Master,
    Slave,
}

pub struct Sh2Bus<'a> {
    pub boot_rom: &'static [u8],
    pub boot_rom_mask: usize,
    pub which: WhichCpu,
    pub registers: &'a mut Sega32XRegisters,
    pub sdram: &'a mut Sdram,
}

macro_rules! memory_map {
    ($self:expr, $address:expr, {
        boot_rom => $boot_rom:expr,
        system_registers => $system_registers:expr,
        sdram => $sdram:expr,
        _ => $default:expr $(,)?
    }) => {
        match $address {
            0x00000000..=0x00003FFF => $boot_rom,
            0x00004000..=0x000040FF => $system_registers,
            0x06000000..=0x0603FFFF => $sdram,
            _ => $default,
        }
    };
}

impl<'a> BusInterface for Sh2Bus<'a> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        memory_map!(self, address, {
            boot_rom => read_u8(self.boot_rom, self.boot_rom_mask, address),
            system_registers => {
                let value = self.registers.sh2_read(address, self.which);
                (value >> (8 * ((address & 1) ^ 1))) as u8
            },
            sdram => {
                let word = self.sdram[((address & SDRAM_MASK) >> 1) as usize];
                (word >> (8 * ((address & 1) ^ 1))) as u8
            },
            _ => todo!("SH-2 read byte {address:08X}")
        })
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        memory_map!(self, address, {
            boot_rom => read_u16(self.boot_rom, self.boot_rom_mask, address),
            system_registers => self.registers.sh2_read(address, self.which),
            sdram => self.sdram[((address & SDRAM_MASK) >> 1) as usize],
            _ => todo!("SH-2 read word {address:08X}"),
        })
    }

    #[inline]
    fn read_longword(&mut self, address: u32) -> u32 {
        memory_map!(self, address, {
            boot_rom => read_u32(self.boot_rom, self.boot_rom_mask, address),
            system_registers => {
                let high = self.registers.sh2_read(address, self.which);
                let low = self.registers.sh2_read(address | 2, self.which);
                (u32::from(high) << 16) | u32::from(low)
            },
            sdram => todo!("longword read from SDRAM {address:08X}"),
            _ => todo!("SH-2 read longword {address:08X}")
        })
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        todo!("SH-2 write byte {address:08X} {value:02X}")
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        memory_map!(self, address, {
            boot_rom => {},
            system_registers => todo!("system register word write {address:08X} {value:04X}"),
            sdram => {
                self.sdram[((address & SDRAM_MASK) >> 1) as usize] = value;
            },
            _ => todo!("SH-2 write word {address:08X} {value:04X}")
        });
    }

    #[inline]
    fn write_longword(&mut self, address: u32, value: u32) {
        todo!("SH-2 write longword {address:08X} {value:08X}")
    }

    #[inline]
    fn reset(&self) -> bool {
        self.registers.system.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        // TODO
        0
    }
}

#[inline]
fn read_u8(slice: &[u8], mask: usize, address: u32) -> u8 {
    slice[(address as usize) & mask]
}

#[inline]
fn read_u16(slice: &[u8], mask: usize, address: u32) -> u16 {
    let address = (address as usize) & mask;
    u16::from_be_bytes([slice[address], slice[address + 1]])
}

#[inline]
fn read_u32(slice: &[u8], mask: usize, address: u32) -> u32 {
    let address = (address as usize) & mask;
    u32::from_be_bytes(slice[address..address + 4].try_into().unwrap())
}
