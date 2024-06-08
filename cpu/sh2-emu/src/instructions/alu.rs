use crate::instructions::{parse_register_high, parse_register_low, parse_signed_immediate};
use crate::Sh2;

pub fn add_imm_rn(cpu: &mut Sh2, opcode: u16) {
    let register = parse_register_high(opcode) as usize;
    let immediate = parse_signed_immediate(opcode);

    cpu.registers.gpr[register] = cpu.registers.gpr[register].wrapping_add(immediate as u32);
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
