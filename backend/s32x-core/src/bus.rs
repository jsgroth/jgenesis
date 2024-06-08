//! 32X memory mapping for the 68000 and SH-2s

use crate::core::{Sdram, Sega32X};
use crate::registers::{Access, SystemRegisters};
use genesis_core::memory::PhysicalMedium;
use genesis_core::GenesisRegion;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::bus::BusInterface;

const M68K_VECTORS: &[u8; 256] = include_bytes!("m68k_vectors.bin");

const SDRAM_MASK: u32 = 0x3FFFF;

// 68000 memory map
impl PhysicalMedium for Sega32X {
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x0000FF => {
                if self.registers.adapter_enabled {
                    M68K_VECTORS[address as usize]
                } else {
                    self.rom.get(address as usize).copied().unwrap_or(0xFF)
                }
            }
            0x000010..=0x3FFFFF => {
                // TODO access only when RV=1 or adapter disabled
                self.rom.get(address as usize).copied().unwrap_or(0xFF)
            }
            0xA15100..=0xA1512F => {
                log::trace!("M68K read byte {address:06X}");
                let word = self.registers.m68k_read(address & !1);
                if !address.bit(0) { word.msb() } else { word.lsb() }
            }
            0xA15180..=0xA1518F => {
                log::trace!("M68K read byte {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    let word = self.vdp.read_register(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            }
            _ => todo!("read byte {address:06X}"),
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x0000FF => {
                if self.registers.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(M68K_VECTORS[address..address + 2].try_into().unwrap())
                } else {
                    self.rom.get_u16(address)
                }
            }
            0x000100..=0x3FFFFF => {
                // TODO access only when RV=1 or adapter disabled
                self.rom.get_u16(address)
            }
            0x880000..=0x8FFFFF => {
                // TODO access only when RV=0
                if self.registers.adapter_enabled {
                    self.rom.get_u16(address & 0x7FFFF)
                } else {
                    0xFF
                }
            }
            0xA15100..=0xA1512F => {
                log::trace!("M68K read word {address:06X}");
                self.registers.m68k_read(address)
            }
            // 32X ID - "MARS"
            0xA130EC => u16::from_be_bytes([b'M', b'A']),
            0xA130EE => u16::from_be_bytes([b'R', b'S']),
            _ => todo!("read word {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        todo!("read word for DMA {address:06X}")
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            0xA15100..=0xA1512F => {
                log::trace!("M68K write byte {address:06X} {value:02X}");

                let mut word = self.registers.m68k_read(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.registers.m68k_write(address & !1, word);
            }
            0xA15180..=0xA1518F => {
                log::trace!("M68K write byte {address:06X} {value:02X}");

                let mut word = self.vdp.read_register(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.vdp.write_register(address & !1, word);
            }
            _ => todo!("M68K write byte {address:06X} {value:02X}"),
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            0x000000..=0x3FFFFF => {
                log::warn!("M68K ROM write: {address:06X} {value:04X}");
            }
            0xA15100..=0xA1512F => {
                // System registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                self.registers.m68k_write(address, value);
            }
            0xA15130..=0xA1513F => {
                // PWM registers
                log::warn!("Ignored PWM register write {address:06X} {value:04X}");
            }
            0xA15180..=0xA1518F => {
                // VDP registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_register(address, value);
                }
            }
            0xA15200..=0xA153FF => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_cram(address, value);
                }
            }
            _ => todo!("M68K write word {address:06X} {value:04X}"),
        }
    }

    fn region(&self) -> GenesisRegion {
        // TODO
        GenesisRegion::Americas
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichCpu {
    Master,
    Slave,
}

// SH-2 memory map
pub struct Sh2Bus<'a> {
    pub boot_rom: &'static [u8],
    pub boot_rom_mask: usize,
    pub which: WhichCpu,
    pub registers: &'a mut SystemRegisters,
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
                log::trace!("SH-2 {:?} read byte {address:08X}", self.which);
                let value = self.registers.sh2_read(address & !1, self.which);
                if !address.bit(0) { value.msb() } else { value.lsb() }
            },
            sdram => {
                let word = self.sdram[((address & SDRAM_MASK) >> 1) as usize];
                if !address.bit(0) { word.msb() } else { word.lsb() }
            },
            _ => todo!("SH-2 read byte {address:08X}")
        })
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        memory_map!(self, address, {
            boot_rom => read_u16(self.boot_rom, self.boot_rom_mask, address),
            system_registers => {
                log::trace!("SH-2 {:?} read word {address:08X}", self.which);
                self.registers.sh2_read(address, self.which)
            },
            sdram => self.sdram[((address & SDRAM_MASK) >> 1) as usize],
            _ => todo!("SH-2 read word {address:08X}"),
        })
    }

    #[inline]
    fn read_longword(&mut self, address: u32) -> u32 {
        memory_map!(self, address, {
            boot_rom => read_u32(self.boot_rom, self.boot_rom_mask, address),
            system_registers => {
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read longword {address:08X}", self.which);
                }
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
        self.registers.reset_sh2
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
