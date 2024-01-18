use crate::sm83::bus::BusInterface;
use crate::sm83::{Flags, Sm83};
use jgenesis_common::num::GetBit;

macro_rules! impl_rla_op {
    ($name:ident, thru_carry: $thru_carry:expr) => {
        pub(super) fn $name(&mut self) {
            let carry = self.registers.a.bit(7);
            self.registers.a = if $thru_carry {
                (self.registers.a << 1) | u8::from(self.registers.f.carry)
            } else {
                (self.registers.a << 1) | (self.registers.a >> 7)
            };
            self.registers.f = Flags { zero: false, subtract: false, half_carry: false, carry };
        }
    };
}

macro_rules! impl_rra_op {
    ($name:ident, thru_carry: $thru_carry:expr) => {
        pub(super) fn $name(&mut self) {
            let carry = self.registers.a.bit(0);
            self.registers.a = if $thru_carry {
                (self.registers.a >> 1) | (u8::from(self.registers.f.carry) << 7)
            } else {
                (self.registers.a >> 1) | (self.registers.a << 7)
            };
            self.registers.f = Flags { zero: false, subtract: false, half_carry: false, carry };
        }
    };
}

impl Sm83 {
    // AND: Bitwise and
    pub(super) fn and_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a = and(self.registers.a, operand, &mut self.registers.f);
    }

    // AND: Bitwise and
    pub(super) fn and_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a = and(self.registers.a, operand, &mut self.registers.f);
    }

    // OR: Bitwise inclusive or
    pub(super) fn or_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a = or(self.registers.a, operand, &mut self.registers.f);
    }

    // OR: Bitwise inclusive or
    pub(super) fn or_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a = or(self.registers.a, operand, &mut self.registers.f);
    }

    // XOR: Bitwise exclusive or
    pub(super) fn xor_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a = xor(self.registers.a, operand, &mut self.registers.f);
    }

    // XOR: Bitwise exclusive or
    pub(super) fn xor_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a = xor(self.registers.a, operand, &mut self.registers.f);
    }

    // CPL: Complement accumulator
    pub(super) fn cpl(&mut self) {
        self.registers.a = !self.registers.a;

        self.registers.f.subtract = true;
        self.registers.f.half_carry = true;
    }

    // RLCA: Rotate accumulator left
    impl_rla_op!(rlca, thru_carry: false);

    // RRCA: Rotate accumualtor right
    impl_rra_op!(rrca, thru_carry: false);

    // RLA: Rotate accumulator left through carry
    impl_rla_op!(rla, thru_carry: true);

    // RRA: Rotate accumulator right through carry
    impl_rra_op!(rra, thru_carry: true);

    // RLC: Rotate register left
    pub(super) fn rlc_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let rotated = (value << 1) | (value >> 7);
        self.write_register(bus, opcode, rotated);

        self.registers.f = flags_for_shift_op(rotated, value.bit(7));
    }

    // RRC: Rotate register right
    pub(super) fn rrc_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let rotated = (value >> 1) | (value << 7);
        self.write_register(bus, opcode, rotated);

        self.registers.f = flags_for_shift_op(rotated, value.bit(0));
    }

    // RL: Rotate register left through carry
    pub(super) fn rl_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let rotated = (value << 1) | u8::from(self.registers.f.carry);
        self.write_register(bus, opcode, rotated);

        self.registers.f = flags_for_shift_op(rotated, value.bit(7));
    }

    // RR: Rotate register right through carry
    pub(super) fn rr_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let rotated = (value >> 1) | (u8::from(self.registers.f.carry) << 7);
        self.write_register(bus, opcode, rotated);

        self.registers.f = flags_for_shift_op(rotated, value.bit(0));
    }

    // SLA: Shift left
    pub(super) fn sla<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let shifted = value << 1;
        self.write_register(bus, opcode, shifted);

        self.registers.f = flags_for_shift_op(shifted, value.bit(7));
    }

    // SRA: Arithmetic shift right
    pub(super) fn sra<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let shifted = (value >> 1) | (value & 0x80);
        self.write_register(bus, opcode, shifted);

        self.registers.f = flags_for_shift_op(shifted, value.bit(0));
    }

    // SRL: Logical shift right
    pub(super) fn srl<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let shifted = value >> 1;
        self.write_register(bus, opcode, shifted);

        self.registers.f = flags_for_shift_op(shifted, value.bit(0));
    }

    // SWAP: Swap nibbles
    pub(super) fn swap<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        self.write_register(bus, opcode, (value >> 4) | (value << 4));

        self.registers.f =
            Flags { zero: value == 0, subtract: false, half_carry: false, carry: false };
    }

    // BIT: Test bit
    pub(super) fn bit<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let bit = (opcode >> 3) & 0x7;

        self.registers.f.zero = !value.bit(bit);
        self.registers.f.subtract = false;
        self.registers.f.half_carry = true;
    }

    // SET: Set bit
    pub(super) fn set<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let bit = (opcode >> 3) & 0x7;
        self.write_register(bus, opcode, value | (1 << bit));
    }

    // RES: Reset bit
    pub(super) fn res<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        let bit = (opcode >> 3) & 0x7;
        self.write_register(bus, opcode, value & !(1 << bit));
    }
}

fn flags_for_shift_op(value: u8, carry: bool) -> Flags {
    Flags { zero: value == 0, subtract: false, half_carry: false, carry }
}

macro_rules! impl_bitwise_op {
    ($name:ident, $op:tt, half_carry: $half_carry:expr) => {
        fn $name(accumulator: u8, operand: u8, flags: &mut Flags) -> u8 {
            let value = accumulator $op operand;
            *flags = Flags {
                zero: value == 0,
                subtract: false,
                half_carry: $half_carry,
                carry: false,
            };
            value
        }
    }
}

impl_bitwise_op!(and, &, half_carry: true);
impl_bitwise_op!(or, |, half_carry: false);
impl_bitwise_op!(xor, ^, half_carry: false);
