//! SH-2 arithmetic instructions

use crate::Sh2;
use crate::bus::BusInterface;
use crate::instructions::{rm, rn};
use jgenesis_common::num::SignBit;

// ADD Rm, Rn
// Addition
pub fn add_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_add(cpu.registers.gpr[m]);
}

// ADD #imm, Rn
// Addition with immediate operand
pub fn add_imm_rn(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    let imm = opcode as i8;
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_add(imm as u32);
}

// ADDC Rm, Rn
// Addition with carry
pub fn addc(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let (partial_sum, carry1) = cpu.registers.gpr[m].overflowing_add(cpu.registers.gpr[n]);
    let (sum, carry2) = partial_sum.overflowing_add(cpu.registers.sr.t.into());

    cpu.registers.gpr[n] = sum;
    cpu.registers.sr.t = carry1 || carry2;
}

// ADDV Rm, Rn
// Addition with signed overflow check
pub fn addv(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let source_sign = cpu.registers.gpr[m].sign_bit();
    let destination_sign = cpu.registers.gpr[n].sign_bit();
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_add(cpu.registers.gpr[m]);

    // Signed overflow occurs when the operands have the same sign and the sum has a different sign
    let sum_sign = cpu.registers.gpr[n].sign_bit();
    cpu.registers.sr.t = source_sign == destination_sign && source_sign != sum_sign;
}

// SUB Rm, Rn
// Subtraction
pub fn sub_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_sub(cpu.registers.gpr[m]);
}

// SUBC Rm, Rn
// Subtraction with carry
pub fn subc(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let (partial_diff, borrow1) = cpu.registers.gpr[n].overflowing_sub(cpu.registers.gpr[m]);
    let (difference, borrow2) = partial_diff.overflowing_sub(cpu.registers.sr.t.into());

    cpu.registers.gpr[n] = difference;
    cpu.registers.sr.t = borrow1 || borrow2;
}

// SUBV Rm, Rn
// Subtraction with signed underflow check
pub fn subv(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let source_sign = cpu.registers.gpr[m].sign_bit();
    let dest_sign = cpu.registers.gpr[n].sign_bit();
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_sub(cpu.registers.gpr[m]);

    // Signed overflow occurs when the operands have different signs and the difference sign does
    // not match the left operand's sign
    let difference_sign = cpu.registers.gpr[n].sign_bit();
    cpu.registers.sr.t = source_sign != dest_sign && difference_sign != dest_sign;
}

// NEG Rm, Rn
// Negate
pub fn neg(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = 0_u32.wrapping_sub(cpu.registers.gpr[m]);
}

// NEGC Rm, Rn
// Negate with carry
pub fn negc(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let (partial_diff, borrow1) = 0_u32.overflowing_sub(cpu.registers.gpr[m]);
    let (difference, borrow2) = partial_diff.overflowing_sub(cpu.registers.sr.t.into());
    cpu.registers.gpr[n] = difference;
    cpu.registers.sr.t = borrow1 || borrow2;
}

macro_rules! impl_compare {
    ($name:ident, |$rn:ident| $compare:expr) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            let $rn = cpu.registers.gpr[rn(opcode)];
            cpu.registers.sr.t = $compare;
        }
    };
    ($name:ident, |$rm:ident, $rn:ident| $compare:expr) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            let $rm = cpu.registers.gpr[rm(opcode)];
            let $rn = cpu.registers.gpr[rn(opcode)];
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
    let imm = opcode as i8;
    cpu.registers.sr.t = cpu.registers.gpr[0] == imm as u32;
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
pub fn cmp_str(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let xor = cpu.registers.gpr[m] ^ cpu.registers.gpr[n];
    cpu.registers.sr.t = (xor & 0xFF == 0)
        || ((xor >> 8) & 0xFF == 0)
        || ((xor >> 16) & 0xFF == 0)
        || ((xor >> 24) & 0xFF == 0);
}

// EXTS.B Rm, Rn
// Sign extend byte
pub fn exts_b(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[m] as i8 as u32;
}

// EXTS.W Rm, Rn
// Sign extend word
pub fn exts_w(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[m] as i16 as u32;
}

// EXTU.B Rm, Rn
// Zero extend byte
pub fn extu_b(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[m] & 0xFF;
}

// EXTU.W Rm, Rn
// Zero extend word
pub fn extu_w(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[m] & 0xFFFF;
}

// DT Rn
// Decrement and test
pub fn dt(cpu: &mut Sh2, opcode: u16) {
    let n = rn(opcode);
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_sub(1);
    cpu.registers.sr.t = cpu.registers.gpr[n] == 0;
}

// MUL.L Rm, Rn
// 32-bit x 32-bit -> 32-bit multiplication
pub fn mul(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);
    cpu.registers.macl = cpu.registers.gpr[m].wrapping_mul(cpu.registers.gpr[n]);
}

// MULS.W Rm, Rn
// Signed 16-bit x 16-bit -> 32-bit multiplication
pub fn muls(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let operand_l: i32 = (cpu.registers.gpr[m] as i16).into();
    let operand_r: i32 = (cpu.registers.gpr[n] as i16).into();
    cpu.registers.macl = (operand_l * operand_r) as u32;
}

// MULU.W Rm, Rn
// Unsigned 16-bit x 16-bit -> 32-bit multiplication
pub fn mulu(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let operand_l = cpu.registers.gpr[m] & 0xFFFF;
    let operand_r = cpu.registers.gpr[n] & 0xFFFF;
    cpu.registers.macl = operand_l * operand_r;
}

// DMULS.L Rm, Rn
// Signed 32-bit x 32-bit -> 64-bit multiplication
pub fn dmuls(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let operand_l: i64 = (cpu.registers.gpr[m] as i32).into();
    let operand_r: i64 = (cpu.registers.gpr[n] as i32).into();
    cpu.registers.set_mac(operand_l * operand_r);
}

// DMULU Rm, Rn
// Unsigned 32-bit x 32-bit -> 64-bit multiplication
pub fn dmulu(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let product = u64::from(cpu.registers.gpr[m]) * u64::from(cpu.registers.gpr[n]);
    cpu.registers.macl = product as u32;
    cpu.registers.mach = (product >> 32) as u32;
}

// MAC.W @Rm+, @Rn+
// Multiply and accumulate with word operands
pub fn mac_w<B: BusInterface + ?Sized>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let operand_l = cpu.read_word(cpu.registers.gpr[m], bus) as i16;
    cpu.registers.gpr[m] = cpu.registers.gpr[m].wrapping_add(2);

    let operand_r = cpu.read_word(cpu.registers.gpr[n], bus) as i16;
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_add(2);

    let product = i64::from(operand_l) * i64::from(operand_r);

    if cpu.registers.sr.s {
        // 16-bit x 16-bit + 32-bit -> 32-bit, with saturation
        let sum = i64::from(cpu.registers.macl as i32) + product;
        cpu.registers.macl = sum.clamp(i32::MIN.into(), i32::MAX.into()) as u32;
        // TODO set overflow bit in MACH? manual suggests that only SH-1 does this
    } else {
        // 16-bit x 16-bit + 64-bit -> 64-bit
        let sum = product.wrapping_add(cpu.registers.mac());
        cpu.registers.set_mac(sum);
    }
}

// MAC.L @Rm+, @Rn+
// Multiply and accumulate with longword operands
pub fn mac_l<B: BusInterface + ?Sized>(cpu: &mut Sh2, opcode: u16, bus: &mut B) {
    let m = rm(opcode);
    let n = rn(opcode);

    let operand_l = cpu.read_longword(cpu.registers.gpr[m], bus) as i32;
    cpu.registers.gpr[m] = cpu.registers.gpr[m].wrapping_add(4);

    let operand_r = cpu.read_longword(cpu.registers.gpr[n], bus) as i32;
    cpu.registers.gpr[n] = cpu.registers.gpr[n].wrapping_add(4);

    let product = i64::from(operand_l) * i64::from(operand_r);
    let product_sum = product.wrapping_add(cpu.registers.mac());

    if cpu.registers.sr.s {
        // Saturate to signed 48-bit
        let clamped = product_sum.clamp(-(1 << 47), (1 << 47) - 1);
        cpu.registers.set_mac(clamped);
    } else {
        cpu.registers.set_mac(product_sum);
    }
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
    let divisor = cpu.registers.gpr[rm(opcode)];
    let dividend = cpu.registers.gpr[rn(opcode)];

    cpu.registers.sr.m = divisor.sign_bit();
    cpu.registers.sr.q = dividend.sign_bit();
    cpu.registers.sr.t = cpu.registers.sr.m != cpu.registers.sr.q;
}

// DIV1 Rm, Rn
// Division single step
pub fn div1(cpu: &mut Sh2, opcode: u16) {
    let m = rm(opcode);
    let n = rn(opcode);

    let divisor = cpu.registers.gpr[m];
    let mut dividend = cpu.registers.gpr[n];

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
    cpu.registers.gpr[n] = dividend;
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
