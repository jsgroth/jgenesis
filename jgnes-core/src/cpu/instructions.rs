//! Code for emulating the 6502 CPU instruction set.
//!
//! Instructions are broken down into a list of cycle ops that specify exactly what the CPU should
//! do on each cycle of each instruction.

mod ops;
#[cfg(test)]
mod tests;

use bincode::{Decode, Encode};
pub use ops::execute_cycle;

#[derive(Debug, Clone, Encode, Decode)]
pub struct InstructionState {
    pub opcode: u8,
    pub executing_interrupt: bool,
    pub cycle: u8,
    pub operand_first_byte: u8,
    pub operand_second_byte: u8,
    pub target_first_byte: u8,
    pub target_second_byte: u8,
    pub indirect_byte: u8,
    pub interrupt_vector: u16,
    pub pending_interrupt: bool,
    pub instruction_complete: bool,
    pub terminated: bool,
}

impl InstructionState {
    pub fn from_opcode(opcode: u8) -> Self {
        Self {
            opcode,
            executing_interrupt: false,
            cycle: 0,
            operand_first_byte: 0,
            operand_second_byte: 0,
            target_first_byte: 0,
            target_second_byte: 0,
            indirect_byte: 0,
            interrupt_vector: 0,
            pending_interrupt: false,
            instruction_complete: false,
            terminated: false,
        }
    }

    pub fn interrupt_service() -> Self {
        Self {
            executing_interrupt: true,
            ..Self::from_opcode(0)
        }
    }
}
