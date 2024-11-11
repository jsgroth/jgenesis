use crate::instructions::Condition;
use jgenesis_common::num::GetBit;

struct DecodeTableEntry {
    mask: u32,
    target: u32,
    decode_fn: fn(u32) -> String,
}

impl DecodeTableEntry {
    const fn new(mask: u32, target: u32, decode_fn: fn(u32) -> String) -> Self {
        Self { mask, target, decode_fn }
    }
}

const ARM_DECODE_TABLE: &[DecodeTableEntry] = &[
    DecodeTableEntry::new(0x0FFFFFF0, 0x012FFF10, arm_bx),
    DecodeTableEntry::new(0x0E000000, 0x0A000000, arm_b),
    DecodeTableEntry::new(0x0FC000F0, 0x00000090, |opcode| {
        todo!("disassemble MUL/MLA {opcode:08X}")
    }),
    DecodeTableEntry::new(0x0FC000F0, 0x00400090, |opcode| {
        todo!("disassemble MULL/MLAL {opcode:08X}")
    }),
    DecodeTableEntry::new(0x0FB00FF0, 0x01000090, |opcode| {
        todo!("disassemble single data swap {opcode:08X}")
    }),
    DecodeTableEntry::new(0x0E400F90, 0x00000090, |opcode| {
        todo!("disassemble halfword transfer register offset {opcode:08X}")
    }),
    DecodeTableEntry::new(0x0E400090, 0x00400090, arm_ldrh_immediate),
    DecodeTableEntry::new(0x0FBF0FFF, 0x10F00000, arm_mrs),
    DecodeTableEntry::new(0x0DBEF000, 0x0128F000, arm_msr),
    DecodeTableEntry::new(0x0C000000, 0x00000000, arm_alu),
    DecodeTableEntry::new(0x0E000010, 0x06000010, |_| "Undefined".into()),
    DecodeTableEntry::new(0x0C000000, 0x04000000, arm_ldr),
];

pub fn arm(opcode: u32) -> String {
    for &DecodeTableEntry { mask, target, decode_fn } in ARM_DECODE_TABLE {
        if opcode & mask == target {
            return decode_fn(opcode);
        }
    }

    todo!("disassemble {opcode:08X}")
}

fn arm_bx(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let rn = opcode & 0xF;
    format!("BX{cond} R{rn}")
}

fn arm_b(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let link = if opcode.bit(24) { "L" } else { "" };
    let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
    format!("B{link}{cond} {offset}")
}

fn arm_alu(opcode: u32) -> String {
    let operation = match (opcode >> 21) & 0xF {
        0 => "AND",
        1 => "EOR",
        2 => "SUB",
        3 => "RSB",
        4 => "ADD",
        5 => "ADC",
        6 => "SBC",
        7 => "RSC",
        8 => "TST",
        9 => "TEQ",
        10 => "CMP",
        11 => "CMN",
        12 => "ORR",
        13 => "MOV",
        14 => "BIC",
        15 => "MVN",
        _ => unreachable!(),
    };

    let set_conditions = if opcode.bit(20) { "S" } else { "" };

    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    let condition = Condition::from_arm_opcode(opcode).suffix();

    let operand2 = if opcode.bit(25) {
        let imm = opcode & 0xFF;
        let rotation = ((opcode >> 8) & 0xF) << 1;
        format!("#0x{:X}", imm.rotate_right(rotation))
    } else {
        let rm = opcode & 0xF;
        let mut s = format!("R{rm}");

        let shift = (opcode >> 4) & 0xFF;
        if shift != 0 {
            let shift_type = shift_type_str(shift >> 1);

            if shift.bit(0) {
                let rs = shift >> 4;
                s.push_str(&format!(", {shift_type} R{rs}"));
            } else {
                let mut amount = shift >> 3;
                if amount == 0 {
                    amount = 32;
                }

                s.push_str(&format!(", {shift_type} #{amount}"));
            }
        }

        s
    };

    if operation == "MOV" || operation == "MVN" {
        format!("{operation}{condition}{set_conditions} R{rd}, {operand2}")
    } else if matches!(operation, "CMP" | "CMN" | "TEQ" | "TST") {
        format!("{operation}{condition} R{rn}, {operand2}")
    } else {
        format!("{operation}{condition}{set_conditions} R{rd}, R{rn}, {operand2}")
    }
}

fn shift_type_str(shift_type: u32) -> &'static str {
    match shift_type & 3 {
        0 => "LSL",
        1 => "LSR",
        2 => "ASR",
        3 => "ROR",
        _ => unreachable!(),
    }
}

fn arm_mrs(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let psr = if opcode.bit(22) { "SPSR" } else { "CPSR" };
    let rd = (opcode >> 12) & 0xF;

    format!("MRS{cond} R{rd}, {psr}")
}

fn arm_msr(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let psr = if opcode.bit(22) { "SPSR" } else { "CPSR" };
    let flags_only = if opcode.bit(16) { "" } else { "_flg" };

    let expression = if !opcode.bit(16) && opcode.bit(25) {
        let immediate = opcode & 0xFF;
        let rotation = ((opcode >> 8) & 0xF) << 1;
        let value = immediate.rotate_right(rotation);
        format!("#0x{value:X}")
    } else {
        let rm = opcode & 0xF;
        format!("R{rm}")
    };

    format!("MSR{cond} {psr}{flags_only}, {expression}")
}

fn arm_ldr(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;
    let load = opcode.bit(20);
    let write_back = opcode.bit(21);
    let byte = opcode.bit(22);
    let add = opcode.bit(23);
    let pre_indexed = opcode.bit(24);
    let immediate = !opcode.bit(25);

    let add_str = if add { "" } else { "-" };

    let offset = if immediate {
        let offset = opcode & 0xFFF;
        format!("#{add_str}0x{offset:X}")
    } else {
        let rm = opcode & 0xF;
        let shift = (opcode >> 4) & 0xFF;
        let shift_type = shift_type_str(shift >> 1);
        let mut shift_amount = shift >> 3;
        if shift_type != "LSL" && shift_amount == 0 {
            shift_amount = 32;
        }

        if shift == 0 {
            format!("{add_str}R{rm}")
        } else {
            format!("{add_str}R{rm}, {shift_type} {shift_amount}")
        }
    };

    let address = if pre_indexed {
        let write_back_str = if write_back { " {!}" } else { "" };
        format!("[R{rn}, {offset}]{write_back_str}")
    } else {
        format!("[R{rn}], {offset}")
    };

    let operation = if load { "LDR" } else { "STR" };
    let byte_suffix = if byte { "B" } else { "" };

    format!("{operation}{cond}{byte_suffix} R{rd}, {address}")
}

fn arm_ldrh_immediate(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let data_type = match (opcode >> 5) & 3 {
        0 => "SWP",
        1 => "H",
        2 => "SB",
        3 => "SH",
        _ => unreachable!(),
    };

    let operation = if opcode.bit(20) { "LDR" } else { "STR" };

    let rd = (opcode >> 12) & 0xF;
    let rn = (opcode >> 16) & 0xF;

    let offset = ((opcode >> 4) & 0xF0) | (opcode & 0xF);
    let sign = if opcode.bit(23) { "+" } else { "-" };

    let write_back = if opcode.bit(21) { " {!}" } else { "" };

    let address = if opcode.bit(24) {
        format!("[R{rn}, #{sign}0x{offset:X}]{write_back}")
    } else {
        format!("[R{rn}], #{sign}0x{offset:X}{write_back}")
    };

    format!("{operation}{cond}{data_type} R{rd}, {address}")
}
