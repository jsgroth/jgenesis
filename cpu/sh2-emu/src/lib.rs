pub mod bus;
mod cache;
mod disassemble;
mod divu;
mod dma;
mod frt;
mod instructions;
mod registers;
mod sci;
mod wdt;

use crate::bus::BusInterface;
use crate::cache::CpuCache;
use crate::divu::DivisionUnit;
use crate::dma::{
    DmaAddressMode, DmaChannel, DmaController, DmaTransferAddressMode, DmaTransferUnit,
};
use crate::frt::FreeRunTimer;
use crate::registers::{Sh2Registers, Sh7604Registers};
use crate::sci::SerialInterface;
use crate::wdt::WatchdogTimer;
use bincode::{Decode, Encode};
use std::env;

const RESET_PC_VECTOR: u32 = 0x00000000;
const RESET_SP_VECTOR: u32 = 0x00000004;

const RESET_INTERRUPT_MASK: u8 = 15;
const RESET_VBR: u32 = 0x00000000;

const BASE_IRL_VECTOR_NUMBER: u32 = 64;

// R15 is the hardware stack pointer
const SP: usize = 15;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh2 {
    registers: Sh2Registers,
    cache: CpuCache,
    sh7604: Sh7604Registers,
    dmac: DmaController,
    free_run_timer: FreeRunTimer,
    watchdog_timer: WatchdogTimer,
    divu: DivisionUnit,
    serial: SerialInterface,
    reset_pending: bool,
    name: String,
    trace_log_enabled: bool,
}

impl Sh2 {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new(name: String) -> Self {
        let trace_log_enabled = match env::var("SH2_LOG") {
            Ok(log_name) => name == log_name,
            Err(_) => true,
        };

        Self {
            registers: Sh2Registers::default(),
            cache: CpuCache::new(),
            sh7604: Sh7604Registers::new(),
            dmac: DmaController::new(),
            free_run_timer: FreeRunTimer::new(),
            watchdog_timer: WatchdogTimer::new(),
            divu: DivisionUnit::new(),
            serial: SerialInterface::new(name.clone()),
            reset_pending: false,
            name,
            trace_log_enabled,
        }
    }

    #[inline]
    pub fn execute<B: BusInterface>(&mut self, mut ticks: u64, bus: &mut B) {
        if ticks == 0 {
            return;
        }

        if bus.reset() {
            self.reset_pending = true;
            return;
        }

        if self.reset_pending {
            self.reset_pending = false;

            // First 8 bytes of the address space contain the reset vector and the initial SP
            // TODO use different vectors for manual reset vs. power-on reset? 32X doesn't depend on this
            self.registers.pc = bus.read_longword(RESET_PC_VECTOR);
            self.registers.next_pc = self.registers.pc.wrapping_add(2);
            self.registers.next_op_in_delay_slot = false;

            self.registers.gpr[SP] = bus.read_longword(RESET_SP_VECTOR);

            self.registers.sr.interrupt_mask = RESET_INTERRUPT_MASK;
            self.registers.vbr = RESET_VBR;

            self.cache.purge_all();

            log::trace!(
                "[{}] Reset SH-2; PC is {:08X} and SP is {:08X}",
                self.name,
                self.registers.pc,
                self.registers.gpr[SP]
            );

            return;
        }

        for _ in 0..ticks {
            if !self.try_tick_dma(bus) {
                break;
            }
        }

        // Interrupts cannot trigger in a delay slot per the SH7604 hardware manual
        // Before checking for interrupts, make sure the CPU is not in a delay slot
        if self.registers.next_op_in_delay_slot {
            self.execute_single_instruction(bus);
            ticks -= 1;
        }

        debug_assert!(
            !self.registers.next_op_in_delay_slot,
            "SH-2 executed two simultaneous delay slot instructions, PC={:08X}",
            self.registers.pc
        );

        let external_interrupt_level = bus.interrupt_level();
        let internal_interrupt_level = self.sh7604.internal_interrupt_level.priority;

        if external_interrupt_level > self.registers.sr.interrupt_mask
            && external_interrupt_level >= internal_interrupt_level
        {
            let vector_number = BASE_IRL_VECTOR_NUMBER + u32::from(external_interrupt_level >> 1);
            self.handle_interrupt(external_interrupt_level, vector_number, bus);
            return;
        }

        if internal_interrupt_level > self.registers.sr.interrupt_mask {
            let vector_number: u32 = self.sh7604.internal_interrupt_level.vector_number.into();
            self.handle_interrupt(internal_interrupt_level, vector_number, bus);
            return;
        }

        for _ in 0..ticks {
            self.execute_single_instruction(bus);
        }
    }

    #[inline]
    fn execute_single_instruction<B: BusInterface>(&mut self, bus: &mut B) {
        let pc = self.registers.pc;
        let opcode = self.read_opcode(pc, bus);
        self.registers.pc = self.registers.next_pc;
        self.registers.next_pc = self.registers.pc.wrapping_add(2);
        self.registers.next_op_in_delay_slot = false;

        if log::log_enabled!(log::Level::Trace) && self.trace_log_enabled {
            log::trace!(
                "[{}] Executing opcode {opcode:04X} at PC {pc:08X}: {}",
                self.name,
                disassemble::disassemble(opcode)
            );
            log::trace!("  Registers: {:08X?}", self.registers.gpr);
            log::trace!(
                "  GBR={:08X} VBR={:08X} PR={:08X}",
                self.registers.gbr,
                self.registers.vbr,
                self.registers.pr
            );
            log::trace!("  SR={:?}", self.registers.sr);
        }

        instructions::execute(self, opcode, bus);
    }

    #[inline]
    pub fn tick_peripherals<B: BusInterface>(&mut self, system_cycles: u64, bus: &mut B) {
        self.watchdog_timer.tick(system_cycles);
        self.serial.process(system_cycles, bus);
        self.update_internal_interrupt_level();
    }

    fn read_byte<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u8 {
        match address >> 29 {
            0 => self.cached_read_byte(address, bus),
            1 => bus.read_byte(address & 0x1FFFFFFF),
            2 => {
                log::warn!(
                    "[{}] Read byte from associative purge address: {address:08X}",
                    self.name
                );
                0
            }
            6 => self.cache.read_data_array_u8(address),
            7 => self.read_internal_register_byte(address),
            _ => todo!("Unexpected SH-2 address, byte read: {address:08X}"),
        }
    }

    fn cached_read_byte<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u8 {
        if let Some(value) = self.cache.read_u8(address) {
            return value;
        }

        if self.cache.should_replace_data() {
            let longword = self.cache.replace(address, bus);
            longword.to_be_bytes()[(address & 3) as usize]
        } else {
            bus.read_byte(address & 0x1FFFFFFF)
        }
    }

    fn read_word<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u16 {
        self.read_word_generic::<_, false>(address, bus)
    }

    fn read_opcode<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u16 {
        self.read_word_generic::<_, true>(address, bus)
    }

    #[inline]
    fn read_word_generic<B: BusInterface, const INSTRUCTION: bool>(
        &mut self,
        address: u32,
        bus: &mut B,
    ) -> u16 {
        match address >> 29 {
            0 => self.cached_read_word::<_, INSTRUCTION>(address, bus),
            1 => bus.read_word(address & 0x1FFFFFFF),
            2 => {
                log::warn!(
                    "[{}] Read word from associative purge address: {address:08X}",
                    self.name
                );
                0
            }
            6 => self.cache.read_data_array_u16(address),
            7 => self.read_internal_register_word(address),
            _ => todo!("Unexpected SH-2 address, word read: {address:08X}"),
        }
    }

    fn cached_read_word<B: BusInterface, const INSTRUCTION: bool>(
        &mut self,
        address: u32,
        bus: &mut B,
    ) -> u16 {
        if let Some(value) = self.cache.read_u16(address) {
            return value;
        }

        if (INSTRUCTION && self.cache.should_replace_instruction())
            || (!INSTRUCTION && self.cache.should_replace_data())
        {
            let longword = self.cache.replace(address, bus);
            (longword >> (16 * (((address >> 1) & 1) ^ 1))) as u16
        } else {
            bus.read_word(address & 0x1FFFFFFF)
        }
    }

    fn read_longword<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u32 {
        match address >> 29 {
            0 => self.cached_read_longword(address, bus),
            1 => bus.read_longword(address & 0x1FFFFFFF),
            2 => {
                log::warn!(
                    "[{}] Read longword from associative purge address: {address:08X}",
                    self.name
                );
                0
            }
            3 => self.cache.read_address_array(address),
            6 => self.cache.read_data_array_u32(address),
            7 => self.read_internal_register_longword(address),
            _ => todo!("Unexpected SH-2 address, longword read: {address:08X}"),
        }
    }

    fn cached_read_longword<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u32 {
        if let Some(value) = self.cache.read_u32(address) {
            return value;
        }

        if self.cache.should_replace_data() {
            self.cache.replace(address, bus)
        } else {
            bus.read_longword(address & 0x1FFFFFFF)
        }
    }

    fn write_byte<B: BusInterface>(&mut self, address: u32, value: u8, bus: &mut B) {
        match address >> 29 {
            0 => {
                bus.write_byte(address & 0x1FFFFFFF, value);
                self.cache.write_through_u8(address, value);
            }
            1 => bus.write_byte(address & 0x1FFFFFFF, value),
            2 => self.cache.associative_purge(address),
            6 => self.cache.write_data_array_u8(address, value),
            7 => self.write_internal_register_byte(address, value),
            _ => todo!("Unexpected SH-2 address, byte write: {address:08X} {value:02X}"),
        }
    }

    fn write_word<B: BusInterface>(&mut self, address: u32, value: u16, bus: &mut B) {
        match address >> 29 {
            0 => {
                bus.write_word(address & 0x1FFFFFFF, value);
                self.cache.write_through_u16(address, value);
            }
            1 => bus.write_word(address & 0x1FFFFFFF, value),
            2 => self.cache.associative_purge(address),
            6 => self.cache.write_data_array_u16(address, value),
            7 => self.write_internal_register_word(address, value),
            _ => todo!("Unexpected SH-2 address, word write: {address:08X} {value:04X}"),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_longword<B: BusInterface>(&mut self, address: u32, value: u32, bus: &mut B) {
        match address >> 29 {
            0 => {
                bus.write_longword(address & 0x1FFFFFFF, value);
                self.cache.write_through_u32(address, value);
            }
            1 => bus.write_longword(address & 0x1FFFFFFF, value),
            2 => self.cache.associative_purge(address),
            3 => self.cache.write_address_array(address, value),
            6 => self.cache.write_data_array_u32(address, value),
            7 => self.write_internal_register_longword(address, value),
            _ => todo!("Unexpected SH-2 address, longword write: {address:08X} {value:08X}"),
        }
    }

    fn handle_interrupt<B: BusInterface>(
        &mut self,
        interrupt_level: u8,
        vector_number: u32,
        bus: &mut B,
    ) {
        let mut sp = self.registers.gpr[SP].wrapping_sub(4);
        self.write_longword(sp, self.registers.sr.into(), bus);

        sp = sp.wrapping_sub(4);
        self.write_longword(sp, self.registers.pc, bus);

        self.registers.gpr[SP] = sp;
        self.registers.sr.interrupt_mask = interrupt_level;

        let vector_addr = self.registers.vbr.wrapping_add(vector_number << 2);
        self.registers.pc = self.read_longword(vector_addr, bus);
        self.registers.next_pc = self.registers.pc.wrapping_add(2);
        self.registers.next_op_in_delay_slot = false;

        log::debug!(
            "[{}] Handled interrupt of level {interrupt_level} with vector number {vector_number}, jumped to {:08X}",
            self.name,
            self.registers.pc
        );
    }

    fn try_tick_dma<B: BusInterface>(&mut self, bus: &mut B) -> bool {
        let Some(channel) = self.dmac.channel_ready(bus) else { return false };

        log::debug!(
            "[{}] Progressing DMA{channel}: src={:08X}, dest={:08X}, unit={:?}, size={:06X}",
            self.name,
            self.dmac.channels[channel].source_address,
            self.dmac.channels[channel].destination_address,
            self.dmac.channels[channel].control.transfer_size,
            self.dmac.channels[channel].transfer_count
        );

        // TODO handle single address mode?
        assert_eq!(
            self.dmac.channels[channel].control.transfer_address_mode,
            DmaTransferAddressMode::Dual
        );

        match self.dmac.channels[channel].control.transfer_size {
            DmaTransferUnit::Byte => {
                let source_addr = self.dmac.channels[channel].source_address;
                let byte = self.read_byte(source_addr, bus);

                apply_dma_source_address_mode(&mut self.dmac.channels[channel], 1);

                let dest_addr = self.dmac.channels[channel].destination_address;
                self.write_byte(dest_addr, byte, bus);

                apply_dma_destination_address_mode(&mut self.dmac.channels[channel], 1);

                self.dmac.channels[channel].transfer_count =
                    self.dmac.channels[channel].transfer_count.wrapping_sub(1);
            }
            DmaTransferUnit::Word => {
                let source_addr = self.dmac.channels[channel].source_address;
                let word = self.read_word(source_addr, bus);

                apply_dma_source_address_mode(&mut self.dmac.channels[channel], 2);

                let dest_addr = self.dmac.channels[channel].destination_address;
                self.write_word(dest_addr, word, bus);

                apply_dma_destination_address_mode(&mut self.dmac.channels[channel], 2);

                self.dmac.channels[channel].transfer_count =
                    self.dmac.channels[channel].transfer_count.wrapping_sub(1);
            }
            DmaTransferUnit::Longword => {
                let source_addr = self.dmac.channels[channel].source_address;
                let longword = self.read_longword(source_addr, bus);

                apply_dma_source_address_mode(&mut self.dmac.channels[channel], 4);

                let dest_addr = self.dmac.channels[channel].destination_address;
                self.write_longword(dest_addr, longword, bus);

                apply_dma_destination_address_mode(&mut self.dmac.channels[channel], 4);

                self.dmac.channels[channel].transfer_count =
                    self.dmac.channels[channel].transfer_count.wrapping_sub(1);
            }
            DmaTransferUnit::SixteenByte => {
                for _ in 0..4 {
                    let source_addr = self.dmac.channels[channel].source_address;
                    let longword = self.read_longword(source_addr, bus);

                    // Source address mode is ignored for 16-byte transfers
                    self.dmac.channels[channel].source_address =
                        self.dmac.channels[channel].source_address.wrapping_add(4);

                    let dest_addr = self.dmac.channels[channel].destination_address;
                    self.write_longword(dest_addr, longword, bus);

                    apply_dma_destination_address_mode(&mut self.dmac.channels[channel], 4);

                    self.dmac.channels[channel].transfer_count =
                        self.dmac.channels[channel].transfer_count.wrapping_sub(1);
                    if self.dmac.channels[channel].transfer_count == 0 {
                        break;
                    }
                }
            }
        }

        let transfer_complete = self.dmac.channels[channel].transfer_count == 0;
        self.dmac.channels[channel].control.dma_complete = transfer_complete;

        self.update_internal_interrupt_level();

        if log::log_enabled!(log::Level::Debug) && transfer_complete {
            log::debug!("[{}] DMA{channel} complete", self.name);
        }

        true
    }

    fn update_internal_interrupt_level(&mut self) {
        self.sh7604.update_interrupt_level(&self.dmac, &self.watchdog_timer, &self.serial);
    }
}

fn apply_dma_source_address_mode(channel: &mut DmaChannel, size: u32) {
    match channel.control.source_address_mode {
        DmaAddressMode::AutoIncrement => {
            channel.source_address = channel.source_address.wrapping_add(size);
        }
        DmaAddressMode::AutoDecrement => {
            channel.source_address = channel.source_address.wrapping_sub(size);
        }
        DmaAddressMode::Fixed | DmaAddressMode::Invalid => {}
    }
}

fn apply_dma_destination_address_mode(channel: &mut DmaChannel, size: u32) {
    match channel.control.destination_address_mode {
        DmaAddressMode::AutoIncrement => {
            channel.destination_address = channel.destination_address.wrapping_add(size);
        }
        DmaAddressMode::AutoDecrement => {
            channel.destination_address = channel.destination_address.wrapping_sub(size);
        }
        DmaAddressMode::Fixed | DmaAddressMode::Invalid => {}
    }
}
