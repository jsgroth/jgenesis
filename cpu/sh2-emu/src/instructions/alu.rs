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

// SUB Rm, Rn
// Subtraction
pub fn sub_rm_rn(cpu: &mut Sh2, opcode: u16) {
    let source = parse_register_low(opcode) as usize;
    let destination = parse_register_high(opcode) as usize;

    cpu.registers.gpr[destination] =
        cpu.registers.gpr[destination].wrapping_sub(cpu.registers.gpr[source]);
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

    fn mn_opcode(rm: u16, rn: u16) -> u16 {
        (rm << 4) | (rn << 8)
    }

    #[test]
    fn unsigned_division_w() {
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
}
