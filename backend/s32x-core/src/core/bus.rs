use crate::core::registers::Sega32XRegisters;
use sh2_emu::bus::BusInterface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichCpu {
    Master,
    Slave,
}

pub struct Sh2Bus<'a> {
    pub boot_rom: &'static [u8],
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

impl<'a> BusInterface for Sh2Bus<'a> {
    fn read_byte(&mut self, address: u32) -> u8 {
        todo!("SH-2 read byte {address:08X}")
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

fn read_u16(slice: &[u8], address: u32) -> u16 {
    let address = address as usize;
    if address + 2 >= slice.len() {
        return !0;
    }

    u16::from_be_bytes([slice[address], slice[address + 1]])
}

fn read_u32(slice: &[u8], address: u32) -> u32 {
    let address = address as usize;
    if address + 4 >= slice.len() {
        return !0;
    }

    u32::from_be_bytes(slice[address..address + 4].try_into().unwrap())
}
