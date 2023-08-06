use crate::core::instructions::InstructionExecutor;
use crate::core::{Flags, GetBit, IndexRegister, Register16};
use crate::traits::BusInterface;

macro_rules! impl_r_shift_op {
    ($name:ident, $op_fn:ident $(, thru_carry: $thru_carry:expr)?) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<(IndexRegister, i8)>) -> u32 {
            let register = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");

            match index {
                Some((index_register, offset)) => {
                    let index_value = index_register.read_from(self.registers);
                    let address = (i32::from(index_value) + i32::from(offset)) as u16;
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

    impl_r_shift_op!(sra_r, shift_right_arithmetic);
    impl_hl_shift_op!(sra_hl, shift_right_arithmetic);

    impl_r_shift_op!(srl_r, shift_right_logical);
    impl_hl_shift_op!(srl_hl, shift_right_arithmetic);

    impl_rotate_decimal_op!(rld, rotate_left_decimal);
    impl_rotate_decimal_op!(rrd, rotate_right_decimal);

    // RLCA sets flags differently from RLC
    pub(super) fn rlca(&mut self) -> u32 {
        let a = self.registers.a;
        let carry = a.bit(7);

        self.registers.a = (a << 1) | u8::from(carry);
        self.registers
            .f
            .set_half_carry(false)
            .set_subtract(false)
            .set_carry(carry);

        4
    }

    // RLA sets flags differently from RL
    pub(super) fn rla(&mut self) -> u32 {
        let a = self.registers.a;
        let carry = self.registers.f.carry();

        self.registers.a = (a << 1) | u8::from(carry);
        self.registers
            .f
            .set_half_carry(false)
            .set_subtract(false)
            .set_carry(carry);

        4
    }

    // RRCA sets flags differently from RRC
    pub(super) fn rrca(&mut self) -> u32 {
        let a = self.registers.a;
        let carry = a.bit(0);

        self.registers.a = (a >> 1) | (u8::from(carry) << 7);
        self.registers
            .f
            .set_half_carry(false)
            .set_subtract(false)
            .set_carry(carry);

        4
    }

    // RRA sets flags differently from RR
    pub(super) fn rra(&mut self) -> u32 {
        let a = self.registers.a;
        let carry = self.registers.f.carry();

        self.registers.a = (a >> 1) | (u8::from(carry) << 7);
        self.registers
            .f
            .set_half_carry(false)
            .set_subtract(false)
            .set_carry(carry);

        4
    }
}

fn set_flags(flags: &mut Flags, value: u8, carry: bool) {
    flags
        .set_sign_from(value)
        .set_zero_from(value)
        .set_half_carry(false)
        .set_parity_from(value)
        .set_subtract(false)
        .set_carry(carry);
}

fn rotate_left(value: u8, thru_carry: bool, flags: &mut Flags) -> u8 {
    let bit_0 = if thru_carry {
        flags.carry()
    } else {
        value.bit(7)
    };
    let rotated = (value << 1) | u8::from(bit_0);

    set_flags(flags, rotated, value.bit(7));

    rotated
}

fn rotate_right(value: u8, thru_carry: bool, flags: &mut Flags) -> u8 {
    let bit_7 = if thru_carry {
        flags.carry()
    } else {
        value.bit(0)
    };
    let rotated = (value >> 1) | (u8::from(bit_7) << 7);

    set_flags(flags, rotated, value.bit(0));

    rotated
}

fn shift_left_arithmetic(value: u8, flags: &mut Flags) -> u8 {
    let shifted = value << 1;

    set_flags(flags, shifted, value.bit(7));

    shifted
}

fn shift_right_arithmetic(value: u8, flags: &mut Flags) -> u8 {
    let shifted = (value >> 1) | (value >> 7);

    set_flags(flags, shifted, value.bit(0));

    shifted
}

fn shift_right_logical(value: u8, flags: &mut Flags) -> u8 {
    let shifted = value >> 1;

    set_flags(flags, shifted, value.bit(0));

    shifted
}

fn rotate_left_decimal(a: u8, memory_value: u8, flags: &mut Flags) -> (u8, u8) {
    let new_a = (a & 0xF0) | (memory_value >> 4);
    let new_memory_value = (memory_value << 4) | (a & 0x0F);

    set_flags(flags, new_a, flags.carry());

    (new_a, new_memory_value)
}

fn rotate_right_decimal(a: u8, memory_value: u8, flags: &mut Flags) -> (u8, u8) {
    let new_a = (a & 0xF0) | (memory_value & 0x0F);
    let new_memory_value = (memory_value >> 4) | (a << 4);

    set_flags(flags, new_a, flags.carry());

    (new_a, new_memory_value)
}
