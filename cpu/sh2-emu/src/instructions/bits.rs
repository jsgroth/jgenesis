use crate::instructions::parse_unsigned_immediate;
use crate::Sh2;

// TST #imm, R0
// Sets the T bit if (#imm AND R0) is 0 and clears the T bit otherwise
pub fn tst_imm_r0(cpu: &mut Sh2, opcode: u16) {
    let immediate = parse_unsigned_immediate(opcode);
    cpu.registers.sr.t = immediate & cpu.registers.gpr[0] == 0;
}
