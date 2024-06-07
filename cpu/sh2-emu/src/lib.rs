pub mod bus;
mod instructions;

use crate::bus::BusInterface;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct StatusRegister {
    t: bool,
    s: bool,
    interrupt_mask: u8,
    q: bool,
    m: bool,
}

impl Default for StatusRegister {
    fn default() -> Self {
        Self { t: false, s: false, interrupt_mask: 0xF, q: false, m: false }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Registers {
    // General-purpose registers
    gpr: [u32; 16],
    // Status register
    sr: StatusRegister,
    // Global base register (used with GBR addressing modes)
    gbr: u32,
    // Vector base register (base of exception vector area)
    vbr: u32,
    // Multiply-accumulator
    macl: u32,
    mach: u32,
    // Procedure register (return address)
    pr: u32,
    // Program counter
    pc: u32,
    delayed_branch: Option<u32>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh2 {
    registers: Registers,
    reset_pending: bool,
}

impl Sh2 {
    #[must_use]
    pub fn new() -> Self {
        Self { registers: Registers::default(), reset_pending: false }
    }

    pub fn tick<B: BusInterface>(&mut self, bus: &mut B) {
        if bus.reset() {
            self.reset_pending = true;
            return;
        }

        if self.reset_pending {
            self.reset_pending = false;

            // First 8 bytes of the vector table contain the reset vector and the initial SP
            // TODO use different vectors for manual reset vs. power-on reset? 32X doesn't depend on this
            self.registers.pc = bus.read_longword(self.registers.vbr);
            self.registers.gpr[15] = bus.read_longword(self.registers.vbr.wrapping_add(4));

            return;
        }

        let opcode = bus.read_word(self.registers.pc);
        let (new_pc, in_delay_slot) = match self.registers.delayed_branch.take() {
            Some(pc) => (pc, true),
            None => (self.registers.pc.wrapping_add(2), false),
        };
        self.registers.pc = new_pc;

        // Interrupts cannot trigger in a delay slot per the SH7604 hardware manual
        let interrupt_level = bus.interrupt_level();
        if !in_delay_slot && interrupt_level > self.registers.sr.interrupt_mask {
            todo!("handle interrupt of level {interrupt_level}")
        }

        instructions::execute(self, opcode, bus);
    }
}
