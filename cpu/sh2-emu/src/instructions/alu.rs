//! SH-2 arithmetic instructions

use crate::Sh2;
use crate::bus::BusInterface;
use crate::instructions::{rm, rn};
use jgenesis_common::num::SignBit;

macro_rules! impl_compare {
    ($name:ident, |$rn:ident| $compare:expr) => {
        pub(crate) fn $name(&mut self, opcode: u16) {
            let $rn = self.registers.gpr[rn(opcode)];
            self.registers.sr.t = $compare;
        }
    };
    ($name:ident, |$rm:ident, $rn:ident| $compare:expr) => {
        pub(crate) fn $name(&mut self, opcode: u16) {
            let $rm = self.registers.gpr[rm(opcode)];
            let $rn = self.registers.gpr[rn(opcode)];
            self.registers.sr.t = $compare;
        }
    };
}

impl Sh2 {
    // ADD Rm, Rn
    // Addition
    pub(crate) fn add_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_add(self.registers.gpr[m]);
    }

    // ADD #imm, Rn
    // Addition with immediate operand
    pub(crate) fn add_imm_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        let imm = opcode as i8;
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_add(imm as u32);
    }

    // ADDC Rm, Rn
    // Addition with carry
    pub(crate) fn addc(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let (partial_sum, carry1) = self.registers.gpr[m].overflowing_add(self.registers.gpr[n]);
        let (sum, carry2) = partial_sum.overflowing_add(self.registers.sr.t.into());

        self.registers.gpr[n] = sum;
        self.registers.sr.t = carry1 || carry2;
    }

    // ADDV Rm, Rn
    // Addition with signed overflow check
    pub(crate) fn addv(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let source_sign = self.registers.gpr[m].sign_bit();
        let destination_sign = self.registers.gpr[n].sign_bit();
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_add(self.registers.gpr[m]);

        // Signed overflow occurs when the operands have the same sign and the sum has a different sign
        let sum_sign = self.registers.gpr[n].sign_bit();
        self.registers.sr.t = source_sign == destination_sign && source_sign != sum_sign;
    }

    // SUB Rm, Rn
    // Subtraction
    pub(crate) fn sub_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        self.registers.gpr[n] = self.registers.gpr[n].wrapping_sub(self.registers.gpr[m]);
    }

    // SUBC Rm, Rn
    // Subtraction with carry
    pub(crate) fn subc(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let (partial_diff, borrow1) = self.registers.gpr[n].overflowing_sub(self.registers.gpr[m]);
        let (difference, borrow2) = partial_diff.overflowing_sub(self.registers.sr.t.into());

        self.registers.gpr[n] = difference;
        self.registers.sr.t = borrow1 || borrow2;
    }

    // SUBV Rm, Rn
    // Subtraction with signed underflow check
    pub(crate) fn subv(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let source_sign = self.registers.gpr[m].sign_bit();
        let dest_sign = self.registers.gpr[n].sign_bit();
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_sub(self.registers.gpr[m]);

        // Signed overflow occurs when the operands have different signs and the difference sign does
        // not match the left operand's sign
        let difference_sign = self.registers.gpr[n].sign_bit();
        self.registers.sr.t = source_sign != dest_sign && difference_sign != dest_sign;
    }

    // NEG Rm, Rn
    // Negate
    pub(crate) fn neg(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = 0_u32.wrapping_sub(self.registers.gpr[m]);
    }

    // NEGC Rm, Rn
    // Negate with carry
    pub(crate) fn negc(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let (partial_diff, borrow1) = 0_u32.overflowing_sub(self.registers.gpr[m]);
        let (difference, borrow2) = partial_diff.overflowing_sub(self.registers.sr.t.into());
        self.registers.gpr[n] = difference;
        self.registers.sr.t = borrow1 || borrow2;
    }

    // CMP/EQ Rm, Rn
    // Set the T flag if Rm = Rn
    impl_compare!(cmp_eq_rm_rn, |rm, rn| rm == rn);

    // CMP/EQ #imm, R0
    // Set the T flag if R0 = #imm
    pub(crate) fn cmp_eq_imm_r0(&mut self, opcode: u16) {
        let imm = opcode as i8;
        self.registers.sr.t = self.registers.gpr[0] == imm as u32;
    }

    // CMP/GE Rm, Rn
    // Set the T flag if Rn >= Rm (signed)
    impl_compare!(cmp_ge, |rm, rn| (rn as i32) >= (rm as i32));

    // CMP/GT Rm, Rn
    // Set the T flag if Rn > Rm (signed)
    impl_compare!(cmp_gt, |rm, rn| (rn as i32) > (rm as i32));

    // CMP/HI Rm, Rn
    // Set the T flag if Rn > Rm (unsigned)
    impl_compare!(cmp_hi, |rm, rn| rn > rm);

    // CMP/HS Rm, Rn
    // Set the T flag if Rn >= Rm (unsigned)
    impl_compare!(cmp_hs, |rm, rn| rn >= rm);

    // CMP/PL Rn
    // Set the T flag if Rn > 0
    impl_compare!(cmp_pl, |rn| (rn as i32) > 0);

    // CMP/PZ Rn
    // Set the T flag if Rn >= 0
    impl_compare!(cmp_pz, |rn| (rn as i32) >= 0);

    // CMP/STR Rm, Rn
    // Set the T flag if any individual byte is equal in Rm and Rn
    pub(crate) fn cmp_str(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let xor = self.registers.gpr[m] ^ self.registers.gpr[n];
        self.registers.sr.t = (xor & 0xFF == 0)
            || ((xor >> 8) & 0xFF == 0)
            || ((xor >> 16) & 0xFF == 0)
            || ((xor >> 24) & 0xFF == 0);
    }

    // EXTS.B Rm, Rn
    // Sign extend byte
    pub(crate) fn exts_b(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[m] as i8 as u32;
    }

    // EXTS.W Rm, Rn
    // Sign extend word
    pub(crate) fn exts_w(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[m] as i16 as u32;
    }

    // EXTU.B Rm, Rn
    // Zero extend byte
    pub(crate) fn extu_b(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[m] & 0xFF;
    }

    // EXTU.W Rm, Rn
    // Zero extend word
    pub(crate) fn extu_w(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[m] & 0xFFFF;
    }

    // DT Rn
    // Decrement and test
    pub(crate) fn dt(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_sub(1);
        self.registers.sr.t = self.registers.gpr[n] == 0;
    }

    // MUL.L Rm, Rn
    // 32-bit x 32-bit -> 32-bit multiplication
    pub(crate) fn mul(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.macl = self.registers.gpr[m].wrapping_mul(self.registers.gpr[n]);
    }

    // MULS.W Rm, Rn
    // Signed 16-bit x 16-bit -> 32-bit multiplication
    pub(crate) fn muls(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let operand_l: i32 = (self.registers.gpr[m] as i16).into();
        let operand_r: i32 = (self.registers.gpr[n] as i16).into();
        self.registers.macl = (operand_l * operand_r) as u32;
    }

    // MULU.W Rm, Rn
    // Unsigned 16-bit x 16-bit -> 32-bit multiplication
    pub(crate) fn mulu(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let operand_l = self.registers.gpr[m] & 0xFFFF;
        let operand_r = self.registers.gpr[n] & 0xFFFF;
        self.registers.macl = operand_l * operand_r;
    }

    // DMULS.L Rm, Rn
    // Signed 32-bit x 32-bit -> 64-bit multiplication
    pub(crate) fn dmuls(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let operand_l: i64 = (self.registers.gpr[m] as i32).into();
        let operand_r: i64 = (self.registers.gpr[n] as i32).into();
        self.registers.set_mac(operand_l * operand_r);
    }

    // DMULU Rm, Rn
    // Unsigned 32-bit x 32-bit -> 64-bit multiplication
    pub(crate) fn dmulu(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let product = u64::from(self.registers.gpr[m]) * u64::from(self.registers.gpr[n]);
        self.registers.macl = product as u32;
        self.registers.mach = (product >> 32) as u32;
    }

    // MAC.W @Rm+, @Rn+
    // Multiply and accumulate with word operands
    pub(crate) fn mac_w(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        let m = rm(opcode);
        let n = rn(opcode);

        let operand_l = self.read_word(self.registers.gpr[m], bus) as i16;
        self.registers.gpr[m] = self.registers.gpr[m].wrapping_add(2);

        let operand_r = self.read_word(self.registers.gpr[n], bus) as i16;
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_add(2);

        let product = i64::from(operand_l) * i64::from(operand_r);

        if self.registers.sr.s {
            // 16-bit x 16-bit + 32-bit -> 32-bit, with saturation
            let sum = i64::from(self.registers.macl as i32) + product;
            self.registers.macl = sum.clamp(i32::MIN.into(), i32::MAX.into()) as u32;
            // TODO set overflow bit in MACH? manual suggests that only SH-1 does this
        } else {
            // 16-bit x 16-bit + 64-bit -> 64-bit
            let sum = product.wrapping_add(self.registers.mac());
            self.registers.set_mac(sum);
        }
    }

    // MAC.L @Rm+, @Rn+
    // Multiply and accumulate with longword operands
    pub(crate) fn mac_l(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        let m = rm(opcode);
        let n = rn(opcode);

        let operand_l = self.read_longword(self.registers.gpr[m], bus) as i32;
        self.registers.gpr[m] = self.registers.gpr[m].wrapping_add(4);

        let operand_r = self.read_longword(self.registers.gpr[n], bus) as i32;
        self.registers.gpr[n] = self.registers.gpr[n].wrapping_add(4);

        let product = i64::from(operand_l) * i64::from(operand_r);
        let product_sum = product.wrapping_add(self.registers.mac());

        if self.registers.sr.s {
            // Saturate to signed 48-bit
            let clamped = product_sum.clamp(-(1 << 47), (1 << 47) - 1);
            self.registers.set_mac(clamped);
        } else {
            self.registers.set_mac(product_sum);
        }
    }

    // DIV0U
    // Initialization step for unsigned division
    pub(crate) fn div0u(&mut self) {
        self.registers.sr.m = false;
        self.registers.sr.q = false;
        self.registers.sr.t = false;
    }

    // DIV0S Rm, Rn
    // Initialization step for signed division
    pub(crate) fn div0s(&mut self, opcode: u16) {
        let divisor = self.registers.gpr[rm(opcode)];
        let dividend = self.registers.gpr[rn(opcode)];

        self.registers.sr.m = divisor.sign_bit();
        self.registers.sr.q = dividend.sign_bit();
        self.registers.sr.t = self.registers.sr.m != self.registers.sr.q;
    }

    // DIV1 Rm, Rn
    // Division single step
    pub(crate) fn div1(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let divisor = self.registers.gpr[m];
        let mut dividend = self.registers.gpr[n];

        let prev_sign_bit = dividend.sign_bit();
        dividend = (dividend << 1) | u32::from(self.registers.sr.t);

        let prev_dividend = dividend;
        let overflowed = if self.registers.sr.q == self.registers.sr.m {
            dividend = dividend.wrapping_sub(divisor);
            dividend > prev_dividend
        } else {
            dividend = dividend.wrapping_add(divisor);
            dividend < prev_dividend
        };

        self.registers.sr.q = overflowed ^ prev_sign_bit ^ self.registers.sr.m;
        self.registers.sr.t = self.registers.sr.q == self.registers.sr.m;
        self.registers.gpr[n] = dividend;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mn_opcode(rm: u16, rn: u16) -> u16 {
        (rm << 4) | (rn << 8)
    }

    #[test]
    fn unsigned_division() {
        let mut cpu = Sh2::new(String::new());

        cpu.registers.gpr[0] = 100000;
        cpu.registers.gpr[1] = 300 << 16;

        cpu.div0u();

        let opcode = mn_opcode(1, 0);
        for _ in 0..16 {
            cpu.div1(opcode);
        }

        cpu.registers.gpr[0] = (cpu.registers.gpr[0] << 1) | u32::from(cpu.registers.sr.t);

        assert_eq!(cpu.registers.gpr[0] & 0xFFFF, 333);
    }

    #[test]
    fn signed_division() {
        let mut cpu = Sh2::new(String::new());

        for _ in 0..100 {
            let dividend: i16 = rand::random();
            let mut divisor = rand::random::<i16>() >> 7;
            while divisor == 0 {
                divisor = rand::random();
            }

            cpu.registers.gpr[0] = (divisor as u32) << 16;
            cpu.registers.gpr[1] = dividend as u32;

            cpu.registers.gpr[3] = cpu.registers.gpr[1];
            cpu.rotcl(3 << 8);
            cpu.subc(mn_opcode(2, 1));

            cpu.div0s(mn_opcode(0, 1));
            for _ in 0..16 {
                cpu.div1(mn_opcode(0, 1));
            }

            cpu.registers.gpr[1] = (cpu.registers.gpr[1] as i16) as u32;

            cpu.rotcl(1 << 8);
            cpu.addc(mn_opcode(2, 1));

            let quotient = cpu.registers.gpr[1] as i16;
            assert_eq!(quotient, dividend / divisor);
        }
    }
}
