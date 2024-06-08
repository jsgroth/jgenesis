use crate::instructions::parse_signed_immediate;
use crate::Sh2;

macro_rules! impl_conditional_branch {
    ($name:ident $(, $not:tt)?) => {
        pub fn $name(cpu: &mut Sh2, opcode: u16) {
            if $($not)? cpu.registers.sr.t {
                let displacement = parse_signed_immediate(opcode) << 1;
                cpu.registers.pc = cpu.registers.next_pc.wrapping_add(displacement as u32);
                cpu.registers.next_pc = cpu.registers.pc.wrapping_add(2);
            }
        }
    }
}

// BF label
// Branch if false
impl_conditional_branch!(bf, !);

// BT label
// Branch if true
impl_conditional_branch!(bt);
