use crate::instructions::{parse_register_high, parse_register_low, parse_signed_immediate};
use crate::Sh2;

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
