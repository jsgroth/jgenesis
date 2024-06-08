pub fn disassemble(opcode: u16) -> String {
    match opcode {
        0b0000_0000_0001_1001 => "DIV0U".into(),
        0b0000_0000_0000_1011 => "RTS".into(),
        0b0000_0000_0000_1000 => "CLRT".into(),
        0b0000_0000_0010_1000 => "CLRMAC".into(),
        0b0000_0000_0000_1001 => "NOP".into(),
        0b0000_0000_0010_1011 => "RTE".into(),
        0b0000_0000_0001_1000 => "SETT".into(),
        0b0000_0000_0001_1011 => "SLEEP".into(),
        _ => decode_xnnx(opcode),
    }
}

#[inline]
fn decode_xnnx(opcode: u16) -> String {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => {
            format!("MOV R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0000 => {
            format!("MOV.B R{}, @R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0001 => {
            format!("MOV.W R{}, @R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0010 => {
            format!("MOV.L R{}, @R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0000 => {
            format!("MOV.B @R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0001 => {
            format!("MOV.W @R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0010 => {
            format!("MOV.L @R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0100 => {
            format!("MOV.B R{}, @-R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0101 => {
            format!("MOV.W R{}, @-R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0110 => {
            format!("MOV.L R{}, @-R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0100 => {
            format!("MOV.B @R{}+, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0101 => {
            format!("MOV.W @R{}+, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0110 => {
            format!("MOV.L @R{}+, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0100 => {
            format!("MOV.B R{}, @(R0,R{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0101 => {
            format!("MOV.W R{}, @(R0,R{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0110 => {
            format!("MOV.L R{}, @(R0,R{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1100 => {
            format!("MOV.B @(R0,R{}), R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1101 => {
            format!("MOV.W @(R0,R{}), R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1110 => {
            format!("MOV.L @(R0,R{}), R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1000 => {
            format!("SWAP.B R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1001 => {
            format!("SWAP.W R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1101 => {
            format!("XTRCT R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1100 => {
            format!("ADD R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1110 => {
            format!("ADDC R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1111 => {
            format!("ADDV R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0000 => {
            format!("CMP/EQ R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0010 => {
            format!("CMP/HS R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0011 => {
            format!("CMP/GE R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0110 => {
            format!("CMP/HI R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0111 => {
            format!("CMP/GT R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1100 => {
            format!("CMP/ST R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0100 => {
            format!("DIV1 R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0111 => {
            format!("DIV0S R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1101 => {
            format!("DMULS.L R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0101 => {
            format!("DMULU.L R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1110 => {
            format!("EXTS.B R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1111 => {
            format!("EXTS.W R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1100 => {
            format!("EXTU.B R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1101 => {
            format!("EXTU.W R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1111 => {
            format!("MAC.L @R{}+, @R{}+", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0100_0000_0000_1111 => {
            format!("MAC @R{}+, @R{}+", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0111 => {
            format!("MUL.L R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1111 => {
            format!("MULS.W R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1110 => {
            format!("MULU.W R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1011 => {
            format!("NEG R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1010 => {
            format!("NEGC R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1000 => {
            format!("SUB R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1010 => {
            format!("SUBC R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1011 => {
            format!("SUBV R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1001 => {
            format!("AND R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0111 => {
            format!("NOT R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1011 => {
            format!("OR R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1000 => {
            format!("TST R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1010 => {
            format!("XOR R{}, R{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        _ => decode_xxnn(opcode),
    }
}

#[inline]
fn decode_xxnn(opcode: u16) -> String {
    match opcode & 0b1111_1111_0000_0000 {
        0b1000_0000_0000_0000 => format!(
            "MOV.B R0, @({},R{})",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0001_0000_0000 => format!(
            "MOV.W R0, @({},R{})",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0100_0000_0000 => format!(
            "MOV.B @({},R{}), R0",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0101_0000_0000 => format!(
            "MOV.W @({},R{}), R0",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1100_0000_0000_0000 => format!("MOV.B R0, @({},GBR)", parse_8bit_displacement(opcode)),
        0b1100_0001_0000_0000 => format!("MOV.W R0, @({},GBR)", parse_8bit_displacement(opcode)),
        0b1100_0010_0000_0000 => format!("MOV.L R0, @({},GBR)", parse_8bit_displacement(opcode)),
        0b1100_0100_0000_0000 => format!("MOV.B @({},GBR), R0", parse_8bit_displacement(opcode)),
        0b1100_0101_0000_0000 => format!("MOV.W @({},GBR), R0", parse_8bit_displacement(opcode)),
        0b1100_0110_0000_0000 => format!("MOV.L @({},GBR), R0", parse_8bit_displacement(opcode)),
        0b1100_0111_0000_0000 => format!("MOVA @({},PC), R0", parse_8bit_displacement(opcode)),
        0b1000_1000_0000_0000 => format!("CMP/EQ #{}, R0", parse_signed_immediate(opcode)),
        0b1100_1001_0000_0000 => format!("AND #{}, R0", parse_unsigned_immediate(opcode)),
        0b1100_1101_0000_0000 => format!("AND.B #{}, @(R0,GBR)", parse_unsigned_immediate(opcode)),
        0b1100_1011_0000_0000 => format!("OR #{}, R0", parse_unsigned_immediate(opcode)),
        0b1100_1111_0000_0000 => format!("OR.B #{}, @(R0,GBR)", parse_unsigned_immediate(opcode)),
        0b1100_1000_0000_0000 => format!("TST #{}, R0", parse_unsigned_immediate(opcode)),
        0b1100_1100_0000_0000 => format!("TST.B #{}, @(R0,GBR)", parse_unsigned_immediate(opcode)),
        0b1100_1010_0000_0000 => format!("XOR #{}, R0", parse_unsigned_immediate(opcode)),
        0b1100_1110_0000_0000 => format!("XOR.B #{}, @(R0,GBR)", parse_unsigned_immediate(opcode)),
        0b1000_1011_0000_0000 => format!("BF {}", parse_signed_immediate(opcode)),
        0b1000_1111_0000_0000 => format!("BF/S {}", parse_signed_immediate(opcode)),
        0b1000_1001_0000_0000 => format!("BT {}", parse_signed_immediate(opcode)),
        0b1000_1101_0000_0000 => format!("BT/S {}", parse_signed_immediate(opcode)),
        0b1100_0011_0000_0000 => format!("TRAPA #{}", parse_signed_immediate(opcode)),
        _ => decode_xnxx(opcode),
    }
}

#[inline]
fn decode_xnxx(opcode: u16) -> String {
    match opcode & 0b1111_0000_1111_1111 {
        0b0000_0000_0010_1001 => format!("MOVT R{}", parse_register_high(opcode)),
        0b0100_0000_0001_0001 => format!("CMP/PZ R{}", parse_register_high(opcode)),
        0b0100_0000_0001_0101 => format!("CMP/PL R{}", parse_register_high(opcode)),
        0b0100_0000_0001_0000 => format!("DT R{}", parse_register_high(opcode)),
        0b0100_0000_0001_1011 => format!("TAS.B @R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0100 => format!("ROTL R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0101 => format!("ROTR R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0100 => format!("ROTCL R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0101 => format!("ROTCR R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0000 => format!("SHAL R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0001 => format!("SHAR R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0000 => format!("SHLL R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0001 => format!("SHLR R{}", parse_register_high(opcode)),
        0b0100_0000_0000_1000 => format!("SHLL2 R{}", parse_register_high(opcode)),
        0b0100_0000_0000_1001 => format!("SHLR2 R{}", parse_register_high(opcode)),
        0b0100_0000_0001_1000 => format!("SHLL8 R{}", parse_register_high(opcode)),
        0b0100_0000_0001_1001 => format!("SHLR8 R{}", parse_register_high(opcode)),
        0b0100_0000_0010_1000 => format!("SHLL16 R{}", parse_register_high(opcode)),
        0b0100_0000_0010_1001 => format!("SHLR16 R{}", parse_register_high(opcode)),
        0b0000_0000_0010_0011 => format!("BRAF R{}", parse_register_high(opcode)),
        0b0000_0000_0000_0011 => format!("BSRF R{}", parse_register_high(opcode)),
        0b0100_0000_0010_1011 => format!("JMP @R{}", parse_register_high(opcode)),
        0b0100_0000_0000_1011 => format!("JSR @R{}", parse_register_high(opcode)),
        0b0100_0000_0000_1110 => format!("LDC R{}, SR", parse_register_high(opcode)),
        0b0100_0000_0001_1110 => format!("LDC R{}, GBR", parse_register_high(opcode)),
        0b0100_0000_0010_1110 => format!("LDC R{}, VBR", parse_register_high(opcode)),
        0b0100_0000_0000_0111 => format!("LDC.L @R{}+, SR", parse_register_high(opcode)),
        0b0100_0000_0001_0111 => format!("LDC.L @R{}+, GBR", parse_register_high(opcode)),
        0b0100_0000_0010_0111 => format!("LDC.L @R{}+, VBR", parse_register_high(opcode)),
        0b0100_0000_0000_1010 => format!("LDS R{}, MACH", parse_register_high(opcode)),
        0b0100_0000_0001_1010 => format!("LDS R{}, MACL", parse_register_high(opcode)),
        0b0100_0000_0010_1010 => format!("LDS R{}, PR", parse_register_high(opcode)),
        0b0100_0000_0000_0110 => format!("LDS.L @R{}+, MACH", parse_register_high(opcode)),
        0b0100_0000_0001_0110 => format!("LDS.L @R{}+, MACL", parse_register_high(opcode)),
        0b0100_0000_0010_0110 => format!("LDS.L @R{}+, PR", parse_register_high(opcode)),
        0b0000_0000_0000_0010 => format!("STC SR, R{}", parse_register_high(opcode)),
        0b0000_0000_0001_0010 => format!("STC GBR, R{}", parse_register_high(opcode)),
        0b0000_0000_0010_0010 => format!("STC VBR, R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0011 => format!("STC.L SR, @-R{}", parse_register_high(opcode)),
        0b0100_0000_0001_0011 => format!("STC.L GBR, @-R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0011 => format!("STC.L VBR, @-R{}", parse_register_high(opcode)),
        0b0000_0000_0000_1010 => format!("STS MACH, R{}", parse_register_high(opcode)),
        0b0000_0000_0001_1010 => format!("STS MACL, R{}", parse_register_high(opcode)),
        0b0000_0000_0010_1010 => format!("STS PR, R{}", parse_register_high(opcode)),
        0b0100_0000_0000_0010 => format!("STS.L MACH, @-R{}", parse_register_high(opcode)),
        0b0100_0000_0001_0010 => format!("STS.L MACL, @-R{}", parse_register_high(opcode)),
        0b0100_0000_0010_0010 => format!("STS.L PR, @-R{}", parse_register_high(opcode)),
        _ => decode_xnnn(opcode),
    }
}

#[inline]
fn decode_xnnn(opcode: u16) -> String {
    match opcode & 0b1111_0000_0000_0000 {
        0b1110_0000_0000_0000 => {
            format!("MOV.B #{}, R{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1001_0000_0000_0000 => format!(
            "MOV.W ({},PC), R{}",
            parse_8bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b1101_0000_0000_0000 => format!(
            "MOV.L ({},PC), R{}",
            parse_8bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b0001_0000_0000_0000 => format!(
            "MOV.L R{}, @({},R{})",
            parse_register_low(opcode),
            parse_4bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b0101_0000_0000_0000 => format!(
            "MOV.L @({},R{}), R{}",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode),
            parse_register_high(opcode)
        ),
        0b0111_0000_0000_0000 => {
            format!("ADD #{}, R{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1010_0000_0000_0000 => format!("BRA {}", parse_12bit_displacement(opcode)),
        0b1011_0000_0000_0000 => format!("BSR {}", parse_12bit_displacement(opcode)),
        _ => todo!("illegal (?) SH-2 opcode {opcode:04X}"),
    }
}

#[inline]
fn parse_4bit_displacement(opcode: u16) -> u16 {
    opcode & 0xF
}

#[inline]
fn parse_8bit_displacement(opcode: u16) -> u16 {
    opcode & 0xFF
}

#[inline]
fn parse_12bit_displacement(opcode: u16) -> i32 {
    (((opcode as i16) << 4) >> 4).into()
}

#[inline]
fn parse_signed_immediate(opcode: u16) -> i8 {
    opcode as i8
}

#[inline]
fn parse_unsigned_immediate(opcode: u16) -> u8 {
    opcode as u8
}

#[inline]
fn parse_register_low(opcode: u16) -> u16 {
    (opcode >> 4) & 0xF
}

#[inline]
fn parse_register_high(opcode: u16) -> u16 {
    (opcode >> 8) & 0xF
}
