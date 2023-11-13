use crate::coprocessors::upd77c25::{FlagsRegister, Upd77c25, Upd77c25Variant};
use jgenesis_common::num::{GetBit, SignBit};

pub fn execute(cpu: &mut Upd77c25) {
    if cpu.idling {
        return;
    }

    let opcode = cpu.program_rom[cpu.registers.pc as usize];
    log::trace!("Got opcode {opcode:06X} from PC {:03X}", cpu.registers.pc);
    cpu.registers.pc = (cpu.registers.pc + 1) & cpu.pc_mask;

    match opcode & 0xC00000 {
        0x000000 | 0x400000 => execute_alu(cpu, opcode),
        0x800000 => execute_jump(cpu, opcode),
        0xC00000 => execute_load(cpu, opcode),
        _ => unreachable!("value & 0xC00000 is always one of the above values"),
    }
}

fn execute_alu(cpu: &mut Upd77c25, opcode: u32) {
    let alu_input = (opcode >> 20) & 0x3;
    let alu_opcode = (opcode >> 16) & 0xF;

    let source_register = (opcode >> 4) & 0xF;
    let source = read_register(cpu, source_register);

    let operand = match alu_input {
        0x0 => cpu.ram[cpu.registers.dp as usize],
        0x1 => source,
        0x2 => ((2 * cpu.registers.kl()) >> 16) as u16,
        0x3 => (2 * cpu.registers.kl()) as u16,
        _ => unreachable!("value & 0x03 is always <= 0x03"),
    };
    let operand = operand as i16;

    let (accumulator, flags, other_flags) = if opcode.bit(15) {
        (&mut cpu.registers.accumulator_b, &mut cpu.registers.flags_b, cpu.registers.flags_a)
    } else {
        (&mut cpu.registers.accumulator_a, &mut cpu.registers.flags_a, cpu.registers.flags_b)
    };

    match alu_opcode {
        0x00 => {}
        0x01 => or(accumulator, operand, flags),
        0x02 => and(accumulator, operand, flags),
        0x03 => xor(accumulator, operand, flags),
        0x04 => sub(accumulator, operand, false, flags),
        0x05 => add(accumulator, operand, false, flags),
        0x06 => sub(accumulator, operand, other_flags.c, flags),
        0x07 => add(accumulator, operand, other_flags.c, flags),
        0x08 => sub(accumulator, 1, false, flags),
        0x09 => add(accumulator, 1, false, flags),
        0x0A => not(accumulator, flags),
        0x0B => sar1(accumulator, flags),
        0x0C => rcl1(accumulator, other_flags.c, flags),
        0x0D => sll2(accumulator, flags),
        0x0E => sll4(accumulator, flags),
        0x0F => xchg(accumulator, flags),
        _ => unreachable!("value & 0x0F is always <= 0x0F"),
    }

    let dest_register = opcode & 0xF;
    write_register(cpu, dest_register, source);

    log::trace!("  Copied {source:04X} from {source_register:02X} to {dest_register:02X}");

    let dpl_adjust = (opcode >> 13) & 0x3;
    match dpl_adjust {
        0x00 => {}
        0x01 => {
            cpu.registers.dp =
                (cpu.registers.dp & !0x0F) | (cpu.registers.dp.wrapping_add(1) & 0x0F);
        }
        0x02 => {
            cpu.registers.dp =
                (cpu.registers.dp & !0x0F) | (cpu.registers.dp.wrapping_sub(1) & 0x0F);
        }
        0x03 => {
            cpu.registers.dp &= !0x0F;
        }
        _ => unreachable!("value & 0x03 is always <= 0x03"),
    }

    let dph_adjust = ((opcode >> 9) & 0xF) as u16;
    cpu.registers.dp ^= dph_adjust << 4;

    let rp_adjust = opcode.bit(8);
    if rp_adjust {
        cpu.registers.rp = cpu.registers.rp.wrapping_sub(1) & cpu.rp_mask;
    }

    let ret = opcode.bit(22);
    if ret {
        cpu.registers.pc = cpu.registers.pop_stack();
    }

    log::trace!(
        "  ALU op: input={alu_input}, opcode={alu_opcode:02X}, accumulator={}, operand={operand:04X}, dpl_adjust={dpl_adjust}, dph_adjust={dph_adjust:X}, rp_adjust={rp_adjust}, ret={ret}",
        if opcode.bit(15) { "B" } else { "A" }
    );
}

fn or(accumulator: &mut i16, operand: i16, flags: &mut FlagsRegister) {
    *accumulator |= operand;
    *flags = set_flags_bit_op(*accumulator);
}

fn and(accumulator: &mut i16, operand: i16, flags: &mut FlagsRegister) {
    *accumulator &= operand;
    *flags = set_flags_bit_op(*accumulator);
}

fn xor(accumulator: &mut i16, operand: i16, flags: &mut FlagsRegister) {
    *accumulator ^= operand;
    *flags = set_flags_bit_op(*accumulator);
}

fn sub(accumulator: &mut i16, operand: i16, borrow: bool, flags: &mut FlagsRegister) {
    let (partial_unsigned_diff, carry1) = (*accumulator as u16).overflowing_sub(operand as u16);
    let (unsigned_diff, carry2) = partial_unsigned_diff.overflowing_sub(borrow.into());
    let new_borrow = carry1 || carry2;

    let signed_diff = i32::from(*accumulator) - i32::from(operand) - i32::from(borrow);
    let overflow = signed_diff < i16::MIN.into() || signed_diff > i16::MAX.into();

    *accumulator = unsigned_diff as i16;
    set_flags_add_sub(*accumulator, new_borrow, overflow, flags);
}

fn add(accumulator: &mut i16, operand: i16, carry: bool, flags: &mut FlagsRegister) {
    let (partial_unsigned_sum, carry1) = (*accumulator as u16).overflowing_add(operand as u16);
    let (unsigned_sum, carry2) = partial_unsigned_sum.overflowing_add(carry.into());
    let new_carry = carry1 || carry2;

    let signed_sum = i32::from(*accumulator) + i32::from(operand) + i32::from(carry);
    let overflow = signed_sum < i16::MIN.into() || signed_sum > i16::MAX.into();

    *accumulator = unsigned_sum as i16;
    set_flags_add_sub(*accumulator, new_carry, overflow, flags);
}

fn not(accumulator: &mut i16, flags: &mut FlagsRegister) {
    *accumulator = !(*accumulator);
    *flags = set_flags_bit_op(*accumulator);
}

fn sar1(accumulator: &mut i16, flags: &mut FlagsRegister) {
    let carry = accumulator.bit(0);
    *accumulator >>= 1;
    *flags = set_flags_shift_op(*accumulator, carry);
}

fn rcl1(accumulator: &mut i16, carry: bool, flags: &mut FlagsRegister) {
    let new_carry = accumulator.sign_bit();
    *accumulator = (*accumulator << 1) | i16::from(carry);
    *flags = set_flags_shift_op(*accumulator, new_carry);
}

fn sll2(accumulator: &mut i16, flags: &mut FlagsRegister) {
    *accumulator = (*accumulator << 2) | 0x03;
    *flags = set_flags_bit_op(*accumulator);
}

fn sll4(accumulator: &mut i16, flags: &mut FlagsRegister) {
    *accumulator = (*accumulator << 4) | 0x0F;
    *flags = set_flags_bit_op(*accumulator);
}

fn xchg(accumulator: &mut i16, flags: &mut FlagsRegister) {
    *accumulator = accumulator.swap_bytes();
    *flags = set_flags_bit_op(*accumulator);
}

fn set_flags_bit_op(accumulator: i16) -> FlagsRegister {
    FlagsRegister {
        z: accumulator == 0,
        c: false,
        s0: accumulator < 0,
        s1: accumulator < 0,
        ov0: false,
        ov1: false,
    }
}

fn set_flags_shift_op(accumulator: i16, carry: bool) -> FlagsRegister {
    FlagsRegister {
        z: accumulator == 0,
        c: carry,
        s0: accumulator < 0,
        s1: accumulator < 0,
        ov0: false,
        ov1: false,
    }
}

fn set_flags_add_sub(accumulator: i16, carry: bool, overflow: bool, flags: &mut FlagsRegister) {
    flags.z = accumulator == 0;
    flags.c = carry;
    flags.s0 = accumulator < 0;
    flags.ov0 = overflow;

    if overflow {
        flags.s1 = accumulator < 0;
        flags.ov1 = !flags.ov1;
    }
}

fn execute_load(cpu: &mut Upd77c25, opcode: u32) {
    log::trace!("  Load opcode: {opcode:06X}");

    let value = (opcode >> 6) as u16;
    let dest = opcode & 0xF;
    write_register(cpu, dest, value);
}

fn execute_jump(cpu: &mut Upd77c25, opcode: u32) {
    log::trace!("  Jump opcode: {:03X}", (opcode >> 13) & 0x1FF);

    let opcode_u16 = opcode as u16;
    let mut jump_addr = match cpu.variant {
        Upd77c25Variant::Dsp => (opcode_u16 >> 2) & 0x7FF,
        Upd77c25Variant::St010 | Upd77c25Variant::St011 => {
            // Normal jumps can't touch the highest bit of address, and bits 1-0 are used as A11-A12
            (cpu.registers.pc & 0x2000) | ((opcode_u16 >> 2) & 0x7FF) | ((opcode_u16 & 0x03) << 11)
        }
    };

    let should_jump = match (opcode >> 13) & 0x1FF {
        // JMPSO
        0x000 => {
            jump_addr = cpu.registers.so & cpu.pc_mask;
            true
        }
        // JNCA / JCA
        0x080 => !cpu.registers.flags_a.c,
        0x082 => cpu.registers.flags_a.c,
        // JNCB / JCB
        0x084 => !cpu.registers.flags_b.c,
        0x086 => cpu.registers.flags_b.c,
        // JNZA / JZA
        0x088 => !cpu.registers.flags_a.z,
        0x08A => cpu.registers.flags_a.z,
        // JNZB / JZB
        0x08C => !cpu.registers.flags_b.z,
        0x08E => cpu.registers.flags_b.z,
        // JNOVA0 / JOVA0
        0x090 => !cpu.registers.flags_a.ov0,
        0x092 => cpu.registers.flags_a.ov0,
        // JNOVB0 / JOVB0
        0x094 => !cpu.registers.flags_b.ov0,
        0x096 => cpu.registers.flags_b.ov0,
        // JNOVA1 / JOVA1
        0x098 => !cpu.registers.flags_a.ov1,
        0x09A => cpu.registers.flags_a.ov1,
        // JNOVB1 / JOVB1
        0x09C => !cpu.registers.flags_b.ov1,
        0x09E => cpu.registers.flags_b.ov1,
        // JNSA0 / JSA0
        0x0A0 => !cpu.registers.flags_a.s0,
        0x0A2 => cpu.registers.flags_a.s0,
        // JNSB0 / JSB0
        0x0A4 => !cpu.registers.flags_b.s0,
        0x0A6 => cpu.registers.flags_b.s0,
        // JNSA1 / JSA1
        0x0A8 => !cpu.registers.flags_a.s1,
        0x0AA => cpu.registers.flags_a.s1,
        // JNSB1 / JSB1
        0x0AC => !cpu.registers.flags_b.s1,
        0x0AE => cpu.registers.flags_b.s1,
        // JDPL0 / JDPLN0
        0x0B0 => cpu.registers.dp & 0x0F == 0,
        0x0B1 => cpu.registers.dp & 0x0F != 0,
        // JDPLF / JDPLNF
        0x0B2 => cpu.registers.dp & 0x0F == 0x0F,
        0x0B3 => cpu.registers.dp & 0x0F != 0x0F,
        // JNSIAK / JSIAK / JNSOAK / JSOAK; not implemented
        0x0B4 | 0x0B6 | 0x0B8 | 0x0BA => {
            log::warn!("Unimplemented uPD77C25 jump opcode (serial registers): {opcode:06X}");
            false
        }
        // JNRQM / JRQM
        0x0BC => !cpu.registers.sr.request_for_master,
        0x0BE => cpu.registers.sr.request_for_master,
        // JMP
        0x100 => {
            jump_addr &= 0x1FFF;
            true
        }
        0x101 => {
            jump_addr |= 0x2000;
            true
        }
        // CALL
        0x140 => {
            jump_addr &= 0x1FFF;
            cpu.registers.push_stack(cpu.registers.pc);
            true
        }
        0x141 => {
            jump_addr |= 0x2000;
            cpu.registers.push_stack(cpu.registers.pc);
            true
        }
        _ => panic!("invalid uPD77C25 jump opcode: {opcode:06X}"),
    };

    if should_jump {
        log::trace!("  Jumping to {jump_addr:03X}");

        if jump_addr == cpu.registers.pc.wrapping_sub(1) & cpu.pc_mask {
            log::trace!("  Detected idle loop; halting CPU until next SNES DR write");
            cpu.idling = true;
        }

        cpu.registers.pc = jump_addr;
    }
}

fn read_register(cpu: &mut Upd77c25, register: u32) -> u16 {
    log::trace!("  Reading register {register:02X}");

    match register {
        0x00 => cpu.registers.trb as u16,
        0x01 => cpu.registers.accumulator_a as u16,
        0x02 => cpu.registers.accumulator_b as u16,
        0x03 => cpu.registers.tr as u16,
        0x04 => cpu.registers.dp,
        0x05 => cpu.registers.rp,
        0x06 => cpu.data_rom[cpu.registers.rp as usize],
        0x07 => 0x8000 - u16::from(cpu.registers.flags_a.s1),
        0x08 => {
            // Reading DR sets RQM
            cpu.registers.sr.request_for_master = true;
            cpu.registers.dr
        }
        0x09 => {
            // Reading DRNF does not set RQM
            cpu.registers.dr
        }
        0x0A => u16::from(u8::from(cpu.registers.sr)) << 8,
        0x0B | 0x0C => cpu.registers.so,
        0x0D => cpu.registers.k as u16,
        0x0E => cpu.registers.l as u16,
        0x0F => cpu.ram[cpu.registers.dp as usize],
        _ => panic!("invalid uPD77C25 register read: {register:02X}"),
    }
}

#[allow(clippy::match_same_arms)]
fn write_register(cpu: &mut Upd77c25, register: u32, value: u16) {
    log::trace!("  Writing {value:04X} to register {register:02X}");

    match register {
        0x00 => {}
        0x01 => cpu.registers.accumulator_a = value as i16,
        0x02 => cpu.registers.accumulator_b = value as i16,
        0x03 => cpu.registers.tr = value as i16,
        0x04 => cpu.registers.dp = value & cpu.dp_mask,
        0x05 => cpu.registers.rp = value & cpu.rp_mask,
        0x06 => cpu.registers.upd_write_data(value),
        0x07 => cpu.registers.sr.write(value),
        0x08 | 0x09 => cpu.registers.so = value,
        0x0A => cpu.registers.k = value as i16,
        0x0B => {
            cpu.registers.k = value as i16;
            cpu.registers.l = cpu.data_rom[cpu.registers.rp as usize] as i16;
        }
        0x0C => {
            cpu.registers.l = value as i16;
            cpu.registers.k = cpu.ram[(cpu.registers.dp | 0x40) as usize] as i16;
        }
        0x0D => cpu.registers.l = value as i16,
        0x0E => cpu.registers.trb = value as i16,
        0x0F => cpu.ram[cpu.registers.dp as usize] = value,
        _ => panic!("invalid uPD77C25 register write: {register:02X} {value:04X}"),
    }
}
