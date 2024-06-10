//! 32X memory mapping for the 68000 and SH-2s

use crate::cartridge::Cartridge;
use crate::core::{Sdram, Sega32X};
use crate::registers::{Access, SystemRegisters};
use crate::vdp::Vdp;
use genesis_core::memory::PhysicalMedium;
use genesis_core::GenesisRegion;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::bus::BusInterface;

const H_INT_VECTOR_ADDR: usize = 0x000070;

const SDRAM_MASK: u32 = 0x3FFFF;

impl Sega32X {
    fn h_int_vector(&self) -> u32 {
        u32::from_be_bytes(
            self.m68k_vectors[H_INT_VECTOR_ADDR..H_INT_VECTOR_ADDR + 4].try_into().unwrap(),
        )
    }
}

// 68000 memory map
impl PhysicalMedium for Sega32X {
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x0000FF => {
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    self.m68k_vectors[address as usize]
                } else {
                    self.cartridge.read_byte(address)
                }
            }
            0x000100..=0x3FFFFF => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                if !self.registers.adapter_enabled || self.registers.dma.rom_to_vram_dma {
                    self.cartridge.read_byte(address)
                } else {
                    0xFF
                }
            }
            0x840000..=0x87FFFF => {
                if self.registers.vdp_access == Access::M68k {
                    let word = self.vdp.read_frame_buffer(address);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            }
            0x880000..=0x8FFFFF => {
                // First 512KB of ROM
                self.cartridge.read_byte(address & 0x7FFFF)
            }
            0x900000..=0x9FFFFF => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_byte(rom_addr)
            }
            0xA130F1 => self.cartridge.read_ram_register(),
            0xA15100..=0xA1512F => {
                // System registers
                if log::log_enabled!(log::Level::Trace) && !(0xA15120..=0xA1512F).contains(&address)
                {
                    log::trace!("M68K read byte {address:06X}");
                }
                let word = self.registers.m68k_read(address & !1);
                if !address.bit(0) { word.msb() } else { word.lsb() }
            }
            0xA15180..=0xA1518F => {
                // 32X VDP registers
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
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(self.m68k_vectors[address..address + 2].try_into().unwrap())
                } else {
                    self.cartridge.read_word(address)
                }
            }
            0x000100..=0x3FFFFF => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                if !self.registers.adapter_enabled || self.registers.dma.rom_to_vram_dma {
                    self.cartridge.read_word(address)
                } else {
                    0xFFFF
                }
            }
            0x840000..=0x87FFFF => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_frame_buffer(address)
                } else {
                    0xFFFF
                }
            }
            0x880000..=0x8FFFFF => {
                // First 512KB of ROM
                self.cartridge.read_word(address & 0x7FFFF)
            }
            0x900000..=0x9FFFFF => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_word(rom_addr)
            }
            0xA15100..=0xA1512F => {
                // System registers
                if log::log_enabled!(log::Level::Trace) && !(0xA15120..=0xA1512F).contains(&address)
                {
                    log::trace!("M68K read word {address:06X}");
                }
                self.registers.m68k_read(address)
            }
            0xA15180..=0xA1518F => {
                // 32X VDP registers
                log::trace!("M68K read word {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_register(address)
                } else {
                    0xFFFF
                }
            }
            0xA15200..=0xA153FF => {
                // 32X CRAM
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_cram(address)
                } else {
                    0xFFFF
                }
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
            0x000070..=0x000073 => {
                self.m68k_vectors[address as usize] = value;
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            0x000100..=0x3FFFFF => self.cartridge.write_byte(address, value),
            0x840000..=0x85FFFF => {
                if value != 0 && self.registers.vdp_access == Access::M68k {
                    let mut word = self.vdp.read_frame_buffer(address & !1);
                    if !address.bit(0) {
                        word.set_msb(value)
                    } else {
                        word.set_lsb(value)
                    };
                    self.vdp.write_frame_buffer(address & !1, word);
                }
            }
            0x860000..=0x87FFFF => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.frame_buffer_overwrite_byte(address, value);
                }
            }
            0xA130F1 => self.cartridge.write_ram_register(value),
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
            0x000070 | 0x000072 => {
                self.m68k_vectors[address as usize] = value.msb();
                self.m68k_vectors[(address + 1) as usize] = value.lsb();
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            0x840000..=0x85FFFF => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_frame_buffer(address, value);
                }
            }
            0x860000..=0x87FFFF => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                }
            }
            0x900000..=0x9FFFFF => {
                log::warn!("Write to ROM {address:06X} {value:04X}");
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
    pub cartridge: &'a Cartridge,
    pub vdp: &'a mut Vdp,
    pub registers: &'a mut SystemRegisters,
    pub sdram: &'a mut Sdram,
}

macro_rules! memory_map {
    ($self:expr, $address:expr, {
        boot_rom => $boot_rom:expr,
        system_registers => $system_registers:expr,
        vdp => $vdp:expr,
        cram => $cram:expr,
        cartridge => $cartridge:expr,
        frame_buffer => $frame_buffer:expr,
        frame_buffer_overwrite => $frame_buffer_overwrite:expr,
        sdram => $sdram:expr,
        _ => $default:expr $(,)?
    }) => {
        match $address {
            0x00000000..=0x00003FFF => $boot_rom,
            0x00004000..=0x000040FF => $system_registers,
            0x00004100..=0x000041FF => $vdp,
            0x00004200..=0x000043FF => $cram,
            0x02000000..=0x023FFFFF => $cartridge,
            0x04000000..=0x0401FFFF => $frame_buffer,
            0x04020000..=0x0403FFFF => $frame_buffer_overwrite,
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
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read byte {address:08X}", self.which);
                }
                let value = self.registers.sh2_read(address & !1, self.which, self.vdp);
                if !address.bit(0) { value.msb() } else { value.lsb() }
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_register(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_cram(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            },
            cartridge => {
                self.cartridge.read_byte(address & 0x3FFFFF)
            },
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_frame_buffer(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            },
            frame_buffer_overwrite => {
                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_frame_buffer(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    0xFF
                }
            },
            sdram => {
                let word = self.sdram[((address & SDRAM_MASK) >> 1) as usize];
                if !address.bit(0) { word.msb() } else { word.lsb() }
            },
            _ => todo!("SH-2 {:?} read byte {address:08X}", self.which)
        })
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        memory_map!(self, address, {
            boot_rom => read_u16(self.boot_rom, self.boot_rom_mask, address),
            system_registers => {
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read word {address:08X}", self.which);
                }
                self.registers.sh2_read(address, self.which, self.vdp)
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_register(address)
                } else {
                    0xFFFF
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_cram(address)
                } else {
                    0xFFFF
                }
            },
            cartridge => self.cartridge.read_word(address & 0x3FFFFF),
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_frame_buffer(address)
                } else {
                    0xFFFF
                }
            },
            frame_buffer_overwrite => todo!("FB overwrite word read {address:08X}"),
            sdram => self.sdram[((address & SDRAM_MASK) >> 1) as usize],
            _ => todo!("SH-2 {:?} read word {address:08X}", self.which),
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
                let high = self.registers.sh2_read(address, self.which, self.vdp);
                let low = self.registers.sh2_read(address | 2, self.which, self.vdp);
                (u32::from(high) << 16) | u32::from(low)
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_register(address);
                    let low_word = self.vdp.read_register(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    0xFFFFFFFF
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_cram(address);
                    let low_word = self.vdp.read_cram(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    0xFFFFFFFF
                }
            },
            cartridge => self.cartridge.read_longword(address & 0x3FFFFF),
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_frame_buffer(address);
                    let low_word = self.vdp.read_frame_buffer(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    0xFFFFFFFF
                }
            },
            frame_buffer_overwrite => todo!("FB overwrite longword read {address:08X}"),
            sdram => {
                let word_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                let high_word = self.sdram[word_addr];
                let low_word = self.sdram[word_addr | 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            },
            _ => todo!("SH-2 {:?} read longword {address:08X}", self.which)
        })
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        memory_map!(self, address, {
            boot_rom => {},
            system_registers => {
                log::trace!("SH-2 {:?} byte write {address:08X} {value:02X}", self.which);
                let mut word = self.registers.sh2_read(address & !1, self.which, self.vdp);
                if !address.bit(0) { word.set_msb(value) } else { word.set_lsb(value) };
                self.registers.sh2_write(address & !1, word, self.which, self.vdp);
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    let mut word = self.vdp.read_register(address & !1);
                    if !address.bit(0) { word.set_msb(value) } else { word.set_lsb(value) };
                    self.vdp.write_register(address & !1, word);
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    let mut word = self.vdp.read_cram(address & !1);
                    if !address.bit(0) { word.set_msb(value) } else { word.set_lsb(value) };
                    self.vdp.write_cram(address & !1, word);
                }
            },
            cartridge => {},
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    // Treat write as an overwrite because 0 bytes are never written to the frame buffer
                    self.vdp.frame_buffer_overwrite_byte(address, value);
                } else {
                    log::warn!("Frame buffer write without access: {address:08X} {value:02X}");
                }
            },
            frame_buffer_overwrite => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_byte(address, value);
                } else {
                    log::warn!("Frame buffer write without access: {address:08X} {value:02X}");
                }
            },
            sdram => {
                let word_addr = ((address & SDRAM_MASK) >> 1) as usize;
                if !address.bit(0) {
                    self.sdram[word_addr].set_msb(value);
                } else {
                    self.sdram[word_addr].set_lsb(value);
                }
            },
            _ => todo!("SH-2 {:?} write byte {address:08X} {value:02X}", self.which)
        });
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        memory_map!(self, address, {
            boot_rom => {},
            system_registers => {
                log::trace!("SH-2 {:?} word write {address:08X} {value:04X}", self.which);
                self.registers.sh2_write(address, value, self.which, self.vdp);
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, value);
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, value);
                }
            },
            cartridge => {},
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer(address, value);
                } else {
                    log::warn!("Frame buffer write without access: {address:08X} {value:04X}");
                }
            },
            frame_buffer_overwrite => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!("Frame buffer write without access: {address:08X} {value:04X}");
                }
            },
            sdram => {
                self.sdram[((address & SDRAM_MASK) >> 1) as usize] = value;
            },
            _ => todo!("SH-2 {:?} write word {address:08X} {value:04X}", self.which)
        });
    }

    #[inline]
    fn write_longword(&mut self, address: u32, value: u32) {
        memory_map!(self, address, {
            boot_rom => {},
            system_registers => {
                log::trace!("SH-2 {:?} longword write {address:08X} {value:08X}", self.which);
                self.registers.sh2_write(address, (value >> 16) as u16, self.which, self.vdp);
                self.registers.sh2_write(address | 2, value as u16, self.which, self.vdp);
            },
            vdp => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, (value >> 16) as u16);
                    self.vdp.write_register(address | 2, value as u16);
                }
            },
            cram => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, (value >> 16) as u16);
                    self.vdp.write_cram(address | 2, value as u16);
                }
            },
            cartridge => {},
            frame_buffer => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer(address, (value >> 16) as u16);
                    self.vdp.write_frame_buffer(address | 2, value as u16);
                } else {
                    log::warn!("Frame buffer write without access: {address:08X} {value:08X}");
                }
            },
            frame_buffer_overwrite => todo!("FB overwrite longword write {address:08X}"),
            sdram => {
                let sdram_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                self.sdram[sdram_addr] = (value >> 16) as u16;
                self.sdram[sdram_addr | 1] = value as u16;
            },
            _ => todo!("SH-2 {:?} write longword {address:08X} {value:08X}", self.which)
        });
    }

    #[inline]
    fn reset(&self) -> bool {
        self.registers.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        let interrupts = match self.which {
            WhichCpu::Master => &self.registers.master_interrupts,
            WhichCpu::Slave => &self.registers.slave_interrupts,
        };

        if interrupts.reset_pending {
            14
        } else if interrupts.v_pending {
            12
        } else if interrupts.h_pending {
            10
        } else if interrupts.command_pending && interrupts.command_enabled {
            8
        } else if interrupts.pwm_pending {
            6
        } else {
            0
        }
    }

    #[inline]
    fn dma_request_0(&self) -> bool {
        !self.registers.dma.fifo.is_empty()
    }

    #[inline]
    fn dma_request_1(&self) -> bool {
        false
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
