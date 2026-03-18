use crate::core::instructions::{
    BlockMode, InstructionExecutor, parity_flag, sign_flag, zero_flag,
};
use crate::core::{Flags, Register16};
use crate::debug::BusDebugExt;
use crate::traits::BusInterface;

impl<B: BusInterface> InstructionExecutor<'_, '_, B> {
    pub(super) fn in_a_n(&mut self) -> u32 {
        let operand = self.fetch_operand();
        let io_address = u16::from_be_bytes([self.cpu.registers.a, operand]);

        self.cpu.registers.a = self.bus.read_io_debug(io_address, self.cpu);

        11
    }

    pub(super) fn in_r_c(&mut self, opcode: u8) -> u32 {
        let register = super::parse_register_from_opcode(opcode >> 3, None);
        let io_address = u16::from_be_bytes([self.cpu.registers.b, self.cpu.registers.c]);
        let value = self.bus.read_io_debug(io_address, self.cpu);

        if let Some(register) = register {
            register.write_to(value, &mut self.cpu.registers);
        }

        self.cpu.registers.f = Flags {
            sign: sign_flag(value),
            zero: zero_flag(value),
            half_carry: false,
            overflow: parity_flag(value),
            subtract: false,
            ..self.cpu.registers.f
        };

        12
    }

    pub(super) fn in_block(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let b = self.cpu.registers.b;
        let io_address = u16::from_be_bytes([b, self.cpu.registers.c]);
        let value = self.bus.read_io_debug(io_address, self.cpu);

        let hl = Register16::HL.read_from(&self.cpu.registers);
        self.bus.write_memory_debug(hl, value, self.cpu);

        self.cpu.registers.b = b.wrapping_sub(1);

        Register16::HL.write_to(mode.apply(hl), &mut self.cpu.registers);

        let should_repeat = repeat && b != 1;
        if should_repeat {
            self.cpu.registers.pc -= 2;
        }

        self.cpu.registers.f =
            Flags { zero: repeat || b == 1, subtract: true, ..self.cpu.registers.f };

        if should_repeat { 21 } else { 16 }
    }

    pub(super) fn out_n_a(&mut self) -> u32 {
        let operand = self.fetch_operand();
        let io_address = u16::from_be_bytes([self.cpu.registers.a, operand]);

        self.bus.write_io_debug(io_address, self.cpu.registers.a, self.cpu);

        11
    }

    pub(super) fn out_c_r(&mut self, opcode: u8) -> u32 {
        let register = super::parse_register_from_opcode(opcode >> 3, None);
        let io_address = u16::from_be_bytes([self.cpu.registers.b, self.cpu.registers.c]);
        let value = match register {
            Some(register) => register.read_from(&self.cpu.registers),
            None => 0,
        };

        self.bus.write_io_debug(io_address, value, self.cpu);

        12
    }

    pub(super) fn out_block(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let hl = Register16::HL.read_from(&self.cpu.registers);
        let value = self.bus.read_memory_debug(hl, self.cpu);

        let b = self.cpu.registers.b;
        self.cpu.registers.b = b.wrapping_sub(1);

        let io_address = u16::from_be_bytes([self.cpu.registers.b, self.cpu.registers.c]);
        self.bus.write_io_debug(io_address, value, self.cpu);

        Register16::HL.write_to(mode.apply(hl), &mut self.cpu.registers);

        let should_repeat = repeat && b != 1;
        if should_repeat {
            self.cpu.registers.pc -= 2;
        }

        self.cpu.registers.f =
            Flags { zero: repeat || b == 1, subtract: true, ..self.cpu.registers.f };

        if should_repeat { 21 } else { 16 }
    }
}
