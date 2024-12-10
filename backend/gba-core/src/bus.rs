//! GBA bus / memory map

use crate::apu::Apu;
use crate::control::ControlRegisters;
use crate::input::GbaInputs;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::timers::Timers;
use arm7tdmi_emu::bus::BusInterface;
use jgenesis_common::num::{GetBit, U16Ext};

// $00000000-$00003FFF: BIOS ROM (16KB)
pub const BIOS_START: u32 = 0x00000000;
pub const BIOS_END: u32 = 0x00FFFFFF;

// $02000000-$0203FFFF: External working RAM (256KB)
pub const EWRAM_START: u32 = 0x02000000;
pub const EWRAM_END: u32 = 0x02FFFFFF;

// $03000000-$03007FFF: Internal working RAM (32KB)
pub const IWRAM_START: u32 = 0x03000000;
pub const IWRAM_END: u32 = 0x03FFFFFF;

// $04000000-$040003FF: Memory-mapped I/O registers
pub const MMIO_START: u32 = 0x04000000;
pub const MMIO_END: u32 = 0x04FFFFFF;

// $05000000-$050003FF: Palette RAM (1KB)
pub const PALETTES_START: u32 = 0x05000000;
pub const PALETTES_END: u32 = 0x05FFFFFF;

// $06000000-$06017FFF: VRAM (96KB)
pub const VRAM_START: u32 = 0x06000000;
pub const VRAM_END: u32 = 0x06FFFFFF;

// $07000000-$070003FF: OAM (1KB)
pub const OAM_START: u32 = 0x07000000;
pub const OAM_END: u32 = 0x07FFFFFF;

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
    pub apu: &'a mut Apu,
    pub memory: &'a mut Memory,
    pub control: &'a mut ControlRegisters,
    pub timers: &'a mut Timers,
    pub inputs: GbaInputs,
}

// TODO open bus
// TODO BIOS read restrictions (can only read BIOS while executing in BIOS)
impl Bus<'_> {
    fn read_io_register(&mut self, address: u32) -> u16 {
        let value = match address {
            0x04000000..=0x04000056 => self.ppu.read_register(address),
            0x04000060..=0x040000AF => self.apu.read_register(address),
            0x040000B0..=0x040000DF => self.control.read_dma_register(address),
            0x04000100..=0x0400010F => self.timers.read_register(address),
            0x04000120..=0x0400012F | 0x04000134..=0x0400015F => {
                log::error!("Serial register read: {address:08X}");
                0
            }
            0x04000130 => self.read_keyinput(),
            0x04000200 => self.control.read_ie(),
            0x04000202 => self.control.read_if(),
            0x04000204 => self.control.read_waitcnt(),
            0x04000208 => self.control.read_ime(),
            0x04000300 => self.control.postflg,
            0x0400020A | 0x04000410 => {
                log::warn!("Invalid address read {address:08X}");
                !0
            }
            _ => todo!("I/O register read {address:08X}"),
        };

        log::trace!("I/O register read {address:08X} {value:04X}");

        value
    }

    fn read_io_register_u8(&mut self, address: u32) -> u8 {
        log::trace!("8-bit I/O register read {address:08X}");

        let halfword = self.read_io_register(address & !1);
        (halfword >> (8 * (address & 1))) as u8
    }

    fn read_io_register_u32(&mut self, address: u32) -> u32 {
        log::trace!("32-bit I/O register read {address:08X}");

        let low_halfword: u32 = self.read_io_register(address & !2).into();
        let high_halfword: u32 = self.read_io_register(address | 2).into();
        (high_halfword << 16) | low_halfword
    }

    // $04000130: KEYINPUT (Key status)
    fn read_keyinput(&self) -> u16 {
        u16::from(!self.inputs.a)
            | (u16::from(!self.inputs.b) << 1)
            | (u16::from(!self.inputs.select) << 2)
            | (u16::from(!self.inputs.start) << 3)
            | (u16::from(!self.inputs.right) << 4)
            | (u16::from(!self.inputs.left) << 5)
            | (u16::from(!self.inputs.up) << 6)
            | (u16::from(!self.inputs.down) << 7)
            | (u16::from(!self.inputs.r) << 8)
            | (u16::from(!self.inputs.l) << 9)
    }

    fn write_io_register(&mut self, address: u32, value: u16) {
        log::trace!("I/O register write: {address:08X} {value:04X}");

        match address {
            0x04000000..=0x0400005F => self.ppu.write_register(address, value),
            0x04000060..=0x040000AF => self.apu.write_register(address, value),
            0x040000B0..=0x040000DF => self.control.write_dma_register(address, value),
            0x040000E0..=0x040000FF => {}
            0x04000100..=0x0400010F => self.timers.write_register(address, value),
            0x04000110..=0x0400011F => {}
            0x04000120..=0x0400012F | 0x04000134..=0x0400015F => {
                log::error!("Serial register write: {address:08X} {value:04X}");
            }
            // KEYINPUT, not writable
            0x04000130 => {}
            0x04000132 => {
                log::error!("KEYCNT write: {address:08X} {value:04X}");
            }
            0x04000200 => self.control.write_ie(value),
            0x04000202 => self.control.write_if(value),
            0x04000204 => self.control.write_waitcnt(value),
            // Unused
            0x04000206 => {}
            0x04000208 => self.control.write_ime(value),
            // Unused
            0x0400020A..=0x040002FF => {}
            0x04000300 => self.control.write_postflg(value),
            // Unused/unknown
            0x04000410 => {}
            _ => todo!("I/O register write {address:08X} {value:04X}"),
        }
    }

    fn write_io_register_u8(&mut self, address: u32, value: u8) {
        log::trace!("8-bit I/O register write: {address:08X} {value:02X}");

        if (0x04000060..=0x040000AF).contains(&address) {
            // Special case 8-bit APU register writes - read-then-write doesn't work for some of these
            // because of write-only bits
            self.apu.write_register_u8(address, value);
            return;
        }

        // TODO is this safe? do any I/O register reads have side effects?
        let mut halfword = self.read_io_register(address & !1);
        if !address.bit(0) {
            halfword.set_lsb(value);
        } else {
            halfword.set_msb(value);
        }
        self.write_io_register(address & !1, halfword);
    }

    fn write_io_register_u32(&mut self, address: u32, value: u32) {
        log::trace!("32-bit I/O register write: {address:08X} {value:08X}");

        self.write_io_register(address & !2, value as u16);
        self.write_io_register(address | 2, (value >> 16) as u16);
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
            EWRAM_START..=EWRAM_END => self.memory.read_ewram_byte(address),
            MMIO_START..=MMIO_END => self.read_io_register_u8(address),
            VRAM_START..=VRAM_END => self.ppu.read_vram_byte(address),
            PALETTES_START..=PALETTES_END => self.ppu.read_palette_byte(address),
            CARTRIDGE_RAM_START..=CARTRIDGE_RAM_END => {
                self.memory.cartridge.read_sram_byte(address)
            }
            BIOS_START..=BIOS_END => self.memory.read_bios_byte(address),
            _ => todo!("read byte {address:08X}"),
        }
    }

    #[inline]
    fn read_halfword(&mut self, address: u32) -> u16 {
        match address {
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_2_END => {
                self.memory.cartridge.read_rom_halfword(address)
            }
            IWRAM_START..=IWRAM_END => self.memory.read_iwram_halfword(address),
            EWRAM_START..=EWRAM_END => self.memory.read_ewram_halfword(address),
            MMIO_START..=MMIO_END => self.read_io_register(address),
            VRAM_START..=VRAM_END => self.ppu.read_vram_halfword(address),
            OAM_START..=OAM_END => self.ppu.read_oam_halfword(address),
            PALETTES_START..=PALETTES_END => self.ppu.read_palette_halfword(address),
            BIOS_START..=BIOS_END => self.memory.read_bios_halfword(address),
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
            EWRAM_START..=EWRAM_END => self.memory.read_ewram_word(address),
            MMIO_START..=MMIO_END => self.read_io_register_u32(address),
            VRAM_START..=VRAM_END => self.ppu.read_vram_word(address),
            OAM_START..=OAM_END => self.ppu.read_oam_word(address),
            PALETTES_START..=PALETTES_END => self.ppu.read_palette_word(address),
            BIOS_START..=BIOS_END => self.memory.read_bios_word(address),
            0x10000000..=0xFFFFFFFF => {
                log::error!("Read word invalid address {address:08X}");
                // TODO should be open bus?
                !0
            }
            _ => todo!("read word {address:08X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_byte(address, value),
            EWRAM_START..=EWRAM_END => self.memory.write_ewram_byte(address, value),
            MMIO_START..=MMIO_END => self.write_io_register_u8(address, value),
            CARTRIDGE_RAM_START..=CARTRIDGE_RAM_END => {
                self.memory.cartridge.write_sram_byte(address, value);
            }
            BIOS_START..=BIOS_END => {
                log::warn!("BIOS ROM write {address:08X} {value:02X}");
            }
            _ => todo!("write byte {address:08X} {value:02X}"),
        }
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_halfword(address, value),
            EWRAM_START..=EWRAM_END => self.memory.write_ewram_halfword(address, value),
            MMIO_START..=MMIO_END => self.write_io_register(address, value),
            VRAM_START..=VRAM_END => self.ppu.write_vram_halfword(address, value),
            OAM_START..=OAM_END => self.ppu.write_oam_halfword(address, value),
            PALETTES_START..=PALETTES_END => self.ppu.write_palette_halfword(address, value),
            BIOS_START..=BIOS_END => {
                log::warn!("BIOS ROM write {address:08X} {value:04X}");
            }
            _ => todo!("write halfword {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32) {
        match address {
            IWRAM_START..=IWRAM_END => self.memory.write_iwram_word(address, value),
            EWRAM_START..=EWRAM_END => self.memory.write_ewram_word(address, value),
            MMIO_START..=MMIO_END => self.write_io_register_u32(address, value),
            VRAM_START..=VRAM_END => self.ppu.write_vram_word(address, value),
            OAM_START..=OAM_END => self.ppu.write_oam_word(address, value),
            PALETTES_START..=PALETTES_END => self.ppu.write_palette_word(address, value),
            CARTRIDGE_ROM_0_START..=CARTRIDGE_ROM_0_END => {
                log::warn!("Cartridge ROM write {address:08X} {value:08X}");
            }
            BIOS_START..=BIOS_END => {
                log::warn!("BIOS ROM write {address:08X} {value:08X}");
            }
            _ => todo!("write word {address:08X} {value:08X}"),
        }
    }

    #[inline]
    fn irq(&self) -> bool {
        self.control.irq()
    }
}
