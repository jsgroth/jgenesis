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

// R15 is the hardware stack pointer
const SP: usize = 15;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh2 {
    registers: Sh2Registers,
    bus_control: BusControllerRegisters,
    reset_pending: bool,
    name: String,
}

impl Sh2 {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            registers: Sh2Registers::default(),
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

        let pc = self.registers.pc;
        let opcode = bus.read_word(pc);
        self.registers.pc = self.registers.next_pc;
        self.registers.next_pc = self.registers.pc.wrapping_add(2);

        let in_delay_slot = self.registers.next_op_in_delay_slot;
        self.registers.next_op_in_delay_slot = false;

        // Interrupts cannot trigger in a delay slot per the SH7604 hardware manual
        let interrupt_level = bus.interrupt_level();
        if !in_delay_slot && interrupt_level > self.registers.sr.interrupt_mask {
            todo!("handle interrupt of level {interrupt_level}")
        }

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
            _ => todo!("Unexpected SH-2 address, word read: {address:08X}"),
        }
    }

    fn read_longword<B: BusInterface>(&mut self, address: u32, bus: &mut B) -> u32 {
        match address >> 29 {
            0 | 1 => bus.read_longword(address & 0x1FFFFFFF),
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
            7 => self.write_internal_register_longword(address, value),
            _ => todo!("Unexpected SH-2 address, longword write: {address:08X} {value:08X}"),
        }
    }
}
