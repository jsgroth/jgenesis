use crate::core::instructions::{
    parity_flag, sign_flag, zero_flag, BlockMode, InstructionExecutor,
};
use crate::core::{Flags, Register16};
use crate::traits::BusInterface;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn in_a_n(&mut self) -> u32 {
        let operand = self.fetch_operand();
        let io_address = u16::from_be_bytes([self.registers.a, operand]);

        self.registers.a = self.bus.read_io(io_address);

        11
    }

    pub(super) fn in_r_c(&mut self, opcode: u8) -> u32 {
        let register = super::parse_register_from_opcode(opcode >> 3, None);
        let io_address = u16::from_be_bytes([self.registers.b, self.registers.c]);
        let value = self.bus.read_io(io_address);

        if let Some(register) = register {
            register.write_to(value, self.registers);
        }

        self.registers.f = Flags {
            sign: sign_flag(value),
            zero: zero_flag(value),
            half_carry: false,
            overflow: parity_flag(value),
            subtract: false,
            ..self.registers.f
        };

        12
    }

    pub(super) fn in_block(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let b = self.registers.b;
        let io_address = u16::from_be_bytes([b, self.registers.c]);
        let value = self.bus.read_io(io_address);

        let hl = Register16::HL.read_from(self.registers);
        self.bus.write_memory(hl, value);

        self.registers.b = b.wrapping_sub(1);

        Register16::HL.write_to(mode.apply(hl), self.registers);

        let should_repeat = repeat && b != 1;
        if should_repeat {
            self.registers.pc -= 2;
        }

        self.registers.f = Flags {
            zero: repeat || b == 1,
            subtract: true,
            ..self.registers.f
        };

        if should_repeat {
            21
        } else {
            16
        }
    }

    pub(super) fn out_n_a(&mut self) -> u32 {
        let operand = self.fetch_operand();
        let io_address = u16::from_be_bytes([self.registers.a, operand]);

        self.bus.write_io(io_address, self.registers.a);

        11
    }

    pub(super) fn out_c_r(&mut self, opcode: u8) -> u32 {
        let register = super::parse_register_from_opcode(opcode >> 3, None);
        let io_address = u16::from_be_bytes([self.registers.b, self.registers.c]);
        let value = match register {
            Some(register) => register.read_from(self.registers),
            None => 0,
        };

        self.bus.write_io(io_address, value);

        12
    }

    pub(super) fn out_block(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let hl = Register16::HL.read_from(self.registers);
        let value = self.bus.read_memory(hl);

        let b = self.registers.b;
        self.registers.b = b.wrapping_sub(1);

        let io_address = u16::from_be_bytes([self.registers.b, self.registers.c]);
        self.bus.write_io(io_address, value);

        Register16::HL.write_to(mode.apply(hl), self.registers);

        let should_repeat = repeat && b != 1;
        if should_repeat {
            self.registers.pc -= 2;
        }

        self.registers.f = Flags {
            zero: repeat || b == 1,
            subtract: true,
            ..self.registers.f
        };

        if should_repeat {
            21
        } else {
            16
        }
    }
}
