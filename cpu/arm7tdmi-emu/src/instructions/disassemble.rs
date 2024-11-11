use crate::instructions::Condition;
use jgenesis_common::num::GetBit;

pub fn arm(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    if opcode & 0x0FFFFFF0 == 0x012FFF10 {
        todo!("disassemble BX {opcode:08X}")
    }

    if opcode & 0x0E000000 == 0x0A000000 {
        // B / BL
        let link = if opcode.bit(24) { "L" } else { "" };
        let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
        return format!("B{link}{cond} {offset}");
    }

    if opcode & 0x0FC000F0 == 0x00000090 {
        todo!("disassemble MUL/MLA {opcode:08X}")
    }

    if opcode & 0x0FC000F0 == 0x00400090 {
        todo!("disassemble MULL/MLAL {opcode:08X}")
    }

    if opcode & 0x0FB00FF0 == 0x01000090 {
        todo!("disassemble single data swap {opcode:08X}")
    }

    if opcode & 0x0E400F90 == 0x00000090 {
        todo!("disassemble halfword transfer register offset {opcode:08X}")
    }

    if opcode & 0x0E400090 == 0x00400090 {
        return arm_ldrh_immediate(opcode);
    }

    if opcode & 0x0C000000 == 0x00000000 {
        return arm_alu(opcode);
    }

    todo!("disassemble {opcode:08X}")
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
            let shift_type = match (shift >> 1) & 3 {
                0 => "LSL",
                1 => "LSR",
                2 => "ASR",
                3 => "ROR",
                _ => unreachable!(),
            };

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
    let sign = if opcode.bit(23) { "" } else { "-" };

    let write_back = if opcode.bit(21) { " {!}" } else { "" };

    let address = if opcode.bit(24) {
        format!("[R{rn}, #{sign}0x{offset:X}]{write_back}")
    } else {
        format!("[R{rn}], #{sign}0x{offset:X}{write_back}")
    };

    format!("{operation}{cond}{data_type} R{rd}, {address}")
}
