//! 32X memory mapping for the 68000 and SH-2s

use crate::bootrom;
use crate::cartridge::Cartridge;
use crate::core::{Sdram, Sega32X, SerialInterface};
use crate::pwm::PwmChip;
use crate::registers::{Access, SystemRegisters};
use crate::vdp::Vdp;
use genesis_core::GenesisRegion;
use genesis_core::memory::PhysicalMedium;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::bus::BusInterface;
use std::array;

const SDRAM_MASK: u32 = 0x3FFFF;

// $000000-$0000FF: 256-byte fixed vector ROM (except for HINT vector which is R/W)
const M68K_VECTORS_START: u32 = 0x000000;
const M68K_VECTORS_END: u32 = 0x0000FF;

const H_INT_VECTOR_START: u32 = 0x000070;
const H_INT_VECTOR_END: u32 = 0x000073;

// $000100-$3FFFFF: Cartridge
const M68K_CARTRIDGE_START: u32 = 0x000100;
const M68K_CARTRIDGE_END: u32 = 0x3FFFFF;

// $400000-$7FFFFF: Sega CD if connected (?)
const M68K_SEGA_CD_START: u32 = 0x400000;
const M68K_SEGA_CD_END: u32 = 0x7FFFFF;

// $840000-$85FFFF: Frame buffer
const M68K_FRAME_BUFFER_START: u32 = 0x840000;
const M68K_FRAME_BUFFER_END: u32 = 0x85FFFF;

// $860000-$87FFFF: Frame buffer overwrite image
const M68K_OVERWRITE_IMAGE_START: u32 = 0x860000;
const M68K_OVERWRITE_IMAGE_END: u32 = 0x87FFFF;

// $880000-$8FFFFF: First 512KB of cartridge memory
const M68K_FIRST_CART_BANK_START: u32 = 0x880000;
const M68K_FIRST_CART_BANK_END: u32 = 0x8FFFFF;

// $900000-$9FFFFF: Mappable 1MB cartridge bank
const M68K_MAPPABLE_CART_BANK_START: u32 = 0x900000;
const M68K_MAPPABLE_CART_BANK_END: u32 = 0x9FFFFF;

// $A130EC-$A130EF: 32X ID ("MARS")
const M68K_32X_ID_START: u32 = 0xA130EC;
const M68K_32X_ID_END: u32 = 0xA130EF;
const M68K_32X_ID: [u8; 4] = [b'M', b'A', b'R', b'S'];

// $A130F1: Cartridge RAM register
const M68K_CART_RAM_REGISTER_BYTE: u32 = 0xA130F1;
const M68K_CART_RAM_REGISTER_WORD: u32 = 0xA130F0;

// $A130F2-$A130FF: Cartridge bank registers (when using Sega SSF mapper)
const M68K_SSF_BANK_REGISTERS_START: u32 = 0xA130F2;
const M68K_SSF_BANK_REGISTERS_END: u32 = 0xA130FF;

// $A15100-$A1512F: 32X system registers
const M68K_SYSTEM_REGISTERS_START: u32 = 0xA15100;
const M68K_SYSTEM_REGISTERS_END: u32 = 0xA1512F;

// $A15130-$A1513F: PWM registers
const M68K_PWM_START: u32 = 0xA15130;
const M68K_PWM_END: u32 = 0xA1513F;

// $A15180-$A1518F: VDP registers
const M68K_VDP_START: u32 = 0xA15180;
const M68K_VDP_END: u32 = 0xA1518F;

// $A15200-$A153FF: VDP palette RAM
const M68K_CRAM_START: u32 = 0xA15200;
const M68K_CRAM_END: u32 = 0xA153FF;

impl Sega32X {
    fn h_int_vector(&self) -> u32 {
        u32::from_be_bytes(
            self.m68k_vectors[H_INT_VECTOR_START as usize..(H_INT_VECTOR_START + 4) as usize]
                .try_into()
                .unwrap(),
        )
    }
}

macro_rules! word_to_byte {
    ($address:expr, $($op:tt)*) => {
        {
            let word = $($op)*($address & !1);
            if !$address.bit(0) { word.msb() } else { word.lsb() }
        }
    }
}

// 68000 memory map
impl PhysicalMedium for Sega32X {
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            M68K_VECTORS_START..=M68K_VECTORS_END => {
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    self.m68k_vectors[address as usize]
                } else {
                    self.cartridge.read_byte(address)
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                // TODO is this right? some games read from ROM without setting RV=1; allow them to go through
                if self.registers.adapter_enabled && !self.registers.dma.rom_to_vram_dma {
                    log::warn!("ROM byte read with RV=0: {address:06X}");
                }
                self.cartridge.read_byte(address)
            }
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.vdp.read_frame_buffer)
                } else {
                    log::warn!("Frame buffer byte read with FM=1: {address:06X}");
                    0xFF
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                // First 512KB of ROM
                self.cartridge.read_byte(address & 0x7FFFF)
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_byte(rom_addr)
            }
            M68K_CART_RAM_REGISTER_BYTE => self.cartridge.read_ram_register(),
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K read byte {address:06X}");
                word_to_byte!(address, self.registers.m68k_read)
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // 32X VDP registers
                log::trace!("M68K read byte {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read while FM=1: {address:06X}");
                    0xFF
                }
            }
            M68K_PWM_START..=M68K_PWM_END => {
                word_to_byte!(address, self.pwm.read_register)
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                word_to_byte!(address, self.vdp.read_cram)
            }
            M68K_32X_ID_START..=M68K_32X_ID_END => M68K_32X_ID[(address & 3) as usize],
            // TODO Sega CD is mapped here if connected?
            M68K_SEGA_CD_START..=M68K_SEGA_CD_END => {
                log::debug!("Byte read from Sega CD address: {address:06X}");
                0
            }
            _ => {
                log::warn!("M68K byte read from unexpected address: {address:06X}");
                0
            }
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            M68K_VECTORS_START..=M68K_VECTORS_END => {
                // Hardcoded vectors when 32X is enabled, first 256 bytes of ROM otherwise
                if self.registers.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(self.m68k_vectors[address..address + 2].try_into().unwrap())
                } else {
                    self.cartridge.read_word(address)
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                // TODO is this right? some games read from ROM without setting RV=1; allow them to go through
                if self.registers.adapter_enabled && !self.registers.dma.rom_to_vram_dma {
                    log::warn!("ROM word read with RV=0: {address:06X}");
                }
                self.cartridge.read_word(address)
            }
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_frame_buffer(address)
                } else {
                    log::warn!("Frame buffer word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                // First 512KB of ROM
                self.cartridge.read_word(address & 0x7FFFF)
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.read_word(rom_addr)
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K read word {address:06X}");
                self.registers.m68k_read(address)
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // 32X VDP registers
                log::trace!("M68K read word {address:06X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            M68K_PWM_START..=M68K_PWM_END => {
                // PWM registers
                self.pwm.read_register(address)
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                // 32X CRAM
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.read_cram(address)
                } else {
                    log::warn!("CRAM word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            M68K_32X_ID_START..=M68K_32X_ID_END => {
                if !address.bit(1) {
                    u16::from_be_bytes(M68K_32X_ID[0..2].try_into().unwrap())
                } else {
                    u16::from_be_bytes(M68K_32X_ID[2..4].try_into().unwrap())
                }
            }
            M68K_SEGA_CD_START..=M68K_SEGA_CD_END => {
                // TODO Sega CD is mapped here if plugged in
                log::debug!("Invalid word read {address:06X}");
                0
            }
            _ => todo!("read word {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        if !self.registers.dma.rom_to_vram_dma {
            // TODO should these reads be blocked?
            log::debug!("Cartridge read for DMA with RV=0 {address:06X}");
        }

        if !(0x000000..=0x3FFFFF).contains(&address) {
            log::warn!("VDP DMA read from an invalid address {address:06X}");
            return 0xFFFF;
        }

        self.cartridge.read_word(address)
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            H_INT_VECTOR_START..=H_INT_VECTOR_END => {
                self.m68k_vectors[address as usize] = value;
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => self.cartridge.write_byte(address, value),
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_frame_buffer_byte(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:02X}");
                }
            }
            M68K_CART_RAM_REGISTER_BYTE => self.cartridge.write_ram_register(value),
            M68K_SSF_BANK_REGISTERS_START..=M68K_SSF_BANK_REGISTERS_END => {
                self.cartridge.write_mapper_bank_register(address, value);
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                log::trace!("M68K write byte {address:06X} {value:02X}");
                self.registers.m68k_write_byte(address, value);
            }
            M68K_VDP_START..=M68K_VDP_END => {
                log::trace!("M68K write byte {address:06X} {value:02X}");

                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:02X}");
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                self.cartridge.write_byte(address & 0x7FFFF, value);
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                let rom_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.write_byte(rom_addr, value);
            }
            M68K_PWM_START..=M68K_PWM_END => {
                let mut word = self.pwm.read_register(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.pwm.m68k_write_register(address & !1, word);
            }
            M68K_VECTORS_START..=M68K_VECTORS_END | M68K_SEGA_CD_START..=M68K_SEGA_CD_END => {
                log::debug!("M68K write to vector ROM address {address:06X} {value:02X}");
            }
            _ => log::warn!("M68K write byte to invalid address: {address:06X} {value:02X}"),
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            H_INT_VECTOR_START..=H_INT_VECTOR_END => {
                self.m68k_vectors[address as usize] = value.msb();
                self.m68k_vectors[(address + 1) as usize] = value.lsb();
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            M68K_FRAME_BUFFER_START..=M68K_FRAME_BUFFER_END => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_frame_buffer_word(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_OVERWRITE_IMAGE_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=1: {address:06X} {value:04X}"
                    );
                }
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                self.registers.m68k_write(address, value);
            }
            M68K_PWM_START..=M68K_PWM_END => {
                // PWM registers
                log::trace!("M68K PWM register write {address:06X} {value:04X}");
                self.pwm.m68k_write_register(address, value);
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // VDP registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                if self.registers.vdp_access == Access::M68k {
                    self.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                self.cartridge.write_word(address, value);
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                self.cartridge.write_word(address & 0x7FFFF, value);
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                let cart_addr =
                    (u32::from(self.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.cartridge.write_word(cart_addr, value);
            }
            // Not sure this is right but Doom 32X Resurrection writes to this address
            M68K_CART_RAM_REGISTER_WORD => self.cartridge.write_ram_register(value as u8),
            // TODO Sega CD is $400000-$7FFFFF if plugged in
            M68K_VECTORS_START..=M68K_VECTORS_END | M68K_SEGA_CD_START..=M68K_SEGA_CD_END => {
                log::debug!("M68K write to invalid address {address:06X} {value:04X}");
            }
            _ => log::warn!("M68K write word to invalid address: {address:06X} {value:04X}"),
        }
    }

    fn region(&self) -> GenesisRegion {
        self.region
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichCpu {
    Master,
    Slave,
}

// SH-2 memory map
pub struct Sh2Bus<'a> {
    pub which: WhichCpu,
    pub cartridge: &'a mut Cartridge,
    pub vdp: &'a mut Vdp,
    pub pwm: &'a mut PwmChip,
    pub registers: &'a mut SystemRegisters,
    pub sdram: &'a mut Sdram,
    pub serial: &'a mut SerialInterface,
    pub cycle_counter: u64,
}

// $00000000-$00003FFF: Boot ROM
const SH2_BOOT_ROM_START: u32 = 0x00000000;
const SH2_BOOT_ROM_END: u32 = 0x00003FFF;

// $00004000-$0000402F: 32X system registers
const SH2_SYSTEM_REGISTERS_START: u32 = 0x00004000;
const SH2_SYSTEM_REGISTERS_END: u32 = 0x0000402F;

// $00004030-$0000403F: PWM registers
const SH2_PWM_START: u32 = 0x00004030;
const SH2_PWM_END: u32 = 0x0000403F;

// $00004100-$000041FF: VDP registers
const SH2_VDP_START: u32 = 0x00004100;
const SH2_VDP_END: u32 = 0x000041FF;

// $00004200-$000043FF: VDP palette RAM
const SH2_CRAM_START: u32 = 0x00004200;
const SH2_CRAM_END: u32 = 0x000043FF;

// $02000000-$023FFFFF: Cartridge
const SH2_CARTRIDGE_START: u32 = 0x02000000;
const SH2_CARTRIDGE_END: u32 = 0x023FFFFF;

// $04000000-$0401FFFF: Frame buffer
const SH2_FRAME_BUFFER_START: u32 = 0x04000000;
const SH2_FRAME_BUFFER_END: u32 = 0x0401FFFF;

// $04020000-$0403FFFF: Frame buffer overwrite image
const SH2_OVERWRITE_IMAGE_START: u32 = 0x04020000;
const SH2_OVERWRITE_IMAGE_END: u32 = 0x0403FFFF;

// $04040000-$0405FFFF: Frame buffer (mirror)
// The unreleased SoulStar X prototype seems to expect the frame buffer to be mirrored at these addresses
const SH2_FB_MIRROR_START: u32 = 0x04040000;
const SH2_FB_MIRROR_END: u32 = 0x0405FFFF;

// $06000000-$0603FFFF: SDRAM
const SH2_SDRAM_START: u32 = 0x06000000;
const SH2_SDRAM_END: u32 = 0x0603FFFF;

// All values are minus one because every access takes at least 1 cycle
const SH2_CARTRIDGE_CYCLES: u64 = 5;
const SH2_FRAME_BUFFER_READ_CYCLES: u64 = 4;
const SH2_VDP_CYCLES: u64 = 4;
const SH2_SDRAM_READ_CYCLES: u64 = 11;
const SH2_SDRAM_WRITE_CYCLES: u64 = 1;

impl BusInterface for Sh2Bus<'_> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        self.cycle_counter += 1;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                self.cycle_counter += SH2_SDRAM_READ_CYCLES;

                let word = self.sdram[((address & SDRAM_MASK) >> 1) as usize];
                if !address.bit(0) { word.msb() } else { word.lsb() }
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += SH2_CARTRIDGE_CYCLES;

                self.cartridge.read_byte(address & 0x3FFFFF)
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => match self.which {
                WhichCpu::Master => read_u8(bootrom::SH2_MASTER, address),
                WhichCpu::Slave => read_u8(bootrom::SH2_SLAVE, address),
            },
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                log::trace!("SH-2 {:?} read byte {address:08X}", self.which);
                let value =
                    self.registers.sh2_read(address & !1, self.which, self.vdp, self.cycle_counter);
                if !address.bit(0) { value.msb() } else { value.lsb() }
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    word_to_byte!(address, self.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    let word = self.vdp.read_cram(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    log::warn!("CRAM byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FB_MIRROR_END => {
                self.cycle_counter += SH2_FRAME_BUFFER_READ_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    word_to_byte!(address, self.vdp.read_frame_buffer)
                } else {
                    log::warn!("Frame buffer byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            SH2_PWM_START..=SH2_PWM_END => {
                word_to_byte!(address, self.pwm.read_register)
            }
            _ => {
                log::warn!("SH-2 {:?} invalid address byte read {address:08X}", self.which);
                0
            }
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        self.cycle_counter += 1;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                self.cycle_counter += SH2_SDRAM_READ_CYCLES;
                self.sdram[((address & SDRAM_MASK) >> 1) as usize]
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += SH2_CARTRIDGE_CYCLES;
                self.cartridge.read_word(address & 0x3FFFFF)
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => match self.which {
                WhichCpu::Master => read_u16(bootrom::SH2_MASTER, address),
                WhichCpu::Slave => read_u16(bootrom::SH2_SLAVE, address),
            },
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                log::trace!("SH-2 {:?} read word {address:08X}", self.which);
                self.registers.sh2_read(address, self.which, self.vdp, self.cycle_counter)
            }
            SH2_PWM_START..=SH2_PWM_END => {
                log::trace!("SH-2 {:?} PWM register read {address:08X}", self.which);
                self.pwm.read_register(address)
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_cram(address)
                } else {
                    log::warn!("CRAM word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FB_MIRROR_END => {
                self.cycle_counter += SH2_FRAME_BUFFER_READ_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.read_frame_buffer(address)
                } else {
                    log::warn!("Frame buffer word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            0x02400000..=0x027FFFFF => {
                // Not sure what these addresses are; Doom 32X Resurrection reads from them
                // Sega CD maybe?
                log::debug!("Invalid address word read {address:08X}");
                0
            }
            _ => {
                log::warn!("SH-2 {:?} invalid address word read {address:08X}", self.which);
                0
            }
        }
    }

    #[inline]
    fn read_longword(&mut self, address: u32) -> u32 {
        self.cycle_counter += 2;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                // Subtract one because SDRAM access times are not doubled for longword reads
                self.cycle_counter += SH2_SDRAM_READ_CYCLES - 1;

                let word_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                let high_word = self.sdram[word_addr];
                let low_word = self.sdram[word_addr | 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += 2 * SH2_CARTRIDGE_CYCLES;
                self.cartridge.read_longword(address & 0x3FFFFF)
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => match self.which {
                WhichCpu::Master => read_u32(bootrom::SH2_MASTER, address),
                WhichCpu::Slave => read_u32(bootrom::SH2_SLAVE, address),
            },
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read longword {address:08X}", self.which);
                }
                let high =
                    self.registers.sh2_read(address, self.which, self.vdp, self.cycle_counter);
                let low =
                    self.registers.sh2_read(address | 2, self.which, self.vdp, self.cycle_counter);
                (u32::from(high) << 16) | u32::from(low)
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_register(address);
                    let low_word = self.vdp.read_register(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("VDP register longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_cram(address);
                    let low_word = self.vdp.read_cram(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("CRAM longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FB_MIRROR_END => {
                self.cycle_counter += 2 * SH2_FRAME_BUFFER_READ_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    let high_word = self.vdp.read_frame_buffer(address);
                    let low_word = self.vdp.read_frame_buffer(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("Frame buffer longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            _ => {
                log::warn!("SH-2 {:?} invalid address longword read {address:08X}", self.which);
                0
            }
        }
    }

    #[inline]
    fn read_cache_line(&mut self, address: u32) -> [u32; 4] {
        if (SH2_SDRAM_START..=SH2_SDRAM_END).contains(&address) {
            // The SH-2s can read a full 16-byte cache line in 12 cycles
            self.cycle_counter += SH2_SDRAM_READ_CYCLES + 1;

            let base_addr = ((address & SDRAM_MASK) >> 1) as usize;
            return array::from_fn(|i| {
                let high_word = self.sdram[base_addr | (i << 1)];
                let low_word = self.sdram[(base_addr | (i << 1)) + 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            });
        }

        array::from_fn(|i| self.read_longword(address | ((i as u32) << 2)))
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        self.cycle_counter += 1;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                self.cycle_counter += SH2_SDRAM_WRITE_CYCLES;

                let word_addr = ((address & SDRAM_MASK) >> 1) as usize;
                if !address.bit(0) {
                    self.sdram[word_addr].set_msb(value);
                } else {
                    self.sdram[word_addr].set_lsb(value);
                }
            }
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                log::trace!("SH-2 {:?} byte write {address:08X} {value:02X}", self.which);
                self.registers.sh2_write_byte(
                    address,
                    value,
                    self.which,
                    self.vdp,
                    self.cycle_counter,
                );
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:02X}");
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram_byte(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:02X}");
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FB_MIRROR_END => {
                if self.registers.vdp_access == Access::Sh2 {
                    // Treat write as an overwrite because 0 bytes are never written to the frame buffer
                    self.vdp.write_frame_buffer_byte(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:02X}");
                }
            }
            SH2_PWM_START..=SH2_PWM_END => {
                let mut word = self.pwm.read_register(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.pwm.sh2_write_register(address & !1, word);
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += SH2_CARTRIDGE_CYCLES;

                // TODO can the SH-2s write to cartridge RAM?
                self.cartridge.write_byte(address & 0x3FFFFF, value);
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => {
                log::debug!(
                    "Ignoring SH-2 {:?} byte write to boot ROM: {address:08X} {value:02X}",
                    self.which
                );
            }
            _ => {
                log::warn!(
                    "SH-2 {:?} invalid address byte write: {address:08X} {value:02X}",
                    self.which
                );
            }
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        self.cycle_counter += 1;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                self.cycle_counter += SH2_SDRAM_WRITE_CYCLES;
                self.sdram[((address & SDRAM_MASK) >> 1) as usize] = value;
            }
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                log::trace!("SH-2 {:?} word write {address:08X} {value:04X}", self.which);
                self.registers.sh2_write(address, value, self.which, self.vdp, self.cycle_counter);
            }
            SH2_PWM_START..=SH2_PWM_END => {
                log::trace!("SH-2 {:?} PWM register write {address:08X} {value:04X}", self.which);
                self.pwm.sh2_write_register(address, value);
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:04X}");
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:04X}");
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FRAME_BUFFER_END
            | SH2_FB_MIRROR_START..=SH2_FB_MIRROR_END => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer_word(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:04X}");
                }
            }
            SH2_OVERWRITE_IMAGE_START..=SH2_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=0: {address:08X} {value:04X}"
                    );
                }
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += SH2_CARTRIDGE_CYCLES;

                // TODO can the SH-2s write to cartridge RAM?
                self.cartridge.write_word(address & 0x3FFFFF, value);
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => {
                log::debug!("SH-2 {:?} write to boot ROM: {address:08X} {value:04X}", self.which);
            }
            _ => {
                log::warn!(
                    "SH-2 {:?} invalid address write: {address:08X} {value:04X}",
                    self.which
                );
            }
        }
    }

    #[inline]
    fn write_longword(&mut self, address: u32, value: u32) {
        self.cycle_counter += 2;

        match address {
            SH2_SDRAM_START..=SH2_SDRAM_END => {
                self.cycle_counter += 2 * SH2_SDRAM_WRITE_CYCLES;

                let sdram_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
                self.sdram[sdram_addr] = (value >> 16) as u16;
                self.sdram[sdram_addr | 1] = value as u16;
            }
            SH2_SYSTEM_REGISTERS_START..=SH2_SYSTEM_REGISTERS_END => {
                log::trace!("SH-2 {:?} longword write {address:08X} {value:08X}", self.which);
                self.registers.sh2_write(
                    address,
                    (value >> 16) as u16,
                    self.which,
                    self.vdp,
                    self.cycle_counter,
                );
                self.registers.sh2_write(
                    address | 2,
                    value as u16,
                    self.which,
                    self.vdp,
                    self.cycle_counter,
                );
            }
            SH2_VDP_START..=SH2_VDP_END => {
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_register(address, (value >> 16) as u16);
                    self.vdp.write_register(address | 2, value as u16);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:08X}");
                }
            }
            SH2_CRAM_START..=SH2_CRAM_END => {
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_cram(address, (value >> 16) as u16);
                    self.vdp.write_cram(address | 2, value as u16);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:08X}");
                }
            }
            SH2_FRAME_BUFFER_START..=SH2_FRAME_BUFFER_END
            | SH2_FB_MIRROR_START..=SH2_FB_MIRROR_END => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.write_frame_buffer_word(address, (value >> 16) as u16);
                    self.vdp.write_frame_buffer_word(address | 2, value as u16);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:08X}");
                }
            }
            SH2_OVERWRITE_IMAGE_START..=SH2_OVERWRITE_IMAGE_END => {
                if self.registers.vdp_access == Access::Sh2 {
                    self.vdp.frame_buffer_overwrite_word(address, (value >> 16) as u16);
                    self.vdp.frame_buffer_overwrite_word(address | 2, value as u16);
                } else {
                    log::warn!("Frame buffer write with FM=0: {address:08X} {value:08X}");
                }
            }
            SH2_PWM_START..=SH2_PWM_END => {
                self.pwm.sh2_write_register(address, (value >> 16) as u16);
                self.pwm.sh2_write_register(address | 2, value as u16);
            }
            SH2_CARTRIDGE_START..=SH2_CARTRIDGE_END => {
                self.cycle_counter += 2 * SH2_CARTRIDGE_CYCLES;

                // TODO can the SH-2s write to cartridge RAM?
                self.cartridge.write_word(address & 0x3FFFFF, (value >> 16) as u16);
                self.cartridge.write_word((address & 0x3FFFFF) | 1, value as u16);
            }
            SH2_BOOT_ROM_START..=SH2_BOOT_ROM_END => {
                log::debug!(
                    "SH-2 {:?} longword write to boot ROM address: {address:08X} {value:08X}",
                    self.which
                );
            }
            _ => {
                log::warn!(
                    "SH-2 {:?} invalid address write: {address:08X} {value:08X}",
                    self.which
                );
            }
        }
    }

    #[inline]
    fn reset(&self) -> bool {
        self.registers.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        match self.which {
            WhichCpu::Master => self.registers.master_interrupts.current_interrupt_level,
            WhichCpu::Slave => self.registers.slave_interrupts.current_interrupt_level,
        }
    }

    #[inline]
    fn dma_request_0(&self) -> bool {
        !self.registers.dma.fifo.sh2_is_empty()
    }

    #[inline]
    fn dma_request_1(&self) -> bool {
        self.pwm.dma_request_1()
    }

    #[inline]
    fn serial_rx(&mut self) -> Option<u8> {
        match self.which {
            WhichCpu::Master => self.serial.slave_to_master.take(),
            WhichCpu::Slave => self.serial.master_to_slave.take(),
        }
    }

    #[inline]
    fn serial_tx(&mut self, value: u8) {
        match self.which {
            WhichCpu::Master => self.serial.master_to_slave = Some(value),
            WhichCpu::Slave => self.serial.slave_to_master = Some(value),
        }
    }
}

#[inline]
fn read_u8<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u8 {
    slice[(address as usize) & (LEN - 1)]
}

#[inline]
fn read_u16<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u16 {
    let address = (address as usize) & (LEN - 1) & !1;
    u16::from_be_bytes([slice[address], slice[address + 1]])
}

#[inline]
fn read_u32<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u32 {
    let address = (address as usize) & (LEN - 1) & !3;
    u32::from_be_bytes(slice[address..address + 4].try_into().unwrap())
}
