pub mod bus;
mod disassemble;
mod instructions;
mod registers;

use crate::bus::BusInterface;
use crate::registers::{BusControllerRegisters, Sh2Registers};
use bincode::{Decode, Encode};

const RESET_PC_VECTOR: u32 = 0x00000000;
const RESET_SP_VECTOR: u32 = 0x00000004;

const RESET_INTERRUPT_MASK: u8 = 15;
const RESET_VBR: u32 = 0x00000000;

const BASE_IRL_VECTOR_NUMBER: u32 = 64;

// R15 is the hardware stack pointer
const SP: usize = 15;

const CACHE_LEN: usize = 4 * 1024;

type CpuCache = [u8; CACHE_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh2 {
    registers: Sh2Registers,
    cache: Box<CpuCache>,
    bus_control: BusControllerRegisters,
    reset_pending: bool,
    name: String,
}

impl Sh2 {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            registers: Sh2Registers::default(),
            cache: vec![0; CACHE_LEN].into_boxed_slice().try_into().unwrap(),
            bus_control: BusControllerRegisters::new(),
            reset_pending: false,
            name,
        }
    }

    pub fn tick<B: BusInterface>(&mut self, bus: &mut B) {
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

            log::trace!(
                "[{}] Reset SH-2; PC is {:08X} and SP is {:08X}",
                self.name,
                self.registers.pc,
                self.registers.gpr[SP]
            );

            return;
        }

        // Interrupts cannot trigger in a delay slot per the SH7604 hardware manual
        // TODO check for internal peripheral interrupts
        let interrupt_level = bus.interrupt_level();
        if !self.registers.next_op_in_delay_slot
            && interrupt_level > self.registers.sr.interrupt_mask
        {
            self.handle_irl_interrupt(interrupt_level, bus);
            return;
        }

        let pc = self.registers.pc;
        let opcode = self.read_word(pc, bus);
        self.registers.pc = self.registers.next_pc;
        self.registers.next_pc = self.registers.pc.wrapping_add(2);
        self.registers.next_op_in_delay_slot = false;

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "[{}] Executing opcode {opcode:04X} at PC {pc:08X}: {}",
                self.name,
                disassemble::disassemble(opcode)
            );
            log::trace!("  Registers: {:08X?}", self.registers.gpr);
        }

        instructions::execute(self, opcode, bus);
    }

    fn read_byte<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u8 {
        match address >> 29 {
            0 | 1 => bus.read_byte(address & 0x1FFFFFFF),
            _ => todo!("Unexpected SH-2 address, byte read: {address:08X}"),
        }
    }

    fn read_word<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u16 {
        match address >> 29 {
            0 | 1 => bus.read_word(address & 0x1FFFFFFF),
            6 => self.read_cache_u16(address),
            _ => todo!("Unexpected SH-2 address, word read: {address:08X}"),
        }
    }

    fn read_longword<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u32 {
        match address >> 29 {
            0 | 1 => bus.read_longword(address & 0x1FFFFFFF),
            6 => self.read_cache_u32(address),
            7 => self.read_internal_register_longword(address),
            _ => todo!("Unexpected SH-2 address, longword read: {address:08X}"),
        }
    }

    fn write_byte<B: BusInterface>(&mut self, address: u32, value: u8, bus: &mut B) {
        match address >> 29 {
            0 | 1 => bus.write_byte(address & 0x1FFFFFFF, value),
            7 => self.write_internal_register_byte(address, value),
            _ => todo!("Unexpected SH-2 address, byte write: {address:08X} {value:02X}"),
        }
    }

    fn write_word<B: BusInterface>(&mut self, address: u32, value: u16, bus: &mut B) {
        match address >> 29 {
            0 | 1 => bus.write_word(address & 0x1FFFFFFF, value),
            7 => self.write_internal_register_word(address, value),
            _ => todo!("Unexpected SH-2 address, word write: {address:08X} {value:04X}"),
        }
    }

    fn write_longword<B: BusInterface>(&mut self, address: u32, value: u32, bus: &mut B) {
        match address >> 29 {
            0 | 1 => bus.write_longword(address & 0x1FFFFFFF, value),
            6 => self.write_cache_u32(address, value),
            7 => self.write_internal_register_longword(address, value),
            _ => todo!("Unexpected SH-2 address, longword write: {address:08X} {value:08X}"),
        }
    }

    fn read_cache_u16(&self, address: u32) -> u16 {
        let cache_addr = (address as usize) & (CACHE_LEN - 1) & !1;
        u16::from_be_bytes([self.cache[cache_addr], self.cache[cache_addr + 1]])
    }

    fn read_cache_u32(&self, address: u32) -> u32 {
        let cache_addr = (address as usize) & (CACHE_LEN - 1) & !3;
        u32::from_be_bytes(self.cache[cache_addr..cache_addr + 4].try_into().unwrap())
    }

    fn write_cache_u32(&mut self, address: u32, value: u32) {
        let cache_addr = (address as usize) & (CACHE_LEN - 1) & !3;
        self.cache[cache_addr..cache_addr + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn handle_irl_interrupt<B: BusInterface>(&mut self, interrupt_level: u8, bus: &mut B) {
        let mut sp = self.registers.gpr[SP].wrapping_sub(4);
        self.write_longword(sp, self.registers.sr.into(), bus);

        sp = sp.wrapping_sub(4);
        self.write_longword(sp, self.registers.pc, bus);

        self.registers.gpr[SP] = sp;
        self.registers.sr.interrupt_mask = interrupt_level;

        let vector_number = BASE_IRL_VECTOR_NUMBER + u32::from(interrupt_level >> 1);
        let vector_addr = self.registers.vbr.wrapping_add(vector_number << 2);
        self.registers.pc = self.read_longword(vector_addr, bus);
        self.registers.next_pc = self.registers.pc.wrapping_add(2);
        self.registers.next_op_in_delay_slot = false;

        log::debug!("Handled IRL{interrupt_level} interrupt, jumped to {:08X}", self.registers.pc);
    }
}
