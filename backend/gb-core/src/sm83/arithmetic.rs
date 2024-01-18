use crate::sm83::bus::BusInterface;
use crate::sm83::{Flags, Sm83};

impl Sm83 {
    // ADD: Add without carry
    pub(super) fn add_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a = add(self.registers.a, operand, false, &mut self.registers.f);
    }

    // ADD: Add without carry
    pub(super) fn add_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a = add(self.registers.a, operand, false, &mut self.registers.f);
    }

    // ADC: Add with carry
    pub(super) fn adc_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a =
            add(self.registers.a, operand, self.registers.f.carry, &mut self.registers.f);
    }

    // ADC: Add with carry
    pub(super) fn adc_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a =
            add(self.registers.a, operand, self.registers.f.carry, &mut self.registers.f);
    }

    // SUB: Subtract without carry
    pub(super) fn sub_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a = subtract(self.registers.a, operand, false, &mut self.registers.f);
    }

    // SUB: Subtract without carry
    pub(super) fn sub_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a = subtract(self.registers.a, operand, false, &mut self.registers.f);
    }

    // SBC: Subtract with carry
    pub(super) fn sbc_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        self.registers.a =
            subtract(self.registers.a, operand, self.registers.f.carry, &mut self.registers.f);
    }

    // SBC: Subtract with carry
    pub(super) fn sbc_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        self.registers.a =
            subtract(self.registers.a, operand, self.registers.f.carry, &mut self.registers.f);
    }

    // CP: Compare
    pub(super) fn cp_a_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = self.read_register(bus, opcode);
        subtract(self.registers.a, operand, false, &mut self.registers.f);
    }

    // CP: Compare
    pub(super) fn cp_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus);
        subtract(self.registers.a, operand, false, &mut self.registers.f);
    }

    // INC r: Increment register
    pub(super) fn inc_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode >> 3).wrapping_add(1);
        self.write_register(bus, opcode >> 3, value);

        self.registers.f.zero = value == 0;
        self.registers.f.subtract = false;
        self.registers.f.half_carry = value & 0x0F == 0;
    }

    // DEC r: Decrement register
    pub(super) fn dec_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode >> 3).wrapping_sub(1);
        self.write_register(bus, opcode >> 3, value);

        self.registers.f.zero = value == 0;
        self.registers.f.subtract = true;
        self.registers.f.half_carry = value & 0x0F == 0x0F;
    }

    // INC rr: Increment register pair or SP
    pub(super) fn inc_rr<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        match (opcode >> 4) & 0x3 {
            0x0 => self.registers.increment_bc(),
            0x1 => self.registers.increment_de(),
            0x2 => self.registers.increment_hl(),
            0x3 => self.registers.increment_sp(),
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }

        // INC rr takes 2 M-cycles
        bus.idle();
    }

    // DEC rr: Decrement register pair or SP
    pub(super) fn dec_rr<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        match (opcode >> 4) & 0x3 {
            0x0 => self.registers.decrement_bc(),
            0x1 => self.registers.decrement_de(),
            0x2 => self.registers.decrement_hl(),
            0x3 => self.registers.decrement_sp(),
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }

        // DEC rr takes 2 M-cycles
        bus.idle();
    }

    // ADD HL, rr: 16-bit add
    pub(super) fn add_hl_rr<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let operand = match (opcode >> 4) & 0x3 {
            0x0 => self.registers.bc(),
            0x1 => self.registers.de(),
            0x2 => self.registers.hl(),
            0x3 => self.registers.sp,
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        };

        let (sum, flags) = add_u16(self.registers.hl(), operand);
        self.registers.set_hl(sum);

        // 16-bit addition does not modify the Z flag
        self.registers.f = Flags { zero: self.registers.f.zero, ..flags };

        // 16-bit addition takes 2 M-cycles
        bus.idle();
    }

    // ADD SP, i8: Add signed immediate value to stack pointer
    pub(super) fn add_sp_e<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus) as i8;

        let (sum, flags) = add_sp(self.registers.sp, operand as u16);
        self.registers.sp = sum;

        // Adding to stack pointer always sets Z flag to false
        self.registers.f = Flags { zero: false, ..flags };

        // This instruction takes 4 M-cycles: opcode read + operand read + 2 idle cycles
        bus.idle();
        bus.idle();
    }

    // DAA: Decimal adjust accumulator
    pub(super) fn daa(&mut self) {
        if self.registers.f.subtract {
            // Last op was subtraction
            let mut value = self.registers.a;

            if self.registers.f.half_carry {
                value = value.wrapping_sub(0x06);
            }

            if self.registers.f.carry {
                value = value.wrapping_sub(0x60);
            }

            self.registers.a = value;
            self.registers.f.zero = value == 0;
            self.registers.f.half_carry = false;
        } else {
            // Last op was addition
            let mut value = self.registers.a;
            let mut carry = false;

            if value > 0x99 || self.registers.f.carry {
                value = value.wrapping_add(0x60);
                carry = true;
            }

            if value & 0x0F > 0x09 || self.registers.f.half_carry {
                value = value.wrapping_add(0x06);
            }

            self.registers.a = value;
            self.registers.f.zero = value == 0;
            self.registers.f.half_carry = false;
            self.registers.f.carry = carry;
        }
    }

    pub(super) fn ld_hl_sp_e<B: BusInterface>(&mut self, bus: &mut B) {
        let operand = self.fetch_operand(bus) as i8;
        let (sum, flags) = add_sp(self.registers.sp, operand as u16);

        self.registers.set_hl(sum);

        // Adding to stack pointer always sets Z flag to false
        self.registers.f = Flags { zero: false, ..flags };

        bus.idle();
    }
}

fn add(accumulator: u8, operand: u8, carry_in: bool, flags: &mut Flags) -> u8 {
    let carry_in: u8 = carry_in.into();

    let (partial_sum, carry1) = accumulator.overflowing_add(operand);
    let (sum, carry2) = partial_sum.overflowing_add(carry_in);
    let carry = carry1 || carry2;

    let half_carry = (accumulator & 0x0F) + (operand & 0x0F) + carry_in > 0x0F;

    *flags = Flags { zero: sum == 0, subtract: false, half_carry, carry };

    sum
}

fn add_u16(l_value: u16, r_value: u16) -> (u16, Flags) {
    let [l_lsb, l_msb] = l_value.to_le_bytes();
    let [r_lsb, r_msb] = r_value.to_le_bytes();
    let mut flags = Flags { zero: false, subtract: false, half_carry: false, carry: false };

    let (sum_lsb, lsb_carry) = l_lsb.overflowing_add(r_lsb);
    let sum_msb = add(l_msb, r_msb, lsb_carry, &mut flags);
    let sum = u16::from_le_bytes([sum_lsb, sum_msb]);

    (sum, flags)
}

// This differs from add_u16 in that it sets the flags based on the LSB addition instead of MSB
fn add_sp(l_value: u16, r_value: u16) -> (u16, Flags) {
    let [l_lsb, l_msb] = l_value.to_le_bytes();
    let [r_lsb, r_msb] = r_value.to_le_bytes();
    let mut flags = Flags { zero: false, subtract: false, half_carry: false, carry: false };

    let sum_lsb = add(l_lsb, r_lsb, false, &mut flags);
    let sum_msb = l_msb.wrapping_add(r_msb).wrapping_add(flags.carry.into());
    let sum = u16::from_le_bytes([sum_lsb, sum_msb]);

    (sum, flags)
}

fn subtract(accumulator: u8, operand: u8, carry_in: bool, flags: &mut Flags) -> u8 {
    let carry_in: u8 = carry_in.into();

    let (partial_diff, borrow1) = accumulator.overflowing_sub(operand);
    let (difference, borrow2) = partial_diff.overflowing_sub(carry_in);
    let borrow = borrow1 || borrow2;

    let half_borrow = accumulator & 0x0F < (operand & 0x0F) + carry_in;

    *flags =
        Flags { zero: difference == 0, subtract: true, half_carry: half_borrow, carry: borrow };

    difference
}
