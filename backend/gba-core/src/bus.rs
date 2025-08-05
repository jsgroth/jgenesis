//! GBA memory map

use crate::api::BusState;
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::{DmaState, TransferUnit};
use crate::input::GbaInputsExt;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::timers::Timers;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};
use bincode::{Decode, Encode};
use gba_config::GbaInputs;
use jgenesis_proc_macros::PartialClone;

const EWRAM_WAIT: u64 = 2;

// EWRAM has a 16-bit data bus; word accesses take an extra S cycle
const EWRAM_WAIT_WORD: u64 = 1 + 2 * EWRAM_WAIT;

// TODO accurate cartridge timing
// using the full number of wait cycles will produce too-slow timing without prefetch emulation
const ROM_WAIT: u64 = 1;

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Bus {
    pub ppu: Ppu,
    pub apu: Apu,
    pub memory: Memory,
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub dma: DmaState,
    pub timers: Timers,
    pub interrupts: InterruptRegisters,
    pub inputs: GbaInputs,
    pub state: BusState,
}

impl Bus {
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

    #[allow(clippy::match_same_arms)]
    fn read_io_register(&mut self, address: u32) -> u16 {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
                // TODO open bus if invalid address
                self.ppu.read_register(address).unwrap_or(0)
            }
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.read_register_halfword(address)
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                // TODO open bus if invalid address
                self.dma.read_register(address).unwrap_or(0)
            }
            0x4000100..=0x400010F => {
                // Timer registers
                self.timers.read_register(
                    address,
                    self.state.cycles,
                    &mut self.apu,
                    &mut self.dma,
                    &mut self.interrupts,
                )
            }
            0x4000130 => self.inputs.to_keyinput(),
            0x4000200 => self.interrupts.read_ie(),
            0x4000202 => self.interrupts.read_if(),
            0x4000204 => self.memory.read_waitcnt(),
            0x4000206 => 0, // High halfword of word-size WAITCNT reads
            0x4000208 => self.interrupts.read_ime(),
            0x400020A => 0, // High halfword of word-size IME reads
            0x4000300 => self.memory.read_postflg().into(),
            0x4000302 => 0, // High halfword of word-size POSTFLG reads
            _ => {
                log::warn!("Unhandled I/O register read {address:08X}");
                // TODO open bus if invalid address
                0
            }
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_io_register(&mut self, address: u32, value: u16) {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
                self.ppu.write_register(address, value);
            }
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.write_register_halfword(address, value);
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                self.dma.write_register(address, value, &mut self.cartridge);
            }
            0x4000100..=0x400010F => {
                // Timer registers
                self.timers.write_register(
                    address,
                    value,
                    self.state.cycles,
                    &mut self.apu,
                    &mut self.dma,
                    &mut self.interrupts,
                );
            }
            0x4000200 => self.interrupts.write_ie(value),
            0x4000202 => self.interrupts.write_if(value),
            0x4000204 => self.memory.write_waitcnt(value),
            0x4000206 => {} // High halfword of word writes to WAITCNT
            0x4000208 => self.interrupts.write_ime(value),
            0x400020A => {} // High halfword of word writes to IME
            0x4000300 => self.memory.write_postflg_haltcnt(value),
            0x4000302 => {} // High halfword of word writes to POSTFLG/HALTCNT
            _ => log::warn!("Unhandled I/O register halfword write {address:08X} {value:04X}"),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_io_register_byte(&mut self, address: u32, value: u8) {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.sync_ppu();
                self.ppu.write_register_byte(address, value);
            }
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.write_register(address, value);
            }
            0x4000202 => self.interrupts.write_if(value.into()),
            0x4000203 => self.interrupts.write_if(u16::from(value) << 8),
            0x4000208 => self.interrupts.write_ime(value.into()),
            0x4000300 => self.memory.write_postflg(value),
            0x4000301 => {
                // TODO HALTCNT (undocumented halt register)
            }
            0x4000410 => {} // Unknown; BIOS writes 0xFF to this register
            _ => log::warn!("Unhandled I/O register byte write {address:08X} {value:02X}"),
        }
    }

    // Returns whether a DMA is still actively in progress
    pub fn try_progress_dma(&mut self) -> bool {
        // Limit number of iterations in order to occasionally step other components, particularly the PPU
        for _ in 0..20 {
            self.dma.decrement_start_latency(self.state.cycles);

            let Some(transfer) = self.dma.next_transfer(&mut self.interrupts) else { return false };

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

            self.sync_ppu();
            // TODO APU
        }

        true
    }

    pub fn sync_ppu(&mut self) {
        self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
    }
}

impl BusInterface for Bus {
    #[inline]
    fn read_byte(&mut self, address: u32, _cycle: MemoryCycle) -> u8 {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x0003FFF => self.read_bios_byte(address),
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT;
                self.memory.read_ewram_byte(address)
            }
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_byte(address),
            0x4000000..=0x4FFFFFF => {
                let halfword = self.read_io_register(address & !1);
                halfword.to_le_bytes()[(address & 1) as usize]
            }
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.palette_ram_in_use());

                let halfword = self.ppu.read_palette_ram(address & !1);
                halfword.to_le_bytes()[(address & 1) as usize]
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.vram_in_use());

                let halfword = self.ppu.read_vram(address & !1);
                halfword.to_le_bytes()[(address & 1) as usize]
            }
            0x7000000..=0x7FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.oam_in_use());

                let halfword = self.ppu.read_oam(address & !1);
                halfword.to_le_bytes()[(address & 1) as usize]
            }
            0x8000000..=0xDFFFFFF => {
                self.state.cycles += ROM_WAIT;
                self.cartridge.read_rom_byte(address)
            }
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;

                // TODO open bus if invalid
                self.cartridge.read_sram(address).unwrap_or(0)
            }
            0x0004000..=0x1FFFFFF | 0x10000000..=0xFFFFFFFF => {
                // TODO open bus
                log::warn!("Open bus byte read {address:08X}");
                0
            }
        }
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, _cycle: MemoryCycle) -> u16 {
        self.state.cycles += 1;

        match address {
            0x0000000..=0x0003FFF => self.read_bios_halfword(address),
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT;
                self.memory.read_ewram_halfword(address)
            }
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_halfword(address),
            0x4000000..=0x4FFFFFF => self.read_io_register(address),
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.palette_ram_in_use());

                self.ppu.read_palette_ram(address)
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.vram_in_use());

                self.ppu.read_vram(address)
            }
            0x7000000..=0x7FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.oam_in_use());

                self.ppu.read_oam(address)
            }
            0x8000000..=0xDFFFFFF => {
                self.state.cycles += ROM_WAIT;
                self.cartridge.read_rom_halfword(address)
            }
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;

                // TODO open bus if invalid
                let byte = self.cartridge.read_sram(address).unwrap_or(0);
                u16::from_le_bytes([byte; 2])
            }
            0x0004000..=0x1FFFFFF | 0x10000000..=0xFFFFFFFF => {
                // TODO open bus
                log::warn!("Open bus halfword read {address:08X}");
                0
            }
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
            0x0000000..=0x0003FFF => self.read_bios_word(address),
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT_WORD;
                self.memory.read_ewram_word(address)
            }
            0x3000000..=0x3FFFFFF => self.memory.read_iwram_word(address),
            0x4000000..=0x4FFFFFF => {
                two_halfword_reads(address, |address| self.read_io_register(address))
            }
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                // Extra cycle for word-size palette RAM reads
                self.state.cycles += 1 + u64::from(self.ppu.palette_ram_in_use());

                two_halfword_reads(address, |address| self.ppu.read_palette_ram(address))
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                // Extra cycle for word-size VRAM reads
                self.state.cycles += 1 + u64::from(self.ppu.vram_in_use());

                two_halfword_reads(address, |address| self.ppu.read_vram(address))
            }
            0x7000000..=0x7FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.oam_in_use());

                two_halfword_reads(address, |address| self.ppu.read_oam(address))
            }
            0x8000000..=0xCFFFFFF => {
                // Cartridge ROM has a 16-bit data bus
                self.state.cycles += 1 + 2 * ROM_WAIT;
                self.cartridge.read_rom_word(address)
            }
            0xD000000..=0xDFFFFFF => 0xFFFF,
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;

                // TODO open bus if invalid
                let byte = self.cartridge.read_sram(address).unwrap_or(0);
                u32::from_le_bytes([byte; 4])
            }
            0x0004000..=0x1FFFFFF | 0x10000000..=0xFFFFFFFF => {
                // TODO open bus
                log::warn!("Open bus word read {address:08X}");
                0
            }
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
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT;
                self.memory.write_ewram_byte(address, value);
            }
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_byte(address, value),
            0x4000000..=0x4FFFFFF => self.write_io_register_byte(address, value),
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.palette_ram_in_use());

                // 8-bit writes to palette RAM duplicate the byte
                self.ppu.write_palette_ram(address & !1, u16::from_le_bytes([value; 2]));
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.vram_in_use());

                self.ppu.write_vram_byte(address, value);
            }
            0x7000000..=0x7FFFFFF => {
                // 8-bit writes to OAM are ignored
            }
            0x8000000..=0xDFFFFFF => {
                // TODO 8-bit write to EEPROM???
                self.state.cycles += ROM_WAIT;
                self.cartridge.write_rom(address, value.into());
            }
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;
                self.cartridge.write_sram(address, value);
            }
            _ => log::warn!("invalid address byte write {address:08X} {value:02X}"),
        }
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, _cycle: MemoryCycle) {
        self.state.cycles += 1;

        match address {
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT;
                self.memory.write_ewram_halfword(address, value);
            }
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_halfword(address, value),
            0x4000000..=0x4FFFFFF => self.write_io_register(address, value),
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.palette_ram_in_use());

                self.ppu.write_palette_ram(address, value);
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.vram_in_use());

                self.ppu.write_vram(address, value);
            }
            0x7000000..=0x7FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.oam_in_use());

                self.ppu.write_oam(address, value);
            }
            0x8000000..=0xDFFFFFF => {
                self.state.cycles += ROM_WAIT;
                self.cartridge.write_rom(address, value);
            }
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;
                self.cartridge.write_sram(address, value as u8);
            }
            _ => log::warn!("invalid address halfword write {address:08X} {value:04X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, _cycle: MemoryCycle) {
        self.state.cycles += 1;

        match address {
            0x2000000..=0x2FFFFFF => {
                self.state.cycles += EWRAM_WAIT_WORD;
                self.memory.write_ewram_word(address, value);
            }
            0x3000000..=0x3FFFFFF => self.memory.write_iwram_word(address, value),
            0x4000000..=0x4FFFFFF => {
                // TODO do any I/O registers need to write all 32 bits at once?
                self.write_io_register(address, value as u16);
                self.write_io_register(address | 2, (value >> 16) as u16);
            }
            0x5000000..=0x5FFFFFF => {
                self.sync_ppu();
                // Extra cycle for word-size palette RAM writes
                self.state.cycles += 1 + u64::from(self.ppu.palette_ram_in_use());

                self.ppu.write_palette_ram(address, value as u16);
                self.ppu.write_palette_ram(address | 2, (value >> 16) as u16);
            }
            0x6000000..=0x6FFFFFF => {
                self.sync_ppu();
                // Extra cycle for word-size VRAM writes
                self.state.cycles += 1 + u64::from(self.ppu.vram_in_use());

                self.ppu.write_vram(address, value as u16);
                self.ppu.write_vram(address | 2, (value >> 16) as u16);
            }
            0x7000000..=0x7FFFFFF => {
                self.sync_ppu();
                self.state.cycles += u64::from(self.ppu.oam_in_use());

                self.ppu.write_oam(address, value as u16);
                self.ppu.write_oam(address | 2, (value >> 16) as u16);
            }
            0x8000000..=0xDFFFFFF => {
                self.state.cycles += 1 + 2 * ROM_WAIT;
                self.cartridge.write_rom(address, value as u16);
                self.cartridge.write_rom(address | 2, (value >> 16) as u16);
            }
            0xE000000..=0xFFFFFFF => {
                self.state.cycles += self.memory.control().sram_wait;
                self.cartridge.write_sram(address, value as u8);
            }
            _ => log::warn!("invalid address word write {address:08X} {value:08X}"),
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
