//! 32X memory mapping for the 68000 and SH-2s

use crate::bootrom;
use crate::core::{Sega32X, Sega32XBus};
use crate::registers::Access;
use genesis_core::GenesisRegion;
use genesis_core::memory::PhysicalMedium;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::Sh2;
use sh2_emu::bus::BusInterface;
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
            M68K_CART_RAM_REGISTER_BYTE => self.s32x_bus.cartridge.read_ram_register(),
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
            _ => todo!("read word {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        if !self.s32x_bus.registers.dma.rom_to_vram_dma {
            // TODO should these reads be blocked?
            log::debug!("Cartridge read for DMA with RV=0 {address:06X}");
        }

        if !(0x000000..=0x3FFFFF).contains(&address) {
            log::warn!("VDP DMA read from an invalid address {address:06X}");
            return 0xFFFF;
        }

        self.s32x_bus.cartridge.read_word(address)
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
            M68K_CART_RAM_REGISTER_BYTE => self.s32x_bus.cartridge.write_ram_register(value),
            M68K_SSF_BANK_REGISTERS_START..=M68K_SSF_BANK_REGISTERS_END => {
                self.s32x_bus.cartridge.write_mapper_bank_register(address, value);
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
            // Not sure this is right but Doom 32X Resurrection writes to this address
            M68K_CART_RAM_REGISTER_WORD => self.s32x_bus.cartridge.write_ram_register(value as u8),
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

pub struct OtherCpu<'other> {
    pub cpu: &'other mut Sh2,
    pub cycle_counter: &'other mut u64,
}

// SH-2 memory map
pub struct Sh2Bus<'bus, 'other> {
    pub s32x_bus: &'bus mut Sega32XBus,
    pub which: WhichCpu,
    pub cycle_counter: u64,
    pub cycle_limit: u64,
    pub other_sh2: Option<OtherCpu<'other>>,
}

// All values are minus one because every access takes at least 1 cycle
const SH2_CARTRIDGE_CYCLES: u64 = 5;
const SH2_FRAME_BUFFER_READ_CYCLES: u64 = 4;
const SH2_VDP_CYCLES: u64 = 4;
const SH2_SDRAM_READ_CYCLES: u64 = 11;
const SH2_SDRAM_WRITE_CYCLES: u64 = 1;

impl Sh2Bus<'_, '_> {
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

        let Some(OtherCpu { cpu, cycle_counter }) = &mut self.other_sh2 else { return };

        let limit = cmp::min(self.cycle_limit, self.cycle_counter);
        let mut bus = Sh2Bus {
            s32x_bus: &mut *self.s32x_bus,
            which: self.which.other(),
            cycle_counter: **cycle_counter,
            cycle_limit: limit,
            other_sh2: None,
        };

        while bus.cycle_counter < limit {
            cpu.execute(crate::core::SH2_EXECUTION_SLICE_LEN, &mut bus);
        }
        **cycle_counter = bus.cycle_counter;
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn read_byte_00(&mut self, address: u32) -> u8 {
        self.cycle_counter += 1;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!("SH-2 {:?} read byte {address:08X}", self.which);

                self.sync_if_comm_port_accessed(address);

                let value =
                    self.s32x_bus.registers.sh2_read(address & !1, self.which, &self.s32x_bus.vdp);
                if !address.bit(0) { value.msb() } else { value.lsb() }
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                word_to_byte!(address, self.s32x_bus.pwm.read_register)
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    word_to_byte!(address, self.s32x_bus.vdp.read_register)
                } else {
                    log::warn!("VDP register byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    let word = self.s32x_bus.vdp.read_cram(address & !1);
                    if !address.bit(0) { word.msb() } else { word.lsb() }
                } else {
                    log::warn!("CRAM byte read with FM=0: {address:08X}");
                    0xFF
                }
            }
            0x0000..=0x3FFF => {
                // Boot ROM
                match self.which {
                    WhichCpu::Master => read_u8(bootrom::SH2_MASTER, address),
                    WhichCpu::Slave => read_u8(bootrom::SH2_SLAVE, address),
                }
            }
            _ => {
                log::debug!("SH-2 {:?} invalid address byte read {address:08X}", self.which);
                0
            }
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn read_word_00(&mut self, address: u32) -> u16 {
        self.cycle_counter += 1;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!("SH-2 {:?} read word {address:08X}", self.which);

                self.sync_if_comm_port_accessed(address);

                self.s32x_bus.registers.sh2_read(address, self.which, &self.s32x_bus.vdp)
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!("SH-2 {:?} PWM register read {address:08X}", self.which);
                self.s32x_bus.pwm.read_register(address)
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.read_register(address)
                } else {
                    log::warn!("VDP register word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.read_cram(address)
                } else {
                    log::warn!("CRAM word read with FM=0: {address:08X}");
                    0xFFFF
                }
            }
            0x0000..=0x3FFF => {
                // Boot ROM
                match self.which {
                    WhichCpu::Master => read_u16(bootrom::SH2_MASTER, address),
                    WhichCpu::Slave => read_u16(bootrom::SH2_SLAVE, address),
                }
            }
            _ => {
                log::debug!("SH-2 {:?} invalid address word read {address:08X}", self.which);
                0
            }
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn read_longword_00(&mut self, address: u32) -> u32 {
        self.cycle_counter += 2;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                if log::log_enabled!(log::Level::Trace) && !(0x4020..0x4030).contains(&address) {
                    log::trace!("SH-2 {:?} read longword {address:08X}", self.which);
                }

                self.sync_if_comm_port_accessed(address);

                let high =
                    self.s32x_bus.registers.sh2_read(address, self.which, &self.s32x_bus.vdp);
                let low =
                    self.s32x_bus.registers.sh2_read(address | 2, self.which, &self.s32x_bus.vdp);
                (u32::from(high) << 16) | u32::from(low)
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                let high = self.s32x_bus.pwm.read_register(address);
                let low = self.s32x_bus.pwm.read_register(address | 2);
                (u32::from(high) << 16) | u32::from(low)
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    let high_word = self.s32x_bus.vdp.read_register(address);
                    let low_word = self.s32x_bus.vdp.read_register(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("VDP register longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    let high_word = self.s32x_bus.vdp.read_cram(address);
                    let low_word = self.s32x_bus.vdp.read_cram(address | 2);
                    (u32::from(high_word) << 16) | u32::from(low_word)
                } else {
                    log::warn!("CRAM longword read with FM=0: {address:08X}");
                    0xFFFFFFFF
                }
            }
            0x0000..=0x3FFF => {
                // Boot ROM
                match self.which {
                    WhichCpu::Master => read_u32(bootrom::SH2_MASTER, address),
                    WhichCpu::Slave => read_u32(bootrom::SH2_SLAVE, address),
                }
            }
            _ => {
                log::debug!("SH-2 {:?} invalid address longword read {address:08X}", self.which);
                0
            }
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn read_byte_02(&mut self, address: u32) -> u8 {
        self.cycle_counter += 1 + SH2_CARTRIDGE_CYCLES;

        if address & 0x400000 == 0 {
            // Cartridge
            self.s32x_bus.cartridge.read_byte(address & 0x3FFFFF)
        } else {
            // Not sure what these addresses are; Doom 32X Resurrection reads from them
            // Sega CD maybe?
            log::debug!("Invalid address byte read {address:08X}");
            0
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn read_word_02(&mut self, address: u32) -> u16 {
        self.cycle_counter += 1 + SH2_CARTRIDGE_CYCLES;

        if address & 0x400000 == 0 {
            // Cartridge
            self.s32x_bus.cartridge.read_word(address & 0x3FFFFF)
        } else {
            // Not sure what these addresses are; Doom 32X Resurrection reads from them
            // Sega CD maybe?
            log::debug!("Invalid address word read {address:08X}");
            0
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn read_longword_02(&mut self, address: u32) -> u32 {
        self.cycle_counter += 2 * (1 + SH2_CARTRIDGE_CYCLES);

        if address & 0x400000 == 0 {
            // Cartridge
            self.s32x_bus.cartridge.read_longword(address & 0x3FFFFF)
        } else {
            // Not sure what these addresses are; Doom 32X Resurrection reads from them
            // Sega CD maybe?
            log::debug!("Invalid address longword read {address:08X}");
            0
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn read_byte_04(&mut self, address: u32) -> u8 {
        self.cycle_counter += 1 + SH2_FRAME_BUFFER_READ_CYCLES;

        if self.s32x_bus.registers.vdp_access == Access::Sh2 {
            word_to_byte!(address & 0x1FFFF, self.s32x_bus.vdp.read_frame_buffer)
        } else {
            log::warn!("Frame buffer byte read with FM=0: {address:08X}");
            0xFF
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn read_word_04(&mut self, address: u32) -> u16 {
        self.cycle_counter += 1 + SH2_FRAME_BUFFER_READ_CYCLES;

        if self.s32x_bus.registers.vdp_access == Access::Sh2 {
            self.s32x_bus.vdp.read_frame_buffer(address & 0x1FFFF)
        } else {
            log::warn!("Frame buffer word read with FM=0: {address:08X}");
            0xFFFF
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn read_longword_04(&mut self, address: u32) -> u32 {
        self.cycle_counter += 2 * (1 + SH2_FRAME_BUFFER_READ_CYCLES);

        if self.s32x_bus.registers.vdp_access == Access::Sh2 {
            let high_word = self.s32x_bus.vdp.read_frame_buffer(address);
            let low_word = self.s32x_bus.vdp.read_frame_buffer(address | 2);
            (u32::from(high_word) << 16) | u32::from(low_word)
        } else {
            log::warn!("Frame buffer longword read with FM=0: {address:08X}");
            0xFFFFFFFF
        }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn read_byte_06(&mut self, address: u32) -> u8 {
        if address >= 0x06040000 {
            log::debug!("Invalid byte read {address:08X}");
            return 0;
        }

        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        let word = self.s32x_bus.sdram[((address & SDRAM_MASK) >> 1) as usize];
        if !address.bit(0) { word.msb() } else { word.lsb() }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn read_word_06(&mut self, address: u32) -> u16 {
        if address >= 0x06040000 {
            log::debug!("Invalid word read {address:08X}");
            return 0;
        }

        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        self.s32x_bus.sdram[((address & SDRAM_MASK) >> 1) as usize]
    }

    // $06000000-$07FFFFFF: SDRAM
    fn read_longword_06(&mut self, address: u32) -> u32 {
        if address >= 0x06040000 {
            log::debug!("Invalid longword read {address:08X}");
            return 0;
        }

        // SDRAM access times are not doubled for longword reads
        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        let word_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
        let high_word = self.s32x_bus.sdram[word_addr];
        let low_word = self.s32x_bus.sdram[word_addr | 1];
        (u32::from(high_word) << 16) | u32::from(low_word)
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn write_byte_00(&mut self, address: u32, value: u8) {
        self.cycle_counter += 1;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!("SH-2 {:?} byte write {address:08X} {value:02X}", self.which);

                self.sync_if_comm_port_accessed(address);

                self.s32x_bus.registers.sh2_write_byte(
                    address,
                    value,
                    self.which,
                    &mut self.s32x_bus.vdp,
                );
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                let mut word = self.s32x_bus.pwm.read_register(address & !1);
                if !address.bit(0) {
                    word.set_msb(value);
                } else {
                    word.set_lsb(value);
                }
                self.s32x_bus.pwm.sh2_write_register(address & !1, word);
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_register_byte(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:02X}");
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_cram_byte(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:02X}");
                }
            }
            _ => {
                log::debug!(
                    "SH-2 {:?} invalid address byte write: {address:08X} {value:02X}",
                    self.which
                );
            }
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn write_word_00(&mut self, address: u32, value: u16) {
        self.cycle_counter += 1;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!("SH-2 {:?} word write {address:08X} {value:04X}", self.which);

                self.sync_if_comm_port_accessed(address);

                self.s32x_bus.registers.sh2_write(
                    address,
                    value,
                    self.which,
                    &mut self.s32x_bus.vdp,
                );
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!("SH-2 {:?} PWM register write {address:08X} {value:04X}", self.which);
                self.s32x_bus.pwm.sh2_write_register(address, value);
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_register(address, value);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:04X}");
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_cram(address, value);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:04X}");
                }
            }
            _ => {
                log::debug!(
                    "SH-2 {:?} invalid address write: {address:08X} {value:04X}",
                    self.which
                );
            }
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn write_longword_00(&mut self, address: u32, value: u32) {
        self.cycle_counter += 2;

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!("SH-2 {:?} longword write {address:08X} {value:08X}", self.which);

                self.sync_if_comm_port_accessed(address);

                self.s32x_bus.registers.sh2_write(
                    address,
                    (value >> 16) as u16,
                    self.which,
                    &mut self.s32x_bus.vdp,
                );
                self.s32x_bus.registers.sh2_write(
                    address | 2,
                    value as u16,
                    self.which,
                    &mut self.s32x_bus.vdp,
                );
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                self.s32x_bus.pwm.sh2_write_register(address, (value >> 16) as u16);
                self.s32x_bus.pwm.sh2_write_register(address | 2, value as u16);
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_register(address, (value >> 16) as u16);
                    self.s32x_bus.vdp.write_register(address | 2, value as u16);
                } else {
                    log::warn!("VDP register write with FM=0: {address:08X} {value:08X}");
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += 2 * SH2_VDP_CYCLES;

                if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                    self.s32x_bus.vdp.write_cram(address, (value >> 16) as u16);
                    self.s32x_bus.vdp.write_cram(address | 2, value as u16);
                } else {
                    log::warn!("CRAM write with FM=0: {address:08X} {value:08X}");
                }
            }
            _ => {
                log::debug!(
                    "SH-2 {:?} invalid address longword write: {address:08X} {value:08X}",
                    self.which
                );
            }
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn write_byte_02(&mut self, address: u32, value: u8) {
        self.cycle_counter += 1 + SH2_CARTRIDGE_CYCLES;

        if address & 0x400000 == 0 {
            // TODO can the SH-2s write to cartridge RAM?
            self.s32x_bus.cartridge.write_byte(address & 0x3FFFFF, value);
        } else {
            // TODO Sega CD?
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn write_word_02(&mut self, address: u32, value: u16) {
        self.cycle_counter += 1 + SH2_CARTRIDGE_CYCLES;

        if address & 0x400000 == 0 {
            // TODO can the SH-2s write to cartridge RAM?
            self.s32x_bus.cartridge.write_word(address & 0x3FFFFF & !1, value);
        } else {
            // TODO Sega CD?
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn write_longword_02(&mut self, address: u32, value: u32) {
        self.cycle_counter += 2 * (1 + SH2_CARTRIDGE_CYCLES);

        if address & 0x400000 == 0 {
            // TODO can the SH-2s write to cartridge RAM?
            self.s32x_bus.cartridge.write_word(address & 0x3FFFFF & !3, (value >> 16) as u16);
            self.s32x_bus.cartridge.write_word((address & 0x3FFFFF & !3) | 2, value as u16);
        } else {
            // TODO Sega CD?
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn write_byte_04(&mut self, address: u32, value: u8) {
        self.cycle_counter += 1;

        if self.s32x_bus.registers.vdp_access == Access::Sh2 {
            // Treat normal mapping and overwrite image identically because 0 bytes are never
            // written in either case
            self.s32x_bus.vdp.write_frame_buffer_byte(address, value);
        } else {
            log::warn!("Frame buffer write with FM=0: {address:08X} {value:02X}");
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn write_word_04(&mut self, address: u32, value: u16) {
        self.cycle_counter += 1;

        if address & 0x20000 == 0 {
            // Normal frame buffer mapping
            if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                self.s32x_bus.vdp.write_frame_buffer_word(address, value);
            } else {
                log::warn!("Frame buffer write with FM=0: {address:08X} {value:04X}");
            }
        } else {
            // Overwrite image
            if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                self.s32x_bus.vdp.frame_buffer_overwrite_word(address, value);
            } else {
                log::warn!(
                    "Frame buffer overwrite image write with FM=0: {address:08X} {value:04X}"
                );
            }
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn write_longword_04(&mut self, address: u32, value: u32) {
        self.cycle_counter += 2;

        if address & 0x20000 == 0 {
            // Normal frame buffer mapping
            if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                self.s32x_bus.vdp.write_frame_buffer_word(address, (value >> 16) as u16);
                self.s32x_bus.vdp.write_frame_buffer_word(address | 2, value as u16);
            } else {
                log::warn!("Frame buffer write with FM=0: {address:08X} {value:08X}");
            }
        } else {
            // Overwrite image
            if self.s32x_bus.registers.vdp_access == Access::Sh2 {
                self.s32x_bus.vdp.frame_buffer_overwrite_word(address, (value >> 16) as u16);
                self.s32x_bus.vdp.frame_buffer_overwrite_word(address | 2, value as u16);
            } else {
                log::warn!(
                    "Frame buffer overwrite image write with FM=0: {address:08X} {value:08X}"
                );
            }
        }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn write_byte_06(&mut self, address: u32, value: u8) {
        if address >= 0x06040000 {
            log::debug!("Invalid byte write {address:08X} {value:02X}");
            return;
        }

        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        let word_addr = ((address & SDRAM_MASK) >> 1) as usize;
        if !address.bit(0) {
            self.s32x_bus.sdram[word_addr].set_msb(value);
        } else {
            self.s32x_bus.sdram[word_addr].set_lsb(value);
        }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn write_word_06(&mut self, address: u32, value: u16) {
        if address >= 0x06040000 {
            log::debug!("Invalid word write {address:08X} {value:04X}");
            return;
        }

        self.cycle_counter += 1 + SH2_SDRAM_WRITE_CYCLES;

        self.s32x_bus.sdram[((address & SDRAM_MASK) >> 1) as usize] = value;
    }

    // $06000000-$07FFFFFF: SDRAM
    fn write_longword_06(&mut self, address: u32, value: u32) {
        if address >= 0x06040000 {
            log::warn!("Invalid longword write {address:08X} {value:08X}");
            return;
        }

        self.cycle_counter += 2 * (1 + SH2_SDRAM_WRITE_CYCLES);

        let sdram_addr = (((address & SDRAM_MASK) >> 1) & !1) as usize;
        self.s32x_bus.sdram[sdram_addr] = (value >> 16) as u16;
        self.s32x_bus.sdram[sdram_addr | 1] = value as u16;
    }
}

impl BusInterface for Sh2Bus<'_, '_> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32) -> u8; 4] = [
            |bus, address| bus.read_byte_00(address),
            |bus, address| bus.read_byte_02(address),
            |bus, address| bus.read_byte_04(address),
            |bus, address| bus.read_byte_06(address),
        ];

        FNS[((address >> 25) & 3) as usize](self, address)
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32) -> u16; 4] = [
            |bus, address| bus.read_word_00(address),
            |bus, address| bus.read_word_02(address),
            |bus, address| bus.read_word_04(address),
            |bus, address| bus.read_word_06(address),
        ];

        FNS[((address >> 25) & 3) as usize](self, address)
    }

    #[inline]
    fn read_longword(&mut self, address: u32) -> u32 {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32) -> u32; 4] = [
            |bus, address| bus.read_longword_00(address),
            |bus, address| bus.read_longword_02(address),
            |bus, address| bus.read_longword_04(address),
            |bus, address| bus.read_longword_06(address),
        ];

        FNS[((address >> 25) & 3) as usize](self, address)
    }

    #[inline]
    fn read_cache_line(&mut self, address: u32) -> [u32; 4] {
        if (0x06000000..0x06040000).contains(&address) {
            // The SH-2s can read a full 16-byte cache line in 12 cycles
            self.cycle_counter += SH2_SDRAM_READ_CYCLES + 1;

            let base_addr = ((address & SDRAM_MASK & !0xF) >> 1) as usize;
            return array::from_fn(|i| {
                let high_word = self.s32x_bus.sdram[base_addr | (i << 1)];
                let low_word = self.s32x_bus.sdram[(base_addr | (i << 1)) + 1];
                (u32::from(high_word) << 16) | u32::from(low_word)
            });
        }

        array::from_fn(|i| self.read_longword(address | ((i as u32) << 2)))
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32, u8); 4] = [
            |bus, address, value| bus.write_byte_00(address, value),
            |bus, address, value| bus.write_byte_02(address, value),
            |bus, address, value| bus.write_byte_04(address, value),
            |bus, address, value| bus.write_byte_06(address, value),
        ];

        FNS[((address >> 25) & 3) as usize](self, address, value);
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32, u16); 4] = [
            |bus, address, value| bus.write_word_00(address, value),
            |bus, address, value| bus.write_word_02(address, value),
            |bus, address, value| bus.write_word_04(address, value),
            |bus, address, value| bus.write_word_06(address, value),
        ];

        FNS[((address >> 25) & 3) as usize](self, address, value);
    }

    #[inline]
    fn write_longword(&mut self, address: u32, value: u32) {
        const FNS: [fn(&mut Sh2Bus<'_, '_>, u32, u32); 4] = [
            |bus, address, value| bus.write_longword_00(address, value),
            |bus, address, value| bus.write_longword_02(address, value),
            |bus, address, value| bus.write_longword_04(address, value),
            |bus, address, value| bus.write_longword_06(address, value),
        ];

        FNS[((address >> 25) & 3) as usize](self, address, value);
    }

    #[inline]
    fn reset(&self) -> bool {
        self.s32x_bus.registers.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        match self.which {
            WhichCpu::Master => self.s32x_bus.registers.master_interrupts.current_interrupt_level,
            WhichCpu::Slave => self.s32x_bus.registers.slave_interrupts.current_interrupt_level,
        }
    }

    #[inline]
    fn dma_request_0(&self) -> bool {
        !self.s32x_bus.registers.dma.fifo.sh2_is_empty()
    }

    #[inline]
    fn dma_request_1(&self) -> bool {
        self.s32x_bus.pwm.dma_request_1()
    }

    #[inline]
    fn acknowledge_dreq_1(&mut self) {
        self.s32x_bus.pwm.acknowledge_dreq_1();
    }

    #[inline]
    fn serial_rx(&mut self) -> Option<u8> {
        match self.which {
            WhichCpu::Master => self.s32x_bus.serial.slave_to_master.take(),
            WhichCpu::Slave => self.s32x_bus.serial.master_to_slave.take(),
        }
    }

    #[inline]
    fn serial_tx(&mut self, value: u8) {
        match self.which {
            WhichCpu::Master => self.s32x_bus.serial.master_to_slave = Some(value),
            WhichCpu::Slave => self.s32x_bus.serial.slave_to_master = Some(value),
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
