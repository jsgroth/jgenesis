use crate::core::instructions::{
    BlockMode, InstructionExecutor, parity_flag, sign_flag, zero_flag,
};
use crate::core::{Flags, IndexRegister, Register16};
use crate::traits::BusInterface;
use jgenesis_common::num::GetBit;

macro_rules! impl_a_r_add_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(
            &mut self,
            opcode: u8,
            index: Option<IndexRegister>,
            with_carry: bool,
        ) -> u32 {
            let read_target =
                super::parse_register_from_opcode(opcode, index).expect("invalid opcode");
            let operand = read_target.read_from(self.registers);

            self.registers.a = $op_fn(self.registers.a, operand, with_carry, &mut self.registers.f);

            4
        }
    };
}

macro_rules! impl_a_immediate_add_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, with_carry: bool) -> u32 {
            let operand = self.fetch_operand();

            self.registers.a = $op_fn(self.registers.a, operand, with_carry, &mut self.registers.f);

            7
        }
    };
}

macro_rules! impl_a_hl_add_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, index: Option<IndexRegister>, with_carry: bool) -> u32 {
            let address = self.fetch_indirect_hl_address(index);
            let operand = self.bus.read_memory(address);

            self.registers.a = $op_fn(self.registers.a, operand, with_carry, &mut self.registers.f);

            match index {
                Some(_) => 15,
                None => 7,
            }
        }
    };
}

macro_rules! impl_a_r_bit_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
            let read_target =
                super::parse_register_from_opcode(opcode, index).expect("invalid opcode");
            let operand = read_target.read_from(self.registers);

            self.registers.a = $op_fn(self.registers.a, operand, &mut self.registers.f);

            4
        }
    };
}

macro_rules! impl_a_immediate_bit_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self) -> u32 {
            let operand = self.fetch_operand();

            self.registers.a = $op_fn(self.registers.a, operand, &mut self.registers.f);

            7
        }
    };
}

macro_rules! impl_a_hl_bit_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, index: Option<IndexRegister>) -> u32 {
            let address = self.fetch_indirect_hl_address(index);
            let operand = self.bus.read_memory(address);

            self.registers.a = $op_fn(self.registers.a, operand, &mut self.registers.f);

            match index {
                Some(_) => 15,
                None => 7,
            }
        }
    };
}

macro_rules! impl_r_increment_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
            let register =
                super::parse_register_from_opcode(opcode >> 3, index).expect("invalid opcode");
            let original = register.read_from(self.registers);
            let modified = $op_fn(original, &mut self.registers.f);

            register.write_to(modified, self.registers);

            4
        }
    };
}

macro_rules! impl_hl_increment_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, index: Option<IndexRegister>) -> u32 {
            let address = self.fetch_indirect_hl_address(index);
            let original = self.bus.read_memory(address);
            let modified = $op_fn(original, &mut self.registers.f);

            self.bus.write_memory(address, modified);

            match index {
                Some(_) => 19,
                None => 11,
            }
        }
    };
}

macro_rules! impl_16_bit_increment_op {
    ($name:ident, $op_fn:ident) => {
        pub(super) fn $name(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
            let register = super::parse_dd_register(opcode, index);
            let original = register.read_from(self.registers);
            let modified = $op_fn(original);

            register.write_to(modified, self.registers);

            6
        }
    };
}

impl<B: BusInterface> InstructionExecutor<'_, '_, B> {
    impl_a_r_add_op!(add_a_r, add);
    impl_a_immediate_add_op!(add_a_immediate, add);
    impl_a_hl_add_op!(add_a_hl, add);

    impl_a_r_add_op!(sub_a_r, subtract);
    impl_a_immediate_add_op!(sub_a_immediate, subtract);
    impl_a_hl_add_op!(sub_a_hl, subtract);

    impl_a_r_bit_op!(and_a_r, and);
    impl_a_immediate_bit_op!(and_a_immediate, and);
    impl_a_hl_bit_op!(and_a_hl, and);

    impl_a_r_bit_op!(or_a_r, or);
    impl_a_immediate_bit_op!(or_a_immediate, or);
    impl_a_hl_bit_op!(or_a_hl, or);

    impl_a_r_bit_op!(xor_a_r, xor);
    impl_a_immediate_bit_op!(xor_a_immediate, xor);
    impl_a_hl_bit_op!(xor_a_hl, xor);

    impl_a_r_bit_op!(cp_a_r, compare);
    impl_a_immediate_bit_op!(cp_a_immediate, compare);
    impl_a_hl_bit_op!(cp_a_hl, compare);

    impl_r_increment_op!(inc_r, increment);
    impl_hl_increment_op!(inc_hl, increment);

    impl_r_increment_op!(dec_r, decrement);
    impl_hl_increment_op!(dec_hl, decrement);

    impl_16_bit_increment_op!(inc_ss, increment_u16);
    impl_16_bit_increment_op!(dec_ss, decrement_u16);

    pub(super) fn add_hl_ss(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let l_register = index.map_or(Register16::HL, IndexRegister::into);
        let r_register = super::parse_dd_register(opcode, index);

        let l_value = l_register.read_from(self.registers);
        let r_value = r_register.read_from(self.registers);

        let sum = add_u16(l_value, r_value, false, &mut self.registers.f);
        l_register.write_to(sum, self.registers);

        11
    }

    pub(super) fn adc_hl_ss(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);

        let l_value = Register16::HL.read_from(self.registers);
        let r_value = register.read_from(self.registers);

        let sum = add_u16(l_value, r_value, true, &mut self.registers.f);
        Register16::HL.write_to(sum, self.registers);

        15
    }

    pub(super) fn sbc_hl_ss(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);

        let l_value = Register16::HL.read_from(self.registers);
        let r_value = register.read_from(self.registers);

        let difference = sbc_u16(l_value, r_value, &mut self.registers.f);
        Register16::HL.write_to(difference, self.registers);

        15
    }

    pub(super) fn daa(&mut self) -> u32 {
        let a = self.registers.a;
        let flags = self.registers.f;

        let mut diff = 0;
        if flags.half_carry || (a & 0x0F > 0x09) {
            diff |= 0x06;
        }
        let carry = flags.carry || a > 0x99;
        if carry {
            diff |= 0x60;
        }

        let value = if flags.subtract { a.wrapping_sub(diff) } else { a.wrapping_add(diff) };

        let half_carry = a.bit(4) != value.bit(4);

        self.registers.a = value;
        self.registers.f = Flags {
            sign: sign_flag(value),
            zero: zero_flag(value),
            half_carry,
            overflow: parity_flag(value),
            carry,
            ..flags
        };

        4
    }

    pub(super) fn cpl(&mut self) -> u32 {
        self.registers.a = !self.registers.a;
        self.registers.f = Flags { half_carry: true, subtract: true, ..self.registers.f };

        4
    }

    pub(super) fn neg(&mut self) -> u32 {
        self.registers.a = subtract(0, self.registers.a, false, &mut self.registers.f);

        8
    }

    pub(super) fn ccf(&mut self) -> u32 {
        let prev_carry = self.registers.f.carry;
        self.registers.f = Flags {
            half_carry: prev_carry,
            subtract: false,
            carry: !prev_carry,
            ..self.registers.f
        };

        4
    }

    pub(super) fn scf(&mut self) -> u32 {
        self.registers.f =
            Flags { half_carry: false, subtract: false, carry: true, ..self.registers.f };

        4
    }

    pub(super) fn compare_block(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let a = self.registers.a;
        let bc = Register16::BC.read_from(self.registers);
        let hl = Register16::HL.read_from(self.registers);
        let operand = self.bus.read_memory(hl);

        let difference = a.wrapping_sub(operand);
        let half_carry = a & 0x0F < operand & 0x0F;

        Register16::HL.write_to(mode.apply(hl), self.registers);
        Register16::BC.write_to(bc.wrapping_sub(1), self.registers);

        self.registers.f = Flags {
            sign: sign_flag(difference),
            zero: zero_flag(difference),
            half_carry,
            overflow: bc != 1,
            subtract: true,
            ..self.registers.f
        };

        let should_repeat = repeat && difference != 0 && bc != 1;
        if should_repeat {
            self.registers.pc -= 2;
            21
        } else {
            16
        }
    }
}

fn add(l: u8, r: u8, with_carry: bool, flags: &mut Flags) -> u8 {
    let carry_operand = if with_carry { u8::from(flags.carry) } else { 0 };

    let (sum, carry) = match l.overflowing_add(r) {
        (sum, true) => (sum + carry_operand, true),
        (sum, false) => sum.overflowing_add(carry_operand),
    };

    let half_carry = (l & 0x0F) + (r & 0x0F) + carry_operand >= 0x10;
    let bit_6_carry = (l & 0x7F) + (r & 0x7F) + carry_operand >= 0x80;
    let overflow = bit_6_carry != carry;

    *flags = Flags {
        sign: sign_flag(sum),
        zero: zero_flag(sum),
        half_carry,
        overflow,
        subtract: false,
        carry,
        ..*flags
    };

    sum
}

fn add_u16(l: u16, r: u16, with_carry: bool, flags: &mut Flags) -> u16 {
    let carry_operand = if with_carry { u16::from(flags.carry) } else { 0 };

    let (sum, carry) = match l.overflowing_add(r) {
        (sum, true) => (sum + carry_operand, true),
        (sum, false) => sum.overflowing_add(carry_operand),
    };

    let half_carry = (l & 0x0FFF) + (r & 0x0FFF) + carry_operand >= 0x1000;

    *flags = Flags { half_carry, subtract: false, carry, ..*flags };

    if with_carry {
        // S, Z, and P/V are only set in 16-bit ADC, not 16-bit ADD
        let bit_14_carry = (l & 0x7FFF) + (r & 0x7FFF) + carry_operand >= 0x8000;
        let overflow = bit_14_carry != carry;

        *flags = Flags { sign: sum.bit(15), zero: sum == 0, overflow, ..*flags };
    }

    sum
}

fn subtract(l: u8, r: u8, with_carry: bool, flags: &mut Flags) -> u8 {
    let carry_operand = if with_carry { u8::from(flags.carry) } else { 0 };

    let (difference, carry) = match l.overflowing_sub(r) {
        (difference, true) => (difference - carry_operand, true),
        (difference, false) => difference.overflowing_sub(carry_operand),
    };

    let half_carry = l & 0x0F < (r & 0x0F) + carry_operand;
    let bit_6_borrow = l & 0x7F < (r & 0x7F) + carry_operand;
    let overflow = bit_6_borrow != carry;

    *flags = Flags {
        sign: sign_flag(difference),
        zero: zero_flag(difference),
        half_carry,
        overflow,
        subtract: true,
        carry,
        ..*flags
    };

    difference
}

fn sbc_u16(l: u16, r: u16, flags: &mut Flags) -> u16 {
    let carry_operand = u16::from(flags.carry);

    let (difference, carry) = match l.overflowing_sub(r) {
        (difference, true) => (difference - carry_operand, true),
        (difference, false) => difference.overflowing_sub(carry_operand),
    };

    let half_carry = l & 0x0FFF < (r & 0x0FFF) + carry_operand;
    let bit_14_borrow = l & 0x7FFF < (r & 0x7FFF) + carry_operand;
    let overflow = bit_14_borrow != carry;

    *flags = Flags {
        sign: difference.bit(15),
        zero: difference == 0,
        half_carry,
        overflow,
        subtract: true,
        carry,
        ..*flags
    };

    difference
}

fn and(l: u8, r: u8, flags: &mut Flags) -> u8 {
    let value = l & r;

    *flags = Flags {
        sign: sign_flag(value),
        zero: zero_flag(value),
        half_carry: true,
        overflow: parity_flag(value),
        subtract: false,
        carry: false,
        ..*flags
    };

    value
}

fn or(l: u8, r: u8, flags: &mut Flags) -> u8 {
    let value = l | r;

    *flags = Flags {
        sign: sign_flag(value),
        zero: zero_flag(value),
        half_carry: false,
        overflow: parity_flag(value),
        subtract: false,
        carry: false,
        ..*flags
    };

    value
}

fn xor(l: u8, r: u8, flags: &mut Flags) -> u8 {
    let value = l ^ r;

    *flags = Flags {
        sign: sign_flag(value),
        zero: zero_flag(value),
        half_carry: false,
        overflow: parity_flag(value),
        subtract: false,
        carry: false,
        ..*flags
    };

    value
}

fn compare(l: u8, r: u8, flags: &mut Flags) -> u8 {
    let (difference, carry) = l.overflowing_sub(r);

    let half_carry = l & 0x0F < r & 0x0F;
    let bit_6_borrow = l & 0x7F < r & 0x7F;
    let overflow = bit_6_borrow != carry;

    *flags = Flags {
        sign: sign_flag(difference),
        zero: zero_flag(difference),
        half_carry,
        overflow,
        subtract: true,
        carry,
        ..*flags
    };

    l
}

fn increment(value: u8, flags: &mut Flags) -> u8 {
    let half_carry = value & 0x0F == 0x0F;
    let overflow = value == 0x7F;

    let incremented = value.wrapping_add(1);
    *flags = Flags {
        sign: sign_flag(incremented),
        zero: zero_flag(incremented),
        half_carry,
        overflow,
        subtract: false,
        ..*flags
    };

    incremented
}

fn decrement(value: u8, flags: &mut Flags) -> u8 {
    let half_carry = value & 0x0F == 0x00;
    let overflow = value == 0x80;

    let decremented = value.wrapping_sub(1);
    *flags = Flags {
        sign: sign_flag(decremented),
        zero: zero_flag(decremented),
        half_carry,
        overflow,
        subtract: true,
        ..*flags
    };

    decremented
}

fn increment_u16(value: u16) -> u16 {
    value.wrapping_add(1)
}

fn decrement_u16(value: u16) -> u16 {
    value.wrapping_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Registers;
    use crate::traits::InMemoryBus;

    #[test]
    fn cpl() {
        let mut registers = Registers::new();
        let mut bus = InMemoryBus::new();

        registers.a = 0x37;
        registers.f = 0_u8.into();

        InstructionExecutor::new(&mut registers, &mut bus).cpl();

        assert_eq!(registers.a, 0xC8);
        assert_eq!(u8::from(registers.f) & 0xD7, 0x12);

        let value = rand::random();
        registers.a = value;
        registers.f = 0xFF_u8.into();

        InstructionExecutor::new(&mut registers, &mut bus).cpl();

        assert_eq!(registers.a, !value);
        assert_eq!(u8::from(registers.f) & 0xD7, 0xD7);
    }

    #[test]
    fn ccf() {
        let mut registers = Registers::new();
        let mut bus = InMemoryBus::new();

        registers.f = 0_u8.into();

        InstructionExecutor::new(&mut registers, &mut bus).ccf();
        assert_eq!(u8::from(registers.f) & 0xD7, 0x01);

        InstructionExecutor::new(&mut registers, &mut bus).ccf();
        assert_eq!(u8::from(registers.f) & 0xD7, 0x10);

        InstructionExecutor::new(&mut registers, &mut bus).ccf();
        assert_eq!(u8::from(registers.f) & 0xD7, 0x01);

        registers.f = 0xFF_u8.into();

        InstructionExecutor::new(&mut registers, &mut bus).ccf();
        assert_eq!(u8::from(registers.f) & 0xD7, 0xD4);

        InstructionExecutor::new(&mut registers, &mut bus).ccf();
        assert_eq!(u8::from(registers.f) & 0xD7, 0xC5);
    }

    #[test]
    fn scf() {
        let mut registers = Registers::new();
        let mut bus = InMemoryBus::new();

        registers.f = 0_u8.into();

        for _ in 0..2 {
            InstructionExecutor::new(&mut registers, &mut bus).scf();
            assert_eq!(u8::from(registers.f) & 0xD7, 0x01);
        }

        registers.f = 0xFF_u8.into();

        for _ in 0..2 {
            InstructionExecutor::new(&mut registers, &mut bus).scf();
            assert_eq!(u8::from(registers.f) & 0xD7, 0xC5);
        }
    }
}
