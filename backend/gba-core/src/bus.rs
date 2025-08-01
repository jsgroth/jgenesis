//! GBA memory map

use crate::api::BusState;
use crate::cartridge::Cartridge;
use crate::dma::{DmaState, TransferUnit};
use crate::input::GbaInputsExt;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};
use gba_config::GbaInputs;

pub struct Bus<'a> {
    pub ppu: &'a mut Ppu,
    pub memory: &'a mut Memory,
    pub cartridge: &'a mut Cartridge,
    pub dma: &'a mut DmaState,
    pub interrupts: &'a mut InterruptRegisters,
    pub inputs: &'a GbaInputs,
    pub state: BusState,
}

impl Bus<'_> {
    fn read_bios<T>(&mut self, address: u32, word_converter: impl FnOnce(u32) -> T) -> T {
        if self.state.cpu_pc >= 0x1FFFFFF {
            log::debug!("BIOS ROM read {address:08X} while PC is {:08X}", self.state.cpu_pc);
            return word_converter(self.state.last_bios_read);
        }

        let word = self.memory.read_bios_rom(address);
        self.state.last_bios_read = word;
        word_converter(word)
    }

    fn read_bios_byte(&mut self, address: u32) -> u8 {
        self.read_bios(address, |word| word.to_le_bytes()[(address & 3) as usize])
    }

    fn read_bios_halfword(&mut self, address: u32) -> u16 {
        self.read_bios(address, |word| {
            let shift = 8 * (address & 2);
            (word >> shift) as u16
        })
    }

    fn read_bios_word(&mut self, address: u32) -> u32 {
        self.read_bios(address, |word| word)
    }

    fn read_io_register(&mut self, address: u32) -> u16 {
        match address {
            0x4000000..=0x4000054 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, self.interrupts, self.dma);
                self.ppu.read_register(address)
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                self.dma.read_register(address)
            }
            0x4000130 => self.inputs.to_keyinput(),
            0x4000200 => self.interrupts.read_ie(),
            0x4000202 => self.interrupts.read_if(),
            0x4000204 => self.memory.waitcnt,
            0x4000208 => self.interrupts.read_ime(),
            _ => {
                log::warn!("Unhandled I/O register read {address:08X}");
                0
            }
        }
    }

    fn write_io_register(&mut self, address: u32, value: u16) {
        match address {
            0x4000000..=0x4000054 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, self.interrupts, self.dma);
                self.ppu.write_register(address, value);
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                self.dma.write_register(address, value);
            }
            0x4000200 => self.interrupts.write_ie(value),
            0x4000202 => self.interrupts.write_if(value),
            0x4000204 => self.memory.waitcnt = value,
            0x4000208 => self.interrupts.write_ime(value),
            _ => log::warn!("Unhandled I/O register halfword write {address:08X} {value:04X}"),
        }
    }

    fn write_io_register_byte(&mut self, address: u32, value: u8) {
        match address {
            0x4000000..=0x4000054 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, self.interrupts, self.dma);
                self.ppu.write_register_byte(address, value);
            }
            0x4000301 => {
                // TODO HALTCNT (undocumented halt register)
            }
            _ => log::warn!("Unhandled I/O register byte write {address:08X} {value:02X}"),
        }
    }

    // Returns whether a DMA is still actively in progress
    pub fn try_progress_dma(&mut self) -> bool {
        // Limit number of iterations in order to occasionally step other components, particularly the PPU
        for _ in 0..20 {
            self.dma.decrement_start_latency(self.state.cycles);

            let Some(transfer) = self.dma.next_transfer(self.interrupts) else { return false };

            // TODO better timing (e.g. N vs. S cycles)

            match transfer.unit {
                TransferUnit::Halfword => {
                    let value = self.read_halfword(transfer.source & !1, MemoryCycle::S);
                    self.write_halfword(transfer.destination & !1, value, MemoryCycle::S);
                }
                TransferUnit::Word => {
                    let value = self.read_word(transfer.source & !3, MemoryCycle::S);
                    self.write_word(transfer.destination & !3, value, MemoryCycle::S);
                }
            }

            self.ppu.step_to(self.state.cycles, self.interrupts, self.dma);
            // TODO APU
        }

        true
    }
}

impl BusInterface for Bus<'_> {
    #[inline]
    fn read_byte(&mut self, address: u32, _cycle: MemoryCycle) -> u8 {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => self.read_bios_byte(address),
            0x2000000..=0x2FFFFFF => self.memory.read_ewram_byte(address),
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_byte(address),
            0x4000000..=0x4FFFFFF => {
                let halfword = self.read_io_register(address & !1);
                (halfword >> (8 * (address & 1))) as u8
            }
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_byte(address),
            0xE000000..=0xFFFFFFF => self.cartridge.read_sram(address),
            _ => todo!("read byte {address:08X}"),
        }
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, _cycle: MemoryCycle) -> u16 {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => self.read_bios_halfword(address),
            0x2000000..=0x2FFFFFF => self.memory.read_ewram_halfword(address),
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_halfword(address),
            0x4000000..=0x4FFFFFF => self.read_io_register(address),
            0x5000000..=0x5FFFFFF => self.ppu.read_palette_ram(address),
            0x6000000..=0x6FFFFFF => self.ppu.read_vram(address),
            0x7000000..=0x7FFFFFF => self.ppu.read_oam(address),
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_halfword(address),
            _ => todo!("read halfword {address:08X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32, _cycle: MemoryCycle) -> u32 {
        fn two_halfword_reads(address: u32, mut read_fn: impl FnMut(u32) -> u16) -> u32 {
            let low_halfword = read_fn(address);
            let high_halfword = read_fn(address | 2);
            (u32::from(high_halfword) << 16) | u32::from(low_halfword)
        }

        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => self.read_bios_word(address),
            0x2000000..=0x2FFFFFF => self.memory.read_ewram_word(address),
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_word(address),
            0x4000000..=0x4FFFFFF => {
                two_halfword_reads(address, |address| self.read_io_register(address))
            }
            0x5000000..=0x5FFFFFF => {
                two_halfword_reads(address, |address| self.ppu.read_palette_ram(address))
            }
            0x6000000..=0x6FFFFFF => {
                two_halfword_reads(address, |address| self.ppu.read_vram(address))
            }
            0x7000000..=0x7FFFFFF => {
                two_halfword_reads(address, |address| self.ppu.read_oam(address))
            }
            0x8000000..=0xDFFFFFF => self.cartridge.read_rom_word(address),
            0x10000000..=0xFFFFFFFF => {
                log::warn!("Invalid address word read {address:08X}");
                0
            }
            _ => todo!("read word {address:08X}"),
        }
    }

    #[inline]
    fn fetch_opcode_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.state.cpu_pc = address;
        self.read_halfword(address, cycle)
    }

    #[inline]
    fn fetch_opcode_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.state.cpu_pc = address;
        self.read_word(address, cycle)
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8, _cycle: MemoryCycle) {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => {
                log::warn!("BIOS ROM write {address:08X} {value:02X}");
            }
            0x2000000..=0x2FFFFFF => self.memory.write_ewram_byte(address, value),
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_byte(address, value),
            0x4000000..=0x4FFFFFF => self.write_io_register_byte(address, value),
            0xE000000..=0xFFFFFFF => self.cartridge.write_sram(address, value),
            _ => todo!("write byte {address:08X} {value:02X}"),
        }
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, _cycle: MemoryCycle) {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => {
                log::warn!("BIOS ROM write {address:08X} {value:04X}");
            }
            0x2000000..=0x2FFFFFF => self.memory.write_ewram_halfword(address, value),
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_halfword(address, value),
            0x4000000..=0x4FFFFFF => self.write_io_register(address, value),
            0x5000000..=0x5FFFFFF => self.ppu.write_palette_ram(address, value),
            0x6000000..=0x6FFFFFF => self.ppu.write_vram(address, value),
            0x7000000..=0x7FFFFFF => self.ppu.write_oam(address, value),
            _ => todo!("write halfword {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, _cycle: MemoryCycle) {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x1FFFFFF => {
                log::warn!("BIOS ROM write {address:08X} {value:08X}");
            }
            0x2000000..=0x2FFFFFF => self.memory.write_ewram_word(address, value),
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_word(address, value),
            0x4000000..=0x4FFFFFF => {
                // TODO do any I/O registers need to write all 32 bits at once?
                self.write_io_register(address, value as u16);
                self.write_io_register(address | 2, (value >> 16) as u16);
            }
            0x5000000..=0x5FFFFFF => {
                self.ppu.write_palette_ram(address, value as u16);
                self.ppu.write_palette_ram(address | 2, (value >> 16) as u16);
            }
            0x6000000..=0x6FFFFFF => {
                self.ppu.write_vram(address, value as u16);
                self.ppu.write_vram(address | 2, (value >> 16) as u16);
            }
            0x7000000..=0x7FFFFFF => {
                self.ppu.write_oam(address, value as u16);
                self.ppu.write_oam(address | 2, (value >> 16) as u16);
            }
            0x8000000..=0xDFFFFFF => {
                log::warn!("Cartridge word write {address:08X} {value:08X}");
            }
            _ => todo!("write word {address:08X} {value:08X}"),
        }
    }

    #[inline]
    fn irq(&self) -> bool {
        self.interrupts.pending()
    }

    #[inline]
    fn internal_cycles(&mut self, cycles: u32) {
        self.state.cycles += u64::from(cycles);
    }
}
