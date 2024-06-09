use crate::instructions::{parse_register_high, parse_register_low, parse_signed_immediate};
use crate::Sh2;
use jgenesis_common::num::SignBit;

// ADD Rm, Rn
// Addition
pub fn add_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] =
        cpu.registers.gpr[destination].wrapping_add(cpu.registers.gpr[source]);
}

// ADD #imm, Rn
// Addition with immediate operand
pub fn add_imm_rn(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    let immediate = parse_signed_immediate(opcode);

    cpu.registers.gpr[register] = cpu.registers.gpr[register].wrapping_add(immediate as u32);
}

// ADDC Rm, Rn
// Addition with carry
pub fn addc(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let (partial_sum, carry1) =
        cpu.registers.gpr[source].overflowing_add(cpu.registers.gpr[destination]);
    let (sum, carry2) = partial_sum.overflowing_add(cpu.registers.sr.t.into());

    cpu.registers.gpr[destination] = sum;
    cpu.registers.sr.t = carry1 || carry2;
}

// SUB Rm, Rn
// Subtraction
pub fn sub_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    cpu.registers.gpr[destination] =
        cpu.registers.gpr[destination].wrapping_sub(cpu.registers.gpr[source]);
}

// SUBC Rm, Rn
// Subtraction with carry
pub fn subc(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let (partial_diff, borrow1) =
        cpu.registers.gpr[destination].overflowing_sub(cpu.registers.gpr[source]);
    let (difference, borrow2) = partial_diff.overflowing_sub(cpu.registers.sr.t.into());

    cpu.registers.gpr[destination] = difference;
    cpu.registers.sr.t = borrow1 || borrow2;
}

// NEG Rm, Rn
// Negate
pub fn neg(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] = 0_u32.wrapping_sub(cpu.registers.gpr[source]);
}

// NEGC Rm, Rn
// Negate with carry
pub fn negc(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    let (partial_diff, borrow1) = 0_u32.overflowing_sub(cpu.registers.gpr[source]);
    let (difference, borrow2) = partial_diff.overflowing_sub(cpu.registers.sr.t.into());
    cpu.registers.gpr[destination] = difference;
    cpu.registers.sr.t = borrow1 || borrow2;
}

macro_rules! impl_compare {
    ($name:ident, |$rn:ident| $compare:expr) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            let $rn = cpu.registers.gpr[parse_register_high(opcode) as usize];
            cpu.registers.sr.t = $compare;
        }
    };
    ($name:ident, |$rm:ident, $rn:ident| $compare:expr) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            let $rm = cpu.registers.gpr[parse_register_low(opcode) as usize];
            let $rn = cpu.registers.gpr[parse_register_high(opcode) as usize];
            cpu.registers.sr.t = $compare;
        }
    };
}

// CMP/EQ Rm, Rn
// Set the T flag if Rm = Rn
impl_compare!(cmp_eq_rm_rn, |rm, rn| rm == rn);

// CMP/EQ #imm, R0
// Set the T flag if R0 = #imm
pub fn cmp_eq_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let immediate = parse_signed_immediate(opcode);
    cpu.registers.sr.t = cpu.registers.gpr[0] == immediate as u32;
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
impl_compare!(cmp_pz, |rn| !rn.sign_bit());

// EXTU.B Rm, Rn
// Zero extend byte
pub fn extu_b(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] = cpu.registers.gpr[source] & 0xFF;
}

// EXTU.W Rm, Rn
// Zero extend word
pub fn extu_w(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;
    cpu.registers.gpr[destination] = cpu.registers.gpr[source] & 0xFFFF;
}

// DT Rn
// Decrement and test
pub fn dt(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    cpu.registers.gpr[register] = cpu.registers.gpr[register].wrapping_sub(1);
    cpu.registers.sr.t = cpu.registers.gpr[register] == 0;
}

// MULU.W Rm, Rn
// Unsigned 16-bit x 16-bit -> 32-bit multiplication
pub fn mulu(cpu: &mut Sh2, opcode: u16) {
    let rm = parse_register_low(opcode) as usize;
    let rn = parse_register_high(opcode) as usize;
    let product = (cpu.registers.gpr[rm] & 0xFFFF) * (cpu.registers.gpr[rn] & 0xFFFF);
    cpu.registers.macl = product;
}

// DIV0U
// Initialization step for unsigned division
pub fn div0u(cpu: &mut Sh2) {
    cpu.registers.sr.m = false;
    cpu.registers.sr.q = false;
    cpu.registers.sr.t = false;
}

// DIV0S Rm, Rn
// Initialization step for signed division
pub fn div0s(cpu: &mut Sh2, opcode: u16) {
    let divisor = cpu.registers.gpr[parse_register_low(opcode) as usize];
    let dividend = cpu.registers.gpr[parse_register_high(opcode) as usize];

    cpu.registers.sr.m = divisor.sign_bit();
    cpu.registers.sr.q = dividend.sign_bit();
    cpu.registers.sr.t = cpu.registers.sr.m != cpu.registers.sr.q;
}

// DIV1 Rm, Rn
// Division single step
pub fn div1(cpu: &mut Sh2, opcode: u16) {
    let divisor_register = parse_register_low(opcode) as usize;
    let dividend_register = parse_register_high(opcode) as usize;

    let divisor = cpu.registers.gpr[divisor_register];
    let mut dividend = cpu.registers.gpr[dividend_register];

    let prev_sign_bit = dividend.sign_bit();
    dividend = (dividend << 1) | u32::from(cpu.registers.sr.t);

    let prev_dividend = dividend;
    let overflowed = if cpu.registers.sr.q == cpu.registers.sr.m {
        dividend = dividend.wrapping_sub(divisor);
        dividend > prev_dividend
    } else {
        dividend = dividend.wrapping_add(divisor);
        dividend < prev_dividend
    };

    cpu.registers.sr.q = overflowed ^ prev_sign_bit ^ cpu.registers.sr.m;
    cpu.registers.sr.t = cpu.registers.sr.q == cpu.registers.sr.m;
    cpu.registers.gpr[dividend_register] = dividend;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instructions::bits::rotcl;

    fn mn_opcode(rm: u16, rn: u16) -> u16 {
        (rm << 4) | (rn << 8)
    }

    #[test]
    fn unsigned_division() {
        let mut cpu = Sh2::new(String::new());

        cpu.registers.gpr[0] = 100000;
        cpu.registers.gpr[1] = 300 << 16;

        div0u(&mut cpu);

        let opcode = mn_opcode(1, 0);
        for _ in 0..16 {
            div1(&mut cpu, opcode);
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
            rotcl(&mut cpu, 3 << 8);
            subc(&mut cpu, mn_opcode(2, 1));

            div0s(&mut cpu, mn_opcode(0, 1));
            for _ in 0..16 {
                div1(&mut cpu, mn_opcode(0, 1));
            }

            cpu.registers.gpr[1] = (cpu.registers.gpr[1] as i16) as u32;

            rotcl(&mut cpu, 1 << 8);
            addc(&mut cpu, mn_opcode(2, 1));

            let quotient = cpu.registers.gpr[1] as i16;
            assert_eq!(quotient, dividend / divisor);
        }
    }
}
