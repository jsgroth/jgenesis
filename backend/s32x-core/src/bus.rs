//! 32X memory mapping for the 68000 and SH-2s

use crate::bootrom;
use crate::core::{Sega32X, Sega32XBus};
use crate::registers::Access;
use genesis_config::GenesisRegion;
use genesis_core::memory::PhysicalMedium;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::Sh2;
use sh2_emu::bus::{AccessContext, BusInterface, OpSize};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::{array, cmp};

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

// $A130F0-$A130FF: Cartridge registers
const M68K_CART_REGISTERS_START: u32 = 0xA130F0;
const M68K_CART_REGISTERS_END: u32 = 0xA130FF;

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
                if self.s32x_bus.registers.adapter_enabled {
                    self.m68k_vectors[address as usize]
                } else {
                    self.s32x_bus.cartridge.read_byte(address)
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                // TODO is this right? some games read from ROM without setting RV=1; allow them to go through
                if self.s32x_bus.registers.adapter_enabled
                    && !self.s32x_bus.registers.dma.rom_to_vram_dma
                {
                    log::warn!("ROM byte read with RV=0: {address:06X}");
                }
                self.s32x_bus.cartridge.read_byte(address)
            }
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.s32x_bus.vdp.read_frame_buffer)
                } else {
                    log::warn!("Frame buffer byte read with FM=1: {address:06X}");
                    0xFF
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                // First 512KB of ROM
                self.s32x_bus.cartridge.read_byte(address & 0x7FFFF)
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.s32x_bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.s32x_bus.cartridge.read_byte(rom_addr)
            }
            M68K_CART_REGISTERS_START..=M68K_CART_REGISTERS_END => {
                self.s32x_bus.cartridge.read_byte(address)
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K read byte {address:06X}");
                word_to_byte!(address, self.s32x_bus.registers.m68k_read)
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // 32X VDP registers
                log::trace!("M68K read byte {address:06X}");
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    word_to_byte!(address, self.s32x_bus.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read while FM=1: {address:06X}");
                    0xFF
                }
            }
            M68K_PWM_START..=M68K_PWM_END => {
                word_to_byte!(address, self.s32x_bus.pwm.read_register)
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                word_to_byte!(address, self.s32x_bus.vdp.read_cram)
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
                if self.s32x_bus.registers.adapter_enabled {
                    let address = (address & !1) as usize;
                    u16::from_be_bytes(self.m68k_vectors[address..address + 2].try_into().unwrap())
                } else {
                    self.s32x_bus.cartridge.read_word(address)
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                // ROM (only accessible when 32X is disabled or ROM-to-VRAM DMA is enabled)
                // TODO is this right? some games read from ROM without setting RV=1; allow them to go through
                if self.s32x_bus.registers.adapter_enabled
                    && !self.s32x_bus.registers.dma.rom_to_vram_dma
                {
                    log::warn!("ROM word read with RV=0: {address:06X}");
                }
                self.s32x_bus.cartridge.read_word(address)
            }
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.read_frame_buffer(address)
                } else {
                    log::warn!("Frame buffer word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                // First 512KB of ROM
                self.s32x_bus.cartridge.read_word(address & 0x7FFFF)
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                // Mappable 1MB ROM bank
                let rom_addr =
                    (u32::from(self.s32x_bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.s32x_bus.cartridge.read_word(rom_addr)
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K read word {address:06X}");
                self.s32x_bus.registers.m68k_read(address)
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // 32X VDP registers
                log::trace!("M68K read word {address:06X}");
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=1: {address:06X}");
                    0xFFFF
                }
            }
            M68K_PWM_START..=M68K_PWM_END => {
                // PWM registers
                self.s32x_bus.pwm.read_register(address)
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                // 32X CRAM
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.read_cram(address)
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
            _ => {
                log::error!("M68K word read from unexpected address {address:06X}");
                0
            }
        }
    }

    fn read_word_for_dma(&mut self, address: u32, open_bus: &mut u16) -> u16 {
        if !self.s32x_bus.registers.dma.rom_to_vram_dma {
            // TODO should these reads be blocked?
            log::debug!("Cartridge read for DMA with RV=0 {address:06X}");
        }

        if !(0x000000..=0x3FFFFF).contains(&address) {
            log::warn!("VDP DMA read from an invalid address {address:06X}");
            return *open_bus;
        }

        *open_bus = self.s32x_bus.cartridge.read_word(address);
        *open_bus
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            H_INT_VECTOR_START..=H_INT_VECTOR_END => {
                self.m68k_vectors[address as usize] = value;
                log::trace!("68000 HINT vector: {:06X}", self.h_int_vector());
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                self.s32x_bus.cartridge.write_byte(address, value);
            }
            M68K_FRAME_BUFFER_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.write_frame_buffer_byte(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:02X}");
                }
            }
            M68K_CART_REGISTERS_START..=M68K_CART_REGISTERS_END => {
                self.s32x_bus.cartridge.write_byte(address, value);
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                log::trace!("M68K write byte {address:06X} {value:02X}");
                self.s32x_bus.registers.m68k_write_byte(address, value);
            }
            M68K_VDP_START..=M68K_VDP_END => {
                log::trace!("M68K write byte {address:06X} {value:02X}");

                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:02X}");
                }
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                self.s32x_bus.cartridge.write_byte(address & 0x7FFFF, value);
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                let rom_addr =
                    (u32::from(self.s32x_bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.s32x_bus.cartridge.write_byte(rom_addr, value);
            }
            M68K_PWM_START..=M68K_PWM_END => {
                let mut word = self.s32x_bus.pwm.read_register(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.s32x_bus.pwm.m68k_write_register(address & !1, word);
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
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.write_frame_buffer_word(address, value);
                } else {
                    log::warn!("Frame buffer write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_OVERWRITE_IMAGE_START..=M68K_OVERWRITE_IMAGE_END => {
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.frame_buffer_overwrite_word(address, value);
                } else {
                    log::warn!(
                        "Frame buffer overwrite image write with FM=1: {address:06X} {value:04X}"
                    );
                }
            }
            M68K_SYSTEM_REGISTERS_START..=M68K_SYSTEM_REGISTERS_END => {
                // System registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                self.s32x_bus.registers.m68k_write(address, value);
            }
            M68K_PWM_START..=M68K_PWM_END => {
                // PWM registers
                log::trace!("M68K PWM register write {address:06X} {value:04X}");
                self.s32x_bus.pwm.m68k_write_register(address, value);
            }
            M68K_VDP_START..=M68K_VDP_END => {
                // VDP registers
                log::trace!("M68K write word {address:06X} {value:04X}");
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_CRAM_START..=M68K_CRAM_END => {
                if self.s32x_bus.registers.vdp_access == Access::M68k {
                    self.s32x_bus.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=1: {address:06X} {value:04X}");
                }
            }
            M68K_CARTRIDGE_START..=M68K_CARTRIDGE_END => {
                self.s32x_bus.cartridge.write_word(address, value);
            }
            M68K_FIRST_CART_BANK_START..=M68K_FIRST_CART_BANK_END => {
                self.s32x_bus.cartridge.write_word(address & 0x7FFFF, value);
            }
            M68K_MAPPABLE_CART_BANK_START..=M68K_MAPPABLE_CART_BANK_END => {
                let cart_addr =
                    (u32::from(self.s32x_bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                self.s32x_bus.cartridge.write_word(cart_addr, value);
            }
            M68K_CART_REGISTERS_START..=M68K_CART_REGISTERS_END => {
                self.s32x_bus.cartridge.write_word(address, value);
            }
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

impl WhichCpu {
    pub fn other(self) -> Self {
        match self {
            Self::Master => Self::Slave,
            Self::Slave => Self::Master,
        }
    }
}

pub struct OtherCpu {
    cpu: *mut Sh2,
    cycle_counter: *mut u64,
}

// SH-2 memory map
pub struct Sh2Bus {
    s32x_bus: *mut Sega32XBus,
    other_sh2: Option<OtherCpu>,
    pub which: WhichCpu,
    pub cycle_counter: u64,
    pub cycle_limit: u64,
}

sh2_emu::bus::impl_sh2_lookup_table!(Sh2Bus);

pub struct Sh2BusGuard<'bus, 'other> {
    bus: Sh2Bus,
    _bus_marker: PhantomData<&'bus ()>,
    _other_marker: PhantomData<&'other ()>,
}

impl Deref for Sh2BusGuard<'_, '_> {
    type Target = Sh2Bus;

    fn deref(&self) -> &Self::Target {
        &self.bus
    }
}

impl DerefMut for Sh2BusGuard<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bus
    }
}

// All values are minus one because every access takes at least 1 cycle
const SH2_CARTRIDGE_CYCLES: u64 = 7;
const SH2_FRAME_BUFFER_READ_CYCLES: u64 = 4;
const SH2_VDP_CYCLES: u64 = 4;
// SDRAM burst reads take between 10 and 12 cycles; assume always 10 for simplicity
const SH2_SDRAM_READ_CYCLES: u64 = 9;
// SDRAM writes take between 1 and 3 cycles; assume always 1 for simplicity
const SH2_SDRAM_WRITE_CYCLES: u64 = 0;

macro_rules! invalid_size {
    ($size:expr) => {
        panic!("invalid size {}", $size)
    };
}

fn sh2_read_register<const SIZE: u8>(address: u32, mut read_word: impl FnMut(u32) -> u16) -> u32 {
    match SIZE {
        OpSize::BYTE => {
            let word = read_word(address & !1);
            if !address.bit(0) { word.msb().into() } else { word.lsb().into() }
        }
        OpSize::WORD => read_word(address).into(),
        OpSize::LONGWORD => {
            let high: u32 = read_word(address).into();
            let low: u32 = read_word(address | 2).into();
            low | (high << 16)
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_write_register<const SIZE: u8>(
    bus: &mut Sega32XBus,
    address: u32,
    value: u32,
    read_word: impl Fn(&mut Sega32XBus, u32) -> u16,
    write_word: impl Fn(&mut Sega32XBus, u32, u16),
) {
    match SIZE {
        OpSize::BYTE => {
            let mut word = read_word(bus, address & !1);
            if !address.bit(0) {
                word.set_msb(value as u8);
            } else {
                word.set_lsb(value as u8);
            }
            write_word(bus, address & !1, word);
        }
        OpSize::WORD => {
            write_word(bus, address, value as u16);
        }
        OpSize::LONGWORD => {
            write_word(bus, address, (value >> 16) as u16);
            write_word(bus, address | 2, value as u16);
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_read_memory<const SIZE: u8, const N: usize>(memory: &[u8; N], address: u32) -> u32 {
    match SIZE {
        OpSize::BYTE => read_u8(memory, address).into(),
        OpSize::WORD => read_u16(memory, address).into(),
        OpSize::LONGWORD => read_u32(memory, address),
        _ => invalid_size!(SIZE),
    }
}

fn sh2_read_memory_u16<const SIZE: u8, const N: usize>(memory: &[u16; N], address: u32) -> u32 {
    match SIZE {
        OpSize::BYTE => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            let word = memory[memory_addr];
            if !address.bit(0) { word.msb().into() } else { word.lsb().into() }
        }
        OpSize::WORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            memory[memory_addr].into()
        }
        OpSize::LONGWORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1) & !1;
            let high: u32 = memory[memory_addr].into();
            let low: u32 = memory[memory_addr + 1].into();
            low | (high << 16)
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_write_memory_u16<const SIZE: u8, const N: usize>(
    memory: &mut [u16; N],
    address: u32,
    value: u32,
) {
    match SIZE {
        OpSize::BYTE => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            if !address.bit(0) {
                memory[memory_addr].set_msb(value as u8);
            } else {
                memory[memory_addr].set_lsb(value as u8);
            }
        }
        OpSize::WORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            memory[memory_addr] = value as u16;
        }
        OpSize::LONGWORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1) & !1;
            memory[memory_addr] = (value >> 16) as u16;
            memory[memory_addr + 1] = value as u16;
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_vdp_cycles<const SIZE: u8>() -> u64 {
    match SIZE {
        OpSize::BYTE | OpSize::WORD => SH2_VDP_CYCLES,
        OpSize::LONGWORD => 2 * SH2_VDP_CYCLES,
        _ => invalid_size!(SIZE),
    }
}

impl Sh2Bus {
    pub fn create<'bus, 'other>(
        s32x_bus: &'bus mut Sega32XBus,
        which: WhichCpu,
        cycle_counter: u64,
        cycle_limit: u64,
        other_sh2: Option<(&'other mut Sh2, &'other mut u64)>,
    ) -> Sh2BusGuard<'bus, 'other> {
        // SAFETY: Sh2Bus contains raw pointers that are created from mutable references here. The
        // returned bus is only accessible through a guard so that the caller cannot reborrow or
        // move the underlying values until after dropping the guard.
        Sh2BusGuard {
            bus: Sh2Bus {
                s32x_bus,
                other_sh2: other_sh2.map(|(other_cpu, other_cycles)| OtherCpu {
                    cpu: other_cpu,
                    cycle_counter: other_cycles,
                }),
                which,
                cycle_counter,
                cycle_limit,
            },
            _bus_marker: PhantomData,
            _other_marker: PhantomData,
        }
    }

    fn s32x_bus(&mut self) -> &mut Sega32XBus {
        // SAFETY: Mutable reference created from a raw pointer that was originally created from a
        // mutable reference.
        // This method mutably borrows the Sh2Bus so only one mutable reference can be created this
        // way at a time. Other code should not create mutable references directly, and must not
        // touch the pointer while a mutable reference is alive.
        unsafe { &mut *self.s32x_bus }
    }

    fn s32x_bus_shared(&self) -> &Sega32XBus {
        // SAFETY: Same as above but returns a shared reference from a shared Sh2Bus borrow
        unsafe { &*self.s32x_bus }
    }

    // Brutal Unleashed: Above the Claw requires fairly close synchronization to prevent
    // the game from freezing due to the master SH-2 missing a communication port write from
    // the slave SH-2. After the slave SH-2 sees a specific value from the master SH-2, it
    // writes to the communication port twice in quick succession, and the master SH-2 must
    // read the first value before it's overwritten
    fn sync_if_comm_port_accessed(&mut self, address: u32) {
        // $00004020-$0000402F are the communication ports
        if !(0x4020..0x4030).contains(&address) {
            return;
        }

        // SAFETY: All raw pointers used here were created from mutable references and are
        // guaranteed non-null.
        // The original Sh2Bus instance is not used while the other CPU is executing against the
        // bus copy.
        unsafe {
            let Some(OtherCpu { cpu, cycle_counter }) = self.other_sh2 else { return };

            let limit = cmp::min(self.cycle_limit, self.cycle_counter);
            let mut bus = Sh2Bus {
                s32x_bus: self.s32x_bus,
                which: self.which.other(),
                cycle_counter: *cycle_counter,
                cycle_limit: limit,
                other_sh2: None,
            };

            while bus.cycle_counter < limit {
                (*cpu).execute(crate::core::SH2_EXECUTION_SLICE_LEN, &mut bus);
            }
            *cycle_counter = bus.cycle_counter;
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn read_00<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD { 2 } else { 1 };

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!(
                    "SH-2 {:?} read {} {address:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                self.sync_if_comm_port_accessed(address);

                let which = self.which;
                sh2_read_register::<SIZE>(address, |address| {
                    let bus = self.s32x_bus();
                    bus.registers.sh2_read(address, which, &bus.vdp)
                })
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!(
                    "SH-2 {:?} PWM register {} read {address:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                sh2_read_register::<SIZE>(address, |address| {
                    self.s32x_bus().pwm.read_register(address)
                })
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_read_register::<SIZE>(address, |address| {
                        self.s32x_bus().vdp.read_register(address)
                    })
                } else {
                    log::warn!(
                        "VDP register {} read with FM=0: {address:08X}",
                        OpSize::display::<SIZE>()
                    );
                    // TODO open bus?
                    OpSize::mask::<SIZE>()
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_read_register::<SIZE>(address, |address| {
                        self.s32x_bus().vdp.read_cram(address)
                    })
                } else {
                    log::warn!("CRAM {} read with FM=0: {address:08X}", OpSize::display::<SIZE>());
                    // TODO open bus?
                    OpSize::mask::<SIZE>()
                }
            }
            0x0000..=0x3FFF => {
                // Boot ROM
                match self.which {
                    WhichCpu::Master => sh2_read_memory::<SIZE, _>(bootrom::SH2_MASTER, address),
                    WhichCpu::Slave => sh2_read_memory::<SIZE, _>(bootrom::SH2_SLAVE, address),
                }
            }
            _ => {
                log::warn!(
                    "SH-2 {:?} invalid address {} read {address:08X}, ctx: {ctx}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                // TODO open bus?
                0
            }
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn read_02<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD {
            2 * (1 + SH2_CARTRIDGE_CYCLES)
        } else {
            1 + SH2_CARTRIDGE_CYCLES
        };

        if address & 0x400000 == 0 {
            // Cartridge
            match SIZE {
                OpSize::BYTE => self.s32x_bus().cartridge.read_byte(address & 0x3FFFFF).into(),
                OpSize::WORD => self.s32x_bus().cartridge.read_word(address & 0x3FFFFF).into(),
                OpSize::LONGWORD => {
                    let rom_addr = address & 0x3FFFFF & !3;
                    let high: u32 = self.s32x_bus().cartridge.read_word(rom_addr).into();
                    let low: u32 = self.s32x_bus().cartridge.read_word(rom_addr | 2).into();
                    low | (high << 16)
                }
                _ => invalid_size!(SIZE),
            }
        } else {
            // Not sure what these addresses are; Doom 32X Resurrection reads from them
            // Sega CD maybe?
            log::debug!(
                "SH-2 {:?} Invalid address {} read {address:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );

            // TODO open bus?
            0
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn read_04<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD {
            2 * (1 + SH2_FRAME_BUFFER_READ_CYCLES)
        } else {
            1 + SH2_FRAME_BUFFER_READ_CYCLES
        };

        if self.s32x_bus().registers.vdp_access == Access::Sh2 {
            sh2_read_register::<SIZE>(address, |address| {
                self.s32x_bus().vdp.read_frame_buffer(address)
            })
        } else {
            log::warn!(
                "SH-2 {:?} frame buffer {} read with FM=0: {address:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );

            // TODO open bus?
            OpSize::mask::<SIZE>()
        }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn read_06<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        if address >= 0x06040000 {
            log::debug!(
                "SH-2 {:?} invalid {} read {address:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return 0;
        }

        // SDRAM access times are not doubled for longword reads
        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        sh2_read_memory_u16::<SIZE, _>(&self.s32x_bus().sdram, address)
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn write_00<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        self.cycle_counter += if SIZE == OpSize::LONGWORD { 2 } else { 1 };

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!(
                    "SH-2 {:?} {} write {address:08X} {value:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                self.sync_if_comm_port_accessed(address);

                let which = self.which;
                sh2_write_register::<SIZE>(
                    self.s32x_bus(),
                    address,
                    value,
                    |bus, address| bus.registers.sh2_read(address, which, &bus.vdp),
                    |bus, address, word| {
                        bus.registers.sh2_write(address, word, which, &mut bus.vdp);
                    },
                );
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!(
                    "SH-2 {:?} PWM register {} write {address:08X} {value:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                sh2_write_register::<SIZE>(
                    self.s32x_bus(),
                    address,
                    value,
                    |bus, address| bus.pwm.read_register(address),
                    |bus, address, word| bus.pwm.sh2_write_register(address, word),
                );
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_write_register::<SIZE>(
                        self.s32x_bus(),
                        address,
                        value,
                        |bus, address| bus.vdp.read_register(address),
                        |bus, address, word| bus.vdp.write_register(address, word),
                    );
                } else {
                    log::warn!(
                        "VDP register {} write with FM=0: {address:08X} {value:08X}",
                        OpSize::display::<SIZE>()
                    );
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_write_register::<SIZE>(
                        self.s32x_bus(),
                        address,
                        value,
                        |bus, address| bus.vdp.read_cram(address),
                        |bus, address, word| bus.vdp.write_cram(address, word),
                    );
                } else {
                    log::warn!(
                        "CRAM {} write with FM=0: {address:08X} {value:08X}",
                        OpSize::display::<SIZE>()
                    );
                }
            }
            _ => {
                log::warn!(
                    "SH-2 {:?} invalid address {} write: {address:08X} {value:08X}, ctx: {ctx}",
                    self.which,
                    OpSize::display::<SIZE>()
                );
            }
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn write_02<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        self.cycle_counter += if SIZE == OpSize::LONGWORD {
            2 * (1 + SH2_CARTRIDGE_CYCLES)
        } else {
            1 + SH2_CARTRIDGE_CYCLES
        };

        if address & 0x400000 == 0 {
            match SIZE {
                OpSize::BYTE => {
                    self.s32x_bus().cartridge.write_byte(address & 0x3FFFFF, value as u8);
                }
                OpSize::WORD => {
                    self.s32x_bus().cartridge.write_word(address & 0x3FFFFF, value as u16);
                }
                OpSize::LONGWORD => {
                    let rom_addr = address & 0x3FFFFF & !3;
                    self.s32x_bus().cartridge.write_word(rom_addr, (value >> 16) as u16);
                    self.s32x_bus().cartridge.write_word(rom_addr | 2, value as u16);
                }
                _ => invalid_size!(SIZE),
            }
        } else {
            // TODO Sega CD?
            log::debug!(
                "SH-2 {:?} invalid {} write {address:08X} {value:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn write_04<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        if self.s32x_bus().registers.vdp_access != Access::Sh2 {
            log::warn!(
                "SH-2 {:?} frame buffer {} write with FM=0: {address:08X} {value:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return;
        }

        let cycle_counter = self.cycle_counter;
        self.cycle_counter += self.s32x_bus().vdp.frame_buffer_write_latency(cycle_counter);
        if SIZE == OpSize::LONGWORD {
            let cycle_counter = self.cycle_counter;
            self.cycle_counter += self.s32x_bus().vdp.frame_buffer_write_latency(cycle_counter);
        }

        if SIZE == OpSize::BYTE {
            // Treat normal mapping and overwrite image identically because 0 bytes are never
            // written in either case
            self.s32x_bus().vdp.write_frame_buffer_byte(address, value as u8);
            return;
        }

        sh2_write_register::<SIZE>(
            self.s32x_bus(),
            address,
            value,
            |_, _| panic!("read_word should never be called for frame buffer writes"),
            |bus, address, word| {
                if address & 0x20000 == 0 {
                    // Normal frame buffer mapping
                    bus.vdp.write_frame_buffer_word(address, word);
                } else {
                    // Overwrite image
                    bus.vdp.frame_buffer_overwrite_word(address, word);
                }
            },
        );
    }

    // $06000000-$07FFFFFF: SDRAM
    fn write_06<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        if address >= 0x06040000 {
            log::debug!(
                "SH-2 {:?} invalid {} write {address:08X} {value:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return;
        }

        // No latency difference between 16-bit SDRAM writes and 32-bit SDRAM writes
        self.cycle_counter += 1 + SH2_SDRAM_WRITE_CYCLES;

        sh2_write_memory_u16::<SIZE, _>(&mut self.s32x_bus().sdram, address, value);
    }
}

impl BusInterface for Sh2Bus {
    #[inline]
    fn read<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        const BYTE_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::BYTE }>(address, ctx),
            |bus, address, ctx| bus.read_02::<{ OpSize::BYTE }>(address, ctx),
            |bus, address, ctx| bus.read_04::<{ OpSize::BYTE }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::BYTE }>(address, ctx),
        ];

        const WORD_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::WORD }>(address, ctx),
            |bus, address, ctx| bus.read_02::<{ OpSize::WORD }>(address, ctx),
            |bus, address, ctx| bus.read_04::<{ OpSize::WORD }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::WORD }>(address, ctx),
        ];

        const LONGWORD_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::LONGWORD }>(address, ctx),
            |bus, address, ctx| bus.read_02::<{ OpSize::LONGWORD }>(address, ctx),
            |bus, address, ctx| bus.read_04::<{ OpSize::LONGWORD }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::LONGWORD }>(address, ctx),
        ];

        let idx = ((address >> 25) & 3) as usize;
        match SIZE {
            OpSize::BYTE => BYTE_FNS[idx](self, address, ctx),
            OpSize::WORD => WORD_FNS[idx](self, address, ctx),
            OpSize::LONGWORD => LONGWORD_FNS[idx](self, address, ctx),
            _ => invalid_size!(SIZE),
        }
    }

    #[inline]
    fn read_cache_line(&mut self, address: u32, ctx: AccessContext) -> [u32; 4] {
        if (0x06000000..0x06040000).contains(&address) {
            // The SH-2s can read a full 16-byte cache line in 12 cycles
            self.cycle_counter += SH2_SDRAM_READ_CYCLES + 1;

            let base_addr = ((address & SDRAM_MASK & !0xF) >> 1) as usize;
            return array::from_fn(|i| {
                let high_word = self.s32x_bus().sdram[base_addr | (i << 1)];
                let low_word = self.s32x_bus().sdram[(base_addr | (i << 1)) + 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            });
        }

        array::from_fn(|i| self.read_longword(address | ((i as u32) << 2), ctx))
    }

    #[inline]
    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        const BYTE_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::BYTE }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_02::<{ OpSize::BYTE }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_04::<{ OpSize::BYTE }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::BYTE }>(address, value, ctx),
        ];

        const WORD_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::WORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_02::<{ OpSize::WORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_04::<{ OpSize::WORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::WORD }>(address, value, ctx),
        ];

        const LONGWORD_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::LONGWORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_02::<{ OpSize::LONGWORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_04::<{ OpSize::LONGWORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::LONGWORD }>(address, value, ctx),
        ];

        let idx = ((address >> 25) & 3) as usize;
        match SIZE {
            OpSize::BYTE => BYTE_FNS[idx](self, address, value, ctx),
            OpSize::WORD => WORD_FNS[idx](self, address, value, ctx),
            OpSize::LONGWORD => LONGWORD_FNS[idx](self, address, value, ctx),
            _ => invalid_size!(SIZE),
        }
    }

    #[inline]
    fn reset(&self) -> bool {
        self.s32x_bus_shared().registers.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        match self.which {
            WhichCpu::Master => {
                self.s32x_bus_shared().registers.master_interrupts.current_interrupt_level
            }
            WhichCpu::Slave => {
                self.s32x_bus_shared().registers.slave_interrupts.current_interrupt_level
            }
        }
    }

    #[inline]
    fn dma_request_0(&self) -> bool {
        !self.s32x_bus_shared().registers.dma.fifo.sh2_is_empty()
    }

    #[inline]
    fn dma_request_1(&self) -> bool {
        self.s32x_bus_shared().pwm.dma_request_1()
    }

    #[inline]
    fn acknowledge_dreq_1(&mut self) {
        self.s32x_bus().pwm.acknowledge_dreq_1();
    }

    #[inline]
    fn serial_rx(&mut self) -> Option<u8> {
        match self.which {
            WhichCpu::Master => self.s32x_bus().serial.slave_to_master.take(),
            WhichCpu::Slave => self.s32x_bus().serial.master_to_slave.take(),
        }
    }

    #[inline]
    fn serial_tx(&mut self, value: u8) {
        match self.which {
            WhichCpu::Master => self.s32x_bus().serial.master_to_slave = Some(value),
            WhichCpu::Slave => self.s32x_bus().serial.slave_to_master = Some(value),
        }
    }

    #[inline]
    fn increment_cycle_counter(&mut self, cycles: u64) {
        self.cycle_counter += cycles;
    }

    #[inline]
    fn should_stop_execution(&self) -> bool {
        self.cycle_counter >= self.cycle_limit
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
