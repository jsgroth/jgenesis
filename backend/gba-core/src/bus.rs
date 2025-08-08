//! GBA memory map

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::{DmaState, TransferSource, TransferUnit};
use crate::input::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::prefetch::GamePakPrefetcher;
use crate::sio::SerialPort;
use crate::timers::Timers;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle};
use bincode::{Decode, Encode};
use jgenesis_proc_macros::PartialClone;
use std::cmp;

const EWRAM_WAIT: u64 = 2;

// EWRAM has a 16-bit data bus; word accesses take an extra S cycle
const EWRAM_WAIT_WORD: u64 = 1 + 2 * EWRAM_WAIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessContext {
    CpuInstruction,
    CpuData,
    Dma,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub(crate) struct BusState {
    pub cycles: u64,
    pub cpu_pc: u32,
    pub last_bios_read: u32,
    pub open_bus: u32,
    pub active_dma_channel: Option<u8>,
    pub locked: bool,
}

impl BusState {
    pub fn new() -> Self {
        Self {
            cycles: 0,
            cpu_pc: 0,
            last_bios_read: 0,
            open_bus: 0,
            active_dma_channel: None,
            locked: false,
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Bus {
    pub ppu: Ppu,
    pub apu: Apu,
    pub memory: Memory,
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub prefetch: GamePakPrefetcher,
    pub dma: DmaState,
    pub timers: Timers,
    pub interrupts: InterruptRegisters,
    pub sio: SerialPort,
    pub inputs: InputState,
    pub state: BusState,
}

impl Bus {
    #[inline]
    fn read_byte_internal(&mut self, address: u32, cycle: MemoryCycle) -> u8 {
        self.try_progress_dma();
        self.maybe_end_rom_burst_if_not_accessed(address);

        match address {
            0x00000000..=0x00003FFF => self.read_bios_byte(address),
            0x02000000..=0x02FFFFFF => self.read_ewram_byte(address),
            0x03000000..=0x03FFFFFF => self.read_iwram_byte(address),
            0x04000000..=0x04FFFFFF => self.read_io_register_byte(address),
            0x05000000..=0x05FFFFFF => self.read_palette_ram_byte(address),
            0x06000000..=0x06FFFFFF => self.read_vram_byte(address),
            0x07000000..=0x07FFFFFF => self.read_oam_byte(address),
            0x08000000..=0x0DFFFFFF => self.read_cartridge_rom_byte(address, cycle),
            0x0E000000..=0x0FFFFFFF => self.read_cartridge_sram_byte(address),
            0x00004000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.invalid_read_byte(address),
        }
    }

    #[inline]
    fn read_halfword_internal(
        &mut self,
        address: u32,
        cycle: MemoryCycle,
        ctx: AccessContext,
    ) -> u16 {
        if ctx != AccessContext::Dma {
            self.try_progress_dma();
            self.maybe_end_rom_burst_if_not_accessed(address);
        }

        match address {
            0x00000000..=0x00003FFF => self.read_bios_halfword(address),
            0x02000000..=0x02FFFFFF => self.read_ewram_halfword(address),
            0x03000000..=0x03FFFFFF => self.read_iwram_halfword(address),
            0x04000000..=0x04FFFFFF => self.read_io_register_halfword(address),
            0x05000000..=0x05FFFFFF => self.read_palette_ram_halfword(address),
            0x06000000..=0x06FFFFFF => self.read_vram_halfword(address),
            0x07000000..=0x07FFFFFF => self.read_oam_halfword(address),
            0x08000000..=0x0DFFFFFF => self.read_cartridge_rom_halfword(address, cycle, ctx),
            0x0E000000..=0x0FFFFFFF => self.read_cartridge_sram_halfword(address),
            0x00004000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => {
                self.invalid_read_halfword(address)
            }
        }
    }

    #[inline]
    fn read_word_internal(&mut self, address: u32, cycle: MemoryCycle, ctx: AccessContext) -> u32 {
        if ctx != AccessContext::Dma {
            self.try_progress_dma();
            self.maybe_end_rom_burst_if_not_accessed(address);
        }

        match address {
            0x00000000..=0x00003FFF => self.read_bios_word(address),
            0x02000000..=0x02FFFFFF => self.read_ewram_word(address),
            0x03000000..=0x03FFFFFF => self.read_iwram_word(address),
            0x04000000..=0x04FFFFFF => self.read_io_register_word(address),
            0x05000000..=0x05FFFFFF => self.read_palette_ram_word(address),
            0x06000000..=0x06FFFFFF => self.read_vram_word(address),
            0x07000000..=0x07FFFFFF => self.read_oam_word(address),
            0x08000000..=0x0DFFFFFF => self.read_cartridge_rom_word(address, cycle, ctx),
            0x0E000000..=0x0FFFFFFF => self.read_cartridge_sram_word(address),
            0x00004000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.invalid_read_word(),
        }
    }

    #[inline]
    fn write_byte_internal(&mut self, address: u32, value: u8, cycle: MemoryCycle) {
        self.try_progress_dma();
        self.maybe_end_rom_burst_if_not_accessed(address);

        match address {
            0x02000000..=0x02FFFFFF => self.write_ewram_byte(address, value),
            0x03000000..=0x03FFFFFF => self.write_iwram_byte(address, value),
            0x04000000..=0x04FFFFFF => self.write_io_register_byte(address, value),
            0x05000000..=0x05FFFFFF => self.write_palette_ram_byte(address, value),
            0x06000000..=0x06FFFFFF => self.write_vram_byte(address, value),
            0x07000000..=0x07FFFFFF => self.write_oam_byte(value),
            0x08000000..=0x0DFFFFFF => self.write_cartridge_rom_byte(address, value, cycle),
            0x0E000000..=0x0FFFFFFF => self.write_cartridge_sram_byte(address, value),
            0x00000000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.invalid_write(),
        }
    }

    #[inline]
    fn write_halfword_internal(
        &mut self,
        address: u32,
        value: u16,
        cycle: MemoryCycle,
        ctx: AccessContext,
    ) {
        if ctx != AccessContext::Dma {
            self.try_progress_dma();
            self.maybe_end_rom_burst_if_not_accessed(address);
        }

        match address {
            0x02000000..=0x02FFFFFF => self.write_ewram_halfword(address, value),
            0x03000000..=0x03FFFFFF => self.write_iwram_halfword(address, value),
            0x04000000..=0x04FFFFFF => self.write_io_register_halfword(address, value),
            0x05000000..=0x05FFFFFF => self.write_palette_ram_halfword(address, value),
            0x06000000..=0x06FFFFFF => self.write_vram_halfword(address, value),
            0x07000000..=0x07FFFFFF => self.write_oam_halfword(address, value),
            0x08000000..=0x0DFFFFFF => self.write_cartridge_rom_halfword(address, value, cycle),
            0x0E000000..=0x0FFFFFFF => self.write_cartridge_sram_halfword(address, value),
            0x00000000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.invalid_write(),
        }
    }

    #[inline]
    fn write_word_internal(
        &mut self,
        address: u32,
        value: u32,
        cycle: MemoryCycle,
        ctx: AccessContext,
    ) {
        if ctx != AccessContext::Dma {
            self.try_progress_dma();
            self.maybe_end_rom_burst_if_not_accessed(address);
        }

        match address {
            0x02000000..=0x02FFFFFF => self.write_ewram_word(address, value),
            0x03000000..=0x03FFFFFF => self.write_iwram_word(address, value),
            0x04000000..=0x04FFFFFF => self.write_io_register_word(address, value),
            0x05000000..=0x05FFFFFF => self.write_palette_ram_word(address, value),
            0x06000000..=0x06FFFFFF => self.write_vram_word(address, value),
            0x07000000..=0x07FFFFFF => self.write_oam_word(address, value),
            0x08000000..=0x0DFFFFFF => self.write_cartridge_rom_word(address, value, cycle),
            0x0E000000..=0x0FFFFFFF => self.write_cartridge_sram_word(address, value),
            0x00000000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.invalid_write(),
        }
    }

    fn read_open_bus_byte(&self, address: u32) -> u8 {
        let byte = self.state.open_bus.to_le_bytes()[(address & 3) as usize];
        log::debug!("Open bus byte read {address:08X}, returning {byte:02X}");
        byte
    }

    fn update_open_bus_byte(&mut self, byte: u8) {
        self.state.open_bus = u32::from_ne_bytes([byte; 4]);
    }

    fn read_open_bus_halfword(&self, address: u32) -> u16 {
        let halfword = (self.state.open_bus >> (8 * (address & 2))) as u16;
        log::debug!("Open bus halfword read {address:08X}, returning {halfword:04X}");
        halfword
    }

    fn update_open_bus_halfword(&mut self, halfword: u16) {
        let halfword: u32 = halfword.into();
        self.state.open_bus = halfword | (halfword << 16);
    }

    fn increment_cycles_with_prefetch(&mut self, cycles: u64) {
        self.state.cycles += cycles;
        self.advance_prefetch(cycles);
    }

    fn try_read_bios(&mut self, address: u32) -> u32 {
        // BIOS ROM is only readable while executing out of BIOS ROM
        if self.state.cpu_pc < 0x00004000 {
            self.state.last_bios_read = self.memory.read_bios_rom(address);
        } else {
            log::debug!("BIOS ROM read {address:08X} while PC is {:08X}", self.state.cpu_pc);
        }

        self.state.last_bios_read
    }

    // $00000000-$00003FFF: BIOS ROM
    fn read_bios_byte(&mut self, address: u32) -> u8 {
        self.increment_cycles_with_prefetch(1);

        let byte = self.try_read_bios(address).to_le_bytes()[(address & 3) as usize];
        self.update_open_bus_byte(byte);

        byte
    }

    // $00000000-$00003FFF: BIOS ROM
    fn read_bios_halfword(&mut self, address: u32) -> u16 {
        self.increment_cycles_with_prefetch(1);

        let word = self.try_read_bios(address);
        let halfword = (word >> (8 * (address & 2))) as u16;
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $00000000-$00003FFF: BIOS ROM
    fn read_bios_word(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        self.state.open_bus = self.try_read_bios(address);
        self.state.open_bus
    }

    // $02000000-$02FFFFFF: EWRAM
    fn read_ewram_byte(&mut self, address: u32) -> u8 {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT);

        let byte = self.memory.read_ewram_byte(address);
        self.update_open_bus_byte(byte);

        byte
    }

    // $02000000-$02FFFFFF: EWRAM
    fn read_ewram_halfword(&mut self, address: u32) -> u16 {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT);

        let halfword = self.memory.read_ewram_halfword(address);
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $02000000-$02FFFFFF: EWRAM
    fn read_ewram_word(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT_WORD);

        self.state.open_bus = self.memory.read_ewram_word(address);
        self.state.open_bus
    }

    // $02000000-$02FFFFFF: EWRAM
    fn write_ewram_byte(&mut self, address: u32, value: u8) {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT);
        self.update_open_bus_byte(value);

        self.memory.write_ewram_byte(address, value);
    }

    // $02000000-$02FFFFFF: EWRAM
    fn write_ewram_halfword(&mut self, address: u32, value: u16) {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT);
        self.update_open_bus_halfword(value);

        self.memory.write_ewram_halfword(address, value);
    }

    // $02000000-$02FFFFFF: EWRAM
    fn write_ewram_word(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1 + EWRAM_WAIT_WORD);
        self.state.open_bus = value;

        self.memory.write_ewram_word(address, value);
    }

    fn iwram_update_open_bus_byte(&mut self, address: u32, byte: u8) {
        // IWRAM byte accesses update only the accessed 8 data lines, not all 32
        let shift = 8 * (address & 3);
        let mask = 0xFF << shift;
        self.state.open_bus = (self.state.open_bus & !mask) | (u32::from(byte) << shift);
    }

    fn iwram_update_open_bus_halfword(&mut self, address: u32, halfword: u16) {
        // IWRAM halfword accesses update only the accessed 16 data lines, not all 32
        let shift = 8 * (address & 2);
        let mask = 0xFFFF << shift;
        self.state.open_bus = (self.state.open_bus & !mask) | (u32::from(halfword) << shift);
    }

    // $03000000-$03FFFFFF: IWRAM
    fn read_iwram_byte(&mut self, address: u32) -> u8 {
        self.increment_cycles_with_prefetch(1);

        let byte = self.memory.read_iwram_byte(address);
        self.iwram_update_open_bus_byte(address, byte);

        byte
    }

    // $03000000-$03FFFFFF: IWRAM
    fn read_iwram_halfword(&mut self, address: u32) -> u16 {
        self.increment_cycles_with_prefetch(1);

        let halfword = self.memory.read_iwram_halfword(address);
        self.iwram_update_open_bus_halfword(address, halfword);

        halfword
    }

    // $03000000-$03FFFFFF: IWRAM
    fn read_iwram_word(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        self.state.open_bus = self.memory.read_iwram_word(address);
        self.state.open_bus
    }

    // $03000000-$03FFFFFF: IWRAM
    fn write_iwram_byte(&mut self, address: u32, value: u8) {
        self.increment_cycles_with_prefetch(1);
        self.iwram_update_open_bus_byte(address, value);

        self.memory.write_iwram_byte(address, value);
    }

    // $03000000-$03FFFFFF: IWRAM
    fn write_iwram_halfword(&mut self, address: u32, value: u16) {
        self.increment_cycles_with_prefetch(1);
        self.iwram_update_open_bus_halfword(address, value);

        self.memory.write_iwram_halfword(address, value);
    }

    // $03000000-$03FFFFFF: IWRAM
    fn write_iwram_word(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        self.state.open_bus = value;

        self.memory.write_iwram_word(address, value);
    }

    #[allow(clippy::match_same_arms)]
    fn read_io_register(&mut self, address: u32) -> Option<u16> {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
                self.ppu.read_register(address)
            }
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.read_register_halfword(address)
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                self.dma.read_register(address)
            }
            0x4000100..=0x400010F => {
                // Timer registers
                Some(self.timers.read_register(
                    address,
                    self.state.cycles,
                    &mut self.apu,
                    &mut self.dma,
                    &mut self.interrupts,
                ))
            }
            0x4000120..=0x400012F | 0x4000134..=0x400015F => {
                // SIO registers
                Some(self.sio.read_register(address))
            }
            0x4000130 => Some(self.inputs.read_keyinput()),
            0x4000132 => Some(self.inputs.read_keycnt()),
            0x4000200 => Some(self.interrupts.read_ie()),
            0x4000202 => Some(self.interrupts.read_if()),
            0x4000204 => Some(self.memory.read_waitcnt()),
            0x4000206 => Some(0), // High halfword of word-size WAITCNT reads
            0x4000208 => Some(self.interrupts.read_ime()),
            0x400020A => Some(0), // High halfword of word-size IME reads
            0x4000300 => Some(self.memory.read_postflg().into()),
            0x4000302 => Some(0), // High halfword of word-size POSTFLG reads
            _ => None,
        }
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    fn read_io_register_byte(&mut self, address: u32) -> u8 {
        self.increment_cycles_with_prefetch(1);

        let Some(halfword) = self.read_io_register(address & !1) else {
            return self.read_open_bus_byte(address);
        };
        let byte = halfword.to_le_bytes()[(address & 1) as usize];
        self.update_open_bus_byte(byte);

        byte
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    fn read_io_register_halfword(&mut self, address: u32) -> u16 {
        self.increment_cycles_with_prefetch(1);

        let Some(halfword) = self.read_io_register(address & !1) else {
            return self.read_open_bus_halfword(address);
        };
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    fn read_io_register_word(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        let Some(low) = self.read_io_register(address & !3) else { return self.state.open_bus };
        let Some(high) = self.read_io_register((address & !3) | 2) else {
            return self.state.open_bus;
        };

        self.state.open_bus = u32::from(low) | (u32::from(high) << 16);
        self.state.open_bus
    }

    #[allow(clippy::match_same_arms)]
    fn write_io_register(&mut self, address: u32, value: u16) {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
                self.ppu.write_register(address, value, self.state.cycles, &mut self.interrupts);
            }
            0x4000058..=0x400005F => {} // Invalid addresses
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.write_register_halfword(address, value);
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                self.dma.sync(self.state.cycles);
                self.dma.write_register(address, value, &mut self.cartridge);
            }
            0x40000E0..=0x40000FF => {} // Invalid addresses
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
            0x4000110..=0x400011F => {} // Invalid addresses
            0x4000120..=0x400012F | 0x4000134..=0x400015F => {
                // SIO registers
                self.sio.write_register(address, value, self.state.cycles);
            }
            0x4000130 => {} // KEYINPUT, not writable
            0x4000132 => self.inputs.write_keycnt(value, self.state.cycles, &mut self.interrupts),
            0x4000200 => self.interrupts.write_ie(value, self.state.cycles),
            0x4000202 => self.interrupts.write_if(value, self.state.cycles),
            0x4000204 => self.memory.write_waitcnt(value),
            0x4000206 => {} // High halfword of word writes to WAITCNT
            0x4000208 => self.interrupts.write_ime(value, self.state.cycles),
            0x400020A => {}             // High halfword of word writes to IME
            0x400020C..=0x40002FF => {} // Invalid addresses
            0x4000300 => {
                self.memory.write_postflg(value as u8);
                self.interrupts.halt_cpu();
            }
            0x4000302 => {} // High halfword of word writes to POSTFLG/HALTCNT
            _ => log::warn!("Unhandled I/O register halfword write {address:08X} {value:04X}"),
        }
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    #[allow(clippy::match_same_arms)]
    fn write_io_register_byte(&mut self, address: u32, value: u8) {
        self.increment_cycles_with_prefetch(1);
        self.update_open_bus_byte(value);

        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.sync_ppu();
                self.ppu.write_register_byte(
                    address,
                    value,
                    self.state.cycles,
                    &mut self.interrupts,
                );
            }
            0x4000060..=0x40000AF => {
                // APU registers
                self.apu.step_to(self.state.cycles);
                self.apu.write_register(address, value);
            }
            0x4000140 => self.sio.write_register(address, value.into(), self.state.cycles),
            0x4000202 => self.interrupts.write_if(value.into(), self.state.cycles),
            0x4000203 => self.interrupts.write_if(u16::from(value) << 8, self.state.cycles),
            0x4000208 => self.interrupts.write_ime(value.into(), self.state.cycles),
            0x4000300 => self.memory.write_postflg(value),
            0x4000301 => self.interrupts.halt_cpu(),
            0x4000410 => {} // Unknown; BIOS writes 0xFF to this register
            _ => log::warn!("Unhandled I/O register byte write {address:08X} {value:02X}"),
        }
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    fn write_io_register_halfword(&mut self, address: u32, value: u16) {
        self.increment_cycles_with_prefetch(1);
        self.update_open_bus_halfword(value);

        self.write_io_register(address, value);
    }

    // $04000000-$04FFFFFF: Memory-mapped I/O registers
    fn write_io_register_word(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        self.state.open_bus = value;

        self.write_io_register(address & !2, value as u16);
        self.write_io_register(address | 2, (value >> 16) as u16);
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn read_palette_ram_byte(&mut self, address: u32) -> u8 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.palette_ram_in_use()));

        let halfword = self.ppu.read_palette_ram(address);
        let byte = halfword.to_le_bytes()[(address & 1) as usize];
        self.update_open_bus_byte(byte);

        byte
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn read_palette_ram_halfword(&mut self, address: u32) -> u16 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.palette_ram_in_use()));

        let halfword = self.ppu.read_palette_ram(address);
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn read_palette_ram_word(&mut self, address: u32) -> u32 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(2 + u64::from(self.ppu.palette_ram_in_use()));

        let low: u32 = self.ppu.read_palette_ram(address & !2).into();
        let high: u32 = self.ppu.read_palette_ram(address | 2).into();
        self.state.open_bus = low | (high << 16);
        self.state.open_bus
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn write_palette_ram_byte(&mut self, address: u32, value: u8) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.palette_ram_in_use()));
        self.update_open_bus_byte(value);

        // 8-bit writes to palette RAM duplicate the byte
        self.ppu.write_palette_ram(address, u16::from_ne_bytes([value; 2]));
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn write_palette_ram_halfword(&mut self, address: u32, value: u16) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.palette_ram_in_use()));
        self.update_open_bus_halfword(value);

        self.ppu.write_palette_ram(address, value);
    }

    // $05000000-$05FFFFFF: Palette RAM
    fn write_palette_ram_word(&mut self, address: u32, value: u32) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(2 + u64::from(self.ppu.palette_ram_in_use()));
        self.state.open_bus = value;

        self.ppu.write_palette_ram(address & !2, value as u16);
        self.ppu.write_palette_ram(address | 2, (value >> 16) as u16);
    }

    // $06000000-$06FFFFFF: VRAM
    fn read_vram_byte(&mut self, address: u32) -> u8 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.vram_in_use()));

        let byte = self.ppu.read_vram(address).to_le_bytes()[(address & 1) as usize];
        self.update_open_bus_byte(byte);

        byte
    }

    // $06000000-$06FFFFFF: VRAM
    fn read_vram_halfword(&mut self, address: u32) -> u16 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.vram_in_use()));

        let halfword = self.ppu.read_vram(address);
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $06000000-$06FFFFFF: VRAM
    fn read_vram_word(&mut self, address: u32) -> u32 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(2 + u64::from(self.ppu.vram_in_use()));

        let low: u32 = self.ppu.read_vram(address & !2).into();
        let high: u32 = self.ppu.read_vram(address | 2).into();
        self.state.open_bus = low | (high << 16);
        self.state.open_bus
    }

    // $06000000-$06FFFFFF: VRAM
    fn write_vram_byte(&mut self, address: u32, value: u8) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.vram_in_use()));
        self.update_open_bus_byte(value);

        self.ppu.write_vram_byte(address, value);
    }

    // $06000000-$06FFFFFF: VRAM
    fn write_vram_halfword(&mut self, address: u32, value: u16) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1 + u64::from(self.ppu.vram_in_use()));
        self.update_open_bus_halfword(value);

        self.ppu.write_vram(address, value);
    }

    // $06000000-$06FFFFFF: VRAM
    fn write_vram_word(&mut self, address: u32, value: u32) {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(2 + u64::from(self.ppu.vram_in_use()));
        self.state.open_bus = value;

        self.ppu.write_vram(address & !2, value as u16);
        self.ppu.write_vram(address | 2, (value >> 16) as u16);
    }

    // $07000000-$07FFFFFF: OAM
    fn read_oam_byte(&mut self, address: u32) -> u8 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1);

        let byte = self.ppu.read_oam(address).to_le_bytes()[(address & 1) as usize];
        // TODO OAM open bus supposedly behaves differently?
        self.update_open_bus_byte(byte);

        byte
    }

    // $07000000-$07FFFFFF: OAM
    fn read_oam_halfword(&mut self, address: u32) -> u16 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1);

        let halfword = self.ppu.read_oam(address);
        // TODO OAM open bus supposedly behaves differently?
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $07000000-$07FFFFFF: OAM
    fn read_oam_word(&mut self, address: u32) -> u32 {
        self.sync_ppu();

        self.increment_cycles_with_prefetch(1);

        let low: u32 = self.ppu.read_oam(address & !2).into();
        let high: u32 = self.ppu.read_oam(address | 2).into();
        self.state.open_bus = low | (high << 16);
        self.state.open_bus
    }

    // $07000000-$07FFFFFF: OAM
    fn write_oam_byte(&mut self, value: u8) {
        // 8-bit OAM writes are ignored
        self.increment_cycles_with_prefetch(1);
        // TODO OAM open bus supposedly behaves differently?
        self.update_open_bus_byte(value);
    }

    // $07000000-$07FFFFFF: OAM
    fn write_oam_halfword(&mut self, address: u32, value: u16) {
        self.increment_cycles_with_prefetch(1);
        // TODO OAM open bus supposedly behaves differently?
        self.update_open_bus_halfword(value);

        self.ppu.write_oam(address, value);
    }

    // $07000000-$07FFFFFF: OAM
    fn write_oam_word(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        self.state.open_bus = value;

        self.ppu.write_oam(address & !2, value as u16);
        self.ppu.write_oam(address | 2, (value >> 16) as u16);
    }

    pub fn rom_access_cycles(&self, address: u32) -> u64 {
        if self.cartridge.rom_burst_active() {
            1 + self.memory.control().rom_s_wait_states(address)
        } else {
            1 + self.memory.control().rom_n_wait_states(address)
        }
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn read_cartridge_rom_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8 {
        self.stop_prefetch();
        self.maybe_end_rom_burst_on_access(cycle);

        self.state.cycles += self.rom_access_cycles(address);

        let byte = self.cartridge.read_rom(address).to_le_bytes()[(address & 1) as usize];
        self.update_open_bus_byte(byte);

        byte
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn read_cartridge_rom_halfword(
        &mut self,
        address: u32,
        cycle: MemoryCycle,
        ctx: AccessContext,
    ) -> u16 {
        let halfword = match ctx {
            AccessContext::CpuInstruction if self.memory.control().prefetch_enabled => {
                self.state.cycles += 1;

                self.begin_prefetch_read(address);
                self.advance_prefetch(1);
                self.prefetch_read()
            }
            _ => {
                self.stop_prefetch();
                self.maybe_end_rom_burst_on_access(cycle);

                self.state.cycles += self.rom_access_cycles(address);

                self.cartridge.read_rom(address)
            }
        };

        self.update_open_bus_halfword(halfword);
        halfword
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn read_cartridge_rom_word(
        &mut self,
        address: u32,
        cycle: MemoryCycle,
        ctx: AccessContext,
    ) -> u32 {
        let word = match ctx {
            AccessContext::CpuInstruction if self.memory.control().prefetch_enabled => {
                self.state.cycles += 1;

                self.begin_prefetch_read(address);
                self.advance_prefetch(1);

                let low: u32 = self.prefetch_read().into();
                let high: u32 = self.prefetch_read().into();
                low | (high << 16)
            }
            _ => {
                self.stop_prefetch();
                self.maybe_end_rom_burst_on_access(cycle);

                self.state.cycles += self.rom_access_cycles(address);
                let low: u32 = self.cartridge.read_rom(address & !2).into();
                self.state.cycles += self.rom_access_cycles(address);
                let high: u32 = self.cartridge.read_rom(address | 2).into();
                low | (high << 16)
            }
        };

        self.state.open_bus = word;
        word
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn write_cartridge_rom_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle) {
        self.stop_prefetch();
        self.maybe_end_rom_burst_on_access(cycle);

        self.update_open_bus_byte(value);

        self.state.cycles += self.rom_access_cycles(address);

        self.cartridge.write_rom(address, value.into());
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn write_cartridge_rom_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle) {
        self.stop_prefetch();
        self.maybe_end_rom_burst_on_access(cycle);

        self.update_open_bus_halfword(value);

        self.state.cycles += self.rom_access_cycles(address);

        self.cartridge.write_rom(address, value);
    }

    // $08000000-$0DFFFFFF: Cartridge ROM
    fn write_cartridge_rom_word(&mut self, address: u32, value: u32, cycle: MemoryCycle) {
        self.stop_prefetch();
        self.maybe_end_rom_burst_on_access(cycle);

        self.state.open_bus = value;

        self.state.cycles += self.rom_access_cycles(address);
        self.cartridge.write_rom(address & !2, value as u16);
        self.state.cycles += self.rom_access_cycles(address);
        self.cartridge.write_rom(address | 2, (value >> 16) as u16);
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn read_cartridge_sram_byte(&mut self, address: u32) -> u8 {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;

        let byte = self.cartridge.read_sram(address);
        self.update_open_bus_byte(byte);

        byte
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn read_cartridge_sram_halfword(&mut self, address: u32) -> u16 {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;

        let byte = self.cartridge.read_sram(address);

        // 16-bit reads from SRAM duplicate the byte
        let halfword = u16::from_ne_bytes([byte; 2]);
        self.update_open_bus_halfword(halfword);

        halfword
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn read_cartridge_sram_word(&mut self, address: u32) -> u32 {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;

        let byte = self.cartridge.read_sram(address);

        // 32-bit reads from SRAM duplicate the byte
        self.state.open_bus = u32::from_ne_bytes([byte; 4]);
        self.state.open_bus
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn write_cartridge_sram_byte(&mut self, address: u32, value: u8) {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;
        self.update_open_bus_byte(value);

        self.cartridge.write_sram(address, value);
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn write_cartridge_sram_halfword(&mut self, address: u32, value: u16) {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;
        self.update_open_bus_halfword(value);

        // 16-bit SRAM writes only write the addressed byte
        self.cartridge.write_sram(address, value.to_le_bytes()[(address & 1) as usize]);
    }

    // $0E000000-$0FFFFFFF: Cartridge SRAM
    fn write_cartridge_sram_word(&mut self, address: u32, value: u32) {
        self.stop_prefetch();

        self.state.cycles += 1 + self.memory.control().sram_wait;
        self.state.open_bus = value;

        // 32-bit SRAM writes only write the addressed byte
        self.cartridge.write_sram(address, value.to_le_bytes()[(address & 3) as usize]);
    }

    // $00004000-$01FFFFFF / $10000000-$FFFFFFFF: Invalid addresses
    fn invalid_read_byte(&mut self, address: u32) -> u8 {
        self.increment_cycles_with_prefetch(1);
        self.read_open_bus_byte(address)
    }

    // $00004000-$01FFFFFF / $10000000-$FFFFFFFF: Invalid addresses
    fn invalid_read_halfword(&mut self, address: u32) -> u16 {
        self.increment_cycles_with_prefetch(1);
        self.read_open_bus_halfword(address)
    }

    // $00004000-$01FFFFFF / $10000000-$FFFFFFFF: Invalid addresses
    fn invalid_read_word(&mut self) -> u32 {
        self.increment_cycles_with_prefetch(1);
        self.state.open_bus
    }

    // $00004000-$01FFFFFF / $10000000-$FFFFFFFF: Invalid addresses
    fn invalid_write(&mut self) {
        self.increment_cycles_with_prefetch(1);
        // TODO do writes to invalid addresses update open bus?
    }

    pub fn try_progress_dma(&mut self) {
        if self.state.locked {
            // DMA cannot run while CPU is locking the bus (SWAP instruction)
            return;
        }

        loop {
            self.dma.sync(self.state.cycles);

            let Some(transfer) = self.dma.next_transfer(&mut self.interrupts, self.state.cycles)
            else {
                if self.state.active_dma_channel.is_some() {
                    // TODO fix this - should only end ROM burst if DMA accesses ROM
                    // Idle cycle and end ROM burst when DMA finishes
                    self.increment_cycles_with_prefetch(1);

                    self.cartridge.end_rom_burst();
                }
                self.state.active_dma_channel = None;

                return;
            };

            if self.state.active_dma_channel.is_none() {
                // Idle cycle and end ROM burst when DMA starts
                self.increment_cycles_with_prefetch(1);
                self.cartridge.end_rom_burst();
            } else if self.state.active_dma_channel != Some(transfer.channel) {
                // End ROM burst when channel changes
                self.cartridge.end_rom_burst();
            }
            self.state.active_dma_channel = Some(transfer.channel);

            match transfer.unit {
                TransferUnit::Halfword => {
                    let value = match transfer.source {
                        TransferSource::Memory { address } => {
                            let value = self.read_halfword_internal(
                                address & !1,
                                MemoryCycle::S,
                                AccessContext::Dma,
                            );
                            self.dma.update_read_latch_halfword(transfer.channel, value);
                            value
                        }
                        TransferSource::Value(value) => {
                            let shift = 8 * (transfer.destination & 2);
                            (value >> shift) as u16
                        }
                    };
                    self.write_halfword_internal(
                        transfer.destination & !1,
                        value,
                        MemoryCycle::S,
                        AccessContext::Dma,
                    );
                }
                TransferUnit::Word => {
                    let value = match transfer.source {
                        TransferSource::Memory { address } => {
                            let value = self.read_word_internal(
                                address & !3,
                                MemoryCycle::S,
                                AccessContext::Dma,
                            );
                            self.dma.update_read_latch_word(transfer.channel, value);
                            value
                        }
                        TransferSource::Value(value) => value,
                    };
                    self.write_word_internal(
                        transfer.destination & !3,
                        value,
                        MemoryCycle::S,
                        AccessContext::Dma,
                    );
                }
            }

            self.sync_ppu();
            self.sync_timers();
        }
    }

    pub fn sync_ppu(&mut self) {
        self.ppu.step_to(self.state.cycles, &mut self.interrupts, &mut self.dma);
    }

    pub fn sync_timers(&mut self) {
        self.timers.step_to(self.state.cycles, &mut self.apu, &mut self.dma, &mut self.interrupts);
    }

    fn maybe_end_rom_burst_on_access(&mut self, cycle: MemoryCycle) {
        // N cycles always end ROM burst when prefetch is disabled
        if cycle == MemoryCycle::N {
            self.cartridge.end_rom_burst();
        }
    }

    fn maybe_end_rom_burst_if_not_accessed(&mut self, address: u32) {
        // When prefetch is disabled, cycles that don't access ROM end any in-progress ROM burst
        // mgba-suite timing tests exercise this (LDMIA that overflows from OAM to ROM)
        // This does not apply to DMA
        if !self.memory.control().prefetch_enabled && address < 0x08000000 {
            self.cartridge.end_rom_burst();
        }
    }
}

impl BusInterface for Bus {
    #[inline]
    fn read_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8 {
        self.read_byte_internal(address, cycle)
    }

    #[inline]
    fn read_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.read_halfword_internal(address, cycle, AccessContext::CpuData)
    }

    #[inline]
    fn read_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.read_word_internal(address, cycle, AccessContext::CpuData)
    }

    #[inline]
    fn fetch_opcode_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.state.cpu_pc = address;
        self.read_halfword_internal(address, cycle, AccessContext::CpuInstruction)
    }

    #[inline]
    fn fetch_opcode_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.state.cpu_pc = address;
        self.read_word_internal(address, cycle, AccessContext::CpuInstruction)
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle) {
        self.write_byte_internal(address, value, cycle);
    }

    #[inline]
    fn write_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle) {
        self.write_halfword_internal(address, value, cycle, AccessContext::CpuData);
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u32, cycle: MemoryCycle) {
        self.write_word_internal(address, value, cycle, AccessContext::CpuData);
    }

    #[inline]
    fn irq(&self) -> bool {
        self.interrupts.pending()
    }

    #[inline]
    fn internal_cycles(&mut self, cycles: u32) {
        // It seems like DMA can run during internal cycles without halting the CPU?
        let new_cycles = self.state.cycles + u64::from(cycles);
        self.try_progress_dma();
        self.state.cycles = cmp::max(self.state.cycles, new_cycles);
        self.advance_prefetch(cycles.into());

        // "Prefetch disabled bug"
        // When the CPU takes an internal cycle while prefetch is disabled, any in-progress ROM burst ends
        // This forces the next ROM access to use N-cycle timings
        if !self.memory.control().prefetch_enabled {
            self.cartridge.end_rom_burst();
        }
    }

    #[inline]
    fn lock(&mut self) {
        self.state.locked = true;
    }

    #[inline]
    fn unlock(&mut self) {
        self.state.locked = false;
    }
}
