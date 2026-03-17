#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchDisplacement {
    Offset,
    Absolute { pc: u32 },
}

#[derive(Debug, Clone)]
pub struct DisassembleOptions {
    pub branch_displacement: BranchDisplacement,
}

impl Default for DisassembleOptions {
    fn default() -> Self {
        Self { branch_displacement: BranchDisplacement::Offset }
    }
}

#[must_use]
pub fn disassemble(opcode: u16, options: DisassembleOptions) -> String {
    match opcode {
        0b0000_0000_0001_1001 => "div0u".into(),
        0b0000_0000_0000_1011 => "rts".into(),
        0b0000_0000_0000_1000 => "clrt".into(),
        0b0000_0000_0010_1000 => "clrmac".into(),
        0b0000_0000_0000_1001 => "nop".into(),
        0b0000_0000_0010_1011 => "rte".into(),
        0b0000_0000_0001_1000 => "sett".into(),
        0b0000_0000_0001_1011 => "sleep".into(),
        _ => decode_xnnx(opcode, options),
    }
}

#[inline]
fn decode_xnnx(opcode: u16, options: DisassembleOptions) -> String {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => {
            format!("mov r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0000 => {
            format!("mov.b r{}, @r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0001 => {
            format!("mov.w r{}, @r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0010 => {
            format!("mov.l r{}, @r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0000 => {
            format!("mov.b @r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0001 => {
            format!("mov.w @r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0010 => {
            format!("mov.l @r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0100 => {
            format!("mov.b r{}, @-r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0101 => {
            format!("mov.w r{}, @-r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0110 => {
            format!("mov.l r{}, @-r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0100 => {
            format!("mov.b @r{}+, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0101 => {
            format!("mov.w @r{}+, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0110 => {
            format!("mov.l @r{}+, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0100 => {
            format!("mov.b r{}, @(r0,r{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0101 => {
            format!("mov.w r{}, @(r0,r{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0110 => {
            format!("mov.l r{}, @(r0,r{})", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1100 => {
            format!("mov.b @(r0,r{}), r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1101 => {
            format!("mov.w @(r0,r{}), r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1110 => {
            format!("mov.l @(r0,r{}), r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1000 => {
            format!("swap.b r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1001 => {
            format!("swap.w r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1101 => {
            format!("xtrct r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1100 => {
            format!("add r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1110 => {
            format!("addc r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1111 => {
            format!("addv r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0000 => {
            format!("cmp/eq r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0010 => {
            format!("cmp/hs r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0011 => {
            format!("cmp/ge r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0110 => {
            format!("cmp/hi r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0111 => {
            format!("cmp/gt r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1100 => {
            format!("cmp/str r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0100 => {
            format!("div1 r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0111 => {
            format!("div0s r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1101 => {
            format!("dmuls.l r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_0101 => {
            format!("dmulu.l r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1110 => {
            format!("exts.b r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1111 => {
            format!("exts.w r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1100 => {
            format!("extu.b r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1101 => {
            format!("extu.w r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_1111 => {
            format!("mac.l @r{}+, @r{}+", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0100_0000_0000_1111 => {
            format!("mac.w @r{}+, @r{}+", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0000_0000_0000_0111 => {
            format!("mul.l r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1111 => {
            format!("muls.w r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1110 => {
            format!("mulu.w r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1011 => {
            format!("neg r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_1010 => {
            format!("negc r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1000 => {
            format!("sub r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1010 => {
            format!("subc r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0011_0000_0000_1011 => {
            format!("subv r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1001 => {
            format!("and r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0110_0000_0000_0111 => {
            format!("not r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1011 => {
            format!("or r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1000 => {
            format!("tst r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_1010 => {
            format!("xor r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        _ => decode_xxnn(opcode, options),
    }
}

#[inline]
fn decode_xxnn(opcode: u16, options: DisassembleOptions) -> String {
    match opcode & 0b1111_1111_0000_0000 {
        0b1000_0000_0000_0000 => format!(
            "mov.b r0, @({},r{})",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0001_0000_0000 => format!(
            "mov.w r0, @({},r{})",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0100_0000_0000 => format!(
            "mov.b @({},r{}), r0",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1000_0101_0000_0000 => format!(
            "mov.w @({},r{}), r0",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode)
        ),
        0b1100_0000_0000_0000 => format!("mov.b r0, @({},gbr)", parse_8bit_displacement(opcode)),
        0b1100_0001_0000_0000 => format!("mov.w r0, @({},gbr)", parse_8bit_displacement(opcode)),
        0b1100_0010_0000_0000 => format!("mov.l r0, @({},gbr)", parse_8bit_displacement(opcode)),
        0b1100_0100_0000_0000 => format!("mov.b @({},gbr), r0", parse_8bit_displacement(opcode)),
        0b1100_0101_0000_0000 => format!("mov.w @({},gbr), r0", parse_8bit_displacement(opcode)),
        0b1100_0110_0000_0000 => format!("mov.l @({},gbr), r0", parse_8bit_displacement(opcode)),
        0b1100_0111_0000_0000 => format!("mova @({},pc), r0", parse_8bit_displacement(opcode)),
        0b1000_1000_0000_0000 => format!("cmp/eq #{}, r0", parse_signed_immediate(opcode)),
        0b1100_1001_0000_0000 => format!("and #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1101_0000_0000 => format!("and.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode)),
        0b1100_1011_0000_0000 => format!("or #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1111_0000_0000 => format!("or.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode)),
        0b1100_1000_0000_0000 => format!("tst #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1100_0000_0000 => format!("tst.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode)),
        0b1100_1010_0000_0000 => format!("xor #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1110_0000_0000 => format!("xor.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode)),
        0b1000_1011_0000_0000 => {
            format!("bf {}", parse_branch_displacement(opcode, options.branch_displacement))
        }
        0b1000_1111_0000_0000 => {
            format!("bf/s {}", parse_branch_displacement(opcode, options.branch_displacement))
        }
        0b1000_1001_0000_0000 => {
            format!("bt {}", parse_branch_displacement(opcode, options.branch_displacement))
        }
        0b1000_1101_0000_0000 => {
            format!("bt/s {}", parse_branch_displacement(opcode, options.branch_displacement))
        }
        0b1100_0011_0000_0000 => format!("trapa #{}", parse_signed_immediate(opcode)),
        _ => decode_xnxx(opcode),
    }
}

#[inline]
fn decode_xnxx(opcode: u16) -> String {
    match opcode & 0b1111_0000_1111_1111 {
        0b0000_0000_0010_1001 => format!("movt r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0001 => format!("cmp/pz r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0101 => format!("cmp/pl r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0000 => format!("dt r{}", parse_register_high(opcode)),
        0b0100_0000_0001_1011 => format!("tas.b @r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0100 => format!("rotl r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0101 => format!("rotr r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0100 => format!("rotcl r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0101 => format!("rotcr r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0000 => format!("shal r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0001 => format!("shar r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0000 => format!("shll r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0001 => format!("shlr r{}", parse_register_high(opcode)),
        0b0100_0000_0000_1000 => format!("shll2 r{}", parse_register_high(opcode)),
        0b0100_0000_0000_1001 => format!("shlr2 r{}", parse_register_high(opcode)),
        0b0100_0000_0001_1000 => format!("shll8 r{}", parse_register_high(opcode)),
        0b0100_0000_0001_1001 => format!("shlr8 r{}", parse_register_high(opcode)),
        0b0100_0000_0010_1000 => format!("shll16 r{}", parse_register_high(opcode)),
        0b0100_0000_0010_1001 => format!("shlr16 r{}", parse_register_high(opcode)),
        0b0000_0000_0010_0011 => format!("braf r{}", parse_register_high(opcode)),
        0b0000_0000_0000_0011 => format!("bsrf r{}", parse_register_high(opcode)),
        0b0100_0000_0010_1011 => format!("jmp @r{}", parse_register_high(opcode)),
        0b0100_0000_0000_1011 => format!("jsr @r{}", parse_register_high(opcode)),
        0b0100_0000_0000_1110 => format!("ldc r{}, sr", parse_register_high(opcode)),
        0b0100_0000_0001_1110 => format!("ldc r{}, gbr", parse_register_high(opcode)),
        0b0100_0000_0010_1110 => format!("ldc r{}, vbr", parse_register_high(opcode)),
        0b0100_0000_0000_0111 => format!("ldc.l @r{}+, sr", parse_register_high(opcode)),
        0b0100_0000_0001_0111 => format!("ldc.l @r{}+, gbr", parse_register_high(opcode)),
        0b0100_0000_0010_0111 => format!("ldc.l @r{}+, vbr", parse_register_high(opcode)),
        0b0100_0000_0000_1010 => format!("lds r{}, mach", parse_register_high(opcode)),
        0b0100_0000_0001_1010 => format!("lds r{}, macl", parse_register_high(opcode)),
        0b0100_0000_0010_1010 => format!("lds r{}, pr", parse_register_high(opcode)),
        0b0100_0000_0000_0110 => format!("lds.l @r{}+, mach", parse_register_high(opcode)),
        0b0100_0000_0001_0110 => format!("lds.l @r{}+, macl", parse_register_high(opcode)),
        0b0100_0000_0010_0110 => format!("lds.l @r{}+, pr", parse_register_high(opcode)),
        0b0000_0000_0000_0010 => format!("stc sr, r{}", parse_register_high(opcode)),
        0b0000_0000_0001_0010 => format!("stc gbr, r{}", parse_register_high(opcode)),
        0b0000_0000_0010_0010 => format!("stc vbr, r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0011 => format!("stc.l sr, @-r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0011 => format!("stc.l gbr, @-r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0011 => format!("stc.l vbr, @-r{}", parse_register_high(opcode)),
        0b0000_0000_0000_1010 => format!("sts mach, r{}", parse_register_high(opcode)),
        0b0000_0000_0001_1010 => format!("sts macl, r{}", parse_register_high(opcode)),
        0b0000_0000_0010_1010 => format!("sts pr, r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0010 => format!("sts.l mach, @-r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0010 => format!("sts.l macl, @-r{}", parse_register_high(opcode)),
        0b0100_0000_0010_0010 => format!("sts.l pr, @-r{}", parse_register_high(opcode)),
        _ => decode_xnnn(opcode),
    }
}

#[inline]
fn decode_xnnn(opcode: u16) -> String {
    match opcode & 0b1111_0000_0000_0000 {
        0b1110_0000_0000_0000 => {
            format!("mov.b #{}, r{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1001_0000_0000_0000 => format!(
            "mov.w @({},pc), r{}",
            parse_8bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b1101_0000_0000_0000 => format!(
            "mov.l @({},pc), r{}",
            parse_8bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b0001_0000_0000_0000 => format!(
            "mov.l r{}, @({},r{})",
            parse_register_low(opcode),
            parse_4bit_displacement(opcode),
            parse_register_high(opcode)
        ),
        0b0101_0000_0000_0000 => format!(
            "mov.l @({},r{}), r{}",
            parse_4bit_displacement(opcode),
            parse_register_low(opcode),
            parse_register_high(opcode)
        ),
        0b0111_0000_0000_0000 => {
            format!("add #{}, r{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1010_0000_0000_0000 => format!("bra {}", parse_12bit_displacement(opcode)),
        0b1011_0000_0000_0000 => format!("bsr {}", parse_12bit_displacement(opcode)),
        _ => "illegal".into(),
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

#[inline]
fn parse_branch_displacement(opcode: u16, branch_displacement: BranchDisplacement) -> String {
    let displacement = parse_signed_immediate(opcode);

    match branch_displacement {
        BranchDisplacement::Offset => displacement.to_string(),
        BranchDisplacement::Absolute { pc } => {
            let address = pc.wrapping_add(4).wrapping_add_signed(i32::from(displacement) << 1);
            format!("${address:08X}")
        }
    }
}
