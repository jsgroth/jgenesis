use crate::core::registers::Sega32XRegisters;
use sh2_emu::bus::BusInterface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichCpu {
    Master,
    Slave,
}

pub struct Sh2Bus<'a, const BOOT_ROM_LEN: usize> {
    pub boot_rom: &'static [u8; BOOT_ROM_LEN],
    pub registers: &'a mut Sega32XRegisters,
    pub which: WhichCpu,
}

macro_rules! memory_map {
    ($self:expr, $address:expr, {
        boot_rom => $boot_rom:expr,
        _ => $default:expr $(,)?
    }) => {
        match $address & 0x1FFFFFFF {
            0x00000000..=0x00003FFF => $boot_rom,
            _ => $default,
        }
    };
}

impl<'a, const BOOT_ROM_LEN: usize> BusInterface for Sh2Bus<'a, BOOT_ROM_LEN> {
    fn read_byte(&mut self, address: u32) -> u8 {
        memory_map!(self, address, {
            boot_rom => read_u8(self.boot_rom, address),
            _ => todo!("SH-2 read byte {address:08X}")
        })
    }

    fn read_word(&mut self, address: u32) -> u16 {
        memory_map!(self, address, {
            boot_rom => read_u16(self.boot_rom, address),
            _ => todo!("SH-2 read word {address:08X}"),
        })
    }

    fn read_longword(&mut self, address: u32) -> u32 {
        memory_map!(self, address, {
            boot_rom => read_u32(self.boot_rom, address),
            _ => todo!("SH-2 read longword {address:08X}")
        })
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        todo!("SH-2 write byte {address:08X} {value:02X}")
    }

    fn write_word(&mut self, address: u32, value: u16) {
        todo!("SH-2 write word {address:08X} {value:04X}")
    }

    fn write_longword(&mut self, address: u32, value: u32) {
        todo!("SH-2 write longword {address:08X} {value:08X}")
    }

    fn reset(&self) -> bool {
        self.registers.system.reset_sh2
    }

    fn interrupt_level(&self) -> u8 {
        // TODO
        0
    }
}

fn read_u8<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u8 {
    slice[(address as usize) & (LEN - 1)]
}

fn read_u16<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u16 {
    let address = (address as usize) & (LEN - 1) & !1;
    u16::from_be_bytes([slice[address], slice[address + 1]])
}

fn read_u32<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u32 {
    let address = (address as usize) & (LEN - 1) & !3;
    u32::from_be_bytes(slice[address..address + 4].try_into().unwrap())
}
