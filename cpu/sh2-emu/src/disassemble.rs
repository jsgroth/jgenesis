use crate::Sh2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccessSize {
    Byte,
    Word,
    Longword,
}

#[derive(Debug, Clone, Copy)]
pub enum Displacement {
    Immediate(u32),
    Register(u16),
}

impl Displacement {
    #[must_use]
    pub fn resolve(self, size: MemoryAccessSize, cpu: &Sh2) -> u32 {
        match self {
            Self::Immediate(displacement) => match size {
                MemoryAccessSize::Byte => displacement,
                MemoryAccessSize::Word => displacement << 1,
                MemoryAccessSize::Longword => displacement << 2,
            },
            Self::Register(r) => cpu.registers.gpr[r as usize],
        }
    }
}

impl Displacement {
    fn none() -> Self {
        Self::Immediate(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadType {
    Load,
    EffectiveAddress, // MOVA, JMP, JSR, BRAF, BSRF
}

#[derive(Debug, Clone, Copy)]
pub enum MemoryAccess {
    IndirectR { register: u16, displacement: Displacement },
    IndirectRPredecrement { register: u16 },
    IndirectGbr { displacement: Displacement },
    PcRelative { pc: u32, displacement: Displacement },
}

impl MemoryAccess {
    #[must_use]
    pub fn resolve_address(self, size: MemoryAccessSize, cpu: &Sh2) -> u32 {
        match self {
            Self::IndirectR { register, displacement } => {
                cpu.registers().gpr[register as usize].wrapping_add(displacement.resolve(size, cpu))
            }
            Self::IndirectRPredecrement { register } => {
                let address = cpu.registers.gpr[register as usize];
                match size {
                    MemoryAccessSize::Byte => address.wrapping_sub(1),
                    MemoryAccessSize::Word => address.wrapping_sub(2),
                    MemoryAccessSize::Longword => address.wrapping_sub(4),
                }
            }
            Self::IndirectGbr { displacement } => {
                cpu.registers().gbr.wrapping_add(displacement.resolve(size, cpu))
            }
            Self::PcRelative { mut pc, displacement } => {
                pc = pc.wrapping_add(4);
                if size == MemoryAccessSize::Longword {
                    pc &= !3;
                }
                pc.wrapping_add(displacement.resolve(size, cpu))
            }
        }
    }
}

pub struct DisassembledInstruction {
    pub pc: u32,
    pub text: String,
    pub opcode: u16,
    pub memory_read: Option<(MemoryAccess, MemoryAccessSize)>,
    pub memory_read_type: ReadType,
    pub memory_write: Option<(MemoryAccess, MemoryAccessSize)>,
}

impl Default for DisassembledInstruction {
    fn default() -> Self {
        Self::new()
    }
}

impl DisassembledInstruction {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pc: 0,
            text: String::new(),
            opcode: 0,
            memory_read: None,
            memory_read_type: ReadType::Load,
            memory_write: None,
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.memory_read = None;
        self.memory_read_type = ReadType::Load;
        self.memory_write = None;
    }
}

pub fn disassemble_into(pc: u32, opcode: u16, out: &mut DisassembledInstruction) {
    out.clear();

    out.text = match opcode {
        0b0000_0000_0001_1001 => "div0u".into(),
        0b0000_0000_0000_1011 => "rts".into(),
        0b0000_0000_0000_1000 => "clrt".into(),
        0b0000_0000_0010_1000 => "clrmac".into(),
        0b0000_0000_0000_1001 => "nop".into(),
        0b0000_0000_0010_1011 => "rte".into(),
        0b0000_0000_0001_1000 => "sett".into(),
        0b0000_0000_0001_1011 => "sleep".into(),
        _ => decode_xnnx(pc, opcode, out),
    };
}

#[inline]
fn decode_xnnx(pc: u32, opcode: u16, out: &mut DisassembledInstruction) -> String {
    match opcode & 0b1111_0000_0000_1111 {
        0b0110_0000_0000_0011 => {
            format!("mov r{}, r{}", parse_register_low(opcode), parse_register_high(opcode))
        }
        0b0010_0000_0000_0000 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::none() },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b r{}, @r{}", parse_register_low(opcode), dest)
        }
        0b0010_0000_0000_0001 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::none() },
                MemoryAccessSize::Word,
            ));
            format!("mov.w r{}, @r{}", parse_register_low(opcode), dest)
        }
        0b0010_0000_0000_0010 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l r{}, @r{}", parse_register_low(opcode), dest)
        }
        0b0110_0000_0000_0000 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b @r{}, r{}", source, parse_register_high(opcode))
        }
        0b0110_0000_0000_0001 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @r{}, r{}", source, parse_register_high(opcode))
        }
        0b0110_0000_0000_0010 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l @r{}, r{}", source, parse_register_high(opcode))
        }
        0b0010_0000_0000_0100 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b r{}, @-r{}", parse_register_low(opcode), dest)
        }
        0b0010_0000_0000_0101 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Word,
            ));
            format!("mov.w r{}, @-r{}", parse_register_low(opcode), dest)
        }
        0b0010_0000_0000_0110 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l r{}, @-r{}", parse_register_low(opcode), dest)
        }
        0b0110_0000_0000_0100 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b @r{}+, r{}", source, parse_register_high(opcode))
        }
        0b0110_0000_0000_0101 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @r{}+, r{}", source, parse_register_high(opcode))
        }
        0b0110_0000_0000_0110 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l @r{}+, r{}", source, parse_register_high(opcode))
        }
        0b0000_0000_0000_0100 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::Register(0) },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b r{}, @(r0,r{})", parse_register_low(opcode), dest)
        }
        0b0000_0000_0000_0101 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::Register(0) },
                MemoryAccessSize::Word,
            ));
            format!("mov.w r{}, @(r0,r{})", parse_register_low(opcode), dest)
        }
        0b0000_0000_0000_0110 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::Register(0) },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l r{}, @(r0,r{})", parse_register_low(opcode), dest)
        }
        0b0000_0000_0000_1100 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Register(0),
                },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b @(r0,r{}), r{}", source, parse_register_high(opcode))
        }
        0b0000_0000_0000_1101 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Register(0),
                },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @(r0,r{}), r{}", source, parse_register_high(opcode))
        }
        0b0000_0000_0000_1110 => {
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Register(0),
                },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l @(r0,r{}), r{}", source, parse_register_high(opcode))
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
            let source = parse_register_low(opcode);
            let dest = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("mac.l @r{source}+, @r{dest}+")
        }
        0b0100_0000_0000_1111 => {
            let source = parse_register_low(opcode);
            let dest = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Word,
            ));
            out.memory_write = Some((
                MemoryAccess::IndirectR { register: dest, displacement: Displacement::none() },
                MemoryAccessSize::Word,
            ));
            format!("mac.w @r{source}+, @r{dest}+")
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
        _ => decode_xxnn(pc, opcode, out),
    }
}

#[inline]
fn decode_xxnn(pc: u32, opcode: u16, out: &mut DisassembledInstruction) -> String {
    match opcode & 0b1111_1111_0000_0000 {
        0b1000_0000_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let dest = parse_register_low(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR {
                    register: dest,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b r0, @({displacement},r{dest})")
        }
        0b1000_0001_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let dest = parse_register_low(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR {
                    register: dest,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Word,
            ));
            format!("mov.w r0, @({displacement},r{dest})")
        }
        0b1000_0100_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b @({displacement},r{source}), r0")
        }
        0b1000_0101_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let source = parse_register_low(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @({displacement},r{source}), r0")
        }
        0b1100_0000_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_write = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b r0, @({displacement},gbr)")
        }
        0b1100_0001_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_write = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Word,
            ));
            format!("mov.w r0, @({displacement},gbr)")
        }
        0b1100_0010_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_write = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l r0, @({displacement},gbr)")
        }
        0b1100_0100_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Byte,
            ));
            format!("mov.b @({displacement},gbr), r0")
        }
        0b1100_0101_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @({displacement},gbr), r0")
        }
        0b1100_0110_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Immediate(displacement) },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l @({displacement},gbr), r0")
        }
        0b1100_0111_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::PcRelative {
                    pc,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Longword,
            ));
            out.memory_read_type = ReadType::EffectiveAddress;
            format!("mova @({displacement},pc), r0")
        }
        0b1000_1000_0000_0000 => format!("cmp/eq #{}, r0", parse_signed_immediate(opcode)),
        0b1100_1001_0000_0000 => format!("and #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1101_0000_0000 => {
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Register(0) },
                MemoryAccessSize::Byte,
            ));
            out.memory_write = out.memory_read;
            format!("and.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode))
        }
        0b1100_1011_0000_0000 => format!("or #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1111_0000_0000 => {
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Register(0) },
                MemoryAccessSize::Byte,
            ));
            out.memory_write = out.memory_read;
            format!("or.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode))
        }
        0b1100_1000_0000_0000 => format!("tst #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1100_0000_0000 => {
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Register(0) },
                MemoryAccessSize::Byte,
            ));
            out.memory_write = out.memory_read;
            format!("tst.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode))
        }
        0b1100_1010_0000_0000 => format!("xor #{}, r0", parse_unsigned_immediate(opcode)),
        0b1100_1110_0000_0000 => {
            out.memory_read = Some((
                MemoryAccess::IndirectGbr { displacement: Displacement::Register(0) },
                MemoryAccessSize::Byte,
            ));
            out.memory_write = out.memory_read;
            format!("xor.b #{}, @(r0,gbr)", parse_unsigned_immediate(opcode))
        }
        0b1000_1011_0000_0000 => {
            format!("bf {}", branch_destination(pc, parse_signed_immediate(opcode) as u32,))
        }
        0b1000_1111_0000_0000 => {
            format!("bf/s {}", branch_destination(pc, parse_signed_immediate(opcode) as u32,))
        }
        0b1000_1001_0000_0000 => {
            format!("bt {}", branch_destination(pc, parse_signed_immediate(opcode) as u32,))
        }
        0b1000_1101_0000_0000 => {
            format!("bt/s {}", branch_destination(pc, parse_signed_immediate(opcode) as u32,))
        }
        0b1100_0011_0000_0000 => format!("trapa #{}", opcode & 0xFF),
        _ => decode_xnxx(pc, opcode, out),
    }
}

#[inline]
fn decode_xnxx(pc: u32, opcode: u16, out: &mut DisassembledInstruction) -> String {
    match opcode & 0b1111_0000_1111_1111 {
        0b0000_0000_0010_1001 => format!("movt r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0001 => format!("cmp/pz r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0101 => format!("cmp/pl r{}", parse_register_high(opcode)),
        0b0100_0000_0001_0000 => format!("dt r{}", parse_register_high(opcode)),
        0b0100_0000_0001_1011 => {
            let register = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register, displacement: Displacement::none() },
                MemoryAccessSize::Byte,
            ));
            out.memory_write = out.memory_read;
            format!("tas.b @r{register}")
        }
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
        0b0000_0000_0010_0011 => {
            let register = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::PcRelative { pc, displacement: Displacement::Register(register) },
                MemoryAccessSize::Word,
            ));
            out.memory_read_type = ReadType::EffectiveAddress;
            format!("braf r{register}")
        }
        0b0000_0000_0000_0011 => {
            let register = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::PcRelative { pc, displacement: Displacement::Register(register) },
                MemoryAccessSize::Word,
            ));
            out.memory_read_type = ReadType::EffectiveAddress;
            format!("bsrf r{register}")
        }
        0b0100_0000_0010_1011 => {
            let register = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            out.memory_read_type = ReadType::EffectiveAddress;
            format!("jmp @r{register}")
        }
        0b0100_0000_0000_1011 => {
            let register = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            out.memory_read_type = ReadType::EffectiveAddress;
            format!("jsr @r{register}")
        }
        0b0100_0000_0000_1110 => format!("ldc r{}, sr", parse_register_high(opcode)),
        0b0100_0000_0001_1110 => format!("ldc r{}, gbr", parse_register_high(opcode)),
        0b0100_0000_0010_1110 => format!("ldc r{}, vbr", parse_register_high(opcode)),
        0b0100_0000_0000_0111 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("ldc.l @r{source}+, sr")
        }
        0b0100_0000_0001_0111 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("ldc.l @r{source}+, gbr")
        }
        0b0100_0000_0010_0111 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("ldc.l @r{source}+, vbr")
        }
        0b0100_0000_0000_1010 => format!("lds r{}, mach", parse_register_high(opcode)),
        0b0100_0000_0001_1010 => format!("lds r{}, macl", parse_register_high(opcode)),
        0b0100_0000_0010_1010 => format!("lds r{}, pr", parse_register_high(opcode)),
        0b0100_0000_0000_0110 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("lds.l @r{source}+, mach")
        }
        0b0100_0000_0001_0110 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("lds.l @r{source}+, macl")
        }
        0b0100_0000_0010_0110 => {
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR { register: source, displacement: Displacement::none() },
                MemoryAccessSize::Longword,
            ));
            format!("lds.l @r{source}+, pr")
        }
        0b0000_0000_0000_0010 => format!("stc sr, r{}", parse_register_high(opcode)),
        0b0000_0000_0001_0010 => format!("stc gbr, r{}", parse_register_high(opcode)),
        0b0000_0000_0010_0010 => format!("stc vbr, r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0011 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("stc.l sr, @-r{dest}")
        }
        0b0100_0000_0001_0011 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("stc.l gbr, @-r{dest}")
        }
        0b0100_0000_0010_0011 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("stc.l vbr, @-r{dest}")
        }
        0b0000_0000_0000_1010 => format!("sts mach, r{}", parse_register_high(opcode)),
        0b0000_0000_0001_1010 => format!("sts macl, r{}", parse_register_high(opcode)),
        0b0000_0000_0010_1010 => format!("sts pr, r{}", parse_register_high(opcode)),
        0b0100_0000_0000_0010 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("sts.l mach, @-r{dest}")
        }
        0b0100_0000_0001_0010 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("sts.l macl, @-r{dest}")
        }
        0b0100_0000_0010_0010 => {
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectRPredecrement { register: dest },
                MemoryAccessSize::Longword,
            ));
            format!("sts.l pr, @-r{dest}")
        }
        _ => decode_xnnn(pc, opcode, out),
    }
}

#[inline]
fn decode_xnnn(pc: u32, opcode: u16, out: &mut DisassembledInstruction) -> String {
    match opcode & 0b1111_0000_0000_0000 {
        0b1110_0000_0000_0000 => {
            format!("mov.b #{}, r{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1001_0000_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::PcRelative {
                    pc,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Word,
            ));
            format!("mov.w @({},pc), r{}", displacement, parse_register_high(opcode))
        }
        0b1101_0000_0000_0000 => {
            let displacement: u32 = parse_8bit_displacement(opcode).into();
            out.memory_read = Some((
                MemoryAccess::PcRelative {
                    pc,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Longword,
            ));
            format!(
                "mov.l @({},pc), r{}",
                parse_8bit_displacement(opcode),
                parse_register_high(opcode)
            )
        }
        0b0001_0000_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let dest = parse_register_high(opcode);
            out.memory_write = Some((
                MemoryAccess::IndirectR {
                    register: dest,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l r{}, @({},r{})", parse_register_low(opcode), displacement, dest)
        }
        0b0101_0000_0000_0000 => {
            let displacement: u32 = parse_4bit_displacement(opcode).into();
            let source = parse_register_high(opcode);
            out.memory_read = Some((
                MemoryAccess::IndirectR {
                    register: source,
                    displacement: Displacement::Immediate(displacement),
                },
                MemoryAccessSize::Longword,
            ));
            format!("mov.l @({},r{}), r{}", displacement, source, parse_register_high(opcode))
        }
        0b0111_0000_0000_0000 => {
            format!("add #{}, r{}", parse_signed_immediate(opcode), parse_register_high(opcode))
        }
        0b1010_0000_0000_0000 => {
            format!("bra {}", branch_destination(pc, parse_12bit_displacement(opcode) as u32))
        }
        0b1011_0000_0000_0000 => {
            format!("bsr {}", branch_destination(pc, parse_12bit_displacement(opcode) as u32))
        }
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
fn branch_destination(pc: u32, displacement: u32) -> String {
    let address = pc.wrapping_add(4).wrapping_add(displacement << 1);
    format!("${address:08X}")
}
