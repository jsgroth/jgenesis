pub fn instruction_str(opcode: u8, alt1: bool, alt2: bool) -> String {
    match opcode {
        0x00 => "STOP".into(),
        0x01 => "NOP".into(),
        0x02 => "CACHE".into(),
        0x03 => "LSR".into(),
        0x04 => "ROL".into(),
        0x05 => "BRA e".into(),
        0x06 => "BGE e".into(),
        0x07 => "BLT e".into(),
        0x08 => "BNE e".into(),
        0x09 => "BEQ e".into(),
        0x0A => "BPL e".into(),
        0x0B => "BMI e".into(),
        0x0C => "BCC e".into(),
        0x0D => "BCS e".into(),
        0x0E => "BVC e".into(),
        0x0F => "BVS e".into(),
        0x10..=0x1F => format!("TO R{}", opcode & 0x0F),
        0x20..=0x2F => format!("WITH R{}", opcode & 0x0F),
        0x30..=0x3B => {
            if alt1 {
                format!("STB (R{})", opcode & 0x0F)
            } else {
                format!("STW (R{})", opcode & 0x0F)
            }
        }
        0x3C => "LOOP".into(),
        0x3D => "ALT1".into(),
        0x3E => "ALT2".into(),
        0x3F => "ALT3".into(),
        0x40..=0x4B => {
            if alt1 {
                format!("LDB (R{})", opcode & 0x0F)
            } else {
                format!("LDW (R{})", opcode & 0x0F)
            }
        }
        0x4C => {
            if alt1 {
                "RPIX".into()
            } else {
                "PLOT".into()
            }
        }
        0x4D => "SWAP".into(),
        0x4E => {
            if alt1 {
                "CMODE".into()
            } else {
                "COLOR".into()
            }
        }
        0x4F => "NOT".into(),
        0x50..=0x5F => match (alt2, alt1) {
            (false, false) => format!("ADD R{}", opcode & 0x0F),
            (false, true) => format!("ADC R{}", opcode & 0x0F),
            (true, false) => format!("ADD #{}", opcode & 0x0F),
            (true, true) => format!("ADC #{}", opcode & 0x0F),
        },
        0x60..=0x6F => match (alt2, alt1) {
            (false, false) => format!("SUB R{}", opcode & 0x0F),
            (false, true) => format!("SBC R{}", opcode & 0x0F),
            (true, false) => format!("SUB #{}", opcode & 0x0F),
            (true, true) => format!("CMP R{}", opcode & 0x0F),
        },
        0x70 => "MERGE".into(),
        0x71..=0x7F => match (alt2, alt1) {
            (false, false) => format!("AND R{}", opcode & 0x0F),
            (false, true) => format!("BIC R{}", opcode & 0x0F),
            (true, false) => format!("AND #{}", opcode & 0x0F),
            (true, true) => format!("BIC #{}", opcode & 0x0F),
        },
        0x80..=0x8F => match (alt2, alt1) {
            (false, false) => format!("MULT R{}", opcode & 0x0F),
            (false, true) => format!("UMULT R{}", opcode & 0x0F),
            (true, false) => format!("MULT #{}", opcode & 0x0F),
            (true, true) => format!("UMULT #{}", opcode & 0x0F),
        },
        0x90 => "SBK".into(),
        0x91..=0x94 => format!("LINK #{}", opcode & 0x0F),
        0x95 => "SEX".into(),
        0x96 => {
            if alt1 {
                "DIV2".into()
            } else {
                "ASR".into()
            }
        }
        0x97 => "ROR".into(),
        0x98..=0x9D => {
            if alt1 {
                format!("LJMP R{}", opcode & 0x0F)
            } else {
                format!("JMP R{}", opcode & 0x0F)
            }
        }
        0x9E => "LOB".into(),
        0x9F => {
            if alt1 {
                "LMULT".into()
            } else {
                "FMULT".into()
            }
        }
        0xA0..=0xAF => match (alt2, alt1) {
            (false, false) => format!("IBT R{}, #pp", opcode & 0x0F),
            (_, true) => format!("LMS R{}, (yy)", opcode & 0x0F),
            (true, false) => format!("SMS (yy), R{}", opcode & 0x0F),
        },
        0xB0..=0xBF => format!("FROM R{}", opcode & 0x0F),
        0xC0 => "HIB".into(),
        0xC1..=0xCF => match (alt2, alt1) {
            (false, false) => format!("OR R{}", opcode & 0x0F),
            (false, true) => format!("XOR R{}", opcode & 0x0F),
            (true, false) => format!("OR #{}", opcode & 0x0F),
            (true, true) => format!("XOR #{}", opcode & 0x0F),
        },
        0xD0..=0xDE => format!("INC R{}", opcode & 0x0F),
        0xDF => match (alt2, alt1) {
            (false, _) => "GETC".into(),
            (true, false) => "RAMB".into(),
            (true, true) => "ROMB".into(),
        },
        0xE0..=0xEE => format!("DEC R{}", opcode & 0x0F),
        0xEF => match (alt2, alt1) {
            (false, false) => "GETB".into(),
            (false, true) => "GETBH".into(),
            (true, false) => "GETBL".into(),
            (true, true) => "GETBS".into(),
        },
        0xF0..=0xFF => match (alt2, alt1) {
            (false, false) => format!("IWT R{}, #xx", opcode & 0x0F),
            (_, true) => format!("LM R{}, (xx)", opcode & 0x0F),
            (true, false) => format!("SM (xx), R{}", opcode & 0x0F),
        },
    }
}
