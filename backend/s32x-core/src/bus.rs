//! 32X memory mapping for the 68000 and SH-2s

use crate::cartridge::Cartridge;
use crate::core::{Sdram, Sega32X};
use crate::pwm::PwmChip;
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

macro_rules! m68k_vector_rom {
    () => {
        0x000000..=0x0000FF
    };
}

macro_rules! m68k_writable_vector {
    () => {
        0x000070..=0x000073
    };
}

macro_rules! m68k_cartridge {
    () => {
        0x000100..=0x3FFFFF
    };
}

macro_rules! m68k_frame_buffer {
    () => {
        0x840000..=0x85FFFF
    };
}

macro_rules! m68k_overwrite_image {
    () => {
        0x860000..=0x87FFFF
    };
}

// Matches both the normal frame buffer range and the overwrite image range
macro_rules! m68k_frame_buffer_combined {
    () => {
        0x840000..=0x87FFFF
    };
}

macro_rules! m68k_rom_first_512kb {
    () => {
        0x880000..=0x8FFFFF
    };
}

macro_rules! m68k_mappable_rom_bank {
    () => {
        0x900000..=0x9FFFFF
    };
}

macro_rules! m68k_system_registers {
    () => {
        0xA15100..=0xA1512F
    };
}

macro_rules! m68k_pwm_registers {
    () => {
        0xA15130..=0xA1513F
    };
}

macro_rules! m68k_vdp_registers {
    () => {
        0xA15180..=0xA1518F
    };
}

macro_rules! m68k_cram {
    () => {
        0xA15200..=0xA153FF
    };
}

macro_rules! word_to_byte {
    ($address:expr, $($op:tt)*) => {
        {
            let word = $($op)*($address & !1);
            if !$address.bit(0) { word.msb() } else { word.lsb() }
        }
    }
}

const M68K_RAM_REGISTER: u32 = 0xA130F1;
const M68K_32X_ID_HIGH: u32 = 0xA130EC;
const M68K_32X_ID_LOW: u32 = 0xA130EE;

// 68000 memory map
impl PhysicalMedium for Sega32X {
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            m68k_vector_rom!() => {
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    self.m68k_vectors[address as usize]
                } else {
                    self.cartridge.read_byte(address)
                }
            }
            m68k_cartridge!() => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                if !self.registers.adapter_enabled || self.registers.dma.rom_to_vram_dma {
                    self.cartridge.read_byte(address)
                } else {
                    log::warn!("ROM byte read with RV=0: {address:06X}");
                    0xFF
                }
            }
            m68k_frame_buffer_combined!() => {
                if self.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.vdp.read_frame_buffer)
                } else {
                    log::warn!("Frame buffer byte read with FM=1: {address:06X}");
                    0xFF
                }
            }
            m68k_rom_first_512kb!() => {
                // First 512KB of ROM
                self.cartridge.read_byte(address & 0x7FFFF)
            }
            m68k_mappable_rom_bank!() => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_byte(rom_addr)
            }
            M68K_RAM_REGISTER => self.cartridge.read_ram_register(),
            m68k_system_registers!() => {
                // System registers
                log::trace!("M68K read byte {address:06X}");
                word_to_byte!(address, self.registers.m68k_read)
            }
            m68k_vdp_registers!() => {
                // 32X VDP registers
                log::trace!("M68K read byte {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read while FM=1: {address:06X}");
                    0xFF
                }
            }
            _ => todo!("read byte {address:06X}"),
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            m68k_vector_rom!() => {
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(self.m68k_vectors[address..address + 2].try_into().unwrap())
                } else {
                    self.cartridge.read_word(address)
                }
            }
            m68k_cartridge!() => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                if !self.registers.adapter_enabled || self.registers.dma.rom_to_vram_dma {
                    self.cartridge.read_word(address)
                } else {
                    log::warn!("ROM word read with RV=0: {address:06X}");
                    0xFFFF
                }
            }
            m68k_frame_buffer_combined!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_frame_buffer(address)
                } else {
                    log::warn!("Frame buffer word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            m68k_rom_first_512kb!() => {
                // First 512KB of ROM
                self.cartridge.read_word(address & 0x7FFFF)
            }
            m68k_mappable_rom_bank!() => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_word(rom_addr)
            }
            m68k_system_registers!() => {
                // System registers
                log::trace!("M68K read word {address:06X}");
                self.registers.m68k_read(address)
            }
            m68k_vdp_registers!() => {
                // 32X VDP registers
                log::trace!("M68K read word {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            m68k_cram!() => {
                // 32X CRAM
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_cram(address)
                } else {
                    log::warn!("CRAM word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            // 32X ID - "MARS"
            M68K_32X_ID_HIGH => u16::from_be_bytes([b'M', b'A']),
            M68K_32X_ID_LOW => u16::from_be_bytes([b'R', b'S']),
            _ => todo!("read word {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        if !self.registers.dma.rom_to_vram_dma {
            log::warn!("Cartridge read for DMA with RV=0 {address:06X}");
            // return 0xFFFF;
        }

        if !(0x000000..=0x3FFFFF).contains(&address) {
            log::warn!("VDP DMA read from an invalid address {address:06X}");
            return 0xFFFF;
        }

        self.cartridge.read_word(address)
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            m68k_writable_vector!() => {
                self.m68k_vectors[address as usize] = value;
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            m68k_cartridge!() => self.cartridge.write_byte(address, value),
            m68k_frame_buffer!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_frame_buffer_byte(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:02X}");
                }
            }
            m68k_overwrite_image!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.frame_buffer_overwrite_byte(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=1: {address:06X} {value:02X}"
                    );
                }
            }
            M68K_RAM_REGISTER => self.cartridge.write_ram_register(value),
            m68k_system_registers!() => {
                log::trace!("M68K write byte {address:06X} {value:02X}");
                self.registers.m68k_write_byte(address, value);
            }
            m68k_vdp_registers!() => {
                log::trace!("M68K write byte {address:06X} {value:02X}");

                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:02X}");
                }
            }
            m68k_rom_first_512kb!() => {
                self.cartridge.write_byte(address & 0x7FFFF, value);
            }
            m68k_mappable_rom_bank!() => {
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.write_byte(rom_addr, value);
            }
            0x000000..=0x00006F | 0x000074..=0x0000FF => {
                log::warn!("M68K write to invalid address {address:06X} {value:02X}");
            }
            _ => todo!("M68K write byte {address:06X} {value:02X}"),
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            m68k_writable_vector!() => {
                self.m68k_vectors[address as usize] = value.msb();
                self.m68k_vectors[(address + 1) as usize] = value.lsb();
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            m68k_frame_buffer!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_frame_buffer(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:04X}");
                }
            }
            m68k_overwrite_image!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=1: {address:06X} {value:04X}"
                    );
                }
            }
            m68k_system_registers!() => {
                // System registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                self.registers.m68k_write(address, value);
            }
            m68k_pwm_registers!() => {
                // PWM registers
                log::trace!("M68K PWM register write {address:06X} {value:04X}");
                self.pwm.m68k_write_register(address, value);
            }
            m68k_vdp_registers!() => {
                // VDP registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:04X}");
                }
            }
            m68k_cram!() => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=1: {address:06X} {value:04X}");
                }
            }
            m68k_rom_first_512kb!() => {
                // TODO RAM
                log::warn!("M68K cartridge ROM write {address:06X} {value:04X}");
            }
            m68k_mappable_rom_bank!() => {
                // TODO RAM
                log::warn!(
                    "M68K cartridge ROM write {address:06X} {value:04X}, bank {}",
                    self.registers.m68k_rom_bank
                );
            }
            0x000000..=0x00006F | 0x000074..=0x0000FF => {
                log::warn!("M68K write to invalid address {address:06X} {value:04X}");
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
    pub pwm: &'a mut PwmChip,
    pub registers: &'a mut SystemRegisters,
    pub sdram: &'a mut Sdram,
}

macro_rules! sh2_boot_rom {
    () => {
        0x00000000..=0x00003FFF
    };
}

macro_rules! sh2_system_registers {
    () => {
        0x00004000..=0x0000402F
    };
}

macro_rules! sh2_pwm_registers {
    () => {
        0x00004030..=0x0000403F
    };
}

macro_rules! sh2_vdp_registers {
    () => {
        0x00004100..=0x000041FF
    };
}

macro_rules! sh2_cram {
    () => {
        0x00004200..=0x000043FF
    };
}

macro_rules! sh2_cartridge {
    () => {
        0x02000000..=0x023FFFFF
    };
}

macro_rules! sh2_frame_buffer {
    () => {
        0x04000000..=0x0401FFFF
    };
}

macro_rules! sh2_overwrite_image {
    () => {
        0x04020000..=0x0403FFFF
    };
}

// Matches both the normal frame buffer range and the overwrite image range
macro_rules! sh2_frame_buffer_combined {
    () => {
        0x04000000..=0x0403FFFF
    };
}

macro_rules! sh2_sdram {
    () => {
        0x06000000..=0x0603FFFF
    };
}

macro_rules! sh2_invalid_address {
    () => {
        0x08000000..=0x1FFFFFFF
    };
}

impl<'a> BusInterface for Sh2Bus<'a> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            sh2_sdram!() => {
                let word = self.sdram[((address & SDRAM_MASK) >> 1) as usize];
                if !address.bit(0) { word.msb() } else { word.lsb() }
            }
            sh2_boot_rom!() => read_u8(self.boot_rom, self.boot_rom_mask, address),
            sh2_system_registers!() => {
                log::trace!("SH-2 {:?} read byte {address:08X}", self.which);
                let value = self.registers.sh2_read(address & !1, self.which, self.vdp);
                if !address.bit(0) { value.msb() } else { value.lsb() }
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    word_to_byte!(address, self.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_cram(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    log::warn!("CRAM byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            sh2_cartridge!() => self.cartridge.read_byte(address & 0x3FFFFF),
            0x02400000..=0x027FFFFF => {
                // Kolibri sometimes reads from these addresses
                // Maybe they mirror cartridge ROM?
                log::warn!("SH-2 {:?} invalid address byte read {address:08X}", self.which);
                0xFF
            }
            sh2_frame_buffer_combined!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    word_to_byte!(address, self.vdp.read_frame_buffer)
                } else {
                    log::warn!("Frame buffer byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            sh2_pwm_registers!() => {
                word_to_byte!(address, self.pwm.read_register)
            }
            _ => todo!("SH-2 {:?} read byte {address:08X}", self.which),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            sh2_sdram!() => self.sdram[((address & SDRAM_MASK) >> 1) as usize],
            sh2_boot_rom!() => read_u16(self.boot_rom, self.boot_rom_mask, address),
            sh2_system_registers!() => {
                log::trace!("SH-2 {:?} read word {address:08X}", self.which);
                self.registers.sh2_read(address, self.which, self.vdp)
            }
            sh2_pwm_registers!() => {
                log::trace!("SH-2 {:?} PWM register read {address:08X}", self.which);
                self.pwm.read_register(address)
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_cram(address)
                } else {
                    log::warn!("CRAM word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            sh2_cartridge!() => self.cartridge.read_word(address & 0x3FFFFF),
            sh2_frame_buffer_combined!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_frame_buffer(address)
                } else {
                    log::warn!("Frame buffer word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            0x04040000..=0x05FFFFFF => {
                log::warn!("SH-2 {:?} invalid address word read {address:08X}", self.which);
                0xFFFF
            }
            _ => todo!("SH-2 {:?} read word {address:08X}", self.which),
        }
    }

    #[inline]
    fn read_longword(&mut self, address: u32) -> u32 {
        match address {
            sh2_sdram!() => {
                let word_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                let high_word = self.sdram[word_addr];
                let low_word = self.sdram[word_addr | 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            }
            sh2_boot_rom!() => read_u32(self.boot_rom, self.boot_rom_mask, address),
            sh2_system_registers!() => {
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read longword {address:08X}", self.which);
                }
                let high = self.registers.sh2_read(address, self.which, self.vdp);
                let low = self.registers.sh2_read(address | 2, self.which, self.vdp);
                (u32::from(high) << 16) | u32::from(low)
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_register(address);
                    let low_word = self.vdp.read_register(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("VDP register longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_cram(address);
                    let low_word = self.vdp.read_cram(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("CRAM longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            sh2_cartridge!() => self.cartridge.read_longword(address & 0x3FFFFF),
            sh2_frame_buffer_combined!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_frame_buffer(address);
                    let low_word = self.vdp.read_frame_buffer(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("Frame buffer longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            0x04040000..=0x05FFFFFF => {
                log::warn!("SH-2 {:?} invalid address longword read {address:08X}", self.which);
                0xFFFFFFFF
            }
            _ => todo!("SH-2 {:?} read longword {address:08X}", self.which),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            sh2_sdram!() => {
                let word_addr = ((address & SDRAM_MASK) >> 1) as usize;
                if !address.bit(0) {
                    self.sdram[word_addr].set_msb(value);
                } else {
                    self.sdram[word_addr].set_lsb(value);
                }
            }
            sh2_system_registers!() => {
                log::trace!("SH-2 {:?} byte write {address:08X} {value:02X}", self.which);
                self.registers.sh2_write_byte(address, value, self.which, self.vdp);
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:02X}");
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram_byte(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:02X}");
                }
            }
            sh2_frame_buffer!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    // Treat write as an overwrite because 0 bytes are never written to the frame buffer
                    self.vdp.write_frame_buffer_byte(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:02X}");
                }
            }
            sh2_overwrite_image!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_byte(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=0: {address:08X} {value:02X}"
                    );
                }
            }
            sh2_invalid_address!() => {
                log::warn!(
                    "Ignoring SH-2 {:?} byte write to reserved address: {address:08X} {value:02X}",
                    self.which
                );
            }
            0x04040000..=0x0407FFFF => {
                log::warn!("SH-2 invalid address write {address:08X} {value:02X}");
            }
            sh2_boot_rom!() => {
                log::warn!(
                    "Ignoring SH-2 {:?} byte write to boot ROM: {address:08X} {value:02X}",
                    self.which
                );
            }
            _ => todo!("SH-2 {:?} write byte {address:08X} {value:02X}", self.which),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            sh2_sdram!() => {
                self.sdram[((address & SDRAM_MASK) >> 1) as usize] = value;
            }
            sh2_system_registers!() => {
                log::trace!("SH-2 {:?} word write {address:08X} {value:04X}", self.which);
                self.registers.sh2_write(address, value, self.which, self.vdp);
            }
            sh2_pwm_registers!() => {
                log::trace!("SH-2 {:?} PWM register write {address:08X} {value:04X}", self.which);
                self.pwm.sh2_write_register(address, value);
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:04X}");
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:04X}");
                }
            }
            sh2_frame_buffer!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:04X}");
                }
            }
            sh2_overwrite_image!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=0: {address:08X} {value:04X}"
                    );
                }
            }
            _ => todo!("SH-2 {:?} write word {address:08X} {value:04X}", self.which),
        }
    }

    #[inline]
    fn write_longword(&mut self, address: u32, value: u32) {
        match address {
            sh2_sdram!() => {
                let sdram_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                self.sdram[sdram_addr] = (value >> 16) as u16;
                self.sdram[sdram_addr | 1] = value as u16;
            }
            sh2_system_registers!() => {
                log::trace!("SH-2 {:?} longword write {address:08X} {value:08X}", self.which);
                self.registers.sh2_write(address, (value >> 16) as u16, self.which, self.vdp);
                self.registers.sh2_write(address | 2, value as u16, self.which, self.vdp);
            }
            sh2_vdp_registers!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, (value >> 16) as u16);
                    self.vdp.write_register(address | 2, value as u16);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:08X}");
                }
            }
            sh2_cram!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, (value >> 16) as u16);
                    self.vdp.write_cram(address | 2, value as u16);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:08X}");
                }
            }
            sh2_frame_buffer!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer(address, (value >> 16) as u16);
                    self.vdp.write_frame_buffer(address | 2, value as u16);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:08X}");
                }
            }
            sh2_overwrite_image!() => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_word(address, (value >> 16) as u16);
                    self.vdp.frame_buffer_overwrite_word(address | 2, value as u16);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:08X}");
                }
            }
            sh2_pwm_registers!() => {
                self.pwm.sh2_write_register(address, (value >> 16) as u16);
                self.pwm.sh2_write_register(address | 2, value as u16);
            }
            sh2_boot_rom!() => {
                log::warn!(
                    "SH-2 {:?} longword write to boot ROM address: {address:08X} {value:08X}",
                    self.which
                );
            }
            sh2_cartridge!() => {
                log::debug!(
                    "SH-2 {:?} longword write to cartridge address: {address:08X} {value:08X}",
                    self.which
                );
            }
            _ => todo!("SH-2 {:?} write longword {address:08X} {value:08X}", self.which),
        }
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
