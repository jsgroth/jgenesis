//! GBA memory map and bus code

use crate::apu;
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::{DmaState, TransferUnit};
use crate::input::InputState;
use crate::interrupts::{InterruptRegisters, InterruptType};
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::prefetch::GamePakPrefetcher;
use crate::scheduler::{Scheduler, SchedulerEvent};
use crate::sio::SerialPort;
use crate::timers::Timers;
use arm7tdmi_emu::bus::{BusInterface, MemoryCycle, OpSize};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::PartialClone;

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

struct AccessCtx;

impl AccessCtx {
    const CPU_INSTRUCTION: u8 = 0;
    const CPU_DATA: u8 = 1;
    const DMA: u8 = 2;
}

macro_rules! invalid_size {
    ($size:expr) => {
        panic!("Invalid size, must be 0-2: {}", $size)
    };
}

fn word_to_size<const SIZE: u8>(value: u32, address: u32) -> u32 {
    match SIZE {
        OpSize::BYTE => (value >> (8 * (address & 3))) & 0xFF,
        OpSize::HALFWORD => (value >> (8 * (address & 2))) & 0xFFFF,
        OpSize::WORD => value,
        _ => invalid_size!(SIZE),
    }
}

#[derive(Debug, Clone, PartialClone, Encode, Decode)]
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
    pub scheduler: Scheduler,
}

impl Bus {
    #[inline]
    fn read_internal<const SIZE: u8, const CTX: u8>(
        &mut self,
        address: u32,
        cycle: MemoryCycle,
    ) -> u32 {
        if CTX != AccessCtx::DMA {
            self.try_progress_dma();
            self.end_rom_burst_if_not_accessed(address);
            self.interrupts.cpu_bus_cycle(self.state.cycles);
        }

        match address {
            0x00000000..=0x00003FFF => self.read_bios_rom::<SIZE>(address),
            0x02000000..=0x02FFFFFF => self.read_ewram::<SIZE>(address),
            0x03000000..=0x03FFFFFF => self.read_iwram::<SIZE>(address),
            0x04000000..=0x04FFFFFF => self.read_io_register::<SIZE>(address),
            0x05000000..=0x05FFFFFF => self.read_palette_ram::<SIZE>(address),
            0x06000000..=0x06FFFFFF => self.read_vram::<SIZE>(address),
            0x07000000..=0x07FFFFFF => self.read_oam::<SIZE>(address),
            0x08000000..=0x0DFFFFFF => self.read_cartridge_rom::<SIZE, CTX>(address, cycle),
            0x0E000000..=0x0FFFFFFF => self.read_cartridge_sram::<SIZE>(address),
            0x00004000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.read_invalid::<SIZE>(address),
        }
    }

    #[inline]
    fn write_internal<const SIZE: u8, const CTX: u8>(
        &mut self,
        address: u32,
        value: u32,
        cycle: MemoryCycle,
    ) {
        if CTX != AccessCtx::DMA {
            self.try_progress_dma();
            self.end_rom_burst_if_not_accessed(address);
            self.interrupts.cpu_bus_cycle(self.state.cycles);
        }

        match address {
            0x02000000..=0x02FFFFFF => self.write_ewram::<SIZE>(address, value),
            0x03000000..=0x03FFFFFF => self.write_iwram::<SIZE>(address, value),
            0x04000000..=0x04FFFFFF => self.write_io_register::<SIZE>(address, value),
            0x05000000..=0x05FFFFFF => self.write_palette_ram::<SIZE>(address, value),
            0x06000000..=0x06FFFFFF => self.write_vram::<SIZE>(address, value),
            0x07000000..=0x07FFFFFF => self.write_oam::<SIZE>(address, value),
            0x08000000..=0x0DFFFFFF => self.write_cartridge_rom::<SIZE>(address, value, cycle),
            0x0E000000..=0x0FFFFFFF => self.write_cartridge_sram::<SIZE>(address, value),
            0x00000000..=0x01FFFFFF | 0x10000000..=0xFFFFFFFF => self.write_invalid(),
        }
    }

    fn increment_cycles_with_prefetch(&mut self, cycles: u64) {
        self.state.cycles += cycles;
        self.advance_prefetch(cycles);
    }

    fn read_open_bus<const SIZE: u8>(&mut self, address: u32) -> u32 {
        log::debug!("Open bus read of size {}: {address:08X}", OpSize::display(SIZE));
        word_to_size::<SIZE>(self.state.open_bus, address)
    }

    fn update_open_bus<const SIZE: u8>(&mut self, value: u32) -> u32 {
        match SIZE {
            OpSize::BYTE => {
                self.state.open_bus = u32::from_ne_bytes([value as u8; 4]);
            }
            OpSize::HALFWORD => {
                self.state.open_bus = (value & 0xFFFF) | (value << 16);
            }
            OpSize::WORD => {
                self.state.open_bus = value;
            }
            _ => invalid_size!(SIZE),
        }

        value
    }

    fn iwram_update_open_bus<const SIZE: u8>(&mut self, value: u32, address: u32) -> u32 {
        // 8-bit and 16-bit IWRAM accesses only update the accessed bits
        match SIZE {
            OpSize::BYTE => {
                let shift = 8 * (address & 3);
                self.state.open_bus &= !(0xFF << shift);
                self.state.open_bus |= (value & 0xFF) << shift;
            }
            OpSize::HALFWORD => {
                let shift = 8 * (address & 2);
                self.state.open_bus &= !(0xFFFF << shift);
                self.state.open_bus |= (value & 0xFFFF) << shift;
            }
            OpSize::WORD => {
                self.state.open_bus = value;
            }
            _ => invalid_size!(SIZE),
        }

        value
    }

    fn read_bios_rom<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        // BIOS ROM is only readable while executing out of BIOS ROM
        // Reads from other regions return the last value successfully fetched from BIOS ROM
        if self.state.cpu_pc < 0x00004000 {
            self.state.last_bios_read = self.memory.read_bios_rom(address);
        } else {
            log::debug!("BIOS ROM read {address:08X} while PC is {:08X}", self.state.cpu_pc);
        }

        let value = word_to_size::<SIZE>(self.state.last_bios_read, address);
        self.update_open_bus::<SIZE>(value)
    }

    fn ewram_access_cycles<const SIZE: u8>() -> u64 {
        // EWRAM only has a 16-bit data bus, so 32-bit accesses take twice as long
        match SIZE {
            OpSize::BYTE | OpSize::HALFWORD => 3,
            OpSize::WORD => 6,
            _ => invalid_size!(SIZE),
        }
    }

    fn read_ewram<const SIZE: u8>(&mut self, address: u32) -> u32 {
        let cycles = Self::ewram_access_cycles::<SIZE>();
        self.increment_cycles_with_prefetch(cycles);

        let value = match SIZE {
            OpSize::BYTE => self.memory.read_ewram_byte(address).into(),
            OpSize::HALFWORD => self.memory.read_ewram_halfword(address).into(),
            OpSize::WORD => self.memory.read_ewram_word(address),
            _ => invalid_size!(SIZE),
        };

        self.update_open_bus::<SIZE>(value)
    }

    fn write_ewram<const SIZE: u8>(&mut self, address: u32, value: u32) {
        let cycles = Self::ewram_access_cycles::<SIZE>();
        self.increment_cycles_with_prefetch(cycles);
        self.update_open_bus::<SIZE>(value);

        match SIZE {
            OpSize::BYTE => self.memory.write_ewram_byte(address, value as u8),
            OpSize::HALFWORD => self.memory.write_ewram_halfword(address, value as u16),
            OpSize::WORD => self.memory.write_ewram_word(address, value),
            _ => invalid_size!(SIZE),
        }
    }

    fn read_iwram<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        let value = match SIZE {
            OpSize::BYTE => self.memory.read_iwram_byte(address).into(),
            OpSize::HALFWORD => self.memory.read_iwram_halfword(address).into(),
            OpSize::WORD => self.memory.read_iwram_word(address),
            _ => invalid_size!(SIZE),
        };

        self.iwram_update_open_bus::<SIZE>(value, address)
    }

    fn write_iwram<const SIZE: u8>(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        self.iwram_update_open_bus::<SIZE>(value, address);

        match SIZE {
            OpSize::BYTE => self.memory.write_iwram_byte(address, value as u8),
            OpSize::HALFWORD => self.memory.write_iwram_halfword(address, value as u16),
            OpSize::WORD => self.memory.write_iwram_word(address, value),
            _ => invalid_size!(SIZE),
        }
    }

    fn read_io_register<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);

        let Some(value) = self.try_read_io_register::<SIZE>(address) else {
            return self.read_open_bus::<SIZE>(address);
        };

        self.update_open_bus::<SIZE>(value)
    }

    fn try_read_io_register<const SIZE: u8>(&mut self, address: u32) -> Option<u32> {
        let value = match SIZE {
            OpSize::BYTE => {
                let halfword = self.read_io_register_internal(address & !1)?;
                halfword.to_le_bytes()[(address & 1) as usize].into()
            }
            OpSize::HALFWORD => self.read_io_register_internal(address & !1)?.into(),
            OpSize::WORD => {
                let low: u32 = self.read_io_register_internal(address & !3)?.into();
                let high: u32 = self.read_io_register_internal((address & !3) | 2)?.into();
                low | (high << 16)
            }
            _ => invalid_size!(SIZE),
        };
        Some(value)
    }

    #[allow(clippy::match_same_arms)]
    fn read_io_register_internal(&mut self, address: u32) -> Option<u16> {
        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                self.ppu.read_register(
                    address,
                    self.state.cycles,
                    &mut self.dma,
                    &mut self.scheduler,
                )
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
                    &mut self.scheduler,
                ))
            }
            0x4000120..=0x400012F | 0x4000134..=0x400015F => {
                // SIO registers
                Some(self.sio.read_register(address))
            }
            0x4000130 => Some(self.inputs.read_keyinput()),
            0x4000132 => Some(self.inputs.read_keycnt()),
            0x4000200 => Some(self.interrupts.read_ie(self.state.cycles)),
            0x4000202 => Some(self.interrupts.read_if(self.state.cycles)),
            0x4000204 => Some(self.memory.read_waitcnt()),
            0x4000206 => Some(0), // High halfword of word-size WAITCNT reads
            0x4000208 => Some(self.interrupts.read_ime(self.state.cycles)),
            0x400020A => Some(0), // High halfword of word-size IME reads
            0x4000300 => Some(self.memory.read_postflg().into()),
            0x4000302 => Some(0), // High halfword of word-size POSTFLG reads
            _ => None,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_io_register<const SIZE: u8>(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        self.update_open_bus::<SIZE>(value);

        if SIZE == OpSize::WORD {
            if (apu::FIFO_A_ADDRESS..apu::FIFO_B_ADDRESS + 4).contains(&address) {
                // Special case 32-bit Direct Sound FIFO writes because 16-bit and 32-bit writes behave differently
                self.apu.write_register_word(address, value);
            } else {
                // Other 32-bit writes behave identically to two consecutive 16-bit writes
                self.write_io_register_internal::<{ OpSize::HALFWORD }>(address & !3, value as u16);
                self.write_io_register_internal::<{ OpSize::HALFWORD }>(
                    (address & !3) | 2,
                    (value >> 16) as u16,
                );
            }
            return;
        }

        self.write_io_register_internal::<SIZE>(address, value as u16);
    }

    #[allow(clippy::match_same_arms)]
    fn write_io_register_internal<const SIZE: u8>(&mut self, address: u32, value: u16) {
        // Registers where an 8-bit write can be implemented as reading the existing 16-bit value,
        // modifying the target byte, then writing back the modified 16-bit value.
        // This does not work for all registers because many registers are write-only, or have
        // different semantics for reads vs. writes (e.g. IF, timer reload/counter registers)
        const BYTE_READ_THEN_WRITE_ADDRS: &[u32] = &[
            0x4000132, // KEYCNT
            0x4000200, // IE
            0x4000204, // WAITCNT
        ];

        assert_ne!(SIZE, OpSize::WORD);

        if SIZE == OpSize::BYTE && BYTE_READ_THEN_WRITE_ADDRS.contains(&(address & !1)) {
            let existing = self.read_io_register_internal(address & !1).unwrap_or(0);
            let mut bytes = existing.to_le_bytes();
            bytes[(address & 1) as usize] = value as u8;
            self.write_io_register_internal::<{ OpSize::HALFWORD }>(
                address & !1,
                u16::from_le_bytes(bytes),
            );
            return;
        }

        match address {
            0x4000000..=0x4000057 => {
                // PPU registers
                match SIZE {
                    OpSize::BYTE => self.ppu.write_register_byte(
                        address,
                        value as u8,
                        self.state.cycles,
                        &mut self.dma,
                        &mut self.interrupts,
                        &mut self.scheduler,
                    ),
                    OpSize::HALFWORD => self.ppu.write_register(
                        address & !1,
                        value,
                        self.state.cycles,
                        &mut self.dma,
                        &mut self.interrupts,
                        &mut self.scheduler,
                    ),
                    _ => invalid_size!(SIZE),
                }
            }
            0x4000058..=0x400005F => {} // Invalid addresses
            0x4000060..=0x40000AF => {
                // APU registers
                match SIZE {
                    OpSize::BYTE => self.apu.write_register(address, value as u8),
                    OpSize::HALFWORD => self.apu.write_register_halfword(address & !1, value),
                    _ => invalid_size!(SIZE),
                }
            }
            0x40000B0..=0x40000DF => {
                // DMA registers
                match SIZE {
                    OpSize::BYTE => {
                        self.dma.write_register_byte(
                            address,
                            value as u8,
                            self.state.cycles,
                            &mut self.cartridge,
                        );
                    }
                    OpSize::HALFWORD => {
                        self.dma.write_register(
                            address & !1,
                            value,
                            self.state.cycles,
                            &mut self.cartridge,
                        );
                    }
                    _ => invalid_size!(SIZE),
                }
            }
            0x4000100..=0x400010F => {
                // Timer registers
                match SIZE {
                    OpSize::BYTE => self.timers.write_register_byte(
                        address,
                        value as u8,
                        self.state.cycles,
                        &mut self.apu,
                        &mut self.dma,
                        &mut self.interrupts,
                        &mut self.scheduler,
                    ),
                    OpSize::HALFWORD => self.timers.write_register(
                        address,
                        value,
                        self.state.cycles,
                        &mut self.apu,
                        &mut self.dma,
                        &mut self.interrupts,
                        &mut self.scheduler,
                    ),
                    _ => invalid_size!(SIZE),
                }
            }
            0x4000120..=0x400012F | 0x4000134..=0x400015A => {
                // Serial port registers
                self.sio.write_register(address & !1, value, self.state.cycles);
            }
            0x4000132..=0x4000133 => {
                // KEYCNT
                self.inputs.write_keycnt(value, self.state.cycles, &mut self.interrupts);
            }
            0x4000200..=0x4000201 => {
                // IE
                self.interrupts.write_ie(value, self.state.cycles);
            }
            0x4000202..=0x4000203 => {
                // IF
                let mut value = value;
                if SIZE == OpSize::BYTE {
                    value = (value & 0xFF) << (8 * (address & 1));
                }
                self.interrupts.write_if(value, self.state.cycles);
            }
            0x4000204..=0x4000205 => {
                // WAITCNT
                self.memory.write_waitcnt(value);
            }
            0x4000208..=0x4000209 => {
                // IME
                self.interrupts.write_ime(value, self.state.cycles);
            }
            0x4000300..=0x4000301 => {
                // POSTFLG/HALTCNT
                match SIZE {
                    OpSize::BYTE => {
                        if !address.bit(0) {
                            self.memory.write_postflg(value as u8);
                        } else {
                            self.write_haltcnt(value as u8);
                        }
                    }
                    OpSize::HALFWORD => {
                        let [lsb, msb] = value.to_le_bytes();
                        self.memory.write_postflg(lsb);
                        self.write_haltcnt(msb);
                    }
                    _ => invalid_size!(SIZE),
                }
            }
            0x4000410 => {} // Unknown; BIOS writes to this register
            _ => log::debug!("Unhandled I/O register write {address:08X} {value:04X}"),
        }
    }

    fn write_haltcnt(&mut self, value: u8) {
        if self.state.cpu_pc >= 0x00004000 {
            // HALTCNT is only writable when executing from BIOS ROM
            log::debug!("Attempted to write to HALTCNT while PC is {:08X}", self.state.cpu_pc);
            return;
        }

        self.interrupts.write_haltcnt(value);
    }

    fn block_until_palette_ram_free(&mut self) {
        loop {
            self.increment_cycles_with_prefetch(1);
            self.sync_ppu();
            if !self.ppu.palette_ram_in_use() {
                break;
            }
        }
    }

    fn read_palette_ram<const SIZE: u8>(&mut self, address: u32) -> u32 {
        if SIZE == OpSize::WORD {
            let low = self.read_palette_ram::<{ OpSize::HALFWORD }>(address & !2);
            let high = self.read_palette_ram::<{ OpSize::HALFWORD }>(address | 2);
            let value = (low & 0xFFFF) | (high << 16);
            return self.update_open_bus::<{ OpSize::WORD }>(value);
        }

        self.block_until_palette_ram_free();

        let mut value = self.ppu.read_palette_ram(address);
        if SIZE == OpSize::BYTE {
            value = value.to_le_bytes()[(address & 1) as usize].into();
        }

        self.update_open_bus::<SIZE>(value.into())
    }

    fn write_palette_ram<const SIZE: u8>(&mut self, address: u32, value: u32) {
        if SIZE == OpSize::WORD {
            self.write_palette_ram::<{ OpSize::HALFWORD }>(address & !2, value & 0xFFFF);
            self.write_palette_ram::<{ OpSize::HALFWORD }>(address | 2, value >> 16);
            self.update_open_bus::<SIZE>(value);
            return;
        }

        self.update_open_bus::<SIZE>(value);

        self.block_until_palette_ram_free();

        match SIZE {
            OpSize::BYTE => {
                // 8-bit writes to palette RAM perform a 16-bit write with the byte duplicated
                self.ppu.write_palette_ram(address, u16::from_ne_bytes([value as u8; 2]));
            }
            OpSize::HALFWORD => {
                self.ppu.write_palette_ram(address, value as u16);
            }
            _ => invalid_size!(SIZE),
        }
    }

    fn block_until_vram_free(&mut self, address: u32) {
        loop {
            self.increment_cycles_with_prefetch(1);
            self.sync_ppu();
            if !self.ppu.vram_in_use(address) {
                break;
            }
        }
    }

    fn read_vram<const SIZE: u8>(&mut self, address: u32) -> u32 {
        if SIZE == OpSize::WORD {
            let low = self.read_vram::<{ OpSize::HALFWORD }>(address & !2);
            let high = self.read_vram::<{ OpSize::HALFWORD }>(address | 2);
            let value = (low & 0xFFFF) | (high << 16);
            return self.update_open_bus::<{ OpSize::WORD }>(value);
        }

        self.block_until_vram_free(address);

        let mut value = self.ppu.read_vram(address);
        if SIZE == OpSize::BYTE {
            value = value.to_le_bytes()[(address & 1) as usize].into();
        }

        self.update_open_bus::<SIZE>(value.into())
    }

    fn write_vram<const SIZE: u8>(&mut self, address: u32, value: u32) {
        if SIZE == OpSize::WORD {
            self.write_vram::<{ OpSize::HALFWORD }>(address & !2, value & 0xFFFF);
            self.write_vram::<{ OpSize::HALFWORD }>(address | 2, value >> 16);
            self.update_open_bus::<{ OpSize::WORD }>(value);
            return;
        }

        self.update_open_bus::<SIZE>(value);

        self.block_until_vram_free(address);

        match SIZE {
            OpSize::BYTE => {
                self.ppu.write_vram_byte(address, value as u8);
            }
            OpSize::HALFWORD => {
                self.ppu.write_vram(address, value as u16);
            }
            _ => invalid_size!(SIZE),
        }
    }

    fn read_oam<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);
        // TODO should block if OAM is in use - requires precise tracking of when PPU accesses OAM

        let value = match SIZE {
            OpSize::BYTE | OpSize::HALFWORD => {
                let mut value = self.ppu.read_oam(address);
                if SIZE == OpSize::BYTE {
                    value = value.to_le_bytes()[(address & 1) as usize].into();
                }
                value.into()
            }
            OpSize::WORD => {
                let low: u32 = self.ppu.read_oam(address & !2).into();
                let high: u32 = self.ppu.read_oam(address | 2).into();
                low | (high << 16)
            }
            _ => invalid_size!(SIZE),
        };

        // TODO supposedly OAM accesses update open bus differently from other non-IWRAM regions?
        self.update_open_bus::<SIZE>(value)
    }

    fn write_oam<const SIZE: u8>(&mut self, address: u32, value: u32) {
        self.increment_cycles_with_prefetch(1);
        // TODO should block if OAM is in use - requires precise tracking of when PPU accesses OAM

        // TODO supposedly OAM accesses update open bus differently from other non-IWRAM regions?
        self.update_open_bus::<SIZE>(value);

        match SIZE {
            OpSize::BYTE => {
                // 8-bit writes to OAM are ignored
            }
            OpSize::HALFWORD => {
                self.ppu.write_oam(address, value as u16);
            }
            OpSize::WORD => {
                self.ppu.write_oam(address & !2, value as u16);
                self.ppu.write_oam(address | 2, (value >> 16) as u16);
            }
            _ => invalid_size!(SIZE),
        }
    }

    pub fn rom_access_cycles(&self, address: u32) -> u64 {
        if self.cartridge.rom_burst_active() {
            self.memory.control().rom_s_cycles(address)
        } else {
            self.memory.control().rom_n_cycles(address)
        }
    }

    fn read_cartridge_rom<const SIZE: u8, const CTX: u8>(
        &mut self,
        mut address: u32,
        cycle: MemoryCycle,
    ) -> u32 {
        if SIZE == OpSize::WORD {
            address &= !3;
        }

        let prefetch_enabled = self.memory.control().prefetch_enabled;
        let value = if CTX == AccessCtx::CPU_INSTRUCTION
            && (prefetch_enabled || self.prefetch.can_use_for(address))
        {
            assert_ne!(SIZE, OpSize::BYTE);

            if prefetch_enabled {
                self.prepare_prefetch_read(address);
            }

            self.state.cycles += 1;
            self.advance_prefetch(1);
            let mut opcode: u32 = self.prefetch_read().into();

            if SIZE == OpSize::WORD {
                let high: u32 = if prefetch_enabled || self.prefetch.can_use_for(address | 2) {
                    self.prefetch_read().into()
                } else {
                    self.stop_prefetch();
                    self.state.cycles += self.rom_access_cycles(address | 2);
                    self.cartridge.read_rom(address | 2).into()
                };
                opcode |= high << 16;
            }

            opcode
        } else {
            self.stop_prefetch();
            self.maybe_end_rom_burst_on_access(cycle);

            self.state.cycles += self.rom_access_cycles(address);
            let mut value: u32 = self.cartridge.read_rom(address).into();

            match SIZE {
                OpSize::BYTE => {
                    value = value.to_le_bytes()[(address & 1) as usize].into();
                }
                OpSize::HALFWORD => {}
                OpSize::WORD => {
                    self.state.cycles += self.rom_access_cycles(address | 2);
                    let high: u32 = self.cartridge.read_rom(address | 2).into();
                    value |= high << 16;
                }
                _ => invalid_size!(SIZE),
            }

            value
        };

        self.update_open_bus::<SIZE>(value)
    }

    fn write_cartridge_rom<const SIZE: u8>(
        &mut self,
        mut address: u32,
        value: u32,
        cycle: MemoryCycle,
    ) {
        if SIZE == OpSize::WORD {
            address &= !3;
        }

        self.stop_prefetch();
        self.maybe_end_rom_burst_on_access(cycle);

        self.update_open_bus::<SIZE>(value);

        self.state.cycles += self.rom_access_cycles(address);
        self.cartridge.write_rom(address, value as u16);

        if SIZE == OpSize::WORD {
            self.state.cycles += self.rom_access_cycles(address | 2);
            self.cartridge.write_rom(address | 2, (value >> 16) as u16);
        }
    }

    fn read_cartridge_sram<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.stop_prefetch();

        self.state.cycles += self.memory.control().sram_cycles;

        let byte = self.cartridge.read_sram(address);

        // SRAM only has an 8-bit data bus; 16-bit and 32-bit reads duplicate the byte
        let value = match SIZE {
            OpSize::BYTE => byte.into(),
            OpSize::HALFWORD => u16::from_ne_bytes([byte; 2]).into(),
            OpSize::WORD => u32::from_ne_bytes([byte; 4]),
            _ => invalid_size!(SIZE),
        };

        self.update_open_bus::<SIZE>(value)
    }

    fn write_cartridge_sram<const SIZE: u8>(&mut self, address: u32, value: u32) {
        self.stop_prefetch();

        self.state.cycles += self.memory.control().sram_cycles;
        self.update_open_bus::<SIZE>(value);

        // SRAM only has an 8-bit data bus; 16-bit and 32-bit writes only update one byte
        match SIZE {
            OpSize::BYTE => self.cartridge.write_sram(address, value as u8),
            OpSize::HALFWORD => {
                let byte = value.to_le_bytes()[(address & 1) as usize];
                self.cartridge.write_sram(address, byte);
            }
            OpSize::WORD => {
                let byte = value.to_le_bytes()[(address & 3) as usize];
                self.cartridge.write_sram(address, byte);
            }
            _ => invalid_size!(SIZE),
        }
    }

    fn read_invalid<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.increment_cycles_with_prefetch(1);
        self.read_open_bus::<SIZE>(address)
    }

    fn write_invalid(&mut self) {
        self.increment_cycles_with_prefetch(1);
        // TODO do writes to invalid addresses update open bus?
    }

    pub fn try_progress_dma(&mut self) {
        struct AccessedRom(bool);

        impl AccessedRom {
            fn check(&mut self, address: u32, end_rom_burst: impl FnOnce()) {
                if !self.0 && address >= 0x8000000 {
                    end_rom_burst();
                    self.0 = true;
                }
            }
        }

        if self.state.locked {
            // DMA cannot run while CPU is locking the bus (SWAP instruction)
            return;
        }

        let mut accessed_rom = AccessedRom(false);

        loop {
            self.process_scheduler_events();

            let Some(transfer) = self.dma.next_transfer(&mut self.interrupts, self.state.cycles)
            else {
                if self.state.active_dma_channel.is_some() {
                    // Idle cycle and end ROM burst when DMA finishes
                    self.increment_cycles_with_prefetch(1);

                    if accessed_rom.0 || !self.memory.control().prefetch_enabled {
                        self.cartridge.end_rom_burst();
                    }
                }
                self.state.active_dma_channel = None;

                return;
            };

            if self.state.active_dma_channel.is_none() {
                // Idle cycle and end ROM burst when DMA starts
                self.increment_cycles_with_prefetch(1);
            } else if self.state.active_dma_channel != Some(transfer.channel) {
                // End ROM burst when channel changes if previous channel accessed ROM
                if accessed_rom.0 {
                    self.cartridge.end_rom_burst();
                }
                accessed_rom.0 = false;
            }
            self.state.active_dma_channel = Some(transfer.channel);

            match transfer.unit {
                TransferUnit::Halfword => {
                    let value = if let Some(address) = transfer.source {
                        accessed_rom.check(address, || self.cartridge.end_rom_burst());

                        let value = self.read_internal::<{ OpSize::HALFWORD }, { AccessCtx::DMA }>(
                            address & !1,
                            MemoryCycle::S,
                        );
                        self.dma.update_read_latch_halfword(transfer.channel, value as u16);
                        value
                    } else {
                        // Invalid address; reads latched value
                        self.increment_cycles_with_prefetch(1);

                        let shift = 8 * (transfer.destination & 2);
                        (transfer.read_latch >> shift) & 0xFFFF
                    };

                    accessed_rom.check(transfer.destination, || self.cartridge.end_rom_burst());
                    self.write_internal::<{ OpSize::HALFWORD }, { AccessCtx::DMA }>(
                        transfer.destination & !1,
                        value,
                        MemoryCycle::S,
                    );
                }
                TransferUnit::Word => {
                    let value = if let Some(address) = transfer.source {
                        accessed_rom.check(address, || self.cartridge.end_rom_burst());

                        let value = self.read_internal::<{ OpSize::WORD }, { AccessCtx::DMA }>(
                            address & !3,
                            MemoryCycle::S,
                        );
                        self.dma.update_read_latch_word(transfer.channel, value);
                        value
                    } else {
                        // Invalid address; reads latched value
                        self.increment_cycles_with_prefetch(1);
                        transfer.read_latch
                    };

                    accessed_rom.check(transfer.destination, || self.cartridge.end_rom_burst());
                    self.write_internal::<{ OpSize::WORD }, { AccessCtx::DMA }>(
                        transfer.destination & !3,
                        value,
                        MemoryCycle::S,
                    );
                }
            }
        }
    }

    pub fn sync_ppu(&mut self) {
        self.ppu.step_to(self.state.cycles, &mut self.dma, &mut self.scheduler);
    }

    pub fn sync_timers(&mut self) {
        self.timers.step_to(
            self.state.cycles,
            &mut self.apu,
            &mut self.dma,
            &mut self.interrupts,
            &mut self.scheduler,
        );
    }

    pub fn process_scheduler_events(&mut self) {
        if !self.scheduler.is_event_ready(self.state.cycles) {
            return;
        }

        while let Some((event, cycles)) = self.scheduler.pop(self.state.cycles) {
            match event {
                SchedulerEvent::VBlankIrq => {
                    self.interrupts.set_flag(InterruptType::VBlank, cycles);
                    self.ppu.schedule_next_vblank_irq(&mut self.scheduler, cycles);
                }
                SchedulerEvent::HBlankIrq => {
                    self.interrupts.set_flag(InterruptType::HBlank, cycles);
                    self.ppu.schedule_next_hblank_irq(&mut self.scheduler, cycles);
                }
                SchedulerEvent::VCounterIrq => {
                    self.interrupts.set_flag(InterruptType::VCounter, cycles);
                    self.ppu.schedule_next_v_counter_irq(&mut self.scheduler, cycles);
                }
                SchedulerEvent::PpuEvent => {
                    self.sync_ppu();
                }
                SchedulerEvent::TimerOverflow => {
                    self.sync_timers();
                }
                SchedulerEvent::Dummy => {}
            }
        }
    }

    fn maybe_end_rom_burst_on_access(&mut self, cycle: MemoryCycle) {
        // N cycles always end ROM burst when prefetch is disabled
        if cycle == MemoryCycle::N {
            self.cartridge.end_rom_burst();
        }
    }

    fn end_rom_burst_if_not_accessed(&mut self, address: u32) {
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
    fn read<const SIZE: u8>(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.read_internal::<SIZE, { AccessCtx::CPU_DATA }>(address, cycle)
    }

    #[inline]
    fn fetch_opcode<const SIZE: u8>(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.state.cpu_pc = address;
        self.read_internal::<SIZE, { AccessCtx::CPU_INSTRUCTION }>(address, cycle)
    }

    #[inline]
    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, cycle: MemoryCycle) {
        self.write_internal::<SIZE, { AccessCtx::CPU_DATA }>(address, value, cycle);
    }

    #[inline]
    fn irq(&self) -> bool {
        self.interrupts.irq()
    }

    #[inline]
    fn internal_cycles(&mut self, cycles: u32) {
        // DMA can run during internal cycles without halting the CPU
        let new_cycles = self.state.cycles + u64::from(cycles);
        while self.state.cycles < new_cycles {
            self.try_progress_dma();
            self.interrupts.cpu_bus_cycle(self.state.cycles);

            if self.state.cycles < new_cycles {
                self.increment_cycles_with_prefetch(1);
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{GbaAudioConfig, GbaEmulatorConfig};

    #[test]
    fn no_io_addresses_panic() {
        let mut bus = Bus {
            ppu: Ppu::new(),
            apu: Apu::new(GbaAudioConfig::default()),
            memory: Memory::new(vec![0; 16 * 1024], GbaEmulatorConfig::default()).unwrap(),
            cartridge: Cartridge::new(vec![0; 4 * 1024 * 1024], None, None, None),
            prefetch: GamePakPrefetcher::new(),
            dma: DmaState::new(),
            timers: Timers::new(),
            interrupts: InterruptRegisters::new(),
            sio: SerialPort::new(),
            inputs: InputState::new(),
            state: BusState::new(),
            scheduler: Scheduler::new(),
        };

        for address in 0x04000000..=0x0400FFFF {
            bus.read_byte(address, MemoryCycle::S);
            bus.read_halfword(address, MemoryCycle::S);
            bus.read_word(address, MemoryCycle::S);
            bus.write_byte(address, !0, MemoryCycle::S);
            bus.write_halfword(address, !0, MemoryCycle::S);
            bus.write_word(address, !0, MemoryCycle::S);
        }
    }
}
