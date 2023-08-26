use crate::core::instructions::{parity_flag, sign_flag, zero_flag, InstructionExecutor};
use crate::core::{Flags, IndexRegister, Register16, Registers};
use crate::traits::BusInterface;
use jgenesis_traits::num::GetBit;

fn compute_index_address(registers: &Registers, index: IndexRegister, offset: i8) -> u16 {
    let index_value = index.read_from(registers);
    (i32::from(index_value) + i32::from(offset)) as u16
}

macro_rules! impl_r_shift_op {
    ($name:ident, $op_fn:ident $(, thru_carry: $thru_carry:expr)?) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<(IndexRegister, i8)>) -> u32 {
            let register = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");

            match index {
                Some((index_register, offset)) => {
                    let address = compute_index_address(self.registers, index_register, offset);
                    let original = self.bus.read_memory(address);
                    let modified = $op_fn(original, $($thru_carry,)? &mut self.registers.f);

                    self.bus.write_memory(address, modified);
                    register.write_to(modified, self.registers);

                    19
                }
                None => {
                    let original = register.read_from(self.registers);
                    let modified = $op_fn(original, $($thru_carry,)? &mut self.registers.f);

                    register.write_to(modified, self.registers);

                    8
                }
            }
        }
    }
}

macro_rules! impl_hl_shift_op {
    ($name:ident, $op_fn:ident $(, thru_carry: $thru_carry:expr)?) => {
        pub(super) fn $name(&mut self, index: Option<(IndexRegister, i8)>) -> u32 {
            let address = match index {
                Some((index_register, offset)) => {
                    let index_value = index_register.read_from(self.registers);
                    (i32::from(index_value) + i32::from(offset)) as u16
                }
                None => Register16::HL.read_from(self.registers),
            };

            let original = self.bus.read_memory(address);
            let modified = $op_fn(original, $($thru_carry,)? &mut self.registers.f);

            self.bus.write_memory(address, modified);

            match index {
                Some(_) => 19,
                None => 15,
            }
        }
    }
}

macro_rules! impl_rotate_decimal_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self) -> u32 {
            let a = self.registers.a;
            let address = Register16::HL.read_from(self.registers);
            let memory_value = self.bus.read_memory(address);

            let (new_a, new_memory_value) = $op_fn(a, memory_value, &mut self.registers.f);

            self.registers.a = new_a;
            self.bus.write_memory(address, new_memory_value);

            18
        }
    };
}

macro_rules! impl_r_bit_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<(IndexRegister, i8)>) -> u32 {
            let register = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");
            let bit = (opcode >> 3) & 0x07;

            match index {
                Some((index, offset)) => {
                    let address = compute_index_address(self.registers, index, offset);
                    let original = self.bus.read_memory(address);
                    let modified = $op_fn(original, bit);

                    self.bus.write_memory(address, modified);
                    register.write_to(modified, self.registers);

                    19
                }
                None => {
                    let original = register.read_from(self.registers);
                    let modified = $op_fn(original, bit);

                    register.write_to(modified, self.registers);

                    8
                }
            }
        }
    };
}

macro_rules! impl_hl_bit_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<(IndexRegister, i8)>) -> u32 {
            let address = match index {
                Some((index, offset)) => compute_index_address(self.registers, index, offset),
                None => Register16::HL.read_from(self.registers),
            };
            let bit = (opcode >> 3) & 0x07;

            let original = self.bus.read_memory(address);
            let modified = $op_fn(original, bit);

            self.bus.write_memory(address, modified);

            match index {
                Some(_) => 19,
                None => 15,
            }
        }
    };
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    impl_r_shift_op!(rlc_r, rotate_left, thru_carry: false);
    impl_hl_shift_op!(rlc_hl, rotate_left, thru_carry: false);

    impl_r_shift_op!(rl_r, rotate_left, thru_carry: true);
    impl_hl_shift_op!(rl_hl, rotate_left, thru_carry: true);

    impl_r_shift_op!(rrc_r, rotate_right, thru_carry: false);
    impl_hl_shift_op!(rrc_hl, rotate_right, thru_carry: false);

    impl_r_shift_op!(rr_r, rotate_right, thru_carry: true);
    impl_hl_shift_op!(rr_hl, rotate_right, thru_carry: true);

    impl_r_shift_op!(sla_r, shift_left_arithmetic);
    impl_hl_shift_op!(sla_hl, shift_left_arithmetic);

    impl_r_shift_op!(sll_r, shift_left_logical);
    impl_hl_shift_op!(sll_hl, shift_left_logical);

    impl_r_shift_op!(sra_r, shift_right_arithmetic);
    impl_hl_shift_op!(sra_hl, shift_right_arithmetic);

    impl_r_shift_op!(srl_r, shift_right_logical);
    impl_hl_shift_op!(srl_hl, shift_right_logical);

    impl_rotate_decimal_op!(rld, rotate_left_decimal);
    impl_rotate_decimal_op!(rrd, rotate_right_decimal);

    impl_r_bit_op!(set_b_r, set_bit);
    impl_hl_bit_op!(set_b_hl, set_bit);

    impl_r_bit_op!(res_b_r, reset_bit);
    impl_hl_bit_op!(res_b_hl, reset_bit);

    // RLCA sets flags differently from RLC
    pub(super) fn rlca(&mut self) -> u32 {
        let a = self.registers.a;

        self.registers.a = (a << 1) | (a >> 7);
        self.registers.f = Flags {
            half_carry: false,
            subtract: false,
            carry: a.bit(7),
            ..self.registers.f
        };

        4
    }

    // RLA sets flags differently from RL
    pub(super) fn rla(&mut self) -> u32 {
        let a = self.registers.a;

        self.registers.a = (a << 1) | u8::from(self.registers.f.carry);
        self.registers.f = Flags {
            half_carry: false,
            subtract: false,
            carry: a.bit(7),
            ..self.registers.f
        };

        4
    }

    // RRCA sets flags differently from RRC
    pub(super) fn rrca(&mut self) -> u32 {
        let a = self.registers.a;

        self.registers.a = (a >> 1) | (a << 7);
        self.registers.f = Flags {
            half_carry: false,
            subtract: false,
            carry: a.bit(0),
            ..self.registers.f
        };

        4
    }

    // RRA sets flags differently from RR
    pub(super) fn rra(&mut self) -> u32 {
        let a = self.registers.a;

        self.registers.a = (a >> 1) | (u8::from(self.registers.f.carry) << 7);
        self.registers.f = Flags {
            half_carry: false,
            subtract: false,
            carry: a.bit(0),
            ..self.registers.f
        };

        4
    }

    pub(super) fn bit_b_r(&mut self, opcode: u8) -> u32 {
        let register = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");
        let bit = (opcode >> 3) & 0x07;

        bit_test(
            register.read_from(self.registers),
            bit,
            &mut self.registers.f,
        );

        8
    }

    pub(super) fn bit_b_hl(&mut self, opcode: u8, index: Option<(IndexRegister, i8)>) -> u32 {
        let address = match index {
            Some((index, offset)) => compute_index_address(self.registers, index, offset),
            None => Register16::HL.read_from(self.registers),
        };
        let value = self.bus.read_memory(address);
        let bit = (opcode >> 3) & 0x07;

        bit_test(value, bit, &mut self.registers.f);

        match index {
            Some(_) => 16,
            None => 12,
        }
    }
}

fn set_shift_flags(flags: &mut Flags, value: u8, carry: bool) {
    *flags = Flags {
        sign: sign_flag(value),
        zero: zero_flag(value),
        half_carry: false,
        overflow: parity_flag(value),
        subtract: false,
        carry,
        ..*flags
    };
}

fn rotate_left(value: u8, thru_carry: bool, flags: &mut Flags) -> u8 {
    let bit_0 = if thru_carry {
        flags.carry
    } else {
        value.bit(7)
    };
    let rotated = (value << 1) | u8::from(bit_0);

    set_shift_flags(flags, rotated, value.bit(7));

    rotated
}

fn rotate_right(value: u8, thru_carry: bool, flags: &mut Flags) -> u8 {
    let bit_7 = if thru_carry {
        flags.carry
    } else {
        value.bit(0)
    };
    let rotated = (value >> 1) | (u8::from(bit_7) << 7);

    set_shift_flags(flags, rotated, value.bit(0));

    rotated
}

fn shift_left_arithmetic(value: u8, flags: &mut Flags) -> u8 {
    let shifted = value << 1;

    set_shift_flags(flags, shifted, value.bit(7));

    shifted
}

fn shift_left_logical(value: u8, flags: &mut Flags) -> u8 {
    let shifted = (value << 1) | 0x01;

    set_shift_flags(flags, shifted, value.bit(7));

    shifted
}

fn shift_right_arithmetic(value: u8, flags: &mut Flags) -> u8 {
    let shifted = (value >> 1) | (value & 0x80);

    set_shift_flags(flags, shifted, value.bit(0));

    shifted
}

fn shift_right_logical(value: u8, flags: &mut Flags) -> u8 {
    let shifted = value >> 1;

    set_shift_flags(flags, shifted, value.bit(0));

    shifted
}

fn rotate_left_decimal(a: u8, memory_value: u8, flags: &mut Flags) -> (u8, u8) {
    let new_a = (a & 0xF0) | (memory_value >> 4);
    let new_memory_value = (memory_value << 4) | (a & 0x0F);

    set_shift_flags(flags, new_a, flags.carry);

    (new_a, new_memory_value)
}

fn rotate_right_decimal(a: u8, memory_value: u8, flags: &mut Flags) -> (u8, u8) {
    let new_a = (a & 0xF0) | (memory_value & 0x0F);
    let new_memory_value = (memory_value >> 4) | (a << 4);

    set_shift_flags(flags, new_a, flags.carry);

    (new_a, new_memory_value)
}

fn bit_test(value: u8, bit: u8, flags: &mut Flags) {
    let zero = value & (1 << bit) == 0;
    *flags = Flags {
        zero,
        half_carry: true,
        subtract: false,
        ..*flags
    };
}

fn set_bit(value: u8, bit: u8) -> u8 {
    value | (1 << bit)
}

fn reset_bit(value: u8, bit: u8) -> u8 {
    value & !(1 << bit)
}
